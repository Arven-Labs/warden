//! Sharing a code project's conversation with the hub (P124), so it can be gone on from the phone or the web.
//!
//! The desktop keeps its conversations in `conversations/`; the embedded hub (and so the web and the phone) reads and writes
//! the owner's in `conversations-server/root/`. Sharing moves one conversation's file from the first to the second — moved,
//! not copied, so the opencode session in it never lives in two diverging copies — and the desktop goes on working on it
//! from there: `conversation_dir` says where a conversation is, and the list shows both folders.

use std::path::{Path, PathBuf};

use tauri::State;
use warden_bootstrap::{delete_conversation, list_conversations, load_conversation, save_conversation, Conversation};
use warden_server::conversations::is_valid_id;

use crate::AppState;

/// The hub's folder for the owner's conversations, where the web and the phone look.
pub fn hub_dir() -> Option<PathBuf> {
    warden_bootstrap::default_server_conversations_dir().map(|root| warden_bootstrap::users::root_conversations_dir(&root))
}

fn desktop_dir() -> Result<PathBuf, String> {
    warden_bootstrap::default_conversations_dir().ok_or_else(|| "could not determine the OS config directory".to_string())
}

/// Where conversation `id` is: the hub's folder if it was shared, the desktop's otherwise (also for one that doesn't exist yet).
pub fn dir_for(desktop: &Path, hub: Option<&Path>, id: &str) -> PathBuf {
    match hub {
        Some(hub) if is_valid_id(id) && hub.join(format!("{id}.json")).is_file() => hub.to_path_buf(),
        _ => desktop.to_path_buf(),
    }
}

/// `dir_for` with the real folders: for the commands that read or write one conversation.
pub fn conversation_dir(id: &str) -> Result<PathBuf, String> {
    Ok(dir_for(&desktop_dir()?, hub_dir().as_deref(), id))
}

/// Every conversation the desktop can open, newest first: its own and the shared ones. A hub folder that can't be read
/// (not there yet, locked) only means there are none shared.
pub fn all_conversations(desktop: &Path, hub: Option<&Path>) -> anyhow::Result<Vec<Conversation>> {
    let mut conversations = list_conversations(desktop)?;
    if let Some(hub) = hub {
        let shared = list_conversations(hub).unwrap_or_default();
        conversations.retain(|c| !shared.iter().any(|s| s.id == c.id));
        conversations.extend(shared);
    }
    conversations.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
    Ok(conversations)
}

/// The ids in the hub's folder.
pub fn shared_ids(hub: &Path) -> Vec<String> {
    list_conversations(hub).unwrap_or_default().into_iter().map(|c| c.id).collect()
}

/// Moves conversation `id` to the hub (`shared`) or back. `is_code` says whether a project is a code one: only those conversations
/// are shared, because only they have something the other device can continue (the opencode's session in the project's folder).
pub fn share(desktop: &Path, hub: &Path, id: &str, shared: bool, is_code: &dyn Fn(&str) -> bool) -> Result<Conversation, String> {
    if !is_valid_id(id) {
        return Err(format!("invalid conversation id '{id}'"));
    }
    let (from, to) = if shared { (desktop, hub) } else { (hub, desktop) };
    let conversation = load_conversation(from, id).map_err(|e| format!("{e:#}"))?.ok_or_else(|| format!("no conversation with id '{id}' {}", if shared { "on this computer" } else { "shared with the hub" }))?;
    if shared && !conversation.project_id.as_deref().is_some_and(is_code) {
        return Err("only a conversation in a code project can be shared with the hub".to_string());
    }
    if to.join(format!("{id}.json")).exists() {
        return Err(format!("'{id}' already exists on the other side"));
    }
    save_conversation(to, &conversation).map_err(|e| format!("{e:#}"))?;
    if let Err(err) = delete_conversation(from, id) {
        // Not left in both places: two copies would go on separately.
        let _ = std::fs::remove_file(to.join(format!("{id}.json")));
        return Err(format!("{err:#}"));
    }
    Ok(conversation)
}

/// Shares conversation `conversation_id` with the hub, or stops sharing it. Refused while a task of it runs here.
#[tauri::command]
pub fn share_conversation(state: State<'_, AppState>, conversation_id: String, shared: bool) -> Result<Conversation, String> {
    if state.code.is_running(&conversation_id) {
        return Err("a task of this conversation is running: wait for it, or stop it".to_string());
    }
    let hub = hub_dir().ok_or_else(|| "could not determine the OS config directory".to_string())?;
    let orchestrator = { state.orchestrator.lock().unwrap().clone() }?;
    let store = warden_core::project::ProjectStore::new(orchestrator.vault().clone());
    share(&desktop_dir()?, &hub, &conversation_id, shared, &|project| store.get(project).is_ok_and(|p| p.code))
}

/// The ids of the conversations shared with the hub, for the list to mark.
#[tauri::command]
pub fn shared_conversation_ids() -> Vec<String> {
    hub_dir().map(|hub| shared_ids(&hub)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("desktop-share-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        (root.clone(), root.join("conversations"), root.join("conversations-server").join("root"))
    }

    fn conversation(id: &str, project: Option<&str>, updated_at: i64) -> Conversation {
        let mut c: Conversation = serde_json::from_str(&format!(r#"{{"id":"{id}","title":"t","messages":[],"createdAt":1,"updatedAt":{updated_at}}}"#)).unwrap();
        c.project_id = project.map(str::to_string);
        c.engine_session_id = project.map(|_| "ses_1".to_string());
        c
    }

    fn only_code(project: &str) -> bool {
        project == "app"
    }

    #[test]
    fn a_code_conversation_moves_to_the_hub_with_its_session_and_back() {
        let (root, desktop, hub) = dirs("move");
        save_conversation(&desktop, &conversation("c1", Some("app"), 5)).unwrap();

        let shared = share(&desktop, &hub, "c1", true, &only_code).unwrap();
        assert_eq!((shared.project_id.as_deref(), shared.engine_session_id.as_deref()), (Some("app"), Some("ses_1")));
        assert!(load_conversation(&desktop, "c1").unwrap().is_none(), "moved, not copied");
        assert_eq!(load_conversation(&hub, "c1").unwrap().unwrap().engine_session_id.as_deref(), Some("ses_1"));
        assert_eq!(dir_for(&desktop, Some(&hub), "c1"), hub, "the desktop goes on from the hub's folder");
        assert_eq!(shared_ids(&hub), vec!["c1".to_string()]);

        share(&desktop, &hub, "c1", false, &only_code).unwrap();
        assert!(load_conversation(&hub, "c1").unwrap().is_none());
        assert_eq!(dir_for(&desktop, Some(&hub), "c1"), desktop);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn what_is_not_a_code_conversation_or_a_valid_move_is_refused_and_nothing_changes() {
        let (root, desktop, hub) = dirs("refuse");
        save_conversation(&desktop, &conversation("plain", None, 1)).unwrap();
        save_conversation(&desktop, &conversation("docs", Some("notes"), 1)).unwrap();
        save_conversation(&desktop, &conversation("c1", Some("app"), 1)).unwrap();
        save_conversation(&hub, &conversation("c1", Some("app"), 2)).unwrap();

        assert!(share(&desktop, &hub, "plain", true, &only_code).unwrap_err().contains("code project"));
        assert!(share(&desktop, &hub, "docs", true, &only_code).unwrap_err().contains("code project"));
        assert!(share(&desktop, &hub, "c1", true, &only_code).unwrap_err().contains("already exists"));
        assert!(share(&desktop, &hub, "../x", true, &only_code).unwrap_err().contains("invalid"));
        assert!(share(&desktop, &hub, "nope", true, &only_code).unwrap_err().contains("no conversation"));
        assert!(share(&desktop, &hub, "nope", false, &only_code).unwrap_err().contains("shared with the hub"));
        assert!(load_conversation(&desktop, "plain").unwrap().is_some() && load_conversation(&desktop, "docs").unwrap().is_some());
        assert!(load_conversation(&desktop, "c1").unwrap().is_some(), "the one that clashed stays");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_list_joins_both_folders_newest_first_without_repeating_an_id() {
        let (root, desktop, hub) = dirs("list");
        save_conversation(&desktop, &conversation("a", None, 10)).unwrap();
        save_conversation(&desktop, &conversation("b", Some("app"), 1)).unwrap();
        save_conversation(&hub, &conversation("c", Some("app"), 20)).unwrap();
        save_conversation(&hub, &conversation("b", Some("app"), 30)).unwrap();

        let ids: Vec<String> = all_conversations(&desktop, Some(&hub)).unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["b", "c", "a"], "the hub's copy of b wins, once");
        // A hub folder that isn't there yet is just no shared conversations.
        let ids: Vec<String> = all_conversations(&desktop, Some(&root.join("missing"))).unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!(dir_for(&desktop, None, "b"), desktop);
        std::fs::remove_dir_all(&root).ok();
    }
}

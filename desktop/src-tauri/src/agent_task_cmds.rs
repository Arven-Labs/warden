//! The work agents delegated to each other (P123), for the desktop's "Agent work" screen when it shows this computer: the same log
//! the engine writes (`warden_bootstrap::agent_tasks`), read from disk. A hub in use is asked over the wire instead (`listAgentTasks`).

use warden_bootstrap::agent_tasks::TaskControl;
use warden_server::activity_list::to_dto;
use warden_server::agent_task_list::agent_task_dtos;
use warden_server_protocol::protocol::{ActivityEventDto, AgentTaskDto, GrantDto};

/// The tasks of this computer, newest first. No log yet is no tasks.
#[tauri::command]
pub fn list_agent_tasks() -> Vec<AgentTaskDto> {
    warden_bootstrap::resolve_agent_tasks_path(std::env::var("WARDEN_AGENT_TASKS").ok()).map(|path| agent_task_dtos(&path)).unwrap_or_default()
}

/// What the screen shows of the "always" of this computer: the grants and, when they are kept in a file, how many days one may go
/// unused before it lapses (`ttl_days` is absent when they end with the app).
#[derive(Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantsView {
    grants: Vec<GrantDto>,
    ttl_days: Option<u32>,
}

/// What the person said "always" to on this computer (P122), per conversation and agent.
#[tauri::command]
pub fn list_grants(state: tauri::State<'_, crate::AppState>) -> GrantsView {
    grants_view(&state.grants)
}

/// Takes one "always" back, so the agent asks again; returns the list that is left. No pairing key: the engine is this app's own.
#[tauri::command]
pub fn revoke_grant(state: tauri::State<'_, crate::AppState>, conversation_id: String, agent_id: Option<String>, covers: String) -> GrantsView {
    state.grants.revoke(&conversation_id, agent_id.as_deref(), &covers);
    grants_view(&state.grants)
}

fn grants_view(book: &warden_core::autonomy::GrantBook) -> GrantsView {
    let grants = book.list().into_iter().map(|g| GrantDto { conversation_id: g.conversation, agent_id: g.agent, covers: g.covers, last_used_ms: Some(g.last_used_ms) }).collect();
    GrantsView { grants, ttl_days: book.ttl_days() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_tells_the_term_only_for_a_book_with_a_file() {
        let memory = grants_view(&warden_core::autonomy::GrantBook::default());
        assert_eq!(serde_json::to_string(&memory).unwrap(), r#"{"grants":[],"ttlDays":null}"#);
        let kept = grants_view(&warden_core::autonomy::GrantBook::persistent(std::env::temp_dir().join("warden-desktop-grants-view.json"), std::time::Duration::from_secs(7 * 24 * 60 * 60)));
        assert_eq!(kept.ttl_days, Some(7));
    }
}

/// The feed of activity of this computer (P121): the tasks agents delegated, the notes they left each other and the agents one created or
/// removed, read from the files the engine already writes. There are no agent channels here (those are the hub's), so only what exists on
/// this computer shows.
#[tauri::command]
pub fn list_activity() -> Vec<ActivityEventDto> {
    let tasks = warden_bootstrap::resolve_agent_tasks_path(std::env::var("WARDEN_AGENT_TASKS").ok());
    let changes = warden_bootstrap::default_config_path().map(|path| warden_bootstrap::agent_changes::beside(&path));
    let conversations = warden_bootstrap::default_conversations_dir();
    let dirs: Vec<&std::path::Path> = conversations.as_deref().into_iter().collect();
    warden_bootstrap::activity::read_activity(tasks.as_deref(), changes.as_deref(), &dirs, warden_bootstrap::activity::MAX_EVENTS).into_iter().map(to_dto).collect()
}

/// Pauses, resumes or stops (`action`: `pause`, `resume` or `cancel`) a task of this computer's engine, with the subtasks below it.
/// No pairing key: the engine is this app's own. The refusal says why (not running here, already ended, not in that state).
#[tauri::command]
pub async fn control_agent_task(task_id: String, action: String) -> Result<(), String> {
    let path = warden_bootstrap::resolve_agent_tasks_path(std::env::var("WARDEN_AGENT_TASKS").ok()).ok_or_else(|| "there is no task log on this computer".to_string())?;
    let action = TaskControl::parse(&action)?;
    control_agent_task_in(&path, &task_id, action)?;
    // A stop is carried out by the task's own runtime a moment later: wait for it to be written down, so the list the screen reloads says "cancelled".
    if action == TaskControl::Cancel {
        for _ in 0..50 {
            if !warden_core::jobs::task_controls().is_controllable(&task_id) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    Ok(())
}

fn control_agent_task_in(path: &std::path::Path, task_id: &str, action: TaskControl) -> Result<(), String> {
    warden_bootstrap::agent_tasks::control_agent_task(path, task_id, action)
}

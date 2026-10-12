//! Tells the window when a conversation file changes under it (P87). The desktop only emitted `conversations-changed` for what its
//! own process wrote, so a note the CLI left for an agent (another process, the same folder) showed up only the next time the list
//! was read. Every few seconds this looks at the date and size of each `*.json` in the conversation folders and emits the event
//! for the ones that are new or different — no watcher crate, and no file is opened.
//!
//! What the app itself writes is reported too. That is harmless: the window answers the event by reading the saved copy and
//! merging it (`replaceWithSaved` in `App.tsx`), which keeps a message the screen shows and the disk doesn't have yet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Emitter};

/// How often the folders are looked at.
const POLL_EVERY: Duration = Duration::from_secs(3);

/// What was seen of each conversation file last time, so only a change is reported.
#[derive(Default)]
pub struct Watcher {
    /// `None` until the first look: what was already there when the app started is not news.
    seen: Option<HashMap<PathBuf, (SystemTime, u64)>>,
}

impl Watcher {
    /// The ids of the conversations in `dirs` that are new or have a different date or size since the last call. The first call
    /// reports none. A file that disappears is forgotten without a word; a folder that can't be read counts as empty.
    pub fn poll(&mut self, dirs: &[PathBuf]) -> Vec<String> {
        let mut now = HashMap::new();
        for dir in dirs {
            collect(dir, &mut now);
        }
        let Some(before) = self.seen.replace(now.clone()) else { return Vec::new() };
        let mut changed: Vec<String> = now
            .iter()
            .filter(|(path, stamp)| before.get(*path) != Some(*stamp))
            .filter_map(|(path, _)| path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string))
            .collect();
        changed.sort();
        changed.dedup();
        changed
    }
}

fn collect(dir: &Path, into: &mut HashMap<PathBuf, (SystemTime, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_file() {
            into.insert(path, (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len()));
        }
    }
}

/// Starts looking, for as long as the app runs: this computer's conversations and the hub's folder for the shared ones.
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut watcher = Watcher::default();
        loop {
            // Looked up each time, since the folders are not there until the first conversation.
            let dirs: Vec<PathBuf> = [warden_bootstrap::default_conversations_dir(), crate::share_cmds::hub_dir()].into_iter().flatten().collect();
            for id in watcher.poll(&dirs) {
                let _ = app.emit("conversations-changed", id);
            }
            std::thread::sleep(POLL_EVERY);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("warden-conversation-watch-{tag}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn what_was_already_there_is_not_news_and_a_new_or_changed_file_is_said_once() {
        let dir = temp_dir("changes");
        std::fs::write(dir.join("old.json"), "{}").unwrap();
        let mut watcher = Watcher::default();
        let dirs = [dir.clone()];
        assert!(watcher.poll(&dirs).is_empty(), "the first look only takes note");
        assert!(watcher.poll(&dirs).is_empty(), "and nothing changed since");

        std::fs::write(dir.join("agents-1.json"), "{}").unwrap();
        assert_eq!(watcher.poll(&dirs), vec!["agents-1".to_string()], "a new file");
        assert!(watcher.poll(&dirs).is_empty(), "said once");

        std::fs::write(dir.join("agents-1.json"), "{\"messages\":[]}").unwrap();
        assert_eq!(watcher.poll(&dirs), vec!["agents-1".to_string()], "a file that changed size");
        assert!(watcher.poll(&dirs).is_empty());
    }

    #[test]
    fn only_json_files_count_and_a_missing_folder_or_a_deleted_file_is_quiet() {
        let dir = temp_dir("kinds");
        let mut watcher = Watcher::default();
        let dirs = [dir.clone(), dir.join("not-there")];
        watcher.poll(&dirs);

        std::fs::write(dir.join("c1.json.tmp"), "x").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::create_dir(dir.join("folder.json")).unwrap();
        assert!(watcher.poll(&dirs).is_empty(), "a temp file, a text file and a folder are not conversations");

        std::fs::write(dir.join("c1.json"), "{}").unwrap();
        assert_eq!(watcher.poll(&dirs), vec!["c1".to_string()]);
        std::fs::remove_file(dir.join("c1.json")).unwrap();
        assert!(watcher.poll(&dirs).is_empty(), "a deleted conversation is the app's own doing");
    }

    #[test]
    fn two_folders_are_watched_together() {
        let (mine, hub) = (temp_dir("mine"), temp_dir("hub"));
        let mut watcher = Watcher::default();
        let dirs = [mine.clone(), hub.clone()];
        watcher.poll(&dirs);
        std::fs::write(mine.join("a.json"), "{}").unwrap();
        std::fs::write(hub.join("b.json"), "{}").unwrap();
        assert_eq!(watcher.poll(&dirs), vec!["a".to_string(), "b".to_string()]);
    }
}

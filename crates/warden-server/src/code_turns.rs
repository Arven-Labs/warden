//! The code tasks running on the hub (P103 b), so a `CancelTurn` can find the engine's session to stop. Shared by every
//! connection: the person may stop a task from another device than the one that started it.
//!
//! Only the owner runs code tasks (a member's folder would be a path on the owner's machine), so the registry is keyed by
//! conversation alone and a `CancelTurn` from a member is never looked up.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use warden_core::code_engine::opencode::OpencodeEngine;
use warden_core::code_engine::process::OpencodeProcesses;
use warden_core::code_engine::{CodeEngine, CodeModes};
use warden_core::tool::delegate::ModelChoices;
use warden_core::tool::code_task::{CodeEngineSlot, CodeTaskTool};
use warden_core::tool::code_worktree::CodeWorktreeTool;

use crate::engine_models::EngineModels;

/// What names the opencode's command, for a machine where it isn't `opencode` on the `PATH`.
pub const OPENCODE_BIN_ENV: &str = "WARDEN_OPENCODE";

/// How long a project's opencode server is kept after its last task.
const OPENCODE_IDLE: Duration = Duration::from_secs(30 * 60);

/// The hub's code engine: the opencode, started per project folder when a task needs it, with the hub's model as its
/// only provider (`models`). Nothing is started here — a hub with no code project never runs it, and one without it
/// installed finds out when a task asks. Needs a tokio runtime (the manager reaps idle servers in the background).
pub fn opencode_engine(models: &EngineModels) -> Arc<dyn CodeEngine> {
    let binary = std::env::var(OPENCODE_BIN_ENV).ok().filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "opencode".to_string());
    Arc::new(OpencodeEngine::new(Arc::new(OpencodeProcesses::new(binary, Some(models.opencode_config()), OPENCODE_IDLE))))
}

/// Gives `orchestrator` the `code_task` tool (P89): an agent that a person listed it for hands tasks to the engine that
/// `engine` will hold. The hub builds its orchestrator again whenever its settings are saved, so every build goes through
/// here with the same slot. `models` are the ones a task may name (`None` when the hub has no choice to offer).
pub fn register_code_task(orchestrator: &mut warden_core::orchestrator::Orchestrator, engine: &CodeEngineSlot, models: Option<ModelChoices>) {
    let tool = CodeTaskTool::new(orchestrator.vault().clone(), engine.clone());
    orchestrator.register_tool(Arc::new(match models {
        Some(models) => tool.with_models(models),
        None => tool,
    }));
    // P124 — what an isolated task left is looked at, merged or thrown away with this one.
    orchestrator.register_tool(Arc::new(CodeWorktreeTool::new(orchestrator.vault().clone())));
}

#[derive(Clone, Default)]
pub struct CodeTurns {
    running: Arc<Mutex<HashMap<String, Running>>>,
    modes: CodeModes,
}

#[derive(Clone)]
struct Running {
    workdir: String,
    /// Known once the engine has opened (or found) the session; a stop before that has nothing to stop yet.
    session: Option<String>,
}

impl CodeTurns {
    /// How much each conversation asks, shared by every connection: another device can change it too.
    pub fn modes(&self) -> &CodeModes {
        &self.modes
    }

    pub fn begin(&self, conversation_id: &str, workdir: &str) {
        self.running.lock().unwrap().insert(conversation_id.to_string(), Running { workdir: workdir.to_string(), session: None });
    }

    /// Whether a task of the conversation is running now, with or without a session yet.
    pub fn is_running(&self, conversation_id: &str) -> bool {
        self.running.lock().unwrap().contains_key(conversation_id)
    }

    pub fn set_session(&self, conversation_id: &str, session: String) {
        if let Some(running) = self.running.lock().unwrap().get_mut(conversation_id) {
            running.session = Some(session);
        }
    }

    pub fn end(&self, conversation_id: &str) {
        self.running.lock().unwrap().remove(conversation_id);
    }

    /// `begin`, but the entry is taken out when the guard drops — also when the task is cancelled or panics mid-turn, which a
    /// plain `end` after the work would never reach and so would leave the conversation "running" until the app restarts.
    pub fn hold(&self, conversation_id: &str, workdir: &str) -> TurnGuard {
        self.begin(conversation_id, workdir);
        TurnGuard { turns: self.clone(), conversation_id: conversation_id.to_string() }
    }

    /// `(working folder, session)` of the task a conversation is running, if it has got as far as having a session.
    pub fn session_of(&self, conversation_id: &str) -> Option<(String, String)> {
        let running = self.running.lock().unwrap();
        let running = running.get(conversation_id)?;
        Some((running.workdir.clone(), running.session.clone()?))
    }
}

/// Keeps a conversation in the running map for as long as it lives (`CodeTurns::hold`).
pub struct TurnGuard {
    turns: CodeTurns,
    conversation_id: String,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.turns.end(&self.conversation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_held_turn_is_released_when_its_task_is_cancelled_or_panics() {
        let turns = CodeTurns::default();
        let held = turns.clone();
        let task = tokio::spawn(async move {
            let _guard = held.hold("c1", "/repo");
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        assert!(turns.is_running("c1"));
        task.abort();
        let _ = task.await;
        assert!(!turns.is_running("c1"), "an aborted task frees the conversation");

        let held = turns.clone();
        let task = tokio::spawn(async move {
            let _guard = held.hold("c2", "/repo");
            panic!("engine blew up");
        });
        let _ = task.await;
        assert!(!turns.is_running("c2"), "a panic frees the conversation");
    }

    #[test]
    fn a_task_can_be_found_once_it_has_a_session_and_not_after_it_ends() {
        let turns = CodeTurns::default();
        assert_eq!(turns.session_of("c1"), None, "nothing running");
        turns.begin("c1", "/repo");
        assert_eq!(turns.session_of("c1"), None, "no session yet, nothing to stop");
        turns.set_session("c1", "ses_1".into());
        assert_eq!(turns.session_of("c1"), Some(("/repo".into(), "ses_1".into())));
        assert_eq!(turns.clone().session_of("c1"), Some(("/repo".into(), "ses_1".into())), "every connection sees the same");
        assert_eq!(turns.session_of("c2"), None);
        turns.end("c1");
        assert_eq!(turns.session_of("c1"), None);
        turns.set_session("c1", "late".into());
        assert_eq!(turns.session_of("c1"), None, "a session told after the end is not a running task");
    }
}

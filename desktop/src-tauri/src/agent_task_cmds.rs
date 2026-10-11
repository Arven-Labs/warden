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

/// What the person said "always" to on this computer (P122), per conversation and agent, until the app closes.
#[tauri::command]
pub fn list_grants(state: tauri::State<'_, crate::AppState>) -> Vec<GrantDto> {
    grant_dtos(&state.grants)
}

/// Takes one "always" back, so the agent asks again; returns the list that is left. No pairing key: the engine is this app's own.
#[tauri::command]
pub fn revoke_grant(state: tauri::State<'_, crate::AppState>, conversation_id: String, agent_id: Option<String>, covers: String) -> Vec<GrantDto> {
    state.grants.revoke(&conversation_id, agent_id.as_deref(), &covers);
    grant_dtos(&state.grants)
}

fn grant_dtos(book: &warden_core::autonomy::GrantBook) -> Vec<GrantDto> {
    book.list().into_iter().map(|g| GrantDto { conversation_id: g.conversation, agent_id: g.agent, covers: g.covers, last_used_ms: Some(g.last_used_ms) }).collect()
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

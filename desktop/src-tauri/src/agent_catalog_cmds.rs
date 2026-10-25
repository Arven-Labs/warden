//! Tauri commands backing the gallery of ready-made agents (P140) on this computer's own agents. Adding one goes through
//! `edit_agent_org` (an `InstallCatalogAgent` edit, the same door as the rest of the tree); what is here is the list, with each agent's
//! state in the local config, and the skills the agent works with, which go into the local vault after the agent is saved.

use serde::Serialize;
use tauri::State;
use warden_bootstrap::agent_catalog;
use warden_bootstrap::{default_config_path, load_config_from_path};
use warden_core::skill::SkillStore;

use crate::AppState;

/// One agent of the built-in gallery and where it stands among this computer's agents: `notInstalled`, `installed` or `modified`.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAgentPayload {
    id: String,
    role: String,
    description: String,
    skills: Vec<String>,
    status: &'static str,
}

#[tauri::command]
pub fn list_agent_catalog() -> Result<Vec<CatalogAgentPayload>, String> {
    let path = default_config_path().ok_or_else(|| "could not determine the OS config directory".to_string())?;
    let config = load_config_from_path(&path, false).map_err(|e| format!("{e:#}"))?;
    Ok(agent_catalog::entries()
        .iter()
        .map(|entry| CatalogAgentPayload {
            id: entry.id.clone(),
            role: entry.role.clone(),
            description: entry.description.clone(),
            skills: entry.skills.clone(),
            status: agent_catalog::status(&config, entry).as_str(),
        })
        .collect())
}

/// Puts the skills the gallery agent `id` works with in the local vault, without replacing any the person has. The agent is already
/// saved: a vault that can't be reached leaves it in place (the person can add the skills by hand).
pub fn install_skills_of(state: &State<'_, AppState>, id: &str) {
    let Some(entry) = agent_catalog::entries().iter().find(|e| e.id == id) else { return };
    let Ok(orchestrator) = state.orchestrator.lock().unwrap().clone() else { return };
    // What was put in and what was kept is not shown here: the Skills screen lists them.
    let _ = agent_catalog::install_skills(&SkillStore::new(orchestrator.vault().clone()), entry);
}

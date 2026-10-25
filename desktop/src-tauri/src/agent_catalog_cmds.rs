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
    /// Those of `skills` the person already has in a version that differs from the gallery's: installing leaves them as they are.
    skills_kept: Vec<String>,
    status: &'static str,
}

#[tauri::command]
pub fn list_agent_catalog(state: State<'_, AppState>) -> Result<Vec<CatalogAgentPayload>, String> {
    let path = default_config_path().ok_or_else(|| "could not determine the OS config directory".to_string())?;
    let config = load_config_from_path(&path, false).map_err(|e| format!("{e:#}"))?;
    // Without a reachable vault there is nothing to say about the skills: the line is just left out.
    let store = state.orchestrator.lock().unwrap().clone().ok().map(|orchestrator| SkillStore::new(orchestrator.vault().clone()));
    Ok(agent_catalog::entries()
        .iter()
        .map(|entry| CatalogAgentPayload {
            id: entry.id.clone(),
            role: entry.role.clone(),
            description: entry.description.clone(),
            skills: entry.skills.clone(),
            skills_kept: store.as_ref().map(|store| agent_catalog::skills_kept(store, entry)).unwrap_or_default(),
            status: agent_catalog::status(&config, entry).as_str(),
        })
        .collect())
}

/// Puts the skills the gallery agent `id` works with in the local vault, without replacing any the person has. The agent is already
/// saved: a vault that can't be reached leaves it in place (the person can add the skills by hand).
pub fn install_skills_of(state: &State<'_, AppState>, id: &str) {
    let Some(entry) = agent_catalog::entries().iter().find(|e| e.id == id) else { return };
    let Ok(orchestrator) = state.orchestrator.lock().unwrap().clone() else { return };
    // What was kept is not reported here: the gallery's list already says it (`skills_kept`), before and after.
    let _ = agent_catalog::install_skills(&SkillStore::new(orchestrator.vault().clone()), entry);
}

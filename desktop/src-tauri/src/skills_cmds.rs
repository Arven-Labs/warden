//! Tauri commands backing the "Skills" screen (P16) — list/create/edit/delete the skills stored
//! under `skills/` in the vault (`warden_core::skill`), plus a "describe it and the AI drafts it"
//! command. Split out of `lib.rs` the same way `vault_cmds.rs` is. Skills live in the vault, not
//! `config.toml`, so nothing here touches `save_settings`.
//!
//! Attached files (P72 d) have their own commands rather than riding on `SkillPayload`: they only
//! exist for a saved skill, and keeping them out of the payload leaves the list cheap (no file
//! contents) and lets the edit form save the skill without touching its attachments.

use serde::{Deserialize, Serialize};
use tauri::State;
use warden_bootstrap::skill_package;
use warden_bootstrap::{build_model_for, default_config_path, load_config_from_path};
use warden_core::skill::package::MAX_PACKAGE_BYTES;
use warden_core::skill::{catalog, Skill, SkillStore};
use warden_sync::publish;

use crate::AppState;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillPayload {
    name: String,
    description: String,
    body: String,
    /// Agent ids the skill is restricted to (P72 c); empty = every agent. `default` so a caller
    /// that predates the field (or the mobile-style payload without it) still deserializes.
    #[serde(default)]
    agents: Vec<String>,
    /// P104: a suggestion the assistant made (not yet accepted). Saving the form sends none of these back,
    /// which is what accepts it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    proposed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proposed_at: Option<i64>,
    /// P115: the suggestion is a change to this existing skill (display only; the store keeps what is on disk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revises: Option<String>,
}

impl From<Skill> for SkillPayload {
    fn from(skill: Skill) -> Self {
        Self { name: skill.name, description: skill.description, body: skill.body, agents: skill.agents, proposed: skill.proposed, source: skill.source, proposed_at: skill.proposed_at, revises: skill.revises }
    }
}

impl From<SkillPayload> for Skill {
    fn from(payload: SkillPayload) -> Self {
        let proposed = payload.proposed;
        Self {
            name: payload.name.trim().to_string(),
            description: payload.description,
            body: payload.body,
            agents: payload.agents,
            proposed,
            source: payload.source.filter(|_| proposed),
            proposed_at: payload.proposed_at.filter(|_| proposed),
            revises: None,
        }
    }
}

fn store(state: &State<'_, AppState>) -> Result<SkillStore, String> {
    let orchestrator = { state.orchestrator.lock().unwrap().clone() }?;
    Ok(SkillStore::new(orchestrator.vault().clone()))
}

/// `overwrite: false` is "create" (refuses a name already taken, so the New form can't silently
/// replace another skill); `true` is "edit" (the frontend locks the name field then).
fn save_inner(store: &SkillStore, skill: Skill, overwrite: bool) -> Result<(), String> {
    skill.validate().map_err(|e| format!("{e:#}"))?;
    if !overwrite && store.exists(&skill.name) {
        return Err(format!("a skill named '{}' already exists", skill.name));
    }
    store.save(&skill).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn list_skills(state: State<'_, AppState>) -> Result<Vec<SkillPayload>, String> {
    Ok(store(&state)?.list().into_iter().map(Into::into).collect())
}

#[tauri::command]
pub fn save_skill(state: State<'_, AppState>, skill: SkillPayload, overwrite: bool) -> Result<(), String> {
    save_inner(&store(&state)?, skill.into(), overwrite)
}

#[tauri::command]
pub fn delete_skill(state: State<'_, AppState>, name: String) -> Result<(), String> {
    store(&state)?.delete(&name).map_err(|e| format!("{e:#}"))
}

/// One skill of the built-in gallery (P140) and where it stands in this vault: `notInstalled`, `installed` or `modified`.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSkillPayload {
    name: String,
    description: String,
    body: String,
    status: &'static str,
}

fn catalog_of(store: &SkillStore) -> Vec<CatalogSkillPayload> {
    catalog::entries()
        .iter()
        .map(|entry| CatalogSkillPayload {
            name: entry.name.clone(),
            description: entry.description.clone(),
            body: entry.body.clone(),
            status: catalog::status(store, entry).as_str(),
        })
        .collect()
}

#[tauri::command]
pub fn list_skill_catalog(state: State<'_, AppState>) -> Result<Vec<CatalogSkillPayload>, String> {
    Ok(catalog_of(&store(&state)?))
}

/// Installs a skill of the gallery; a skill of that name that differs is only replaced with `overwrite`.
#[tauri::command]
pub fn install_catalog_skill(state: State<'_, AppState>, name: String, overwrite: bool) -> Result<(), String> {
    catalog::install(&store(&state)?, &name, overwrite).map_err(|e| format!("{e:#}"))
}

/// A skill packed into the text of a `*.warden-skill.json` file (P160). `attachments_skipped`: the skill has attached files, and
/// they are not in the package.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportedPackage {
    content: String,
    attachments_skipped: bool,
}

/// What a skill package says it holds, checked but not installed (P160). `finding` is why its text looks unsafe, if it does.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackagePreviewPayload {
    id: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    license: String,
    name: String,
    description: String,
    body: String,
    hash: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    finding: Option<String>,
}

#[tauri::command]
pub fn export_skill_package(state: State<'_, AppState>, name: String, version: String, author: Option<String>, license: String) -> Result<ExportedPackage, String> {
    let (content, attachments_skipped) = skill_package::export_skill_package(&store(&state)?, &name, &version, author.as_deref(), &license)?;
    Ok(ExportedPackage { content, attachments_skipped })
}

#[tauri::command]
pub fn preview_skill_package(state: State<'_, AppState>, content: String) -> Result<PackagePreviewPayload, String> {
    let p = skill_package::preview_skill_package(&store(&state)?, &content)?;
    Ok(PackagePreviewPayload {
        id: p.id,
        version: p.version,
        author: p.author,
        license: p.license,
        name: p.name,
        description: p.description,
        body: p.body,
        hash: p.hash,
        status: p.status,
        finding: p.finding,
    })
}

/// Installs a skill package; a skill of that name that differs is only replaced with `overwrite`, and text that looks unsafe
/// only with `allow_flagged`.
#[tauri::command]
pub fn install_skill_package(state: State<'_, AppState>, content: String, overwrite: bool, allow_flagged: bool) -> Result<(), String> {
    skill_package::install_skill_package(&store(&state)?, &content, overwrite, allow_flagged)
}

/// Reads a package file picked in the Import dialog. Refuses what is bigger than a package can be, before reading it.
#[tauri::command]
pub fn read_skill_package_file(path: String) -> Result<String, String> {
    let size = std::fs::metadata(&path).map_err(|e| format!("{e:#}"))?.len();
    if size > MAX_PACKAGE_BYTES as u64 {
        return Err(format!("This file is {} KiB; a skill package is at most {} KiB", size.div_ceil(1024), MAX_PACKAGE_BYTES / 1024));
    }
    std::fs::read_to_string(&path).map_err(|e| format!("{e:#}"))
}

/// Writes a package to the path picked in the Export dialog.
#[tauri::command]
pub fn write_skill_package_file(path: String, content: String) -> Result<(), String> {
    if content.len() > MAX_PACKAGE_BYTES {
        return Err(format!("A skill package is at most {} KiB", MAX_PACKAGE_BYTES / 1024));
    }
    std::fs::write(&path, content).map_err(|e| format!("{e:#}"))
}

/// The QR to scan with the TruthID app, and the package's hash so the screen can say what is being published (P160).
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PublishBeginPayload {
    qr_svg: String,
    hash: String,
}

/// Step 1 of Publish: checks the package, builds the QR for the TruthID app and keeps the pending pin in `AppState` for
/// `skill_publish_await`. Showing the QR and waiting for the phone are separate calls so the screen can draw the QR before the
/// (human-paced) wait begins, like Sync. The phone is looked for on this computer's network, so this runs here, not on a hub.
#[tauri::command]
pub fn skill_publish_begin(state: State<'_, AppState>, content: String) -> Result<PublishBeginPayload, String> {
    let begin = publish::begin_publish(&content).map_err(|e| format!("{e:#}"))?;
    let qr_json = begin.pending.qr_payload_json().map_err(|e| format!("{e:#}"))?;
    let qr_svg = crate::qr::render_qr_svg(&qr_json)?;
    let hash = begin.hash.clone();
    *state.pending_publish.lock().unwrap() = Some(begin);
    Ok(PublishBeginPayload { qr_svg, hash })
}

/// What a successful publish gives back: the Arweave transaction id (the package is at `ar://<txId>`).
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PublishResultPayload {
    tx_id: String,
}

/// Step 2 of Publish: waits for the TruthID phone to pin the package prepared by `skill_publish_begin`.
#[tauri::command]
pub async fn skill_publish_await(state: State<'_, AppState>) -> Result<PublishResultPayload, String> {
    let begin = state.pending_publish.lock().unwrap().take().ok_or_else(|| "No publish in progress: start one first".to_string())?;
    let outcome = publish::run_publish(begin).await.map_err(|e| format!("{e:#}"))?;
    Ok(PublishResultPayload { tx_id: outcome.tx_id })
}

#[tauri::command]
pub fn list_skill_files(state: State<'_, AppState>, name: String) -> Result<Vec<String>, String> {
    store(&state)?.list_files(&name).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn read_skill_attachment(state: State<'_, AppState>, name: String, file: String) -> Result<String, String> {
    store(&state)?.read_file(&name, &file).map_err(|e| format!("{e:#}"))
}

/// Creates or replaces an attachment; the store enforces the name, size and count limits.
#[tauri::command]
pub fn save_skill_attachment(
    state: State<'_, AppState>,
    name: String,
    file: String,
    content: String,
) -> Result<(), String> {
    store(&state)?.save_file(&name, file.trim(), &content).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn delete_skill_attachment(state: State<'_, AppState>, name: String, file: String) -> Result<(), String> {
    store(&state)?.delete_file(&name, &file).map_err(|e| format!("{e:#}"))
}

/// Asks the model to draft a skill from `prompt` and returns it *unsaved* — the frontend loads it
/// into the form for review. Uses the provider the user picked (`provider_id`, `null` = the active
/// one), same convention as `send_message`.
#[tauri::command]
pub async fn generate_skill_draft(
    state: State<'_, AppState>,
    prompt: String,
    provider_id: Option<String>,
) -> Result<SkillPayload, String> {
    let orchestrator = { state.orchestrator.lock().unwrap().clone() }?;
    let model = match provider_id {
        Some(id) => {
            let path = default_config_path().ok_or_else(|| "could not determine the OS config directory".to_string())?;
            let config = load_config_from_path(&path, false).map_err(|e| format!("{e:#}"))?;
            build_model_for(&config, &id, None).map_err(|e| format!("{e:#}"))?
        }
        None => orchestrator.model().clone(),
    };
    let skill =
        warden_bootstrap::skill_gen::generate_skill_draft(model.as_ref(), &prompt).await.map_err(|e| format!("{e:#}"))?;
    Ok(skill.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use warden_core::memory::Vault;

    fn temp_store() -> SkillStore {
        SkillStore::new(Arc::new(Vault::new(std::env::temp_dir().join(format!(
            "warden-desktop-skills-test-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )))))
    }

    fn skill(name: &str) -> Skill {
        Skill { name: name.into(), description: "d".into(), body: "b".into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None }
    }

    #[test]
    fn agents_survive_the_payload_roundtrip_and_default_to_empty() {
        let restricted = Skill { agents: vec!["writer".into()], ..skill("x") };
        let payload: SkillPayload = restricted.clone().into();
        assert_eq!(Skill::from(payload), restricted);

        let legacy: SkillPayload = serde_json::from_str(r#"{"name":"x","description":"d","body":"b"}"#).unwrap();
        assert!(legacy.agents.is_empty());
    }

    #[test]
    fn create_refuses_an_existing_name_but_edit_overwrites_it() {
        let store = temp_store();
        save_inner(&store, skill("x"), false).unwrap();

        let err = save_inner(&store, skill("x"), false).unwrap_err();
        assert!(err.contains("already exists"));

        let edited = Skill { body: "new".into(), ..skill("x") };
        save_inner(&store, edited, true).unwrap();
        assert_eq!(store.get("x").unwrap().body, "new");
    }

    #[test]
    fn save_rejects_an_invalid_skill_before_touching_disk() {
        let store = temp_store();
        assert!(save_inner(&store, skill("../escape"), false).is_err());
        assert!(save_inner(&store, Skill { body: " ".into(), ..skill("ok") }, false).is_err());
        assert!(store.list().is_empty());
    }

    #[test]
    fn payload_serializes_camel_case_and_trims_the_name() {
        let json = serde_json::to_value(SkillPayload::from(skill("x"))).unwrap();
        assert_eq!(json, serde_json::json!({ "name": "x", "description": "d", "body": "b", "agents": [] }));

        let parsed: SkillPayload = serde_json::from_value(json).unwrap();
        let padded = SkillPayload { name: "  x  ".into(), ..parsed };
        assert_eq!(Skill::from(padded).name, "x");
    }
}

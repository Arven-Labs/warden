//! Bridge functions for the mobile "Skills" screen (P72 b) — the same `warden_core::skill` store the
//! desktop's `skills_cmds.rs` and the CLI's `/skills` use, so the name/description/body rules live
//! in one place instead of a Dart re-implementation of the frontmatter format. Skills are plain
//! `skills/<name>.md` files in the local vault, so they ride the existing sync
//! (`Vault::list_all_files`) to the desktop/server with no change here.
//!
//! Plain blocking `pub fn`s, like `api::sync` — pure file I/O, no Tokio needed.

use std::sync::Arc;

use warden_core::memory::Vault;
use warden_core::skill::{catalog, Skill, SkillStore};

#[derive(Debug, Clone)]
pub struct SkillDto {
    pub name: String,
    pub description: String,
    pub body: String,
    /// An AI suggestion (P104) still waiting for the person to accept it. Saving a DTO with this
    /// `true` keeps it a suggestion; accepting is an explicit save with `false`.
    pub proposed: bool,
    pub source: Option<String>,
    pub proposed_at: Option<i64>,
    /// The suggestion is a change to this existing skill; accepting it applies it there (`SkillStore::save`).
    pub revises: Option<String>,
}

impl From<Skill> for SkillDto {
    fn from(skill: Skill) -> Self {
        Self { name: skill.name, description: skill.description, body: skill.body, proposed: skill.proposed, source: skill.source, proposed_at: skill.proposed_at, revises: skill.revises }
    }
}

fn store(vault_root: &str) -> SkillStore {
    SkillStore::new(Arc::new(Vault::new(vault_root)))
}

pub fn bridge_list_skills(vault_root: String) -> Vec<SkillDto> {
    store(&vault_root).list().into_iter().map(Into::into).collect()
}

/// `overwrite: false` is "create" (refuses a name already taken, so the New form can't silently
/// replace another skill); `true` is "edit" (the Dart side locks the name field then). Same
/// contract as the desktop's `save_skill`.
pub fn bridge_save_skill(vault_root: String, skill: SkillDto, overwrite: bool) -> Result<(), String> {
    let store = store(&vault_root);
    // The mobile UI doesn't edit the agent restriction (P72 c), so an edit keeps whatever the
    // desktop/CLI set — otherwise saving here would silently make the skill global again.
    let agents = if overwrite { store.get(skill.name.trim()).map(|s| s.agents).unwrap_or_default() } else { Vec::new() };
    let name = skill.name.trim().to_string();
    // A suggestion that's edited but not accepted keeps when it was made (same rule as the hub's save).
    let proposed_at = if skill.proposed { skill.proposed_at.or_else(|| store.get(&name).ok().and_then(|s| s.proposed_at)) } else { None };
    let skill = Skill { name, description: skill.description, body: skill.body, agents, proposed: skill.proposed, source: if skill.proposed { skill.source } else { None }, proposed_at, revises: None };
    skill.validate().map_err(|e| format!("{e:#}"))?;
    if !overwrite && store.exists(&skill.name) {
        return Err(format!("a skill named '{}' already exists", skill.name));
    }
    store.save(&skill).map_err(|e| format!("{e:#}"))
}

pub fn bridge_delete_skill(vault_root: String, name: String) -> Result<(), String> {
    store(&vault_root).delete(&name).map_err(|e| format!("{e:#}"))
}

/// One entry of the gallery of ready-made skills (P140), with where it stands in this vault. `status` is the
/// word the hub's wire uses: `notInstalled`, `installed` or `modified`.
#[derive(Debug, Clone)]
pub struct CatalogSkillDto {
    pub name: String,
    pub description: String,
    pub body: String,
    pub status: String,
}

pub fn bridge_list_skill_catalog(vault_root: String) -> Vec<CatalogSkillDto> {
    let store = store(&vault_root);
    catalog::entries()
        .iter()
        .map(|entry| CatalogSkillDto {
            name: entry.name.clone(),
            description: entry.description.clone(),
            body: entry.body.clone(),
            status: catalog::status(&store, entry).as_str().to_string(),
        })
        .collect()
}

/// A skill of that name that differs is only replaced with `overwrite`, so a tap on "Install" never throws away
/// what the person wrote (same contract as the hub's `installCatalogSkill`).
pub fn bridge_install_catalog_skill(vault_root: String, name: String, overwrite: bool) -> Result<(), String> {
    catalog::install(&store(&vault_root), &name, overwrite).map_err(|e| format!("{e:#}"))
}

/// One attachment of a skill (P72 d): its name and its size in bytes. `u32` because the limit is 64 KB, and it keeps the
/// generated Dart a plain `int`.
#[derive(Debug, Clone)]
pub struct SkillFileInfo {
    pub name: String,
    pub size: u32,
}

/// The attachments of `name`, sorted, with their sizes; empty when there are none or the skill is unknown. The text is
/// asked for with `bridge_read_skill_file`.
pub fn bridge_list_skill_files(vault_root: String, name: String) -> Result<Vec<SkillFileInfo>, String> {
    let store = store(&vault_root);
    let names = store.list_files(&name).map_err(|e| format!("{e:#}"))?;
    names
        .into_iter()
        .map(|file| {
            let text = store.read_file(&name, &file).map_err(|e| format!("{e:#}"))?;
            Ok(SkillFileInfo { name: file, size: u32::try_from(text.len()).unwrap_or(u32::MAX) })
        })
        .collect()
}

pub fn bridge_read_skill_file(vault_root: String, name: String, file: String) -> Result<String, String> {
    store(&vault_root).read_file(&name, &file).map_err(|e| format!("{e:#}"))
}

/// Creates or replaces an attachment; the skill must already exist. The name, the 64 KB and the 20 per skill are the
/// `SkillStore`'s rules, the same the CLI, the desktop and the hub apply.
pub fn bridge_save_skill_file(vault_root: String, name: String, file: String, content: String) -> Result<(), String> {
    store(&vault_root).save_file(&name, &file, &content).map_err(|e| format!("{e:#}"))
}

pub fn bridge_delete_skill_file(vault_root: String, name: String, file: String) -> Result<(), String> {
    store(&vault_root).delete_file(&name, &file).map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault() -> String {
        let dir = std::env::temp_dir().join(format!(
            "warden-bridge-skills-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        dir.to_string_lossy().into_owned()
    }

    fn dto(name: &str) -> SkillDto {
        SkillDto { name: name.into(), description: "Reviews a PR".into(), body: "Step 1.\nStep 2.".into(), proposed: false, source: None, proposed_at: None, revises: None }
    }

    #[test]
    fn a_suggestion_stays_pending_until_saved_as_accepted() {
        let vault = temp_vault();
        let suggested = SkillDto { proposed: true, source: Some("conversa".into()), proposed_at: Some(42), ..dto("tip") };
        bridge_save_skill(vault.clone(), suggested.clone(), false).unwrap();
        assert!(bridge_list_skills(vault.clone())[0].proposed);

        // Editing without accepting keeps it a suggestion and keeps when it was made.
        let edited = SkillDto { body: "Changed.".into(), proposed_at: None, ..suggested.clone() };
        bridge_save_skill(vault.clone(), edited, true).unwrap();
        let after = bridge_list_skills(vault.clone()).remove(0);
        assert!(after.proposed);
        assert_eq!(after.proposed_at, Some(42));

        // Accepting is an explicit save with `proposed: false`.
        bridge_save_skill(vault.clone(), SkillDto { proposed: false, ..suggested }, true).unwrap();
        let accepted = bridge_list_skills(vault).remove(0);
        assert!(!accepted.proposed && accepted.source.is_none() && accepted.proposed_at.is_none());
    }

    #[test]
    fn editing_keeps_the_agent_restriction_set_elsewhere() {
        let vault = temp_vault();
        bridge_save_skill(vault.clone(), dto("review-pr"), false).unwrap();
        let store = store(&vault);
        let mut restricted = store.get("review-pr").unwrap();
        restricted.agents = vec!["writer".into()];
        store.save(&restricted).unwrap();

        let edited = SkillDto { body: "New body.".into(), ..dto("review-pr") };
        bridge_save_skill(vault.clone(), edited, true).unwrap();

        let after = store.get("review-pr").unwrap();
        assert_eq!(after.body, "New body.");
        assert_eq!(after.agents, vec!["writer"]);
    }

    #[test]
    fn save_list_and_delete_roundtrip() {
        let vault = temp_vault();
        assert!(bridge_list_skills(vault.clone()).is_empty());

        bridge_save_skill(vault.clone(), dto("review-pr"), false).unwrap();
        let skills = bridge_list_skills(vault.clone());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "review-pr");
        assert_eq!(skills[0].body, "Step 1.\nStep 2.");

        bridge_delete_skill(vault.clone(), "review-pr".into()).unwrap();
        assert!(bridge_list_skills(vault).is_empty());
    }

    #[test]
    fn create_refuses_an_existing_name_but_edit_overwrites_it() {
        let vault = temp_vault();
        bridge_save_skill(vault.clone(), dto("a"), false).unwrap();
        assert!(bridge_save_skill(vault.clone(), dto("a"), false).is_err());

        let edited = SkillDto { description: "New".into(), ..dto("a") };
        bridge_save_skill(vault.clone(), edited, true).unwrap();
        assert_eq!(bridge_list_skills(vault)[0].description, "New");
    }

    #[test]
    fn the_gallery_installs_and_its_status_follows_the_vault() {
        let vault = temp_vault();
        let gallery = bridge_list_skill_catalog(vault.clone());
        assert!(!gallery.is_empty());
        assert!(gallery.iter().all(|e| e.status == "notInstalled"));

        let name = gallery[0].name.clone();
        bridge_install_catalog_skill(vault.clone(), name.clone(), false).unwrap();
        let after = bridge_list_skill_catalog(vault.clone());
        assert_eq!(after[0].status, "installed");
        assert!(after[1..].iter().all(|e| e.status == "notInstalled"));
        assert_eq!(bridge_list_skills(vault.clone())[0].name, name);

        // Editing it makes the gallery entry "modified"; installing again needs the explicit overwrite.
        bridge_save_skill(vault.clone(), SkillDto { body: "My own steps.".into(), ..dto(&name) }, true).unwrap();
        assert_eq!(bridge_list_skill_catalog(vault.clone())[0].status, "modified");
        assert!(bridge_install_catalog_skill(vault.clone(), name.clone(), false).is_err());
        assert_eq!(bridge_list_skills(vault.clone())[0].body, "My own steps.");

        bridge_install_catalog_skill(vault.clone(), name, true).unwrap();
        assert_eq!(bridge_list_skill_catalog(vault)[0].status, "installed");
    }

    fn saved(vault: &str, name: &str) {
        bridge_save_skill(vault.to_string(), dto(name), false).unwrap();
    }

    #[test]
    fn an_attachment_round_trips_and_the_list_has_name_and_size_in_bytes() {
        let vault = temp_vault();
        saved(&vault, "x");
        bridge_save_skill_file(vault.clone(), "x".into(), "b.md".into(), "héllo".into()).unwrap();
        bridge_save_skill_file(vault.clone(), "x".into(), "a.md".into(), "hi".into()).unwrap();

        let files = bridge_list_skill_files(vault.clone(), "x".into()).unwrap();
        let listed: Vec<(&str, u32)> = files.iter().map(|f| (f.name.as_str(), f.size)).collect();
        assert_eq!(listed, vec![("a.md", 2), ("b.md", "héllo".len() as u32)], "sorted, sized in bytes");

        assert_eq!(bridge_read_skill_file(vault.clone(), "x".into(), "b.md".into()).unwrap(), "héllo");
        bridge_delete_skill_file(vault.clone(), "x".into(), "b.md".into()).unwrap();
        assert_eq!(bridge_list_skill_files(vault, "x".into()).unwrap().len(), 1);
    }

    #[test]
    fn editing_the_skill_keeps_its_attachments_and_deleting_it_takes_them() {
        let vault = temp_vault();
        saved(&vault, "x");
        bridge_save_skill_file(vault.clone(), "x".into(), "a.md".into(), "keep me".into()).unwrap();

        bridge_save_skill(vault.clone(), SkillDto { body: "new".into(), ..dto("x") }, true).unwrap();
        assert_eq!(bridge_read_skill_file(vault.clone(), "x".into(), "a.md".into()).unwrap(), "keep me");

        bridge_delete_skill(vault.clone(), "x".into()).unwrap();
        assert!(bridge_list_skill_files(vault, "x".into()).unwrap().is_empty());
    }

    #[test]
    fn hostile_names_are_refused_and_nothing_is_written() {
        let vault = temp_vault();
        saved(&vault, "x");
        let too_long = "a".repeat(warden_core::skill::MAX_FILE_NAME_LEN + 1);
        for bad in ["../x.md", "a/b.md", "a\\b.md", ".hidden", "..", "", "nul\0.md", "sp ace.md", "é.md", too_long.as_str()] {
            assert!(bridge_save_skill_file(vault.clone(), "x".into(), bad.into(), "c".into()).is_err(), "save {bad:?}");
            assert!(bridge_read_skill_file(vault.clone(), "x".into(), bad.into()).is_err(), "read {bad:?}");
            assert!(bridge_delete_skill_file(vault.clone(), "x".into(), bad.into()).is_err(), "delete {bad:?}");
        }
        assert!(bridge_list_skill_files(vault.clone(), "x".into()).unwrap().is_empty());
        for bad in ["../escape", "a/b", ".."] {
            assert!(bridge_save_skill_file(vault.clone(), bad.into(), "a.md".into(), "c".into()).is_err(), "skill {bad}");
            assert!(bridge_list_skill_files(vault.clone(), bad.into()).is_err(), "list skill {bad}");
        }
    }

    #[test]
    fn an_attachment_needs_an_existing_skill_and_an_existing_file() {
        let vault = temp_vault();
        assert!(bridge_save_skill_file(vault.clone(), "ghost".into(), "a.md".into(), "c".into()).is_err());
        assert!(bridge_list_skill_files(vault.clone(), "ghost".into()).unwrap().is_empty());
        saved(&vault, "x");
        assert!(bridge_read_skill_file(vault.clone(), "x".into(), "nope.md".into()).is_err());
        assert!(bridge_delete_skill_file(vault, "x".into(), "nope.md".into()).is_err());
    }

    #[test]
    fn the_size_and_count_limits_hold() {
        let vault = temp_vault();
        saved(&vault, "x");
        let at_limit = "a".repeat(warden_core::skill::MAX_FILE_BYTES);
        bridge_save_skill_file(vault.clone(), "x".into(), "big.md".into(), at_limit.clone()).unwrap();
        assert!(bridge_save_skill_file(vault.clone(), "x".into(), "bigger.md".into(), format!("{at_limit}a")).is_err());
        // Growing an existing one past the limit is refused and keeps the old text.
        assert!(bridge_save_skill_file(vault.clone(), "x".into(), "big.md".into(), format!("{at_limit}a")).is_err());
        assert_eq!(bridge_read_skill_file(vault.clone(), "x".into(), "big.md".into()).unwrap().len(), warden_core::skill::MAX_FILE_BYTES);

        for i in 1..warden_core::skill::MAX_FILES_PER_SKILL {
            bridge_save_skill_file(vault.clone(), "x".into(), format!("f{i}.md"), "c".into()).unwrap();
        }
        assert!(bridge_save_skill_file(vault.clone(), "x".into(), "one-too-many.md".into(), "c".into()).is_err());
        bridge_save_skill_file(vault, "x".into(), "f1.md".into(), "new".into()).unwrap();
    }

    #[test]
    fn the_sync_walk_carries_the_attachments_with_the_skill() {
        let vault = temp_vault();
        saved(&vault, "x");
        bridge_save_skill_file(vault.clone(), "x".into(), "a.md".into(), "c".into()).unwrap();
        let mut files: Vec<String> = Vault::new(&vault).list_all_files().unwrap().into_iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
        files.sort();
        assert_eq!(files, vec!["skills/x.files/a.md".to_string(), "skills/x.md".to_string()]);
    }

    #[test]
    fn installing_a_name_the_gallery_does_not_have_is_an_error() {
        assert!(bridge_install_catalog_skill(temp_vault(), "nope".into(), false).is_err());
    }

    #[test]
    fn invalid_skills_and_unknown_deletes_are_errors() {
        let vault = temp_vault();
        assert!(bridge_save_skill(vault.clone(), dto("../x"), false).is_err());
        assert!(bridge_save_skill(vault.clone(), SkillDto { body: "  ".into(), ..dto("ok") }, false).is_err());
        assert!(bridge_delete_skill(vault, "missing".into()).is_err());
    }
}

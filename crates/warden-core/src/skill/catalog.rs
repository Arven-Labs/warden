//! The gallery of ready-made skills (P140): a handful of skills that ship inside the binary, in the same file format as
//! the ones in the vault (`catalog/skills/<name>.md`), that a person installs with one click. Nothing is downloaded and
//! nothing is signed, because the text is part of the program they already trust; a catalog that comes from outside
//! (the signed `.wpkg` packages of `HOSPEDAGEM.md` section E) is a later step.
//!
//! There is no record of what was installed. An entry's state comes from comparing it with the vault: not there, there
//! and identical, or there but different (the person edited it, or this version of the program carries a newer text).

use std::sync::OnceLock;

use anyhow::{anyhow, bail};

use super::{Skill, SkillStore};

/// The files of `catalog/skills/`, named after the skill they hold.
const FILES: &[(&str, &str)] = &[
    ("code-review-checklist", include_str!("../../catalog/skills/code-review-checklist.md")),
    ("commit-message", include_str!("../../catalog/skills/commit-message.md")),
    ("decision-record", include_str!("../../catalog/skills/decision-record.md")),
    ("email-draft", include_str!("../../catalog/skills/email-draft.md")),
    ("explain-simply", include_str!("../../catalog/skills/explain-simply.md")),
    ("meeting-summary", include_str!("../../catalog/skills/meeting-summary.md")),
    ("research-brief", include_str!("../../catalog/skills/research-brief.md")),
    ("weekly-summary", include_str!("../../catalog/skills/weekly-summary.md")),
];

/// Where an entry of the gallery stands in a vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogStatus {
    /// No skill of that name in the vault.
    NotInstalled,
    /// The vault has this skill, with the same description and text.
    Installed,
    /// The vault has a skill of that name that differs: installing again replaces it.
    Modified,
}

impl CatalogStatus {
    /// The word the wire uses for it.
    pub fn as_str(self) -> &'static str {
        match self {
            CatalogStatus::NotInstalled => "notInstalled",
            CatalogStatus::Installed => "installed",
            CatalogStatus::Modified => "modified",
        }
    }
}

/// Every skill of the gallery, sorted by name. Parsed once.
pub fn entries() -> &'static [Skill] {
    static ENTRIES: OnceLock<Vec<Skill>> = OnceLock::new();
    ENTRIES.get_or_init(|| FILES.iter().map(|(name, raw)| Skill::parse(name, raw)).collect())
}

/// The state of `entry` in the vault `store` reads. A suggestion the assistant made under the same name is not the skill, so
/// it counts as modified: installing the entry replaces it.
pub fn status(store: &SkillStore, entry: &Skill) -> CatalogStatus {
    match store.get(&entry.name) {
        Err(_) => CatalogStatus::NotInstalled,
        Ok(found) if !found.proposed && same_text(&found, entry) => CatalogStatus::Installed,
        Ok(_) => CatalogStatus::Modified,
    }
}

fn same_text(a: &Skill, b: &Skill) -> bool {
    let words = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    words(&a.description) == words(&b.description) && a.body.trim() == b.body.trim()
}

/// Installs the entry called `name`. A skill of that name that differs is only replaced with `overwrite`, so a click on
/// "Install" never throws away what a person wrote; the one that is already identical is left alone.
pub fn install(store: &SkillStore, name: &str, overwrite: bool) -> anyhow::Result<()> {
    let entry = entries().iter().find(|e| e.name == name).ok_or_else(|| anyhow!("the gallery has no skill named '{name}'"))?;
    install_skill(store, entry, overwrite)
}

/// Puts `skill` in the vault the way the gallery does: one that is already identical is left alone, one that differs is only
/// replaced with `overwrite`, and the agents a person restricted the existing skill to are kept. Shared with skill packages.
pub(super) fn install_skill(store: &SkillStore, skill: &Skill, overwrite: bool) -> anyhow::Result<()> {
    match status(store, skill) {
        CatalogStatus::Installed => Ok(()),
        CatalogStatus::Modified if !overwrite => {
            bail!("a skill named '{}' is already in your skills and differs from this one; installing replaces it", skill.name)
        }
        CatalogStatus::NotInstalled | CatalogStatus::Modified => {
            let agents = store.get(&skill.name).map(|s| s.agents).unwrap_or_default();
            store.save(&Skill { agents, proposed: false, source: None, proposed_at: None, revises: None, ..skill.clone() })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::memory::Vault;

    fn temp_store() -> SkillStore {
        let dir = std::env::temp_dir().join(format!(
            "warden-catalog-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        SkillStore::new(Arc::new(Vault::new(dir)))
    }

    #[test]
    fn every_entry_is_a_valid_skill_with_a_unique_name_that_matches_its_file() {
        assert!(!entries().is_empty());
        let mut seen = Vec::new();
        for ((file_name, _), entry) in FILES.iter().zip(entries()) {
            entry.validate().unwrap_or_else(|e| panic!("catalog skill '{file_name}' is invalid: {e:#}"));
            assert_eq!(&entry.name, file_name);
            assert!(!seen.contains(&entry.name), "'{}' is in the gallery twice", entry.name);
            seen.push(entry.name.clone());
            assert!(entry.agents.is_empty() && !entry.proposed, "'{}' must be global and not a suggestion", entry.name);
        }
    }

    #[test]
    fn the_gallery_is_only_text_with_no_links_and_no_commands() {
        for entry in entries() {
            let text = format!("{}\n{}", entry.description, entry.body).to_lowercase();
            for forbidden in ["http://", "https://", "www.", "```", "curl ", "sudo ", "rm -rf", "| sh", "api_key", "password:"] {
                assert!(!text.contains(forbidden), "'{}' contains '{forbidden}'", entry.name);
            }
        }
    }

    #[test]
    fn an_entry_goes_from_not_installed_to_installed_to_modified() {
        let store = temp_store();
        let entry = &entries()[0];
        assert_eq!(status(&store, entry), CatalogStatus::NotInstalled);

        install(&store, &entry.name, false).unwrap();
        assert_eq!(status(&store, entry), CatalogStatus::Installed);
        let saved = store.get(&entry.name).unwrap();
        assert_eq!((saved.description.as_str(), saved.body.as_str()), (entry.description.as_str(), entry.body.as_str()));

        store.save(&Skill { body: format!("{}\nMy own rule.", entry.body), ..saved }).unwrap();
        assert_eq!(status(&store, entry), CatalogStatus::Modified);
    }

    #[test]
    fn installing_never_replaces_a_changed_skill_without_overwrite_and_keeps_its_agents() {
        let store = temp_store();
        let entry = &entries()[0];
        install(&store, &entry.name, false).unwrap();
        let mine = Skill { body: "My own text.".into(), agents: vec!["writer".into()], ..store.get(&entry.name).unwrap() };
        store.save(&mine).unwrap();

        let err = install(&store, &entry.name, false).unwrap_err().to_string();
        assert!(err.contains("differs"), "{err}");
        assert_eq!(store.get(&entry.name).unwrap().body, "My own text.");

        install(&store, &entry.name, true).unwrap();
        let after = store.get(&entry.name).unwrap();
        assert_eq!(after.body, entry.body);
        assert_eq!(after.agents, vec!["writer".to_string()]);
        assert_eq!(status(&store, entry), CatalogStatus::Installed);
    }

    #[test]
    fn an_unknown_name_is_an_error() {
        let err = install(&temp_store(), "no-such-skill", true).unwrap_err().to_string();
        assert!(err.contains("no skill named 'no-such-skill'"), "{err}");
    }

    #[test]
    fn a_suggestion_with_the_same_name_counts_as_modified_and_is_replaced_by_an_install() {
        let store = temp_store();
        let entry = &entries()[0];
        store.save(&Skill { proposed: true, source: Some("c1".into()), proposed_at: Some(1), ..entry.clone() }).unwrap();
        assert_eq!(status(&store, entry), CatalogStatus::Modified);

        install(&store, &entry.name, true).unwrap();
        assert!(!store.get(&entry.name).unwrap().proposed);
        assert_eq!(status(&store, entry), CatalogStatus::Installed);
    }
}

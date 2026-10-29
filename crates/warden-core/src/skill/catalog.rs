//! The gallery of ready-made skills (P140): a handful of skills that ship inside the binary, in the same file format as
//! the ones in the vault (`catalog/skills/<name>.md`), that a person installs with one click. Nothing is downloaded and
//! nothing is signed, because the text is part of the program they already trust; a catalog that comes from outside
//! (the signed `.wpkg` packages of `HOSPEDAGEM.md` section E) is a later step.
//!
//! An entry's state comes from comparing it with the vault: not there, there and identical, or there but different. "Different"
//! is two things: the person edited it, or this version of the program carries a newer text and the skill is untouched. To tell
//! them apart, installing from the gallery writes down a hash of the text it installed in `skills/.catalog.json`; a skill that
//! still has exactly that text and differs from today's entry is `Outdated`, and updating it loses nothing. The record is a
//! dotfile, so it is not a skill and the sync skips it: on another device, or for a skill installed before it existed, there is no
//! record and a different skill stays `Modified`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::OnceLock;

use anyhow::{anyhow, bail};
use sha2::{Digest, Sha256};

use super::{origin, Skill, SkillStore};
use crate::memory::SKILLS_DIR;

/// What the gallery installed, by skill name: the hash of the text, see `text_hash`.
const RECORD_FILE: &str = ".catalog.json";

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
    /// The vault has the text the gallery installed earlier, untouched, and this version of the program has a newer one:
    /// installing again updates it without asking, because nothing of the person's is in it.
    Outdated,
}

impl CatalogStatus {
    /// The word the wire uses for it.
    pub fn as_str(self) -> &'static str {
        match self {
            CatalogStatus::NotInstalled => "notInstalled",
            CatalogStatus::Installed => "installed",
            CatalogStatus::Modified => "modified",
            CatalogStatus::Outdated => "outdated",
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
        Ok(found) if found.proposed => CatalogStatus::Modified,
        Ok(found) if same_text(&found, entry) => CatalogStatus::Installed,
        Ok(found) if read_record(store).get(&entry.name).is_some_and(|installed| *installed == text_hash(&found)) => CatalogStatus::Outdated,
        Ok(_) => CatalogStatus::Modified,
    }
}

fn same_text(a: &Skill, b: &Skill) -> bool {
    words(&a.description) == words(&b.description) && a.body.trim() == b.body.trim()
}

fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `sha256:<hex>` of what `same_text` compares: the description with its whitespace collapsed (so it has no newline), a newline,
/// and the trimmed body. Two skills are `same_text` exactly when their hashes are equal.
fn text_hash(skill: &Skill) -> String {
    let digest = Sha256::digest(format!("{}\n{}", words(&skill.description), skill.body.trim()).as_bytes());
    let mut hex = String::with_capacity(7 + digest.len() * 2);
    hex.push_str("sha256:");
    for byte in digest {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

fn record_path() -> String {
    format!("{SKILLS_DIR}/{RECORD_FILE}")
}

/// What the gallery installed here. A missing or unreadable record is an empty one: nothing is `Outdated` without it.
fn read_record(store: &SkillStore) -> BTreeMap<String, String> {
    store.vault.read(&record_path()).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
}

/// Writes down that `entry` is what the vault holds now. Best effort: the skill is already installed, and without the record the
/// worst that happens is that a later update is offered as "different" instead of "newer".
fn write_record(store: &SkillStore, entry: &Skill) {
    let mut record = read_record(store);
    record.insert(entry.name.clone(), text_hash(entry));
    if let Ok(json) = serde_json::to_string_pretty(&record) {
        let _ = store.vault.write(&record_path(), &json);
    }
}

/// Installs the entry called `name`. A skill of that name that differs is only replaced with `overwrite`, so a click on
/// "Install" never throws away what a person wrote; the one that is already identical is left alone, and an older copy of this
/// same entry that nobody touched is updated.
pub fn install(store: &SkillStore, name: &str, overwrite: bool) -> anyhow::Result<()> {
    let entry = entries().iter().find(|e| e.name == name).ok_or_else(|| anyhow!("the gallery has no skill named '{name}'"))?;
    install_entry(store, entry, overwrite)
}

fn install_entry(store: &SkillStore, entry: &Skill, overwrite: bool) -> anyhow::Result<()> {
    // Only here, for the gallery's own entries: a package from outside goes through `install_skill` and always needs `overwrite`.
    let overwrite = overwrite || status(store, entry) == CatalogStatus::Outdated;
    install_skill(store, entry, overwrite)?;
    write_record(store, entry);
    Ok(())
}

/// Puts `skill` in the vault the way the gallery does: one that is already identical is left alone, one that differs is only
/// replaced with `overwrite`, and the agents a person restricted the existing skill to are kept. Shared with skill packages.
pub(super) fn install_skill(store: &SkillStore, skill: &Skill, overwrite: bool) -> anyhow::Result<()> {
    match status(store, skill) {
        CatalogStatus::Installed => Ok(()),
        CatalogStatus::Modified | CatalogStatus::Outdated if !overwrite => {
            bail!("a skill named '{}' is already in your skills and differs from this one; installing replaces it", skill.name)
        }
        CatalogStatus::NotInstalled | CatalogStatus::Modified | CatalogStatus::Outdated => {
            let agents = store.get(&skill.name).map(|s| s.agents).unwrap_or_default();
            store.save(&Skill { agents, proposed: false, source: None, proposed_at: None, revises: None, ..skill.clone() })?;
            // The text now comes from here, so a curator it was installed from is no longer where it came from. A package install
            // writes its own origin after this returns.
            origin::forget(store, &skill.name);
            Ok(())
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

    /// The same entry with a newer text, as a later version of the program would carry it.
    fn newer(entry: &Skill) -> Skill {
        Skill { body: format!("{}\nA step added in a later version.", entry.body), ..entry.clone() }
    }

    #[test]
    fn an_untouched_older_copy_is_outdated_and_updates_without_asking() {
        let store = temp_store();
        let entry = &entries()[0];
        install(&store, &entry.name, false).unwrap();
        let later = newer(entry);

        assert_eq!(status(&store, entry), CatalogStatus::Installed);
        assert_eq!(status(&store, &later), CatalogStatus::Outdated);

        // No `overwrite`: nothing of the person's is in it. The agents it was restricted to are kept.
        store.save(&Skill { agents: vec!["writer".into()], ..store.get(&entry.name).unwrap() }).unwrap();
        install_entry(&store, &later, false).unwrap();
        let after = store.get(&entry.name).unwrap();
        assert_eq!(after.body, later.body);
        assert_eq!(after.agents, vec!["writer".to_string()]);
        assert_eq!(status(&store, &later), CatalogStatus::Installed);
        // The record follows: an even newer text is outdated again, and the old one is no longer what is installed.
        assert_eq!(status(&store, &newer(&later)), CatalogStatus::Outdated);
    }

    #[test]
    fn an_edited_copy_is_modified_even_when_the_gallery_has_a_newer_text() {
        let store = temp_store();
        let entry = &entries()[0];
        install(&store, &entry.name, false).unwrap();
        store.save(&Skill { body: format!("{}\nMy own rule.", entry.body), ..store.get(&entry.name).unwrap() }).unwrap();
        let later = newer(entry);

        assert_eq!(status(&store, &later), CatalogStatus::Modified);
        assert!(install_entry(&store, &later, false).unwrap_err().to_string().contains("differs"));
        assert!(store.get(&entry.name).unwrap().body.contains("My own rule."));
        install_entry(&store, &later, true).unwrap();
        assert_eq!(status(&store, &later), CatalogStatus::Installed);
    }

    #[test]
    fn without_a_record_a_different_skill_stays_modified() {
        let store = temp_store();
        let entry = &entries()[0];
        // Put the older text in the vault the way a skill installed before the record existed is: no record of it.
        store.save(entry).unwrap();
        assert!(read_record(&store).is_empty());
        assert_eq!(status(&store, &newer(entry)), CatalogStatus::Modified);

        // A record that is unreadable counts as no record.
        install(&store, &entry.name, false).unwrap();
        store.vault.write(&record_path(), "not json").unwrap();
        assert_eq!(status(&store, &newer(entry)), CatalogStatus::Modified);
    }

    #[test]
    fn a_suggestion_under_the_same_name_is_not_an_untouched_copy() {
        let store = temp_store();
        let entry = &entries()[0];
        install(&store, &entry.name, false).unwrap();
        // The assistant proposes the very text that was installed: still a suggestion, so still replaced only on request.
        store.save(&Skill { proposed: true, source: Some("c1".into()), proposed_at: Some(1), ..store.get(&entry.name).unwrap() }).unwrap();
        assert_eq!(status(&store, &newer(entry)), CatalogStatus::Modified);
    }

    #[test]
    fn the_record_is_not_a_skill_and_the_hash_follows_the_comparison() {
        let store = temp_store();
        let entry = &entries()[0];
        install(&store, &entry.name, false).unwrap();
        assert!(store.vault.is_file(&record_path()));
        assert_eq!(store.list().iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), [entry.name.as_str()]);

        // `same_text` ignores the whitespace of the description and around the body; so does the hash.
        let spaced = Skill { description: format!("  {}  ", entry.description.replace(' ', "  ")), body: format!("\n{}\n\n", entry.body), ..entry.clone() };
        assert!(same_text(entry, &spaced));
        assert_eq!(text_hash(entry), text_hash(&spaced));
        assert_ne!(text_hash(entry), text_hash(&newer(entry)));
        assert!(text_hash(entry).starts_with("sha256:") && text_hash(entry).len() == 7 + 64);
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

    #[test]
    fn replacing_a_skill_forgets_where_it_came_from_but_leaving_it_alone_keeps_the_origin() {
        use super::super::origin::{self, Origin};

        let store = temp_store();
        let entry = &entries()[0];
        let from_a_curator = || Origin { key: "aa".repeat(32), version: "1.0.0".into(), hash: "sha256:aa".into() };

        // Identical text: nothing is written, so the origin stays.
        install(&store, &entry.name, false).unwrap();
        origin::record(&store, &entry.name, from_a_curator());
        install(&store, &entry.name, false).unwrap();
        assert_eq!(origin::read(&store, &entry.name), Some(from_a_curator()));

        // Refused for lack of `overwrite`: nothing is written either.
        store.save(&Skill { body: "My own text.".into(), ..store.get(&entry.name).unwrap() }).unwrap();
        install(&store, &entry.name, false).unwrap_err();
        assert_eq!(origin::read(&store, &entry.name), Some(from_a_curator()));

        // Replaced: the text now comes from the gallery, so the curator is no longer where it came from.
        install(&store, &entry.name, true).unwrap();
        assert_eq!(origin::read(&store, &entry.name), None);
    }
}

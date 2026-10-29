//! Where an installed skill came from (P160): the key that signed the package it was installed from, and that package's version and
//! hash. It is what lets a curator's index say "there is a newer one" for a skill the person already has, by matching the skill with
//! the index entries signed by the same key. Only a signed package leaves an origin: without a key there is no curator to ask.
//!
//! The record is a dotfile in the skills folder like `catalog.rs`'s, so it is not a skill and the sync skips it: on another device,
//! or for a skill installed before it existed, there is no origin and nothing is offered. It is separate from `.catalog.json` on
//! purpose: that one makes an untouched gallery skill `Outdated`, which a package never is (`Package::skill_status`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::SkillStore;
use crate::memory::SKILLS_DIR;

const RECORD_FILE: &str = ".origins.json";

/// The package a skill was installed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// The key that signed the package (64 hex characters).
    pub key: String,
    pub version: String,
    /// The package's own `hash` (`sha256:<hex>`).
    pub hash: String,
}

fn record_path() -> String {
    format!("{SKILLS_DIR}/{RECORD_FILE}")
}

/// The record, by skill name. A missing or unreadable file is an empty record: nothing is offered without it.
fn read_all(store: &SkillStore) -> BTreeMap<String, Origin> {
    store.vault.read(&record_path()).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
}

/// Best effort: the skill is already installed, and without the record the worst that happens is that no update is offered for it.
fn write_all(store: &SkillStore, all: &BTreeMap<String, Origin>) {
    if let Ok(json) = serde_json::to_string_pretty(all) {
        let _ = store.vault.write(&record_path(), &json);
    }
}

/// The origin of the skill called `name`, if it has one.
pub fn read(store: &SkillStore, name: &str) -> Option<Origin> {
    read_all(store).remove(name)
}

/// Writes down that the skill called `name` came from `origin`.
pub fn record(store: &SkillStore, name: &str, origin: Origin) {
    let mut all = read_all(store);
    all.insert(name.to_string(), origin);
    write_all(store, &all);
}

/// Forgets the origin of `name`, because its text just changed from somewhere else. Writes nothing when there was none.
pub fn forget(store: &SkillStore, name: &str) {
    let mut all = read_all(store);
    if all.remove(name).is_some() {
        write_all(store, &all);
    }
}

/// Whether the vault has the skill `id` from the curator whose index is signed by `key`, and the entry listed now (`hash`) is not the
/// package it was installed from. It does not say the entry is newer: the hash differs, and a curator that republishes an older
/// version shows up too. It is also true for a skill the person edited since: the preview shows what the update changes, and
/// replacing still asks.
pub fn has_update(store: &SkillStore, key: &str, id: &str, hash: &str) -> bool {
    // A suggestion the assistant made under the same name is not the skill (see `catalog::status`).
    let installed = store.get(id).is_ok_and(|skill| !skill.proposed);
    installed && read(store, id).is_some_and(|origin| origin.key == key && origin.hash != hash)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::memory::Vault;
    use crate::skill::Skill;

    const KEY: &str = "aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11";
    const OTHER_KEY: &str = "bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22";

    fn temp_store() -> SkillStore {
        let dir = std::env::temp_dir().join(format!(
            "warden-origin-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        SkillStore::new(Arc::new(Vault::new(dir)))
    }

    fn skill(name: &str) -> Skill {
        Skill { name: name.into(), description: "Does a thing.".into(), body: "Do it.".into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None }
    }

    fn origin(key: &str, hash: &str) -> Origin {
        Origin { key: key.into(), version: "1.0.0".into(), hash: hash.into() }
    }

    #[test]
    fn an_origin_is_written_down_and_read_back_by_skill_name() {
        let store = temp_store();
        assert_eq!(read(&store, "greet"), None);
        record(&store, "greet", origin(KEY, "sha256:aa"));
        record(&store, "other", origin(OTHER_KEY, "sha256:bb"));
        assert_eq!(read(&store, "greet"), Some(origin(KEY, "sha256:aa")));
        assert_eq!(read(&store, "other"), Some(origin(OTHER_KEY, "sha256:bb")));
        record(&store, "greet", origin(KEY, "sha256:cc"));
        assert_eq!(read(&store, "greet"), Some(origin(KEY, "sha256:cc")));
    }

    #[test]
    fn forgetting_drops_only_that_skill_and_writes_nothing_when_there_was_no_origin() {
        let store = temp_store();
        forget(&store, "greet");
        assert!(store.vault.read(&record_path()).is_err(), "forgetting nothing must not create the file");

        record(&store, "greet", origin(KEY, "sha256:aa"));
        record(&store, "other", origin(KEY, "sha256:bb"));
        forget(&store, "greet");
        assert_eq!(read(&store, "greet"), None);
        assert_eq!(read(&store, "other"), Some(origin(KEY, "sha256:bb")));
    }

    #[test]
    fn a_damaged_record_reads_as_empty_and_the_next_origin_replaces_it() {
        let store = temp_store();
        store.vault.write(&record_path(), "{ not json").unwrap();
        assert_eq!(read(&store, "greet"), None);
        record(&store, "greet", origin(KEY, "sha256:aa"));
        assert_eq!(read(&store, "greet"), Some(origin(KEY, "sha256:aa")));
    }

    #[test]
    fn an_update_is_offered_only_for_the_same_key_a_different_hash_and_a_skill_that_is_still_there() {
        let store = temp_store();
        store.save(&skill("greet")).unwrap();
        record(&store, "greet", origin(KEY, "sha256:aa"));

        assert!(has_update(&store, KEY, "greet", "sha256:new"), "same key, different hash");
        assert!(!has_update(&store, KEY, "greet", "sha256:aa"), "the entry is what is installed");
        assert!(!has_update(&store, OTHER_KEY, "greet", "sha256:new"), "another curator's entry of the same name is not an update");
        assert!(!has_update(&store, KEY, "unknown", "sha256:new"), "no origin recorded for that name");
    }

    #[test]
    fn no_update_is_offered_for_a_skill_that_was_deleted_or_replaced_by_a_suggestion() {
        let store = temp_store();
        record(&store, "greet", origin(KEY, "sha256:aa"));
        assert!(!has_update(&store, KEY, "greet", "sha256:new"), "the skill is not in the vault");

        store.save(&Skill { proposed: true, source: Some("c1".into()), proposed_at: Some(1), ..skill("greet") }).unwrap();
        assert!(!has_update(&store, KEY, "greet", "sha256:new"), "a suggestion is not the installed skill");
    }
}

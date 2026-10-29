//! Curators over the wire (P160): the list the owner keeps (`[[curators]]`), a curator's signed index read from where it is
//! published, and the package one of its entries points to. The list is the owner's to change (`settings::handle_save_curators`, with
//! the pairing key); reading it and fetching through it is for anyone signed in, because what a person installs goes into their own
//! vault, through the same preview as any package.
//!
//! Fetching goes through `HttpsFetcher`, which refuses addresses that only mean something on this machine or this network unless the
//! hub was started with `--allow-private-curators`. The newest `sequence` seen from each curator is kept in `curators_seen.json` beside
//! the config, so an older index someone replays is refused.

use std::path::PathBuf;

use warden_bootstrap::curator::{fetch_indexed_package, read_index_remembering, seen_path, usable_curators, ArweaveClient, Curator, HttpsFetcher};
use warden_bootstrap::{load_config_from_path, FileConfig};
use warden_core::skill::index::Index;
use warden_core::skill::origin;
use warden_core::skill::SkillStore;
use warden_server_protocol::protocol::{CuratorDto, CuratorEntryDto};
use warden_server_protocol::ServerMessage;

use crate::settings::SettingsHost;

/// The curators as they go on the wire: only the usable ones, the same that a fetch looks among.
pub fn curator_dtos(config: &FileConfig) -> Vec<CuratorDto> {
    usable_curators(&config.curators).into_iter().map(|c| CuratorDto { name: c.name, key: c.key, url: c.url }).collect()
}

/// Answers `ListCurators`: the list as the hub keeps it.
pub fn handle_list(host: Option<&dyn SettingsHost>, request_id: u64) -> ServerMessage {
    let Some(host) = host else {
        return ServerMessage::Curators { request_id, curators: Vec::new() };
    };
    match load_config_from_path(&host.config_path(), false) {
        Ok(config) => ServerMessage::Curators { request_id, curators: curator_dtos(&config) },
        Err(err) => ServerMessage::SkillError { request_id, message: format!("{err:#}") },
    }
}

/// The curator called `name` (any case) and the file that remembers their newest index.
fn find(host: Option<&dyn SettingsHost>, name: &str) -> Result<(Curator, PathBuf), String> {
    let host = host.ok_or("this hub has no settings, so no curators")?;
    let path = host.config_path();
    let config = load_config_from_path(&path, false).map_err(|e| format!("{e:#}"))?;
    let curator = usable_curators(&config.curators).into_iter().find(|c| c.name.eq_ignore_ascii_case(name.trim())).ok_or_else(|| format!("there is no curator called '{}'", name.trim()))?;
    Ok((curator, seen_path(&path)))
}

/// Reads `curator`'s index and remembers its number, so an older one is refused the next time.
async fn read_index(host: Option<&dyn SettingsHost>, fetcher: &HttpsFetcher, name: &str) -> Result<(Curator, Index), String> {
    let (curator, seen_file) = find(host, name)?;
    let index = read_index_remembering(fetcher, &ArweaveClient::new_default(), &curator, &seen_file).await?;
    Ok((curator, index))
}

/// The entries of `index` as they go on the wire, each with whether the curator took it back and why, and whether it is an update of
/// a skill the person in `store` installed from this same curator (`origin::has_update`, matched by the key that signed the index).
/// An entry the curator took back is never offered as an update: it cannot be fetched.
pub fn entry_dtos(index: &Index, store: &SkillStore) -> Vec<CuratorEntryDto> {
    // `read_index` only hands out an index whose signature checks out, so this does not fail there; without a key nothing matches.
    let curator_key = index.signer().ok();
    index
        .entries
        .iter()
        .map(|e| {
            let revoked = index.is_revoked(&e.hash);
            CuratorEntryDto {
                id: e.id.clone(),
                version: e.version.clone(),
                hash: e.hash.clone(),
                source: e.source.clone(),
                description: e.description.clone(),
                author: e.author.clone(),
                license: e.license.clone(),
                revoked,
                revoked_reason: index.revoked_reason(&e.hash).map(str::to_string),
                update: !revoked && curator_key.as_deref().is_some_and(|key| origin::has_update(store, key, &e.id, &e.hash)),
            }
        })
        .collect()
}

/// Answers `FetchCuratorIndex`. `store` is the vault of the person who asked, to tell which entries update a skill they have.
pub async fn handle_fetch_index(host: Option<&dyn SettingsHost>, fetcher: &HttpsFetcher, request_id: u64, curator: &str, store: &SkillStore) -> ServerMessage {
    match read_index(host, fetcher, curator).await {
        Ok((curator, index)) => {
            let entries = entry_dtos(&index, store);
            ServerMessage::CuratorIndex { request_id, curator: curator.name, name: index.name.clone(), sequence: index.sequence, entries }
        }
        Err(message) => ServerMessage::SkillError { request_id, message },
    }
}

/// Answers `FetchCuratorPackage`: the index is read again, so a package the curator took back a moment ago is refused.
pub async fn handle_fetch_package(host: Option<&dyn SettingsHost>, fetcher: &HttpsFetcher, request_id: u64, curator: &str, hash: &str) -> ServerMessage {
    let result = async {
        let (_, index) = read_index(host, fetcher, curator).await?;
        let entry = index.entry_by_hash(hash).ok_or("that package is not in the curator's index")?;
        fetch_indexed_package(fetcher, &ArweaveClient::new_default(), &index, entry).await
    }
    .await;
    match result {
        Ok(content) => ServerMessage::SkillPackageFile { request_id, content, attachments_skipped: false },
        Err(message) => ServerMessage::SkillError { request_id, message },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use warden_core::memory::Vault;
    use warden_core::skill::index::{IndexEntry, Revoked};
    use warden_core::skill::origin::{self, Origin};
    use warden_core::skill::signing::generate_keypair;
    use warden_core::skill::{Skill, SkillStore};

    use super::*;

    fn temp_store() -> SkillStore {
        let dir = std::env::temp_dir().join(format!(
            "warden-server-curators-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        SkillStore::new(Arc::new(Vault::new(dir)))
    }

    fn skill(name: &str) -> Skill {
        Skill { name: name.into(), description: "Does a thing.".into(), body: "Do it.".into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None }
    }

    fn hash(digit: char) -> String {
        format!("sha256:{}", digit.to_string().repeat(64))
    }

    fn entry(id: &str, digit: char) -> IndexEntry {
        IndexEntry { id: id.into(), version: "1.0.0".into(), hash: hash(digit), source: format!("https://example.org/{id}.json"), description: "Does a thing.".into(), author: None, license: "MIT".into() }
    }

    fn origin_of(key: &str, digit: char) -> Origin {
        Origin { key: key.into(), version: "1.0.0".into(), hash: hash(digit) }
    }

    #[test]
    fn an_entry_is_marked_for_update_only_when_the_person_has_that_skill_from_the_same_curator_and_the_hash_changed() {
        let (pkcs8, curator_key) = generate_keypair().unwrap();
        let store = temp_store();
        for name in ["changed", "same", "other-curator", "also-changed", "never-installed", "no-origin"] {
            store.save(&skill(name)).unwrap();
        }
        origin::record(&store, "changed", origin_of(&curator_key, '1'));
        origin::record(&store, "same", origin_of(&curator_key, '2'));
        origin::record(&store, "other-curator", origin_of(&"ab".repeat(32), '1'));
        origin::record(&store, "also-changed", origin_of(&curator_key, '1'));
        origin::record(&store, "gone", origin_of(&curator_key, '1'));

        // Each entry has a hash of its own (an index refuses two entries with the same one).
        let entries = vec![entry("changed", '3'), entry("same", '2'), entry("other-curator", '4'), entry("also-changed", '5'), entry("never-installed", '6'), entry("no-origin", '7'), entry("gone", '8')];
        let plain = Index::build("Ana's picks", 1, entries.clone(), Vec::new(), &pkcs8).unwrap();
        // The same index, but the curator took back the two packages that would have been updates.
        let taken_back = vec![Revoked { hash: hash('3'), reason: None }, Revoked { hash: hash('5'), reason: Some("leaked".into()) }];
        let index = Index::build("Ana's picks", 2, entries, taken_back, &pkcs8).unwrap();

        let marked = |dtos: &[CuratorEntryDto]| dtos.iter().filter(|e| e.update).map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(marked(&entry_dtos(&plain, &store)), vec!["changed", "also-changed"], "same key and a different hash, skill still there; not the unchanged, the other curator's, the absent or the one with no origin");
        assert_eq!(marked(&entry_dtos(&index, &store)), Vec::<String>::new(), "an entry the curator took back is never offered as an update");
    }
}

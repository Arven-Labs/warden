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

/// The entries of `index` as they go on the wire, each with whether the curator took it back and why.
pub fn entry_dtos(index: &Index) -> Vec<CuratorEntryDto> {
    index
        .entries
        .iter()
        .map(|e| CuratorEntryDto {
            id: e.id.clone(),
            version: e.version.clone(),
            hash: e.hash.clone(),
            source: e.source.clone(),
            description: e.description.clone(),
            author: e.author.clone(),
            license: e.license.clone(),
            revoked: index.is_revoked(&e.hash),
            revoked_reason: index.revoked_reason(&e.hash).map(str::to_string),
        })
        .collect()
}

/// Answers `FetchCuratorIndex`.
pub async fn handle_fetch_index(host: Option<&dyn SettingsHost>, fetcher: &HttpsFetcher, request_id: u64, curator: &str) -> ServerMessage {
    match read_index(host, fetcher, curator).await {
        Ok((curator, index)) => {
            let entries = entry_dtos(&index);
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

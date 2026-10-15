//! P131 (S0): provider API keys kept out of `config.toml`.
//!
//! `config.toml` is the file people copy, sync and paste into issues, so the keys of
//! `[[providers]]` live in a sibling file (`config.secrets.toml`) that only the owner's account
//! reads (mode 0600 on unix; elsewhere the permissions are the platform default). On load the keys
//! are merged back into each `ProviderConfig`; on save they are split out again. A key still sitting
//! in `config.toml` (an old file, or one edited by hand) wins over the secrets file, so nothing is
//! lost, and the first save moves it across.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::FileConfig;

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SecretsFile {
    #[serde(default)]
    providers: BTreeMap<String, ProviderSecret>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProviderSecret {
    api_key: String,
}

// Never print a key, even by accident in a `{:?}`.
impl std::fmt::Debug for ProviderSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProviderSecret { api_key: <hidden> }")
    }
}

/// The secrets file that belongs to the config at `config_path` (`config.toml` -> `config.secrets.toml`).
/// Named after the config so two configs in one folder (tests, a second profile) never share keys.
pub fn secrets_path(config_path: &Path) -> PathBuf {
    config_path.with_extension("secrets.toml")
}

fn read_secrets(path: &Path) -> anyhow::Result<SecretsFile> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("failed to parse secrets file at {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SecretsFile::default()),
        Err(err) => Err(err).with_context(|| format!("failed to read secrets file at {}", path.display())),
    }
}

/// Fills the `api_key` of every provider that has none in `config.toml` from the secrets file next to it.
pub(crate) fn merge_into(config: &mut FileConfig, config_path: &Path) -> anyhow::Result<()> {
    if config.providers.iter().all(|p| p.api_key.is_some()) {
        return Ok(());
    }
    let secrets = read_secrets(&secrets_path(config_path))?;
    for provider in config.providers.iter_mut().filter(|p| p.api_key.is_none()) {
        if let Some(secret) = secrets.providers.get(&provider.id) {
            provider.api_key = Some(secret.api_key.clone());
        }
    }
    Ok(())
}

/// A copy of `config` with every provider key taken out, and the keys that were in it. An empty key is
/// not a key. Providers that no longer exist drop out of the secrets file as well, because it is
/// rewritten whole from `config` each time.
fn split(config: &FileConfig) -> (FileConfig, SecretsFile) {
    let mut stripped = config.clone();
    let mut secrets = SecretsFile::default();
    for provider in &mut stripped.providers {
        if let Some(key) = provider.api_key.take().filter(|k| !k.is_empty()) {
            secrets.providers.insert(provider.id.clone(), ProviderSecret { api_key: key });
        }
    }
    (stripped, secrets)
}

/// True when the `config.toml` text at hand still holds a provider key in plain text (needs the move).
fn holds_plain_keys(existing: &str) -> bool {
    let Ok(table) = existing.parse::<toml::Table>() else { return false };
    table
        .get("providers")
        .and_then(|v| v.as_array())
        .is_some_and(|list| list.iter().any(|p| p.get("api_key").and_then(|k| k.as_str()).is_some_and(|k| !k.is_empty())))
}

/// What `save_config` needs: the config without its provider keys, after the keys were written to the
/// secrets file. Order matters: the secrets are written (and read back) before `config.toml` loses
/// them, so a failure in between leaves the keys in both places and never in neither. The first time a
/// plain key is moved, the old `config.toml` is kept as `config.toml.bak` (mode 0600 too).
pub(crate) fn store_and_strip(config: &FileConfig, config_path: &Path, existing: Option<&str>) -> anyhow::Result<FileConfig> {
    let (stripped, secrets) = split(config);
    let path = secrets_path(config_path);
    // Nothing to keep and no file yet: don't leave an empty file lying around.
    if secrets.providers.is_empty() && !path.exists() {
        return Ok(stripped);
    }
    if let Some(old) = existing.filter(|text| holds_plain_keys(text)) {
        let backup = config_path.with_extension("toml.bak");
        if !backup.exists() {
            write_private(&backup, old.as_bytes())?;
        }
    }
    let text = toml::to_string_pretty(&secrets).context("failed to render secrets file")?;
    write_private(&path, text.as_bytes())?;
    let back = read_secrets(&path)?;
    anyhow::ensure!(
        back.providers.len() == secrets.providers.len() && back.providers.iter().all(|(id, s)| secrets.providers.get(id).is_some_and(|w| w.api_key == s.api_key)),
        "the secrets file at {} did not read back as written; config.toml was left as it was",
        path.display()
    );
    Ok(stripped)
}

/// Writes `bytes` to `path` through a temporary file and a rename, with mode 0600 from the first byte on
/// unix (so the content is never readable by others, not even for an instant).
pub fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp).with_context(|| format!("failed to write {}", tmp.display()))?;
    // An older temp file may have been left with looser permissions: set them again before writing.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
    file.sync_all().ok();
    drop(file);
    std::fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))
}

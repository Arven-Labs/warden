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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{load_config_from_path, save_config, Provider, ProviderConfig};

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("warden-secrets-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn provider(id: &str, key: Option<&str>) -> ProviderConfig {
        ProviderConfig { id: id.into(), kind: Provider::Openai, api_key: key.map(str::to_string), base_url: None, model: Some("m".into()), node: None }
    }

    fn config(providers: Vec<ProviderConfig>) -> FileConfig {
        FileConfig { providers, ..FileConfig::default() }
    }

    #[test]
    fn the_secrets_file_sits_next_to_its_config_and_is_named_after_it() {
        assert_eq!(secrets_path(Path::new("/x/warden/config.toml")), PathBuf::from("/x/warden/config.secrets.toml"));
        assert_ne!(secrets_path(Path::new("/x/a.toml")), secrets_path(Path::new("/x/b.toml")));
    }

    #[test]
    fn saving_moves_the_keys_out_of_config_toml_and_loading_puts_them_back() {
        let path = dir("split").join("config.toml");
        save_config(&path, &config(vec![provider("main", Some("sk-secret")), provider("empty", Some("")), provider("none", None)])).unwrap();

        let config_text = std::fs::read_to_string(&path).unwrap();
        assert!(!config_text.contains("sk-secret"), "{config_text}");
        let secrets_text = std::fs::read_to_string(secrets_path(&path)).unwrap();
        assert!(secrets_text.contains("sk-secret") && secrets_text.contains("main"), "{secrets_text}");
        assert!(!secrets_text.contains("empty") && !secrets_text.contains("none"), "an empty or missing key is not a secret: {secrets_text}");

        let back = load_config_from_path(&path, true).unwrap();
        assert_eq!(back.providers[0].api_key.as_deref(), Some("sk-secret"));
        assert_eq!(back.providers[2].api_key, None);
    }

    #[cfg(unix)]
    #[test]
    fn both_files_are_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let path = dir("mode").join("config.toml");
        save_config(&path, &config(vec![provider("main", Some("sk-secret"))])).unwrap();
        for file in [path.clone(), secrets_path(&path)] {
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600, "{}", file.display());
        }
    }

    #[test]
    fn a_key_still_in_config_toml_loads_and_wins_over_the_secrets_file() {
        let path = dir("legacy").join("config.toml");
        std::fs::write(&path, "[[providers]]\nid = \"main\"\nkind = \"openai\"\napi_key = \"sk-in-config\"\nmodel = \"m\"\n").unwrap();
        std::fs::write(secrets_path(&path), "[providers.main]\napi_key = \"sk-in-secrets\"\n").unwrap();
        assert_eq!(load_config_from_path(&path, true).unwrap().providers[0].api_key.as_deref(), Some("sk-in-config"));

        // Without one in the config, the secrets file fills it in.
        std::fs::write(&path, "[[providers]]\nid = \"main\"\nkind = \"openai\"\nmodel = \"m\"\n").unwrap();
        assert_eq!(load_config_from_path(&path, true).unwrap().providers[0].api_key.as_deref(), Some("sk-in-secrets"));
    }

    #[test]
    fn the_first_save_of_a_plain_key_keeps_one_private_backup_and_the_next_do_not_touch_it() {
        let path = dir("backup").join("config.toml");
        let old = "# my notes\n[[providers]]\nid = \"main\"\nkind = \"openai\"\napi_key = \"sk-plain\"\nmodel = \"m\"\n";
        std::fs::write(&path, old).unwrap();
        let loaded = load_config_from_path(&path, true).unwrap();
        save_config(&path, &loaded).unwrap();

        let backup = path.with_extension("toml.bak");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), old);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("sk-plain"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777, 0o600);
        }

        // Saving again, with no plain key left, leaves the backup as it was.
        save_config(&path, &loaded).unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), old);
    }

    #[test]
    fn saving_keeps_the_comments_of_config_toml() {
        let path = dir("comments").join("config.toml");
        std::fs::write(&path, "# my notes\n[[providers]]\n# the main one\nid = \"main\"\nkind = \"openai\"\nmodel = \"m\"\n").unwrap();
        let mut loaded = load_config_from_path(&path, true).unwrap();
        loaded.providers[0].api_key = Some("sk-new".into());
        save_config(&path, &loaded).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my notes") && text.contains("# the main one"), "{text}");
        assert!(!text.contains("sk-new"));
    }

    #[test]
    fn a_removed_provider_or_a_cleared_key_leaves_the_secrets_file() {
        let path = dir("removed").join("config.toml");
        save_config(&path, &config(vec![provider("a", Some("sk-a")), provider("b", Some("sk-b"))])).unwrap();
        save_config(&path, &config(vec![provider("a", None)])).unwrap();
        let secrets_text = std::fs::read_to_string(secrets_path(&path)).unwrap();
        assert!(!secrets_text.contains("sk-a") && !secrets_text.contains("sk-b"), "{secrets_text}");
        assert_eq!(load_config_from_path(&path, true).unwrap().providers[0].api_key, None);
    }

    #[test]
    fn a_config_with_no_keys_makes_no_secrets_file() {
        let path = dir("nokeys").join("config.toml");
        save_config(&path, &config(vec![provider("a", None)])).unwrap();
        assert!(!secrets_path(&path).exists());
        assert!(!path.with_extension("toml.bak").exists());
    }

    #[test]
    fn two_configs_in_one_folder_do_not_share_keys() {
        let folder = dir("two");
        let (a, b) = (folder.join("a.toml"), folder.join("b.toml"));
        save_config(&a, &config(vec![provider("main", Some("sk-a"))])).unwrap();
        save_config(&b, &config(vec![provider("main", None)])).unwrap();
        assert_eq!(load_config_from_path(&a, true).unwrap().providers[0].api_key.as_deref(), Some("sk-a"));
        assert_eq!(load_config_from_path(&b, true).unwrap().providers[0].api_key, None);
    }

    #[test]
    fn a_provider_prints_without_its_key() {
        let shown = format!("{:?}", provider("main", Some("sk-secret")));
        assert!(!shown.contains("sk-secret") && shown.contains("<hidden>"), "{shown}");
        assert!(format!("{:?}", provider("main", None)).contains("None"));
    }

    #[test]
    fn a_damaged_secrets_file_is_an_error_not_a_silent_loss() {
        let path = dir("damaged").join("config.toml");
        std::fs::write(&path, "[[providers]]\nid = \"main\"\nkind = \"openai\"\nmodel = \"m\"\n").unwrap();
        std::fs::write(secrets_path(&path), "this is not toml [").unwrap();
        assert!(load_config_from_path(&path, true).is_err());
    }
}

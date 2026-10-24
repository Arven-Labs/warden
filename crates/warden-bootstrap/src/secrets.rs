//! P131 (S0): the secrets kept out of `config.toml`.
//!
//! `config.toml` is the file people copy, sync and paste into issues, so what is secret in it lives in
//! a sibling file (`config.secrets.toml`) that only the owner's account reads (mode 0600 on unix;
//! elsewhere the permissions are the platform default). Three kinds of secret go there:
//! the keys of `[[providers]]`, the values of every `[[mcp_servers]]` entry's `env` (stdio) and
//! `headers` (http), and the five keys of `[api_keys]`. The structure stays in `config.toml` (an MCP
//! server's variable names stay, with an empty value), the value goes to the secrets file.
//! On load the values are merged back; on save they are split out again. A value still sitting in
//! `config.toml` (an old file, or one edited by hand) wins over the secrets file, so nothing is
//! lost, and the first save moves it across.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{FileConfig, McpServerConfig};

// `deny_unknown_fields` stays on: an older Warden that meets a section it does not know must refuse the file, not read it without the
// secrets and write it back without them.
#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct SecretsFile {
    #[serde(default)]
    providers: BTreeMap<String, ProviderSecret>,
    /// By the server's `name`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    mcp: BTreeMap<String, McpSecret>,
    #[serde(default, skip_serializing_if = "ApiKeysSecret::is_empty")]
    api_keys: ApiKeysSecret,
}

impl SecretsFile {
    fn is_empty(&self) -> bool {
        self.providers.is_empty() && self.mcp.is_empty() && self.api_keys.is_empty()
    }
}

#[derive(Deserialize, Serialize, PartialEq)]
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

/// The values of one MCP server's `env` (stdio) or `headers` (http), by the entry's name.
#[derive(Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct McpSecret {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    headers: BTreeMap<String, String>,
}

impl std::fmt::Debug for McpSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "McpSecret {{ env: {:?}, headers: {:?} }}", self.env.keys().collect::<Vec<_>>(), self.headers.keys().collect::<Vec<_>>())
    }
}

/// The five keys of `[api_keys]`.
#[derive(Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ApiKeysSecret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gemini: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    openai: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tavily: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    telegram_bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    whisper: Option<String>,
}

impl ApiKeysSecret {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

impl std::fmt::Debug for ApiKeysSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKeysSecret { <hidden> }")
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

/// Fills what `config.toml` leaves empty from the secrets file next to it: the `api_key` of every provider that has none, the empty
/// values of an MCP server's `env` or `headers` (only the names the config already lists: a name taken out of `config.toml` by hand is
/// not brought back), and the `[api_keys]` that are not set. Whatever `config.toml` still holds wins.
pub(crate) fn merge_into(config: &mut FileConfig, config_path: &Path) -> anyhow::Result<()> {
    let secrets = read_secrets(&secrets_path(config_path))?;
    for provider in config.providers.iter_mut().filter(|p| p.api_key.is_none()) {
        if let Some(secret) = secrets.providers.get(&provider.id) {
            provider.api_key = Some(secret.api_key.clone());
        }
    }
    for server in &mut config.mcp_servers {
        match server {
            McpServerConfig::Stdio { name, env, .. } => {
                if let Some(saved) = secrets.mcp.get(name.as_str()) {
                    fill_empty(env, &saved.env);
                }
            }
            McpServerConfig::Http { name, headers, .. } => {
                if let Some(saved) = secrets.mcp.get(name.as_str()) {
                    fill_empty(headers, &saved.headers);
                }
            }
        }
    }
    let keys = &mut config.api_keys;
    fill_none(&mut keys.gemini, &secrets.api_keys.gemini);
    fill_none(&mut keys.openai, &secrets.api_keys.openai);
    fill_none(&mut keys.tavily, &secrets.api_keys.tavily);
    fill_none(&mut keys.telegram_bot_token, &secrets.api_keys.telegram_bot_token);
    fill_none(&mut keys.whisper, &secrets.api_keys.whisper);
    Ok(())
}

/// Sets each entry of `target` that is there with an empty value to what `saved` has for it.
fn fill_empty(target: &mut HashMap<String, String>, saved: &BTreeMap<String, String>) {
    for (name, value) in saved {
        if let Some(slot) = target.get_mut(name).filter(|slot| slot.is_empty()) {
            *slot = value.clone();
        }
    }
}

fn fill_none(target: &mut Option<String>, saved: &Option<String>) {
    if target.as_deref().is_none_or(str::is_empty) {
        *target = saved.clone();
    }
}

/// Takes the non-empty values out of `values`, leaving their names with an empty value, and returns them.
fn take_values(values: &mut HashMap<String, String>) -> BTreeMap<String, String> {
    let mut taken = BTreeMap::new();
    for (name, value) in values.iter_mut() {
        if !value.is_empty() {
            taken.insert(name.clone(), std::mem::take(value));
        }
    }
    taken
}

fn take_key(key: &mut Option<String>) -> Option<String> {
    key.take().filter(|k| !k.is_empty())
}

/// A copy of `config` with every secret taken out, and the secrets that were in it. An empty key or value is
/// not a secret. Providers and servers that no longer exist drop out of the secrets file as well, because it is
/// rewritten whole from `config` each time.
fn split(config: &FileConfig) -> (FileConfig, SecretsFile) {
    let mut stripped = config.clone();
    let mut secrets = SecretsFile::default();
    for provider in &mut stripped.providers {
        if let Some(key) = provider.api_key.take().filter(|k| !k.is_empty()) {
            secrets.providers.insert(provider.id.clone(), ProviderSecret { api_key: key });
        }
    }
    for server in &mut stripped.mcp_servers {
        match server {
            McpServerConfig::Stdio { name, env, .. } => {
                let taken = take_values(env);
                if !taken.is_empty() {
                    secrets.mcp.entry(name.clone()).or_default().env = taken;
                }
            }
            McpServerConfig::Http { name, headers, .. } => {
                let taken = take_values(headers);
                if !taken.is_empty() {
                    secrets.mcp.entry(name.clone()).or_default().headers = taken;
                }
            }
        }
    }
    let keys = &mut stripped.api_keys;
    secrets.api_keys = ApiKeysSecret {
        gemini: take_key(&mut keys.gemini),
        openai: take_key(&mut keys.openai),
        tavily: take_key(&mut keys.tavily),
        telegram_bot_token: take_key(&mut keys.telegram_bot_token),
        whisper: take_key(&mut keys.whisper),
    };
    (stripped, secrets)
}

fn has_text(value: Option<&toml::Value>) -> bool {
    value.and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty())
}

/// True when the `config.toml` text at hand still holds a secret in plain text (needs the move): a provider key, a value of an MCP
/// server's `env` or `headers`, or one of the `[api_keys]`.
fn holds_plain_keys(existing: &str) -> bool {
    let Ok(table) = existing.parse::<toml::Table>() else { return false };
    let providers = table.get("providers").and_then(|v| v.as_array()).is_some_and(|list| list.iter().any(|p| has_text(p.get("api_key"))));
    let servers = table.get("mcp_servers").and_then(|v| v.as_array()).is_some_and(|list| {
        list.iter().any(|s| ["env", "headers"].iter().any(|field| s.get(field).and_then(|t| t.as_table()).is_some_and(|t| t.values().any(|v| has_text(Some(v))))))
    });
    let keys = table.get("api_keys").and_then(|v| v.as_table()).is_some_and(|t| t.values().any(|v| has_text(Some(v))));
    providers || servers || keys
}

/// What `save_config` needs: the config without its provider keys, after the keys were written to the
/// secrets file. Order matters: the secrets are written (and read back) before `config.toml` loses
/// them, so a failure in between leaves the keys in both places and never in neither. The first time a
/// plain key is moved, the old `config.toml` is kept as `config.toml.bak` (mode 0600 too).
pub fn store_and_strip(config: &FileConfig, config_path: &Path, existing: Option<&str>) -> anyhow::Result<FileConfig> {
    let (stripped, secrets) = split(config);
    let path = secrets_path(config_path);
    // Nothing to keep and no file yet: don't leave an empty file lying around.
    if secrets.is_empty() && !path.exists() {
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
    anyhow::ensure!(back == secrets, "the secrets file at {} did not read back as written; config.toml was left as it was", path.display());
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

    fn stdio(name: &str, env: &[(&str, &str)]) -> McpServerConfig {
        McpServerConfig::Stdio { name: name.into(), command: "npx".into(), args: vec!["-y".into(), "some-mcp".into()], env: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }

    fn http(name: &str, headers: &[(&str, &str)]) -> McpServerConfig {
        McpServerConfig::Http { name: name.into(), url: "https://mcp.example.com".into(), headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), oauth: false }
    }

    fn with_servers(servers: Vec<McpServerConfig>) -> FileConfig {
        FileConfig { mcp_servers: servers, ..FileConfig::default() }
    }

    #[test]
    fn the_values_of_an_mcp_server_leave_config_toml_and_come_back_but_their_names_stay() {
        let path = dir("mcp").join("config.toml");
        let config = with_servers(vec![stdio("github", &[("GITHUB_TOKEN", "ghp_secret_value"), ("NO_VALUE", "")]), http("notes", &[("Authorization", "Bearer bearer_secret")])]);
        save_config(&path, &config).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("ghp_secret_value") && !text.contains("bearer_secret"), "{text}");
        assert!(text.contains("GITHUB_TOKEN") && text.contains("Authorization") && text.contains("NO_VALUE"), "the names stay: {text}");
        let secrets = std::fs::read_to_string(secrets_path(&path)).unwrap();
        assert!(secrets.contains("ghp_secret_value") && secrets.contains("bearer_secret"), "{secrets}");
        assert!(!secrets.contains("NO_VALUE"), "an empty value is not a secret: {secrets}");

        let back = load_config_from_path(&path, true).unwrap();
        assert_eq!(back.mcp_servers, config.mcp_servers);
    }

    #[test]
    fn the_five_api_keys_leave_config_toml_and_come_back() {
        let path = dir("apikeys").join("config.toml");
        let mut config = FileConfig::default();
        config.api_keys.gemini = Some("key-gemini".into());
        config.api_keys.openai = Some("key-openai".into());
        config.api_keys.tavily = Some("key-tavily".into());
        config.api_keys.telegram_bot_token = Some("key-telegram".into());
        config.api_keys.whisper = Some("key-whisper".into());
        save_config(&path, &config).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        for key in ["key-gemini", "key-openai", "key-tavily", "key-telegram", "key-whisper"] {
            assert!(!text.contains(key), "{key} is in config.toml: {text}");
        }
        let back = load_config_from_path(&path, true).unwrap();
        assert_eq!(back.api_keys, config.api_keys);
    }

    #[test]
    fn a_value_still_in_config_toml_wins_and_the_first_save_keeps_one_backup_of_the_old_file() {
        let path = dir("mcp-legacy").join("config.toml");
        let old = "[[mcp_servers]]\nname = \"github\"\ncommand = \"npx\"\n[mcp_servers.env]\nGITHUB_TOKEN = \"in-config\"\n\n[api_keys]\ntavily = \"tavily-in-config\"\n";
        std::fs::write(&path, old).unwrap();
        std::fs::write(secrets_path(&path), "[mcp.github.env]\nGITHUB_TOKEN = \"in-secrets\"\n").unwrap();

        let loaded = load_config_from_path(&path, true).unwrap();
        let McpServerConfig::Stdio { env, .. } = &loaded.mcp_servers[0] else { panic!("{:?}", loaded.mcp_servers) };
        assert_eq!(env["GITHUB_TOKEN"], "in-config");
        assert_eq!(loaded.api_keys.tavily.as_deref(), Some("tavily-in-config"));

        save_config(&path, &loaded).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("in-config") && !text.contains("tavily-in-config"), "{text}");
        assert_eq!(std::fs::read_to_string(path.with_extension("toml.bak")).unwrap(), old, "the old file with the plain values is kept once");
        let back = load_config_from_path(&path, true).unwrap();
        let McpServerConfig::Stdio { env, .. } = &back.mcp_servers[0] else { panic!() };
        assert_eq!(env["GITHUB_TOKEN"], "in-config");
    }

    #[test]
    fn a_name_taken_out_of_config_toml_is_not_brought_back_by_the_secrets_file() {
        let path = dir("mcp-name").join("config.toml");
        std::fs::write(&path, "[[mcp_servers]]\nname = \"github\"\ncommand = \"npx\"\n[mcp_servers.env]\nGITHUB_TOKEN = \"\"\n").unwrap();
        std::fs::write(secrets_path(&path), "[mcp.github.env]\nGITHUB_TOKEN = \"kept\"\nGONE = \"must-not-return\"\n").unwrap();
        let loaded = load_config_from_path(&path, true).unwrap();
        let McpServerConfig::Stdio { env, .. } = &loaded.mcp_servers[0] else { panic!() };
        assert_eq!(env.get("GITHUB_TOKEN").map(String::as_str), Some("kept"));
        assert!(!env.contains_key("GONE"), "{env:?}");
    }

    #[test]
    fn a_removed_or_renamed_mcp_server_leaves_no_value_behind() {
        let path = dir("mcp-gone").join("config.toml");
        save_config(&path, &with_servers(vec![stdio("a", &[("T", "value-a")]), stdio("b", &[("T", "value-b")])])).unwrap();
        save_config(&path, &with_servers(vec![stdio("renamed", &[("T", "value-a")])])).unwrap();
        let secrets = std::fs::read_to_string(secrets_path(&path)).unwrap();
        assert!(secrets.contains("renamed") && secrets.contains("value-a") && !secrets.contains("value-b"), "{secrets}");
        assert!(!secrets.contains("[mcp.a"), "the old name is gone: {secrets}");
        save_config(&path, &with_servers(vec![stdio("renamed", &[("T", "")])])).unwrap();
        assert!(!std::fs::read_to_string(secrets_path(&path)).unwrap().contains("value-a"), "a cleared value leaves the file");
    }

    #[test]
    fn a_secrets_file_from_before_still_reads_and_one_with_a_section_this_version_does_not_know_is_refused() {
        let path = dir("compat").join("config.toml");
        std::fs::write(&path, "[[providers]]\nid = \"main\"\nkind = \"openai\"\nmodel = \"m\"\n").unwrap();
        std::fs::write(secrets_path(&path), "[providers.main]\napi_key = \"sk-old-format\"\n").unwrap();
        assert_eq!(load_config_from_path(&path, true).unwrap().providers[0].api_key.as_deref(), Some("sk-old-format"));

        std::fs::write(secrets_path(&path), "[providers.main]\napi_key = \"sk\"\n[from_the_future]\nx = 1\n").unwrap();
        assert!(load_config_from_path(&path, true).is_err(), "a section this version does not know is refused, never read without its secrets");
    }

    #[test]
    fn none_of_the_new_secrets_print() {
        let (_, secrets) = split(&{
            let mut config = with_servers(vec![stdio("github", &[("TOKEN", "ghp_secret_value")]), http("notes", &[("Authorization", "Bearer bearer_secret")])]);
            config.api_keys.tavily = Some("tavily_secret".into());
            config
        });
        let shown = format!("{secrets:?}");
        assert!(!shown.contains("ghp_secret_value") && !shown.contains("bearer_secret") && !shown.contains("tavily_secret"), "{shown}");
    }

    #[cfg(unix)]
    #[test]
    fn a_secrets_file_made_only_of_mcp_values_is_private_too() {
        use std::os::unix::fs::PermissionsExt;
        let path = dir("mcp-mode").join("config.toml");
        save_config(&path, &with_servers(vec![stdio("github", &[("TOKEN", "ghp_secret_value")])])).unwrap();
        assert_eq!(std::fs::metadata(secrets_path(&path)).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

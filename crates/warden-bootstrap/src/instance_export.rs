//! Exportar e importar a instância (P141, `HOSPEDAGEM.md` fase 1): o cliente hospedado nunca fica preso.
//!
//! O pacote é uma **pasta**: `warden-export.json` (o manifesto) mais os arquivos nos mesmos caminhos relativos da pasta de
//! dados. Só entra o que está na lista de inclusão abaixo (nunca "tudo menos"): a config, o cofre, as conversas, os membros
//! (já cifrados em disco), os logins MCP e as tarefas. Os pareamentos de dispositivos, as chaves da API, os tokens de
//! webhook, a chave do sync, o razão de gastos, as auditorias, os modelos e o TLS são do operador ou do destino, e ficam de fora.
//!
//! Segredos: as chaves dos provedores moram em `config.secrets.toml` (`secrets.rs`) e **só vão com `with_secrets`**. A chave
//! e o endereço do provedor `Cloud` (o gateway hospedado) nunca vão: o cliente põe as próprias chaves no destino. O `config.toml`
//! pode ter outros tokens (bots, `git_sync`, variáveis de MCP) e isto não os remove.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::secrets::{secrets_path, write_private};
use crate::{load_config_from_path, resolve_vault_path, save_config, Overrides, Provider};

pub const FORMAT_VERSION: u32 = 1;
pub const MANIFEST_FILE: &str = "warden-export.json";

const CONFIG_FILE: &str = "config.toml";
const SECRETS_FILE: &str = "config.secrets.toml";
const VAULT_DIR: &str = "vault";
/// Files and folders of the data dir that travel. Anything else is refused on import, whatever the manifest says.
const FILES: &[&str] = &[CONFIG_FILE, "agent_tasks.jsonl"];
const DIRS: &[&str] = &["conversations", "conversations-server", "conversations-telegram", "conversations-whatsapp", "tasks-server", "users", "mcp_oauth"];

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Takes the providers' keys (`config.secrets.toml`) along. Never the `Cloud` provider's.
    pub with_secrets: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Writes over a data dir that already has a config or a vault.
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedFile {
    /// Relative to the package, with `/` on every platform.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub warden_version: String,
    pub created_at_ms: i64,
    pub with_secrets: bool,
    /// Providers that need the person's own key at the destination.
    pub needs_keys: Vec<String>,
    pub files: Vec<ExportedFile>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExportReport {
    pub files: usize,
    pub bytes: u64,
    pub needs_keys: Vec<String>,
    pub with_secrets: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub files: usize,
    pub needs_keys: Vec<String>,
}

/// Copies what travels from `data_dir` into `out` (a folder that doesn't exist yet, or is empty).
pub fn export_instance(data_dir: &Path, out: &Path, options: &ExportOptions) -> anyhow::Result<ExportReport> {
    let config_path = data_dir.join(CONFIG_FILE);
    anyhow::ensure!(config_path.is_file(), "{} has no {CONFIG_FILE} — there is nothing to export", data_dir.display());
    anyhow::ensure!(!out.join(MANIFEST_FILE).exists(), "{} already holds an export — pick an empty folder", out.display());
    if out.exists() {
        anyhow::ensure!(out.read_dir()?.next().is_none(), "{} is not empty — pick an empty folder", out.display());
    }
    std::fs::create_dir_all(out).with_context(|| format!("failed to create {}", out.display()))?;

    // The vault may live outside the data dir (`vault_path`); it travels as `vault/` either way.
    let source_config = load_config_from_path(&config_path, false)?;
    let vault_source = resolve_vault_path(&Overrides::default(), &source_config, data_dir.join(VAULT_DIR));
    let had_key: BTreeSet<String> = source_config.providers.iter().filter(|p| p.api_key.is_some()).map(|p| p.id.clone()).collect();

    // The config goes through the same load/save the app uses, so the keys are split out again and the Cloud one can be cleared.
    std::fs::copy(&config_path, out.join(CONFIG_FILE)).context("failed to copy the config")?;
    let source_secrets = secrets_path(&config_path);
    if source_secrets.is_file() {
        std::fs::copy(&source_secrets, out.join(SECRETS_FILE)).context("failed to copy the keys file")?;
    }
    let mut config = load_config_from_path(&out.join(CONFIG_FILE), false)?;
    let mut needs_keys = BTreeSet::new();
    for provider in &mut config.providers {
        if provider.kind == Provider::Cloud {
            provider.api_key = None;
            provider.base_url = None;
            needs_keys.insert(provider.id.clone());
        } else if !options.with_secrets && had_key.contains(&provider.id) {
            needs_keys.insert(provider.id.clone());
        }
    }
    save_config(&out.join(CONFIG_FILE), &config)?;
    if !options.with_secrets {
        let _ = std::fs::remove_file(out.join(SECRETS_FILE));
    }

    let mut files = Vec::new();
    for name in [CONFIG_FILE, SECRETS_FILE] {
        if out.join(name).is_file() {
            files.push(describe(out, name)?);
        }
    }
    copy_file_if_present(data_dir, out, "agent_tasks.jsonl", &mut files)?;
    for dir in DIRS {
        copy_tree(&data_dir.join(dir), out, dir, &mut files)?;
    }
    copy_tree(&vault_source, out, VAULT_DIR, &mut files)?;

    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        warden_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default(),
        with_secrets: options.with_secrets,
        needs_keys: needs_keys.iter().cloned().collect(),
        files,
    };
    std::fs::write(out.join(MANIFEST_FILE), serde_json::to_vec_pretty(&manifest)?).context("failed to write the manifest")?;
    Ok(ExportReport { files: manifest.files.len(), bytes: manifest.files.iter().map(|f| f.size).sum(), needs_keys: manifest.needs_keys, with_secrets: options.with_secrets })
}

/// Puts a package into `data_dir`. Checks everything before it writes anything.
pub fn import_instance(package: &Path, data_dir: &Path, options: &ImportOptions) -> anyhow::Result<ImportReport> {
    let manifest_text = std::fs::read_to_string(package.join(MANIFEST_FILE)).with_context(|| format!("{} has no {MANIFEST_FILE} — it is not a Warden export", package.display()))?;
    let manifest: Manifest = serde_json::from_str(&manifest_text).context("the manifest is not valid")?;
    anyhow::ensure!(
        manifest.format_version <= FORMAT_VERSION,
        "this export is format {} and this Warden understands up to {FORMAT_VERSION} — update Warden to import it",
        manifest.format_version
    );

    // Everything is checked before the first byte is written.
    let mut targets = Vec::new();
    for file in &manifest.files {
        let relative = safe_relative(&file.path)?;
        let source = package.join(&relative);
        let bytes = std::fs::read(&source).with_context(|| format!("{} is in the manifest but missing from the package", file.path))?;
        anyhow::ensure!(bytes.len() as u64 == file.size, "{} has the wrong size — the package was changed", file.path);
        anyhow::ensure!(sha256_hex(&bytes) == file.sha256, "{} does not match its checksum — the package was changed", file.path);
        targets.push((source, data_dir.join(&relative), relative));
    }
    anyhow::ensure!(targets.iter().any(|(_, _, relative)| relative == Path::new(CONFIG_FILE)), "the package has no {CONFIG_FILE}");

    if !options.force {
        let busy = data_dir.join(CONFIG_FILE).exists() || dir_has_files(&data_dir.join(VAULT_DIR));
        anyhow::ensure!(!busy, "{} already has a config or a vault — stop the hub and import into an empty folder, or pass --force to write over it", data_dir.display());
    }

    for (source, target, relative) in &targets {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
        }
        if relative == Path::new(SECRETS_FILE) {
            write_private(target, &std::fs::read(source)?)?;
        } else {
            std::fs::copy(source, target).with_context(|| format!("failed to write {}", target.display()))?;
        }
    }

    // The vault arrives as `<data dir>/vault`; point the config there, whatever it said where it came from.
    let config_path = data_dir.join(CONFIG_FILE);
    let mut config = load_config_from_path(&config_path, false)?;
    config.vault_path = Some(data_dir.join(VAULT_DIR).to_string_lossy().into_owned());
    save_config(&config_path, &config)?;

    Ok(ImportReport { files: targets.len(), needs_keys: manifest.needs_keys })
}

/// A path from a manifest, only if it is relative, has no `..`, and starts at something that travels.
fn safe_relative(path: &str) -> anyhow::Result<PathBuf> {
    let relative = PathBuf::from(path);
    anyhow::ensure!(!path.is_empty() && !path.contains('\\'), "'{path}' is not a path this package may hold");
    anyhow::ensure!(relative.components().all(|c| matches!(c, Component::Normal(_))), "'{path}' is not a relative path inside the package");
    let first = relative.components().next().and_then(|c| c.as_os_str().to_str()).unwrap_or_default();
    let allowed = FILES.contains(&first) || DIRS.contains(&first) || first == VAULT_DIR || first == SECRETS_FILE;
    anyhow::ensure!(allowed, "'{path}' is not something an export holds");
    Ok(relative)
}

fn describe(root: &Path, relative: &str) -> anyhow::Result<ExportedFile> {
    let bytes = std::fs::read(root.join(relative)).with_context(|| format!("failed to read {relative}"))?;
    Ok(ExportedFile { path: relative.to_string(), size: bytes.len() as u64, sha256: sha256_hex(&bytes) })
}

fn copy_file_if_present(from_root: &Path, to_root: &Path, name: &str, files: &mut Vec<ExportedFile>) -> anyhow::Result<()> {
    let source = from_root.join(name);
    if source.is_file() {
        std::fs::copy(&source, to_root.join(name)).with_context(|| format!("failed to copy {name}"))?;
        files.push(describe(to_root, name)?);
    }
    Ok(())
}

/// Copies the regular files under `source` to `<to_root>/<name>/…`. A symlink is skipped, never followed, so a link inside
/// a vault can't pull a file from elsewhere on the machine into the package.
fn copy_tree(source: &Path, to_root: &Path, name: &str, files: &mut Vec<ExportedFile>) -> anyhow::Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    let mut stack = vec![(source.to_path_buf(), name.to_string())];
    while let Some((dir, relative)) = stack.pop() {
        std::fs::create_dir_all(to_root.join(&relative))?;
        let mut entries: Vec<_> = std::fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let child = format!("{relative}/{}", entry.file_name().to_string_lossy());
            let kind = entry.file_type()?;
            if kind.is_dir() {
                stack.push((entry.path(), child));
            } else if kind.is_file() {
                std::fs::copy(entry.path(), to_root.join(&child)).with_context(|| format!("failed to copy {child}"))?;
                files.push(describe(to_root, &child)?);
            }
        }
    }
    Ok(())
}

fn dir_has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("warden-export-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A data dir with a little of everything that travels and of everything that doesn't.
    fn data_dir(tag: &str) -> PathBuf {
        let dir = temp_dir(tag);
        write(
            &dir,
            "config.toml",
            "active_provider = \"mine\"\n\n[[providers]]\nid = \"mine\"\nkind = \"openai\"\napi_key = \"sk-mine\"\nmodel = \"m\"\n\n[[providers]]\nid = \"hosted\"\nkind = \"cloud\"\napi_key = \"wdn_gateway\"\nbase_url = \"https://gateway.example.com/v1\"\nmodel = \"m\"\n",
        );
        write(&dir, "vault/notes/a.md", "# a\n");
        write(&dir, "vault/skills/s.md", "skill\n");
        write(&dir, "conversations-server/c1.json", "{}");
        write(&dir, "tasks-server/state.json", "{}");
        write(&dir, "users/ana/vault/x.md", "enc");
        write(&dir, "mcp_oauth/server.json", "{}");
        write(&dir, "agent_tasks.jsonl", "{}\n");
        write(&dir, "devices.json", "{\"devices\":{}}");
        write(&dir, "api_keys.json", "[]");
        write(&dir, "webhook_tokens.json", "{}");
        write(&dir, "sync_secrets.json", "{}");
        write(&dir, "spend_ledger.jsonl", "");
        write(&dir, "models/m.onnx", "big");
        dir
    }

    fn paths(manifest: &Manifest) -> Vec<&str> {
        manifest.files.iter().map(|f| f.path.as_str()).collect()
    }

    fn read_manifest(package: &Path) -> Manifest {
        serde_json::from_str(&std::fs::read_to_string(package.join(MANIFEST_FILE)).unwrap()).unwrap()
    }

    #[test]
    fn what_travels_comes_back_identical_and_what_belongs_to_the_operator_stays() {
        let source = data_dir("roundtrip-src");
        let package = temp_dir("roundtrip-pkg").join("export");
        let report = export_instance(&source, &package, &ExportOptions::default()).unwrap();
        assert!(report.files >= 8, "{report:?}");

        let manifest = read_manifest(&package);
        for kept in ["config.toml", "vault/notes/a.md", "vault/skills/s.md", "conversations-server/c1.json", "tasks-server/state.json", "users/ana/vault/x.md", "mcp_oauth/server.json", "agent_tasks.jsonl"] {
            assert!(paths(&manifest).contains(&kept), "{kept} missing from {:?}", paths(&manifest));
        }
        for left_out in ["devices.json", "api_keys.json", "webhook_tokens.json", "sync_secrets.json", "spend_ledger.jsonl", "models/m.onnx"] {
            assert!(!package.join(left_out).exists(), "{left_out} was exported");
        }

        let destination = temp_dir("roundtrip-dst");
        let imported = import_instance(&package, &destination, &ImportOptions::default()).unwrap();
        assert_eq!(imported.files, manifest.files.len());
        for kept in ["vault/notes/a.md", "vault/skills/s.md", "conversations-server/c1.json", "tasks-server/state.json", "users/ana/vault/x.md", "mcp_oauth/server.json", "agent_tasks.jsonl"] {
            assert_eq!(std::fs::read(source.join(kept)).unwrap(), std::fs::read(destination.join(kept)).unwrap(), "{kept} differs");
        }
        assert!(!destination.join("devices.json").exists());
    }

    #[test]
    fn the_cloud_provider_never_takes_its_key_or_its_address_and_the_others_keys_need_the_flag() {
        let source = data_dir("secrets-src");
        let without = temp_dir("secrets-without").join("export");
        let report = export_instance(&source, &without, &ExportOptions::default()).unwrap();
        assert!(!without.join(SECRETS_FILE).exists(), "the keys file travelled without the flag");
        assert_eq!(report.needs_keys, vec!["hosted".to_string(), "mine".to_string()]);
        let config = std::fs::read_to_string(without.join(CONFIG_FILE)).unwrap();
        assert!(!config.contains("sk-mine") && !config.contains("wdn_gateway") && !config.contains("gateway.example.com"), "{config}");

        let with = temp_dir("secrets-with").join("export");
        let report = export_instance(&source, &with, &ExportOptions { with_secrets: true }).unwrap();
        assert_eq!(report.needs_keys, vec!["hosted".to_string()], "only the Cloud provider still needs a key");
        let keys = std::fs::read_to_string(with.join(SECRETS_FILE)).unwrap();
        assert!(keys.contains("sk-mine") && !keys.contains("wdn_gateway"), "{keys}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(with.join(SECRETS_FILE)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn an_import_points_the_config_at_the_vault_it_brought() {
        let source = data_dir("vault-src");
        let package = temp_dir("vault-pkg").join("export");
        export_instance(&source, &package, &ExportOptions::default()).unwrap();
        let destination = temp_dir("vault-dst");
        import_instance(&package, &destination, &ImportOptions::default()).unwrap();
        let config = load_config_from_path(&destination.join(CONFIG_FILE), false).unwrap();
        assert_eq!(config.vault_path.as_deref(), Some(destination.join("vault").to_string_lossy().as_ref()));
    }

    #[test]
    fn a_vault_outside_the_data_dir_travels_as_vault() {
        let source = data_dir("outside-src");
        let outside = temp_dir("outside-vault");
        write(&outside, "elsewhere.md", "outside");
        std::fs::remove_dir_all(source.join("vault")).unwrap();
        let config = std::fs::read_to_string(source.join(CONFIG_FILE)).unwrap();
        std::fs::write(source.join(CONFIG_FILE), format!("vault_path = {:?}\n{config}", outside.to_string_lossy())).unwrap();
        let package = temp_dir("outside-pkg").join("export");
        export_instance(&source, &package, &ExportOptions::default()).unwrap();
        assert_eq!(std::fs::read_to_string(package.join("vault/elsewhere.md")).unwrap(), "outside");
    }

    #[test]
    fn a_newer_format_is_refused() {
        let source = data_dir("newer-src");
        let package = temp_dir("newer-pkg").join("export");
        export_instance(&source, &package, &ExportOptions::default()).unwrap();
        let mut manifest = read_manifest(&package);
        manifest.format_version = FORMAT_VERSION + 1;
        std::fs::write(package.join(MANIFEST_FILE), serde_json::to_vec(&manifest).unwrap()).unwrap();
        let err = import_instance(&package, &temp_dir("newer-dst"), &ImportOptions::default()).unwrap_err();
        assert!(err.to_string().contains("update Warden"), "{err}");
    }

    #[test]
    fn a_changed_file_is_refused_and_nothing_is_written() {
        let source = data_dir("tamper-src");
        let package = temp_dir("tamper-pkg").join("export");
        export_instance(&source, &package, &ExportOptions::default()).unwrap();
        std::fs::write(package.join("vault/notes/a.md"), "# a, changed\n").unwrap();
        let destination = temp_dir("tamper-dst");
        let err = import_instance(&package, &destination, &ImportOptions::default()).unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
        assert!(!destination.join(CONFIG_FILE).exists(), "something was written before the check failed");
    }

    #[test]
    fn paths_that_leave_the_package_or_name_something_that_does_not_travel_are_refused() {
        for bad in ["../evil", "/etc/passwd", "vault/../../evil", "devices.json", "api_keys.json", "models/x", "vault\\x", ""] {
            assert!(safe_relative(bad).is_err(), "'{bad}' got through");
        }
        for good in ["config.toml", "config.secrets.toml", "vault/a/b.md", "users/ana/x", "agent_tasks.jsonl"] {
            assert!(safe_relative(good).is_ok(), "'{good}' was refused");
        }
    }

    #[test]
    fn a_manifest_that_names_a_path_outside_is_refused_even_with_a_matching_checksum() {
        let source = data_dir("evil-src");
        let package = temp_dir("evil-pkg").join("export");
        export_instance(&source, &package, &ExportOptions::default()).unwrap();
        let mut manifest = read_manifest(&package);
        manifest.files.push(ExportedFile { path: "../escape.txt".into(), size: 0, sha256: sha256_hex(b"") });
        std::fs::write(package.join(MANIFEST_FILE), serde_json::to_vec(&manifest).unwrap()).unwrap();
        let err = import_instance(&package, &temp_dir("evil-dst"), &ImportOptions::default()).unwrap_err();
        assert!(err.to_string().contains("not a relative path"), "{err}");
    }

    #[test]
    fn a_data_dir_that_already_has_a_config_is_not_written_over_without_force() {
        let source = data_dir("busy-src");
        let package = temp_dir("busy-pkg").join("export");
        export_instance(&source, &package, &ExportOptions::default()).unwrap();
        let destination = temp_dir("busy-dst");
        write(&destination, CONFIG_FILE, "# mine\n");
        let err = import_instance(&package, &destination, &ImportOptions::default()).unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        assert_eq!(std::fs::read_to_string(destination.join(CONFIG_FILE)).unwrap(), "# mine\n");
        import_instance(&package, &destination, &ImportOptions { force: true }).unwrap();
        assert!(destination.join("vault/notes/a.md").exists());
    }

    #[test]
    fn an_export_refuses_a_folder_that_is_not_empty_and_a_data_dir_with_no_config() {
        let source = data_dir("refuse-src");
        let busy = temp_dir("refuse-busy");
        write(&busy, "x", "x");
        assert!(export_instance(&source, &busy, &ExportOptions::default()).is_err());
        assert!(export_instance(&temp_dir("refuse-empty"), &temp_dir("refuse-out").join("e"), &ExportOptions::default()).is_err());
    }
}

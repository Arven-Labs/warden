use std::collections::HashMap;
use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

/// The vault-encryption key, generated once on the first device and shared with every other
/// Warden install via pairing (`crate::pairing`) — never touches the TruthID wallet or Arweave.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSecrets {
    pub version: u8,
    /// 16 random bytes, hex-encoded — same "opaque unique-enough string" posture as the mobile
    /// client's own `generateDeviceId()` (`mobile/lib/services/device_id.dart`), deliberately not
    /// a `uuid`-crate v4 UUID (buys nothing over 128 bits of raw entropy for an id nothing else
    /// ever needs to parse).
    pub device_id: String,
    #[serde(with = "base64_32")]
    pub vault_key: [u8; 32],
}

/// Non-secret sync tracking state, rewritten after every successful push/pull.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SyncManifest {
    pub version: u8,
    /// Arweave wallet address of the TruthID wallet that has been paying for our pins — learned
    /// once (either from this device's own first push, or from whichever device we paired with)
    /// and reused from then on to discover "latest" via `ArweaveClient::latest_tx_by_owner`.
    pub owner_address: Option<String>,
    /// Most recently applied/pushed Arweave tx id (no `ar://` prefix).
    pub last_tx_id: Option<String>,
    pub last_sync_direction: Option<SyncDirection>,
    pub last_synced_at_ms: Option<i64>,
    /// Monotonic counter carried inside every bundle (`bundle::SyncBundle::manifest_counter`) —
    /// lets `pull` refuse to apply a snapshot older than or equal to what's already applied.
    pub manifest_counter: u64,
    /// Relative vault path -> sha256 hex of its content, as of the last successful push/pull.
    pub vault_files: HashMap<String, String>,
    /// File name (`<server>.json`) -> sha256 hex of the MCP OAuth login saved under that name, as of the last successful
    /// push/pull (P54). Absent in a manifest from before they were synced.
    #[serde(default)]
    pub oauth_files: HashMap<String, String>,
    pub config_present: bool,
    pub config_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncDirection {
    Push,
    Pull,
}

impl SyncManifest {
    pub fn is_paired(&self) -> bool {
        self.owner_address.is_some()
    }
}

/// 16 random bytes, hex-encoded — same "opaque unique-enough string" posture as the mobile
/// client's `generateDeviceId()` (`mobile/lib/services/device_id.dart`), deliberately not a
/// `uuid`-crate v4 UUID.
pub fn generate_device_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn generate_secrets() -> SyncSecrets {
    let mut vault_key = [0u8; 32];
    OsRng.fill_bytes(&mut vault_key);
    SyncSecrets { version: 1, device_id: generate_device_id(), vault_key }
}

pub fn load_secrets(path: &Path) -> anyhow::Result<Option<SyncSecrets>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            // P-11: a file written before `save_secrets` set `0o600` has the umask's mode; `save_secrets` only runs at init and
            // pairing, so an existing install is closed here. A file that cannot be closed is an error, not a silent pass.
            close_to_owner(path)?;
            Ok(Some(serde_json::from_str(&contents)?))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn save_secrets(path: &Path, secrets: &SyncSecrets) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomic(path, serde_json::to_string_pretty(secrets)?.as_bytes(), true)
}

/// Writes `contents` to `path` so that a reader, or a cut in the middle of the write, never sees half of it (P175a): the bytes
/// go to a temporary next to the file, are flushed to disk, and the temporary is renamed over the real name in one step (same
/// directory, so the same filesystem). On any failure the temporary is removed and the file that was there is left untouched.
///
/// With `owner_only` (the vault key, P-11) the temporary is created with `0o600`, so there is no window with the umask's mode,
/// and a leftover temporary from an earlier crash is closed *before* the key goes in, since the mode of `open` only applies to
/// a new file. Without it the umask decides, as `std::fs::write` did. Outside unix there is no mode to set.
///
/// Not synced: the parent directory. Without that, a power cut right after can lose the rename, which leaves the previous file
/// (or none): a consistent state, not a broken one.
fn write_atomic(path: &Path, contents: &[u8], owner_only: bool) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    let result = write_then_swap(&tmp, path, contents, owner_only);
    if result.is_err() {
        // Best effort: if `tmp` is a directory (or gone) this fails too, and the original error is what matters.
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn write_then_swap(tmp: &Path, path: &Path, contents: &[u8], owner_only: bool) -> anyhow::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(tmp)?;
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = owner_only;
    file.write_all(contents)?;
    // Data on disk before the name changes: otherwise a power cut can persist the rename and leave an empty file under it.
    file.sync_all()?;
    drop(file);
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Sets `0o600` when the file is open to its group or to others; leaves one that is already closed untouched.
fn close_to_owner(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub fn load_manifest(path: &Path) -> anyhow::Result<SyncManifest> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(serde_json::from_str(&contents)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SyncManifest { version: 1, ..Default::default() }),
        Err(err) => Err(err.into()),
    }
}

pub fn save_manifest(path: &Path, manifest: &SyncManifest) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomic(path, serde_json::to_string_pretty(manifest)?.as_bytes(), false)
}

mod base64_32 {
    use super::{Engine, BASE64};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 32], D::Error> {
        let encoded = String::deserialize(deserializer)?;
        let decoded = BASE64.decode(encoded.as_bytes()).map_err(serde::de::Error::custom)?;
        decoded.try_into().map_err(|v: Vec<u8>| serde::de::Error::custom(format!("expected 32 bytes, got {}", v.len())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "warden-sync-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    #[test]
    fn secrets_round_trip_through_json() {
        let secrets = generate_secrets();
        let path = temp_path("secrets");
        save_secrets(&path, &secrets).unwrap();
        let loaded = load_secrets(&path).unwrap().unwrap();
        assert_eq!(loaded.device_id, secrets.device_id);
        assert_eq!(loaded.vault_key, secrets.vault_key);
    }

    #[test]
    fn load_secrets_returns_none_when_missing() {
        assert!(load_secrets(&temp_path("missing")).unwrap().is_none());
    }

    #[cfg(unix)]
    fn mode_of(path: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    // P-11: the vault key is for its owner only, like the MCP logins in `oauth.rs`.
    #[cfg(unix)]
    #[test]
    fn a_new_secrets_file_is_owner_only() {
        let path = temp_path("secrets-new-mode");
        save_secrets(&path, &generate_secrets()).unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn saving_over_an_open_secrets_file_closes_it() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("secrets-resave-mode");
        std::fs::write(&path, "stale").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        save_secrets(&path, &generate_secrets()).unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    // Installs that already exist have the file with the umask; `save_secrets` only runs at init and pairing, so loading has
    // to close it too.
    #[cfg(unix)]
    #[test]
    fn loading_an_open_secrets_file_closes_it_and_still_returns_the_key() {
        use std::os::unix::fs::PermissionsExt;
        let secrets = generate_secrets();
        let path = temp_path("secrets-load-mode");
        save_secrets(&path, &secrets).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let loaded = load_secrets(&path).unwrap().unwrap();
        assert_eq!(loaded.vault_key, secrets.vault_key);
        assert_eq!(mode_of(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn loading_an_already_closed_secrets_file_leaves_it_alone() {
        let path = temp_path("secrets-load-closed");
        save_secrets(&path, &generate_secrets()).unwrap();
        load_secrets(&path).unwrap().unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    // P175(a): the state files are written to a temporary next to them and swapped in with a rename, so a cut in the middle of a
    // write never leaves the real file empty or half written (which would make `is_initialized()` true over a file that no
    // longer parses, and lock the device out of `init_fresh` and of pairing).
    fn tmp_of(path: &std::path::Path) -> std::path::PathBuf {
        path.with_extension("tmp")
    }

    #[test]
    fn saving_the_secrets_and_the_manifest_leaves_no_temporary_behind() {
        let secrets_path = temp_path("atomic-secrets-clean");
        let manifest_path = temp_path("atomic-manifest-clean");
        save_secrets(&secrets_path, &generate_secrets()).unwrap();
        save_manifest(&manifest_path, &SyncManifest { version: 1, ..Default::default() }).unwrap();
        assert!(!tmp_of(&secrets_path).exists());
        assert!(!tmp_of(&manifest_path).exists());
    }

    #[test]
    fn a_failed_secrets_save_leaves_the_previous_file_whole() {
        let original = generate_secrets();
        let path = temp_path("atomic-secrets-fail");
        save_secrets(&path, &original).unwrap();
        // A directory where the temporary has to go makes creating it fail, after which the real file must be untouched.
        std::fs::create_dir(tmp_of(&path)).unwrap();

        assert!(save_secrets(&path, &generate_secrets()).is_err());

        let loaded = load_secrets(&path).unwrap().unwrap();
        assert_eq!(loaded.vault_key, original.vault_key, "the old key must still be there, whole");
    }

    #[test]
    fn a_failed_manifest_save_leaves_the_previous_file_whole() {
        let original = SyncManifest { version: 1, owner_address: Some("abc123".to_string()), manifest_counter: 7, ..Default::default() };
        let path = temp_path("atomic-manifest-fail");
        save_manifest(&path, &original).unwrap();
        std::fs::create_dir(tmp_of(&path)).unwrap();

        assert!(save_manifest(&path, &SyncManifest { version: 1, manifest_counter: 8, ..Default::default() }).is_err());

        assert_eq!(load_manifest(&path).unwrap(), original);
    }

    // A temporary left by an earlier crash keeps the mode it had, and the `mode()` of `open` only applies to a new file: it has
    // to be closed before the key goes in.
    #[cfg(unix)]
    #[test]
    fn a_stale_open_temporary_does_not_leak_the_key() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("atomic-secrets-stale-tmp");
        std::fs::write(tmp_of(&path), "left by a crash").unwrap();
        std::fs::set_permissions(tmp_of(&path), std::fs::Permissions::from_mode(0o644)).unwrap();

        save_secrets(&path, &generate_secrets()).unwrap();

        assert_eq!(mode_of(&path), 0o600);
        assert!(!tmp_of(&path).exists());
    }

    #[test]
    fn load_manifest_defaults_when_missing() {
        let manifest = load_manifest(&temp_path("missing-manifest")).unwrap();
        assert!(!manifest.is_paired());
        assert_eq!(manifest.manifest_counter, 0);
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let mut manifest = SyncManifest { version: 1, ..Default::default() };
        manifest.owner_address = Some("abc123".to_string());
        manifest.vault_files.insert("notes/todo.md".to_string(), "deadbeef".to_string());
        let path = temp_path("manifest");
        save_manifest(&path, &manifest).unwrap();
        let loaded = load_manifest(&path).unwrap();
        assert_eq!(loaded, manifest);
    }
}

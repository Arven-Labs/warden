//! Who this computer is to each hub it is a client of: one device id and one device token per saved hub, in `remote_hub.json`
//! beside the `config.toml`. It lives here (not in `warden-server`, where it started) so that every front end on this computer
//! — the desktop and the CLI — reads the same file without depending on the server crate. `warden_server::remote_client`
//! re-exports it, so older imports keep working.
//!
//! Its own identity is not the node's (`node.json`): the same device would otherwise be a node and a client of one hub at once,
//! and pairing again resets its status.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// One hub's view of this client: the device id it knows, and the token it issued for it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RemoteIdentity {
    pub device_id: String,
    #[serde(default)]
    pub device_token: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct IdentityFile {
    #[serde(default)]
    hubs: BTreeMap<String, RemoteIdentity>,
}

/// The identities of one `config.toml`, in `remote_hub.json` beside it, keyed by the saved hub's id. Read again on every
/// call, and written atomically (owner-readable only): the token is what lets this computer speak to that hub.
pub struct RemoteIdentities {
    path: PathBuf,
}

impl RemoteIdentities {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// The file that goes with the `config.toml` at `config_path`.
    pub fn beside(config_path: &Path) -> Self {
        Self::new(config_path.with_file_name("remote_hub.json"))
    }

    fn read(&self) -> anyhow::Result<IdentityFile> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("parsing {}", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(IdentityFile::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", self.path.display())),
        }
    }

    fn write(&self, file: &IdentityFile) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(file)?).with_context(|| format!("writing {}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &self.path).with_context(|| format!("replacing {}", self.path.display()))
    }

    /// This client's identity for `hub_id`: the saved one, or a new device id (`desktop-<name>-<8 hex>`) the first time.
    pub fn load_or_create(&self, hub_id: &str, device_name: &str) -> anyhow::Result<RemoteIdentity> {
        let mut file = self.read()?;
        if let Some(identity) = file.hubs.get(hub_id) {
            return Ok(identity.clone());
        }
        use sha2::{Digest, Sha256};
        let seed = format!("{device_name}-{hub_id}-{:?}-{}", std::time::SystemTime::now(), std::process::id());
        let suffix: String = Sha256::digest(seed.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect();
        let slug: String = device_name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).take(24).collect();
        let identity = RemoteIdentity { device_id: format!("desktop-{slug}-{suffix}"), device_token: None };
        file.hubs.insert(hub_id.to_string(), identity.clone());
        self.write(&file)?;
        Ok(identity)
    }

    /// Keeps the token the hub issued for `hub_id`.
    pub fn set_token(&self, hub_id: &str, token: &str) -> anyhow::Result<()> {
        let mut file = self.read()?;
        let Some(identity) = file.hubs.get_mut(hub_id) else {
            anyhow::bail!("there is no identity for hub '{hub_id}' yet");
        };
        identity.device_token = Some(token.to_string());
        self.write(&file)
    }

    /// Forgets the token for `hub_id` (signing out, or the hub turned it away); the device id stays.
    pub fn clear_token(&self, hub_id: &str) -> anyhow::Result<()> {
        let mut file = self.read()?;
        if let Some(identity) = file.hubs.get_mut(hub_id) {
            if identity.device_token.take().is_some() {
                self.write(&file)?;
            }
        }
        Ok(())
    }

    /// Forgets everything about `hub_id` (the hub was removed from the list).
    pub fn forget(&self, hub_id: &str) -> anyhow::Result<()> {
        let mut file = self.read()?;
        if file.hubs.remove(hub_id).is_some() {
            self.write(&file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_file() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("warden-remote-identities-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("remote_hub.json")
    }

    #[test]
    fn each_hub_gets_its_own_device_id_and_token_and_they_survive_a_reload() {
        let path = temp_file();
        let store = RemoteIdentities::new(path.clone());
        let home = store.load_or_create("hub-1", "My Desktop!").unwrap();
        let vps = store.load_or_create("hub-2", "My Desktop!").unwrap();
        assert!(home.device_id.starts_with("desktop-my-desktop--"), "{}", home.device_id);
        assert_ne!(home.device_id, vps.device_id, "one device id per hub");
        assert_eq!(home.device_token, None);
        assert_eq!(store.load_or_create("hub-1", "another name").unwrap(), home, "the saved one wins over a new name");

        store.set_token("hub-1", "tok-1").unwrap();
        let again = RemoteIdentities::new(path.clone());
        assert_eq!(again.load_or_create("hub-1", "x").unwrap().device_token.as_deref(), Some("tok-1"));
        assert_eq!(again.load_or_create("hub-2", "x").unwrap().device_token, None, "the other hub's token is its own");
        assert!(store.set_token("hub-nope", "t").is_err());

        store.clear_token("hub-1").unwrap();
        let cleared = store.load_or_create("hub-1", "x").unwrap();
        assert_eq!((cleared.device_id.as_str(), cleared.device_token.as_deref()), (home.device_id.as_str(), None), "the id stays, the token goes");
        store.forget("hub-2").unwrap();
        assert_ne!(store.load_or_create("hub-2", "x").unwrap().device_id, vps.device_id, "forgotten, so a new id");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_file_holds_a_device_id_and_a_token_and_is_the_owners() {
        let path = temp_file();
        let store = RemoteIdentities::new(path.clone());
        store.load_or_create("hub-1", "Desktop").unwrap();
        store.set_token("hub-1", "tok-1").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut keys: Vec<&str> = json["hubs"]["hub-1"].as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["device_id", "device_token"], "no key or password field exists: {text}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_broken_file_is_an_error_and_is_left_alone() {
        let path = temp_file();
        std::fs::write(&path, "{ not json").unwrap();
        let store = RemoteIdentities::new(path.clone());
        assert!(store.load_or_create("hub-1", "Desktop").is_err());
        assert!(store.set_token("hub-1", "t").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

//! The hub's pairing key that can change while the hub runs (P36 c).
//!
//! The key a client presents in its first `Hello` is also the credential for the hub's administration, and a revoked device
//! can pair again under a new id (the id is the client's own), so the one thing that really shuts the door is changing the
//! key. It used to be a start-up parameter (`--auth-key`, `WARDEN_SERVER_AUTH_KEY`), so changing it meant restarting. Now a
//! file beside the device registry can hold it: when that file has a usable key it wins over the start-up one, and it is read
//! again each time the key is needed, so `warden-server devices rotate-key` (another process, like `devices approve`) takes
//! effect on the next `Hello` and on the next administration request of a connection that is already open. Devices that
//! already hold a token are not touched by a change (`device_registry`).
//!
//! The file is a few dozen bytes read at most once per message, so it is read each time rather than cached by its timestamp:
//! two changes within one tick of the file system's clock would otherwise look like none.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;

/// The file the key lives in, beside the device registry at `devices_path`.
pub fn pairing_key_path_beside(devices_path: &Path) -> PathBuf {
    devices_path.with_file_name("pairing_key")
}

/// The pairing key of a hub: the start-up one, or the file's when the file has a usable one. Cheap to clone.
#[derive(Clone)]
pub struct PairingKey {
    initial: Arc<str>,
    file: Option<Arc<PathBuf>>,
}

impl PairingKey {
    /// A key that never changes: the start-up one.
    pub fn new(initial: impl Into<Arc<str>>) -> Self {
        Self { initial: initial.into(), file: None }
    }

    /// The same key, but the file at `path` wins over it when it holds a usable one.
    pub fn with_file(mut self, path: PathBuf) -> Self {
        self.file = Some(Arc::new(path));
        self
    }

    /// The key that counts right now. A file that is missing, empty, too weak or unreadable is not an error and not a way in: the
    /// start-up key stays, so a hub never ends up with no key or a guessable one.
    pub fn current(&self) -> Arc<str> {
        match self.file.as_deref().and_then(|path| read_usable(path)) {
            Some(key) => key,
            None => self.initial.clone(),
        }
    }

    /// Whether the file is what `current()` is giving now, not the start-up key.
    pub fn from_file(&self) -> bool {
        self.file.as_deref().is_some_and(|path| read_usable(path).is_some())
    }
}

fn read_usable(path: &Path) -> Option<Arc<str>> {
    let text = std::fs::read_to_string(path).ok()?;
    let key = text.trim();
    warden_bootstrap::is_strong_auth_key(key).then(|| Arc::from(key))
}

/// Writes a new random key to `path` (readable by the owner only, replaced in one step so a reader never sees half of it) and
/// returns it. Safe while `serve` runs: it takes the new key on its next use.
pub fn rotate_at(path: &Path) -> anyhow::Result<String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let key = warden_bootstrap::generate_auth_key();
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
        file.write_all(key.as_bytes())?;
        file.write_all(b"\n")?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "start-up-key-start-up-key-start-up-key-0123456789";

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("warden-pairing-key-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())).join("pairing_key")
    }

    #[test]
    fn with_no_file_the_start_up_key_counts() {
        assert_eq!(&*PairingKey::new(START).current(), START);
        let key = PairingKey::new(START).with_file(temp_path("missing"));
        assert_eq!(&*key.current(), START);
        assert!(!key.from_file());
    }

    #[test]
    fn a_usable_key_in_the_file_wins_and_a_change_of_the_file_is_seen_at_once() {
        let path = temp_path("wins");
        let key = PairingKey::new(START).with_file(path.clone());

        let first = rotate_at(&path).unwrap();
        assert_eq!(&*key.current(), first.as_str());
        assert!(key.from_file());

        let second = rotate_at(&path).unwrap();
        assert_ne!(first, second, "a rotation never gives the same key twice");
        assert_eq!(&*key.current(), second.as_str(), "seen on the very next use, with no restart");
    }

    #[test]
    fn a_file_that_cannot_be_trusted_is_not_a_way_in_and_not_a_way_to_lose_the_key() {
        let path = temp_path("weak");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let key = PairingKey::new(START).with_file(path.clone());
        for contents in ["", "   \n", "short", "a-weak-key"] {
            std::fs::write(&path, contents).unwrap();
            assert_eq!(&*key.current(), START, "{contents:?}");
            assert!(!key.from_file());
        }
        // A directory where the file should be is unreadable, not a crash.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(&*key.current(), START);
    }

    #[test]
    fn taking_the_file_away_goes_back_to_the_start_up_key() {
        let path = temp_path("away");
        let key = PairingKey::new(START).with_file(path.clone());
        let rotated = rotate_at(&path).unwrap();
        assert_eq!(&*key.current(), rotated.as_str());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(&*key.current(), START);
    }

    #[test]
    fn the_key_in_the_file_is_trimmed_and_the_file_is_for_its_owner_only() {
        let path = temp_path("trim");
        let rotated = rotate_at(&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().ends_with('\n'));
        assert_eq!(rotated.len(), 64);
        assert_eq!(&*PairingKey::new(START).with_file(path.clone()).current(), rotated.as_str());
        assert!(!path.with_extension("tmp").exists(), "nothing is left beside it");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "not readable by other users");
        }
    }

    #[test]
    fn the_file_sits_beside_the_device_registry() {
        assert_eq!(pairing_key_path_beside(Path::new("/x/warden/server_devices.json")), PathBuf::from("/x/warden/pairing_key"));
    }
}

//! The key the gateway presents to a model provider (P168 e).
//!
//! It comes from the environment (`api_key_env`, read once at start) or from a file (`api_key_file`: a Docker or systemd
//! secret, kept out of the data volume and its backups). A file is read again when its modification time changes, so a key
//! can be rotated without restarting the gateway and cutting the calls in flight: one `stat` per call, nothing next to the
//! call itself. The value never reaches a log or a `Debug` print; only where it comes from does.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[derive(Clone)]
pub struct ApiKey(Arc<Source>);

enum Source {
    None,
    Env { name: String, value: Option<String> },
    File(FileKey),
}

struct FileKey {
    path: PathBuf,
    state: Mutex<FileState>,
}

struct FileState {
    /// The modification time of the last attempt to read, good or not: a bad file is not read again until it changes.
    seen: Option<SystemTime>,
    /// The last key read successfully, kept when a later read fails (mid-rewrite, deleted, emptied).
    value: Option<String>,
}

/// The file's text without the newline that secret files usually end with. `None`: nothing there.
fn read_secret(path: &Path) -> Result<Option<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read the API key file {}: {e}", path.display()))?;
    let key = text.trim();
    Ok((!key.is_empty()).then(|| key.to_string()))
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl ApiKey {
    /// An upstream that takes no key.
    pub fn none() -> ApiKey {
        ApiKey(Arc::new(Source::None))
    }

    /// The value of the environment variable `name`, as it was when the gateway started (`None`: unset or empty).
    pub fn from_env(name: &str, value: Option<String>) -> ApiKey {
        ApiKey(Arc::new(Source::Env { name: name.to_string(), value }))
    }

    /// Reads `path` now: a file that can't be read is an error, so a gateway told to use a key file never starts without it.
    /// An empty file is no key (the same warning as an unset variable).
    pub fn from_file(path: PathBuf) -> Result<ApiKey, String> {
        // The time is taken before the read: a write that lands in between shows as a change on the next call.
        let seen = modified(&path);
        let value = read_secret(&path)?;
        Ok(ApiKey(Arc::new(Source::File(FileKey { path, state: Mutex::new(FileState { seen, value }) }))))
    }

    /// The key to send now. For a file this is the current content, read again if the file changed since the last call.
    pub fn get(&self) -> Option<String> {
        match &*self.0 {
            Source::None => None,
            Source::Env { value, .. } => value.clone(),
            Source::File(file) => {
                let mtime = modified(&file.path);
                let mut state = file.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                // No mtime means the file is gone for now: keep the key we have.
                if mtime.is_some() && mtime != state.seen {
                    state.seen = mtime;
                    match read_secret(&file.path) {
                        Ok(Some(key)) => {
                            tracing::info!(file = %file.path.display(), "reloaded the API key");
                            state.value = Some(key);
                        }
                        Ok(None) => tracing::warn!(file = %file.path.display(), "the API key file is empty; keeping the previous key"),
                        Err(e) => tracing::warn!("{e}; keeping the previous key"),
                    }
                }
                state.value.clone()
            }
        }
    }

    /// Where the key comes from, for `catalog-check` and logs: never the key itself.
    pub fn origin(&self) -> String {
        match &*self.0 {
            Source::None => "none".to_string(),
            Source::Env { name, .. } => format!("env {name}"),
            Source::File(file) => format!("file {}", file.path.display()),
        }
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApiKey({})", self.origin())
    }
}

impl PartialEq for ApiKey {
    fn eq(&self, other: &ApiKey) -> bool {
        match (&*self.0, &*other.0) {
            (Source::None, Source::None) => true,
            (Source::Env { name: a, value: x }, Source::Env { name: b, value: y }) => a == b && x == y,
            (Source::File(a), Source::File(b)) => a.path == b.path,
            _ => false,
        }
    }
}

impl Eq for ApiKey {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// A directory of its own per test, removed at the end.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let dir = std::env::temp_dir().join(format!("warden-gateway-apikey-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }

        fn path(&self) -> PathBuf {
            self.0.join("key")
        }

        /// Writes the file and sets its modification time to `secs` after the epoch, so a test never depends on the clock's grain.
        fn write(&self, content: &str, secs: u64) {
            std::fs::write(self.path(), content).unwrap();
            let file = std::fs::OpenOptions::new().write(true).open(self.path()).unwrap();
            file.set_modified(UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_file_is_read_and_loses_its_trailing_newline() {
        let dir = Dir::new("read");
        dir.write("sk-secret\n", 1000);
        let key = ApiKey::from_file(dir.path()).unwrap();
        assert_eq!(key.get().as_deref(), Some("sk-secret"));
    }

    #[test]
    fn a_missing_file_stops_the_start_and_names_the_path() {
        let dir = Dir::new("missing");
        let err = ApiKey::from_file(dir.path()).unwrap_err();
        assert!(err.contains(&dir.path().display().to_string()), "{err}");
    }

    #[test]
    fn an_empty_file_is_no_key() {
        let dir = Dir::new("empty");
        dir.write("  \n", 1000);
        assert_eq!(ApiKey::from_file(dir.path()).unwrap().get(), None);
    }

    #[test]
    fn a_changed_file_gives_the_new_key_without_a_restart() {
        let dir = Dir::new("rotate");
        dir.write("key-a", 1000);
        let key = ApiKey::from_file(dir.path()).unwrap();
        assert_eq!(key.get().as_deref(), Some("key-a"));
        dir.write("key-b\n", 2000);
        assert_eq!(key.get().as_deref(), Some("key-b"));
        assert_eq!(key.get().as_deref(), Some("key-b"), "and it stays");
    }

    #[test]
    fn a_file_that_is_emptied_or_deleted_keeps_the_previous_key() {
        let dir = Dir::new("keep");
        dir.write("key-a", 1000);
        let key = ApiKey::from_file(dir.path()).unwrap();
        dir.write("", 2000);
        assert_eq!(key.get().as_deref(), Some("key-a"), "emptied");
        std::fs::remove_file(dir.path()).unwrap();
        assert_eq!(key.get().as_deref(), Some("key-a"), "deleted");
        dir.write("key-c", 3000);
        assert_eq!(key.get().as_deref(), Some("key-c"), "and a new file is picked up again");
    }

    #[test]
    fn the_environment_value_is_fixed_and_none_has_no_key() {
        assert_eq!(ApiKey::from_env("K", Some("v".into())).get().as_deref(), Some("v"));
        assert_eq!(ApiKey::from_env("K", None).get(), None);
        assert_eq!(ApiKey::none().get(), None);
    }

    #[test]
    fn debug_and_origin_show_where_the_key_is_from_and_never_the_key() {
        let dir = Dir::new("debug");
        dir.write("sk-very-secret", 1000);
        let file = ApiKey::from_file(dir.path()).unwrap();
        let env = ApiKey::from_env("GEMINI_KEY", Some("sk-env-secret".into()));
        for key in [&file, &env] {
            let shown = format!("{key:?} {}", key.origin());
            assert!(!shown.contains("secret"), "{shown}");
        }
        assert_eq!(env.origin(), "env GEMINI_KEY");
        assert_eq!(file.origin(), format!("file {}", dir.path().display()));
        assert_eq!(ApiKey::none().origin(), "none");
    }
}

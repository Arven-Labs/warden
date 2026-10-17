//! The orchestrator's own record of its instances: `instances.json` in its state folder.
//!
//! It holds each instance's pairing key, so the file is written with mode 0600, and written whole to a temporary file and
//! renamed over the old one, so a crash never leaves it half written. One orchestrator process per state folder: the lock
//! here covers the threads of one process, not two processes (the `PairingStore` of the hub had that lesson, P151).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::instance::Instance;

/// The host ports instances are published on. A customer's port is the lowest one in this range no instance uses.
pub const PORT_RANGE: std::ops::RangeInclusive<u16> = 20000..=29999;

static WRITES: Mutex<()> = Mutex::new(());

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    instances: Vec<Instance>,
}

pub struct InstanceStore {
    path: PathBuf,
}

impl InstanceStore {
    /// The store inside `state_dir` (made when first written).
    pub fn new(state_dir: impl AsRef<Path>) -> Self {
        Self { path: state_dir.as_ref().join("instances.json") }
    }

    /// The folder everything of the orchestrator's lives in: the record, the backup key and the backups.
    pub fn state_dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    fn load(&self) -> anyhow::Result<File> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("{} is not a valid instance list", self.path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
            Err(err) => Err(err).with_context(|| format!("could not read {}", self.path.display())),
        }
    }

    fn save(&self, file: &File) -> anyhow::Result<()> {
        let dir = self.path.parent().context("the state file has no folder")?;
        std::fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
        let tmp = dir.join(format!("instances.json.tmp-{}", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut out = options.open(&tmp).with_context(|| format!("could not write {}", tmp.display()))?;
        out.write_all(serde_json::to_string_pretty(file)?.as_bytes())?;
        out.sync_all()?;
        std::fs::rename(&tmp, &self.path).with_context(|| format!("could not replace {}", self.path.display()))?;
        Ok(())
    }

    /// Every instance, oldest first.
    pub fn list(&self) -> anyhow::Result<Vec<Instance>> {
        Ok(self.load()?.instances)
    }

    pub fn get(&self, customer_id: &str) -> anyhow::Result<Option<Instance>> {
        Ok(self.load()?.instances.into_iter().find(|i| i.spec.customer_id == customer_id))
    }

    /// Reads, lets `change` alter the list, and writes it back, all under the lock. Nothing is written if `change` fails.
    pub fn update<T>(&self, change: impl FnOnce(&mut Vec<Instance>) -> anyhow::Result<T>) -> anyhow::Result<T> {
        let _guard = WRITES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut file = self.load()?;
        let out = change(&mut file.instances)?;
        self.save(&file)?;
        Ok(out)
    }
}

/// The lowest port of `PORT_RANGE` none of `instances` uses.
pub fn free_port(instances: &[Instance]) -> anyhow::Result<u16> {
    PORT_RANGE.clone().find(|port| !instances.iter().any(|i| i.port == *port)).context("every port of the range is in use")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::{InstanceSpec, State};

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("warden-orchestrator-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn instance(id: &str, port: u16) -> Instance {
        Instance {
            spec: InstanceSpec { customer_id: id.into(), image: "img".into(), memory_mb: None, cpus: None, config: None },
            state: State::Active,
            port,
            pairing_key: "k".repeat(64),
            created_at_ms: 1,
        }
    }

    #[test]
    fn an_empty_store_lists_nothing_and_a_saved_instance_comes_back() {
        let store = InstanceStore::new(dir("roundtrip"));
        assert!(store.list().unwrap().is_empty());
        assert_eq!(store.get("acme").unwrap(), None);
        store.update(|all| Ok(all.push(instance("acme", 20000)))).unwrap();
        assert_eq!(store.get("acme").unwrap(), Some(instance("acme", 20000)));
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn a_change_that_fails_writes_nothing() {
        let store = InstanceStore::new(dir("failed"));
        store.update(|all| Ok(all.push(instance("acme", 20000)))).unwrap();
        let result: anyhow::Result<()> = store.update(|all| {
            all.clear();
            anyhow::bail!("changed my mind")
        });
        assert!(result.is_err());
        assert_eq!(store.list().unwrap().len(), 1, "the failed change was dropped");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_with_the_keys_is_private_and_no_temporary_file_is_left() {
        use std::os::unix::fs::PermissionsExt;
        let state = dir("private");
        let store = InstanceStore::new(&state);
        store.update(|all| Ok(all.push(instance("acme", 20000)))).unwrap();
        let mode = std::fs::metadata(state.join("instances.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let names: Vec<_> = std::fs::read_dir(&state).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["instances.json"]);
    }

    #[test]
    fn the_port_is_the_lowest_one_nobody_uses() {
        assert_eq!(free_port(&[]).unwrap(), 20000);
        assert_eq!(free_port(&[instance("a", 20000), instance("b", 20002)]).unwrap(), 20001);
    }
}

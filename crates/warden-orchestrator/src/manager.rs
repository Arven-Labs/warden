//! Provision, suspend, resume and delete, idempotent by customer id (the contract of `HOSPEDAGEM.md` B1).
//!
//! The store says what should be; the runtime says what is. Every call that can find the two apart brings the container to
//! what the store says (`provision` and `resume` recreate one that has vanished).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context};

use crate::backup::{self, BackupKey};
use crate::instance::{validate_customer_id, volume_name, Instance, InstanceSpec, State};
use crate::runtime::ContainerRuntime;
use crate::store::{free_port, InstanceStore};

pub struct InstanceManager<R: ContainerRuntime> {
    store: InstanceStore,
    runtime: R,
}

impl<R: ContainerRuntime> InstanceManager<R> {
    pub fn new(store: InstanceStore, runtime: R) -> Self {
        Self { store, runtime }
    }

    pub fn runtime(&self) -> &R {
        &self.runtime
    }

    pub fn list(&self) -> anyhow::Result<Vec<Instance>> {
        self.store.list()
    }

    pub fn get(&self, customer_id: &str) -> anyhow::Result<Option<Instance>> {
        self.store.get(customer_id)
    }

    /// Creates the customer's instance, or confirms it. The same spec again changes nothing (and recreates a container
    /// that went missing); a different spec for an existing customer is refused, never applied silently. An instance that
    /// is suspended stays suspended.
    pub async fn provision(&self, spec: InstanceSpec) -> anyhow::Result<Instance> {
        validate_customer_id(&spec.customer_id)?;
        anyhow::ensure!(!spec.image.trim().is_empty(), "the image is empty");
        let candidate = spec.clone();
        // The record goes in first, so two calls for the same customer never both create a container.
        let (instance, created) = self.store.update(|all| {
            if let Some(existing) = all.iter().find(|i| i.spec.customer_id == candidate.customer_id) {
                anyhow::ensure!(
                    existing.spec == candidate,
                    "'{}' already exists with a different spec (differs in: {}); delete it first, or ask for the same spec",
                    candidate.customer_id,
                    existing.spec.differs_from(&candidate).join(", ")
                );
                return Ok((existing.clone(), false));
            }
            let instance = Instance { port: free_port(all)?, spec: candidate, state: State::Active, pairing_key: new_key()?, created_at_ms: now_ms() };
            all.push(instance.clone());
            Ok((instance, true))
        })?;
        if created {
            if let Err(err) = self.create(&instance).await {
                // Nothing runs, so nothing should be remembered: asking again starts clean.
                self.store.update(|all| Ok(all.retain(|i| i.spec.customer_id != instance.spec.customer_id)))?;
                return Err(err).with_context(|| format!("could not start the instance of '{}'", instance.spec.customer_id));
            }
        } else if instance.state == State::Active {
            self.reconcile(&instance).await?;
        }
        Ok(instance)
    }

    /// Stops the instance and keeps its data. Suspending one already suspended is a no-op.
    pub async fn suspend(&self, customer_id: &str) -> anyhow::Result<Instance> {
        let instance = self.require(customer_id)?;
        if instance.state == State::Suspended {
            return Ok(instance);
        }
        self.runtime.stop(&instance.container_name()).await?;
        self.set_state(customer_id, State::Suspended)
    }

    /// Starts a suspended instance again, on the same data. Resuming one that is running is a no-op.
    pub async fn resume(&self, customer_id: &str) -> anyhow::Result<Instance> {
        let instance = self.require(customer_id)?;
        let instance = self.set_state(customer_id, State::Active).map(|_| Instance { state: State::Active, ..instance })?;
        self.reconcile(&instance).await?;
        Ok(instance)
    }

    /// Removes the instance: the container and the record. The volume, with the customer's data, stays unless
    /// `purge_data` (how long to keep it is still open, `HOSPEDAGEM.md` question 4). Deleting what is not there is not an
    /// error; the answer says whether there was an instance. A purge still removes the volume of a customer already
    /// deleted without it: that is how the data is erased once the time to keep it is over.
    pub async fn delete(&self, customer_id: &str, purge_data: bool) -> anyhow::Result<bool> {
        let Some(instance) = self.store.get(customer_id)? else {
            if purge_data {
                validate_customer_id(customer_id)?;
                self.runtime.remove_volume(&volume_name(customer_id)).await?;
            }
            return Ok(false);
        };
        self.runtime.remove(&instance.container_name()).await?;
        if purge_data {
            self.runtime.remove_volume(&instance.volume_name()).await?;
        }
        self.store.update(|all| Ok(all.retain(|i| i.spec.customer_id != customer_id)))?;
        Ok(true)
    }

    /// Restarts the container (SIGTERM, a grace, then up again) on the same volume.
    pub async fn restart(&self, instance: &Instance) -> anyhow::Result<()> {
        self.runtime.restart(&instance.container_name()).await
    }

    /// The customer's backups, oldest first.
    pub fn backups(&self, customer_id: &str) -> anyhow::Result<Vec<PathBuf>> {
        validate_customer_id(customer_id)?;
        backup::list(self.store.state_dir(), customer_id)
    }

    /// Takes a backup of the customer's volume: the instance is stopped (SIGTERM, so the hub closes its files), the volume
    /// archived and encrypted into `backups/<customer>/<time>.wbk`, and the instance started again, whatever happened in
    /// between. One that was suspended is left suspended. Afterwards all but the `keep` newest backups go.
    pub async fn backup(&self, customer_id: &str, keep: usize) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(keep >= 1, "keeping no backups at all would delete the one just taken");
        let instance = self.require(customer_id)?;
        let key = BackupKey::load_or_create(self.store.state_dir())?;
        let name = instance.container_name();
        let was_running = self.runtime.inspect(&name).await?.is_some_and(|info| info.running);
        if was_running {
            self.runtime.stop(&name).await.with_context(|| format!("could not stop '{customer_id}' to back it up"))?;
        }
        let taken = self.write_backup(&instance, &key).await;
        let restarted = if was_running { self.runtime.start(&name).await } else { Ok(()) };
        let path = match (taken, restarted) {
            (Ok(path), Ok(())) => path,
            (Err(err), Ok(())) => return Err(err),
            (Ok(path), Err(err)) => bail!("the backup is at {}, but '{customer_id}' did not start again: {err:#}", path.display()),
            (Err(err), Err(restart)) => return Err(err.context(format!("and '{customer_id}' did not start again either: {restart:#}"))),
        };
        backup::prune(self.store.state_dir(), customer_id, keep)?;
        Ok(path)
    }

    /// A backup of every active instance, one after the other; one failing does not stop the others.
    pub async fn backup_all(&self, keep: usize) -> anyhow::Result<Vec<(String, anyhow::Result<PathBuf>)>> {
        let mut results = Vec::new();
        for instance in self.store.list()?.into_iter().filter(|i| i.state == State::Active) {
            let customer = instance.spec.customer_id.clone();
            results.push((customer.clone(), self.backup(&customer, keep).await));
        }
        Ok(results)
    }

    /// Puts a backup (`which`: a file, or `latest`) in the customer's volume, which must not exist yet: replacing a
    /// customer's data is `delete --purge-data` first, on purpose. The image is the instance's, or `image`. On a failure the
    /// half-filled volume is removed. Returns the file it restored from.
    pub async fn restore(&self, customer_id: &str, which: &str, image: Option<&str>) -> anyhow::Result<PathBuf> {
        validate_customer_id(customer_id)?;
        let file = backup::resolve(self.store.state_dir(), customer_id, which)?;
        let image = self.image_for(customer_id, image)?;
        let key = BackupKey::load(self.store.state_dir())?;
        let volume = volume_name(customer_id);
        anyhow::ensure!(
            !self.runtime.volume_exists(&volume).await?,
            "the volume {volume} already exists; to replace it, delete the instance's data first (`delete {customer_id} --purge-data`)"
        );
        if let Err(err) = self.restore_into(&volume, &image, &file, &key).await {
            let _ = self.runtime.remove_volume(&volume).await;
            return Err(err).with_context(|| format!("could not restore {}", file.display()));
        }
        Ok(file)
    }

    /// Proves a backup is good: restores it into a scratch volume, starts a hub on it with no network and no tasks, waits up
    /// to `timeout` for it to answer `/health`, and removes the scratch container and volume whatever the outcome. A hub
    /// that starts is the proof that the config and the data came back whole. Returns the file it checked.
    pub async fn verify(&self, customer_id: &str, which: &str, image: Option<&str>, timeout: Duration) -> anyhow::Result<PathBuf> {
        validate_customer_id(customer_id)?;
        let file = backup::resolve(self.store.state_dir(), customer_id, which)?;
        let image = self.image_for(customer_id, image)?;
        let key = BackupKey::load(self.store.state_dir())?;
        let existing = self.store.get(customer_id)?;
        let suffix: String = backup::random_bytes::<4>()?.iter().map(|b| format!("{b:02x}")).collect();
        let scratch = Instance {
            spec: InstanceSpec {
                customer_id: format!("verify-{suffix}"),
                image,
                memory_mb: existing.as_ref().and_then(|i| i.spec.memory_mb),
                cpus: existing.as_ref().and_then(|i| i.spec.cpus),
                config: None,
            },
            state: State::Active,
            port: 0,
            pairing_key: new_key()?,
            created_at_ms: now_ms(),
        };
        let checked = async {
            self.restore_into(&scratch.volume_name(), &scratch.spec.image, &file, &key).await?;
            self.runtime.run_isolated(&scratch).await?;
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                if self.runtime.is_healthy(&scratch.container_name()).await? {
                    return Ok(());
                }
                if tokio::time::Instant::now() >= deadline {
                    bail!("the hub started from {} did not answer /health within {}s", file.display(), timeout.as_secs());
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        .await;
        let gone = self.runtime.remove(&scratch.container_name()).await;
        let gone_volume = self.runtime.remove_volume(&scratch.volume_name()).await;
        checked?;
        gone.and(gone_volume).with_context(|| format!("the backup is good, but the scratch '{}' was not cleaned up", scratch.spec.customer_id))?;
        Ok(file)
    }

    fn image_for(&self, customer_id: &str, image: Option<&str>) -> anyhow::Result<String> {
        match (image, self.store.get(customer_id)?) {
            (Some(image), _) => Ok(image.to_string()),
            (None, Some(instance)) => Ok(instance.spec.image),
            (None, None) => bail!("'{customer_id}' is not an instance (any more): say which image to use with --image"),
        }
    }

    /// Archives and encrypts the volume into a new file of the customer's backups folder (written under a temporary name and
    /// renamed, so a backup that is there is whole).
    async fn write_backup(&self, instance: &Instance, key: &BackupKey) -> anyhow::Result<PathBuf> {
        let dir = backup::backups_dir(self.store.state_dir(), &instance.spec.customer_id);
        std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
        let path = backup::new_backup_path(&dir, now_ms());
        let tmp = path.with_extension("wbk.tmp");
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&tmp).await.with_context(|| format!("could not write {}", tmp.display()))?;

        let (mut sink, source) = tokio::io::duplex(backup::PIPE_BYTES);
        let archive = async {
            let archived = self.runtime.archive_volume(&instance.volume_name(), &instance.spec.image, &mut sink).await;
            drop(sink); // the end of the archive is what lets the encrypting side finish
            archived
        };
        let (archived, encrypted) = tokio::join!(archive, backup::encrypt_stream(key, source, &mut file));
        // A failed encryption is the root cause (the archiver then only sees a pipe that closed); an archiver that failed
        // leaves the encrypting side a clean, short end of input, so its own error is the one to report.
        let outcome = encrypted.and(archived);
        let outcome = match outcome {
            Ok(()) => file.sync_all().await.map_err(anyhow::Error::from),
            Err(err) => Err(err),
        };
        match outcome {
            Ok(()) => {
                std::fs::rename(&tmp, &path).with_context(|| format!("could not move the backup to {}", path.display()))?;
                Ok(path)
            }
            Err(err) => {
                let _ = std::fs::remove_file(&tmp);
                Err(err).with_context(|| format!("could not back up '{}'", instance.spec.customer_id))
            }
        }
    }

    /// Decrypts `file` and unpacks it into `volume`.
    async fn restore_into(&self, volume: &str, image: &str, file: &Path, key: &BackupKey) -> anyhow::Result<()> {
        let input = tokio::fs::File::open(file).await.with_context(|| format!("could not open {}", file.display()))?;
        let (sink, source) = tokio::io::duplex(backup::PIPE_BYTES);
        // The unpacker owns its end of the pipe: if it stops, the decrypting side finds the pipe closed instead of waiting.
        let extract = async move {
            let mut source = source;
            self.runtime.extract_volume(volume, image, &mut source).await
        };
        let (decrypted, extracted) = tokio::join!(backup::decrypt_stream(key, input, sink), extract);
        match (decrypted, extracted) {
            (Ok(()), Ok(())) => Ok(()),
            // The unpacker that stopped early makes the decrypting side fail with a broken pipe: that one is not the cause.
            (Err(decrypt), Err(extract)) if is_broken_pipe(&decrypt) => Err(extract),
            (Err(decrypt), _) => Err(decrypt),
            (Ok(()), Err(extract)) => Err(extract),
        }
    }

    /// Brings the container to "running" for an instance that should be: creates it if it is gone, starts it if stopped.
    async fn reconcile(&self, instance: &Instance) -> anyhow::Result<()> {
        let name = instance.container_name();
        match self.runtime.inspect(&name).await? {
            None => self.create(instance).await,
            Some(info) if !info.running => self.runtime.start(&name).await,
            Some(_) => Ok(()),
        }
    }

    /// A new container: the starting config goes into the volume first (not over one already there), because a hub with no
    /// model provider refuses to start.
    async fn create(&self, instance: &Instance) -> anyhow::Result<()> {
        self.runtime.seed_config(instance).await?;
        self.runtime.run(instance).await
    }

    fn require(&self, customer_id: &str) -> anyhow::Result<Instance> {
        match self.store.get(customer_id)? {
            Some(instance) => Ok(instance),
            None => bail!("there is no instance for '{customer_id}'"),
        }
    }

    fn set_state(&self, customer_id: &str, state: State) -> anyhow::Result<Instance> {
        self.store.update(|all| {
            let instance = all.iter_mut().find(|i| i.spec.customer_id == customer_id).with_context(|| format!("there is no instance for '{customer_id}'"))?;
            instance.state = state;
            Ok(instance.clone())
        })
    }
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 32 random bytes from the system, in hex: 64 characters, over the hub's 32-character minimum for a pairing key.
fn new_key() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).context("could not read /dev/urandom")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeRuntime;

    fn manager(name: &str) -> InstanceManager<FakeRuntime> {
        let dir = std::env::temp_dir().join(format!("warden-orchestrator-manager-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        InstanceManager::new(InstanceStore::new(dir), FakeRuntime::default())
    }

    fn spec(id: &str) -> InstanceSpec {
        InstanceSpec { customer_id: id.into(), image: "img:1".into(), memory_mb: None, cpus: None, config: None }
    }

    #[tokio::test]
    async fn the_starting_config_goes_into_the_volume_before_the_container_and_again_for_a_recreated_one() {
        let m = manager("seed");
        let with_config = InstanceSpec { config: Some("active_provider = \"main\"".into()), ..spec("acme") };
        m.provision(with_config.clone()).await.unwrap();
        assert_eq!(m.runtime().seeded(), ["warden-data-acme"]);

        m.provision(with_config.clone()).await.unwrap();
        assert_eq!(m.runtime().seeded().len(), 1, "a container that is there is not seeded again");

        m.runtime().lose_container("warden-acme");
        m.provision(with_config).await.unwrap();
        assert_eq!(m.runtime().seeded().len(), 2, "a recreated one is (the runtime leaves a config already in the volume alone)");

        m.provision(spec("plain")).await.unwrap();
        assert_eq!(m.runtime().seeded().len(), 2, "no config in the spec, nothing to seed");
    }

    #[tokio::test]
    async fn a_refused_spec_names_what_differs_and_does_not_print_the_config() {
        let m = manager("refused");
        m.provision(InstanceSpec { config: Some("token = \"s3cret\"".into()), ..spec("acme") }).await.unwrap();
        let err = m.provision(InstanceSpec { config: Some("token = \"other\"".into()), ..spec("acme") }).await.unwrap_err().to_string();
        assert!(err.contains("differs in: config"), "{err}");
        assert!(!err.contains("s3cret") && !err.contains("other"), "{err}");
    }

    #[tokio::test]
    async fn provisioning_makes_a_running_container_with_its_own_key_and_port() {
        let m = manager("provision");
        let acme = m.provision(spec("acme")).await.unwrap();
        let beta = m.provision(spec("beta")).await.unwrap();
        assert_eq!((acme.port, beta.port), (20000, 20001));
        assert_eq!(acme.pairing_key.len(), 64);
        assert_ne!(acme.pairing_key, beta.pairing_key);
        assert!(m.runtime().container("warden-acme").unwrap().running);
        assert!(m.runtime().has_volume("warden-data-acme"));
        assert_eq!(m.list().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn provisioning_twice_with_the_same_spec_changes_nothing() {
        let m = manager("idempotent");
        let first = m.provision(spec("acme")).await.unwrap();
        let again = m.provision(spec("acme")).await.unwrap();
        assert_eq!(first, again, "same key, same port");
        assert_eq!(m.runtime().container("warden-acme").unwrap().starts, 1, "no second container, no restart");
        assert_eq!(m.list().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_different_spec_for_an_existing_customer_is_refused() {
        let m = manager("different");
        m.provision(spec("acme")).await.unwrap();
        let other = InstanceSpec { image: "img:2".into(), ..spec("acme") };
        let err = m.provision(other).await.unwrap_err().to_string();
        assert!(err.contains("different spec"), "{err}");
        assert_eq!(m.get("acme").unwrap().unwrap().spec.image, "img:1", "the old one is untouched");
    }

    #[tokio::test]
    async fn a_container_that_vanished_is_recreated_and_a_stopped_one_started() {
        let m = manager("reconcile");
        m.provision(spec("acme")).await.unwrap();
        m.runtime().lose_container("warden-acme");
        m.provision(spec("acme")).await.unwrap();
        assert!(m.runtime().container("warden-acme").unwrap().running, "recreated");

        m.runtime().stop("warden-acme").await.unwrap();
        m.provision(spec("acme")).await.unwrap();
        assert!(m.runtime().container("warden-acme").unwrap().running, "started again");
    }

    #[tokio::test]
    async fn a_runtime_that_fails_leaves_no_record_behind() {
        let m = manager("rollback");
        m.runtime().break_container("warden-acme");
        assert!(m.provision(spec("acme")).await.is_err());
        assert!(m.list().unwrap().is_empty(), "asking again starts clean");
        assert!(m.provision(spec("other")).await.is_ok(), "and the next customer is not affected");
    }

    #[tokio::test]
    async fn a_bad_customer_id_or_an_empty_image_is_refused_before_anything_runs() {
        let m = manager("invalid");
        assert!(m.provision(spec("Not Valid")).await.is_err());
        assert!(m.provision(InstanceSpec { image: "  ".into(), ..spec("acme") }).await.is_err());
        assert!(m.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn suspending_stops_the_container_keeps_the_data_and_resuming_brings_it_back() {
        let m = manager("suspend");
        m.provision(spec("acme")).await.unwrap();
        let suspended = m.suspend("acme").await.unwrap();
        assert_eq!(suspended.state, State::Suspended);
        assert!(!m.runtime().container("warden-acme").unwrap().running);
        assert!(m.runtime().has_volume("warden-data-acme"));
        assert_eq!(m.suspend("acme").await.unwrap().state, State::Suspended, "again is a no-op");

        // Asking to provision a suspended customer does not wake it up.
        m.provision(spec("acme")).await.unwrap();
        assert!(!m.runtime().container("warden-acme").unwrap().running);

        let resumed = m.resume("acme").await.unwrap();
        assert_eq!(resumed.state, State::Active);
        assert_eq!(resumed.pairing_key, suspended.pairing_key, "same instance");
        assert!(m.runtime().container("warden-acme").unwrap().running);
        assert_eq!(m.get("acme").unwrap().unwrap().state, State::Active);
        m.resume("acme").await.unwrap();
        assert_eq!(m.runtime().container("warden-acme").unwrap().starts, 2, "resuming a running one starts nothing");
    }

    #[tokio::test]
    async fn suspending_or_resuming_an_unknown_customer_is_an_error() {
        let m = manager("unknown");
        assert!(m.suspend("ghost").await.is_err());
        assert!(m.resume("ghost").await.is_err());
    }

    #[tokio::test]
    async fn deleting_keeps_the_volume_unless_asked_and_deleting_nothing_is_fine() {
        let m = manager("delete");
        m.provision(spec("acme")).await.unwrap();
        m.provision(spec("beta")).await.unwrap();

        assert!(m.delete("acme", false).await.unwrap());
        assert!(m.runtime().container("warden-acme").is_none());
        assert!(m.runtime().has_volume("warden-data-acme"), "the data stays");
        assert!(m.get("acme").unwrap().is_none());

        assert!(m.delete("beta", true).await.unwrap());
        assert!(!m.runtime().has_volume("warden-data-beta"), "purged");

        assert!(!m.delete("acme", false).await.unwrap(), "already gone");
        assert!(m.runtime().has_volume("warden-data-acme"), "and deleting again without a purge leaves the data");
        assert!(!m.delete("acme", true).await.unwrap(), "no instance to delete");
        assert!(!m.runtime().has_volume("warden-data-acme"), "but a purge afterwards erases the data kept earlier");
        assert!(m.delete("Not Valid", true).await.is_err(), "a purge of something that is not a customer id is refused");
        // The port of a deleted instance is free again.
        assert_eq!(m.provision(spec("gamma")).await.unwrap().port, 20000);
    }

    const DATA: &[u8] = b"the customer's notes, the device registry and the ledger";

    /// An instance with something in its volume.
    async fn with_data(name: &str, customer: &str) -> InstanceManager<FakeRuntime> {
        let m = manager(name);
        m.provision(spec(customer)).await.unwrap();
        m.runtime().put_data(&volume_name(customer), DATA);
        m
    }

    #[tokio::test]
    async fn a_backup_stops_the_hub_for_the_copy_and_starts_it_again() {
        let m = with_data("backup-stops", "acme").await;
        let path = m.backup("acme", 7).await.unwrap();

        assert_eq!(m.runtime().archived_running(), [("warden-data-acme".to_string(), false)], "the hub was stopped while its files were read");
        assert!(m.runtime().container("warden-acme").unwrap().running, "and is running again");
        assert_eq!(m.runtime().container("warden-acme").unwrap().starts, 2);
        assert_eq!(m.backups("acme").unwrap(), [path.clone()]);
        let sealed = std::fs::read(&path).unwrap();
        assert!(!sealed.windows(DATA.len()).any(|w| w == DATA), "the file does not hold the data in the clear");
    }

    #[tokio::test]
    async fn a_backup_that_fails_still_starts_the_hub_again_and_leaves_no_file() {
        let m = with_data("backup-fails", "acme").await;
        m.runtime().fail_archiving("warden-data-acme");
        let err = format!("{:#}", m.backup("acme", 7).await.unwrap_err());
        assert!(err.contains("could not back up 'acme'"), "{err}");
        assert!(m.runtime().container("warden-acme").unwrap().running, "the customer is not left without a hub");
        assert!(m.backups("acme").unwrap().is_empty());
        let dir = backup::backups_dir(m.store.state_dir(), "acme");
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert!(left.is_empty(), "no half-written file stays: {left:?}");
    }

    #[tokio::test]
    async fn a_suspended_instance_is_backed_up_without_being_started() {
        let m = with_data("backup-suspended", "acme").await;
        m.suspend("acme").await.unwrap();
        m.backup("acme", 7).await.unwrap();
        assert!(!m.runtime().container("warden-acme").unwrap().running, "suspended stays suspended");
        assert_eq!(m.runtime().archived_running(), [("warden-data-acme".to_string(), false)]);
    }

    #[tokio::test]
    async fn only_the_newest_backups_are_kept() {
        let m = with_data("backup-prune", "acme").await;
        let mut made = Vec::new();
        for _ in 0..4 {
            made.push(m.backup("acme", 2).await.unwrap());
        }
        assert_eq!(m.backups("acme").unwrap(), made[2..], "the two newest");
        assert!(m.backup("acme", 0).await.is_err(), "keeping none would delete the backup just taken");
        assert!(m.backup("ghost", 2).await.is_err());
    }

    #[tokio::test]
    async fn every_active_instance_is_backed_up_and_one_failing_does_not_stop_the_others() {
        let m = with_data("backup-all", "acme").await;
        m.provision(spec("beta")).await.unwrap();
        m.runtime().put_data("warden-data-beta", b"beta");
        m.provision(spec("gamma")).await.unwrap();
        m.runtime().put_data("warden-data-gamma", b"gamma");
        m.suspend("gamma").await.unwrap();
        m.runtime().fail_archiving("warden-data-acme");

        let results = m.backup_all(7).await.unwrap();
        let outcome: Vec<_> = results.iter().map(|(customer, result)| (customer.as_str(), result.is_ok())).collect();
        assert_eq!(outcome, [("acme", false), ("beta", true)], "the suspended one is left out");
    }

    #[tokio::test]
    async fn a_restore_puts_the_data_back_in_a_volume_that_is_not_there() {
        let m = with_data("restore", "acme").await;
        m.backup("acme", 7).await.unwrap();

        let err = m.restore("acme", "latest", None).await.unwrap_err().to_string();
        assert!(err.contains("already exists") && err.contains("--purge-data"), "{err}");

        m.delete("acme", true).await.unwrap();
        assert!(!m.runtime().has_volume("warden-data-acme"));
        let file = m.restore("acme", "latest", Some("img:1")).await.unwrap();
        assert_eq!(file, m.backups("acme").unwrap().pop().unwrap());
        assert_eq!(m.runtime().data("warden-data-acme").as_deref(), Some(DATA));
    }

    #[tokio::test]
    async fn a_restore_of_a_damaged_backup_leaves_no_volume_behind() {
        let m = with_data("restore-damaged", "acme").await;
        let path = m.backup("acme", 7).await.unwrap();
        let mut sealed = std::fs::read(&path).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        std::fs::write(&path, sealed).unwrap();
        m.delete("acme", true).await.unwrap();

        let err = format!("{:#}", m.restore("acme", "latest", Some("img:1")).await.unwrap_err());
        assert!(err.contains("does not open"), "{err}");
        assert!(!m.runtime().has_volume("warden-data-acme"), "nothing half-restored stays");
    }

    #[tokio::test]
    async fn a_restore_needs_to_know_the_image_when_there_is_no_instance() {
        let m = with_data("restore-image", "acme").await;
        m.backup("acme", 7).await.unwrap();
        m.delete("acme", true).await.unwrap();
        let err = m.restore("acme", "latest", None).await.unwrap_err().to_string();
        assert!(err.contains("--image"), "{err}");
        assert!(m.restore("ghost", "latest", Some("img:1")).await.is_err(), "no backups of a customer that never had any");
    }

    #[tokio::test]
    async fn a_restore_without_the_key_that_made_the_backup_says_so() {
        let m = with_data("restore-no-key", "acme").await;
        m.backup("acme", 7).await.unwrap();
        m.delete("acme", true).await.unwrap();
        std::fs::remove_file(m.store.state_dir().join("backup.key")).unwrap();
        let err = m.restore("acme", "latest", Some("img:1")).await.unwrap_err().to_string();
        assert!(err.contains("backup.key"), "{err}");
        assert!(!m.store.state_dir().join("backup.key").exists(), "a restore never makes a new key");
    }

    #[tokio::test]
    async fn verifying_starts_an_isolated_hub_on_the_restored_data_and_cleans_up_after() {
        let m = with_data("verify", "acme").await;
        let file = m.backup("acme", 7).await.unwrap();
        let before = (m.runtime().volumes(), m.runtime().containers());

        assert_eq!(m.verify("acme", "latest", None, Duration::from_secs(1)).await.unwrap(), file);
        let started = m.runtime().isolated_runs();
        assert_eq!(started.len(), 1);
        assert!(started[0].starts_with("warden-verify-"), "{started:?}");
        assert_eq!((m.runtime().volumes(), m.runtime().containers()), before, "the scratch container and volume are gone");
        assert_eq!(m.runtime().data("warden-data-acme").as_deref(), Some(DATA), "the customer's own volume was not touched");
    }

    #[tokio::test]
    async fn a_backup_whose_hub_does_not_come_up_fails_the_check_and_still_cleans_up() {
        let m = with_data("verify-unhealthy", "acme").await;
        m.backup("acme", 7).await.unwrap();
        m.runtime().make_unhealthy();
        let before = (m.runtime().volumes(), m.runtime().containers());

        let err = m.verify("acme", "latest", None, Duration::ZERO).await.unwrap_err().to_string();
        assert!(err.contains("did not answer /health"), "{err}");
        assert_eq!((m.runtime().volumes(), m.runtime().containers()), before);
    }
}

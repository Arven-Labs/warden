//! Provision, suspend, resume and delete, idempotent by customer id (the contract of `HOSPEDAGEM.md` B1).
//!
//! The store says what should be; the runtime says what is. Every call that can find the two apart brings the container to
//! what the store says (`provision` and `resume` recreate one that has vanished).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};

use crate::backup::{self, BackupKey};
use crate::instance::{validate_customer_id, volume_name, Instance, InstanceSpec, State};
use crate::runtime::ContainerRuntime;
use crate::ship::BackupSink;
use crate::store::{free_port, InstanceStore};
use crate::upgrade::{UpgradeOptions, UpgradeOutcome};

pub struct InstanceManager<R: ContainerRuntime> {
    store: InstanceStore,
    runtime: R,
    /// Where a finished backup goes next (off the host); `None`: nowhere, it stays where it was made.
    sink: Option<Arc<dyn BackupSink>>,
}

impl<R: ContainerRuntime> InstanceManager<R> {
    pub fn new(store: InstanceStore, runtime: R) -> Self {
        Self { store, runtime, sink: None }
    }

    /// Hands every backup that `backup` takes to `sink` too (see `ship`).
    pub fn with_sink(mut self, sink: Arc<dyn BackupSink>) -> Self {
        self.sink = Some(sink);
        self
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
            let instance = Instance { port: free_port(all)?, spec: candidate, state: State::Active, pairing_key: new_key()?, created_at_ms: now_ms(), delete_after_ms: None };
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

    /// Starts a suspended instance again, on the same data. Resuming one that is running is a no-op. It also cancels a
    /// delete that was scheduled and is not due yet.
    pub async fn resume(&self, customer_id: &str) -> anyhow::Result<Instance> {
        self.require(customer_id)?;
        let instance = self.set_state(customer_id, State::Active)?;
        self.reconcile(&instance).await?;
        Ok(instance)
    }

    /// Asks for the instance to be deleted `retention` from now: the container stops, and the record and the data stay until
    /// then (`purge_expired` erases them). Asking again keeps the first deadline, so repeating the request never extends or
    /// shortens the time the data is kept.
    pub async fn schedule_delete(&self, customer_id: &str, retention: Duration) -> anyhow::Result<Instance> {
        let instance = self.require(customer_id)?;
        if instance.state == State::Active {
            self.runtime.stop(&instance.container_name()).await?;
        }
        let due = now_ms().saturating_add(u64::try_from(retention.as_millis()).unwrap_or(u64::MAX));
        self.store.update(|all| {
            let instance = all.iter_mut().find(|i| i.spec.customer_id == customer_id).with_context(|| format!("there is no instance for '{customer_id}'"))?;
            instance.state = State::Suspended;
            instance.delete_after_ms.get_or_insert(due);
            Ok(instance.clone())
        })
    }

    /// Takes back a scheduled delete. The instance stays suspended: `resume` wakes it. Without a pending delete it is a no-op.
    pub fn cancel_delete(&self, customer_id: &str) -> anyhow::Result<Instance> {
        self.store.update(|all| {
            let instance = all.iter_mut().find(|i| i.spec.customer_id == customer_id).with_context(|| format!("there is no instance for '{customer_id}'"))?;
            instance.delete_after_ms = None;
            Ok(instance.clone())
        })
    }

    /// The instances whose deadline is `now_ms` or earlier: what `purge_expired` would erase, without erasing anything.
    pub fn expired(&self, now_ms: u64) -> anyhow::Result<Vec<Instance>> {
        Ok(self.store.list()?.into_iter().filter(|i| i.delete_after_ms.is_some_and(|at| at <= now_ms)).collect())
    }

    /// Erases, data included, every instance whose deadline is `now_ms` or earlier. One that fails is reported and stays
    /// recorded, so the next pass tries it again; it doesn't stop the others. `now_ms` is a parameter so the deadline is
    /// testable to the millisecond.
    pub async fn purge_expired(&self, now_ms: u64) -> anyhow::Result<Vec<(String, anyhow::Result<()>)>> {
        let mut out = Vec::new();
        for customer_id in self.expired(now_ms)?.into_iter().map(|i| i.spec.customer_id) {
            let result = self.delete(&customer_id, true).await.map(|_| ());
            out.push((customer_id, result));
        }
        Ok(out)
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

    /// A backup on the host and, if a sink is set (`with_sink`), off it: the file `backup_local` makes is handed to the sink.
    /// A failure to hand it over is an error that names the file: the backup is there on the host, just not safe from the host.
    pub async fn backup(&self, customer_id: &str, keep: usize) -> anyhow::Result<PathBuf> {
        let path = self.backup_local(customer_id, keep).await?;
        self.ship(customer_id, &path).await?;
        Ok(path)
    }

    /// Hands a backup file to the sink, if there is one.
    async fn ship(&self, customer_id: &str, path: &Path) -> anyhow::Result<()> {
        let Some(sink) = &self.sink else {
            return Ok(());
        };
        sink.ship(customer_id, path).await.map_err(|err| anyhow::anyhow!("the backup is at {}, but it could not be shipped off the host: {err:#}", path.display()))
    }

    /// Takes a backup of the customer's volume: the instance is stopped (SIGTERM, so the hub closes its files), the volume
    /// archived and encrypted into `backups/<customer>/<time>.wbk`, and the instance started again, whatever happened in
    /// between. One that was suspended is left suspended. Afterwards all but the `keep` newest backups go.
    pub async fn backup_local(&self, customer_id: &str, keep: usize) -> anyhow::Result<PathBuf> {
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
            delete_after_ms: None,
        };
        let checked = async {
            self.restore_into(&scratch.volume_name(), &scratch.spec.image, &file, &key).await?;
            self.runtime.run_isolated(&scratch).await?;
            self.wait_healthy(&scratch.container_name(), timeout, &format!("the hub started from {}", file.display())).await
        }
        .await;
        let gone = self.runtime.remove(&scratch.container_name()).await;
        let gone_volume = self.runtime.remove_volume(&scratch.volume_name()).await;
        checked?;
        gone.and(gone_volume).with_context(|| format!("the backup is good, but the scratch '{}' was not cleaned up", scratch.spec.customer_id))?;
        Ok(file)
    }

    /// Waits up to `timeout` for the hub in the container to answer `/health` (asked from inside it); `what` names it in the
    /// error.
    async fn wait_healthy(&self, container: &str, timeout: Duration, what: &str) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.runtime.is_healthy(container).await? {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("{what} did not answer /health within {}s", timeout.as_secs());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Moves every instance that is not on `image` to it: the first `canary` alone, then waves of `wave_size` (each wave's
    /// instances at the same time). The first one that has to be rolled back (or is down) stops the waves that have not
    /// begun: they come back as `Skipped`. Instances already on the image are left out of the report.
    pub async fn upgrade(&self, image: &str, options: &UpgradeOptions) -> anyhow::Result<Vec<(String, UpgradeOutcome)>> {
        anyhow::ensure!(!image.trim().is_empty(), "the image is empty");
        anyhow::ensure!(options.canary >= 1 && options.wave_size >= 1, "the canary and the waves need at least one instance each");
        let todo: Vec<String> = self.store.list()?.into_iter().filter(|i| i.spec.image != image).map(|i| i.spec.customer_id).collect();
        let (canary, rest) = todo.split_at(options.canary.min(todo.len()));
        let waves = std::iter::once(canary).chain(rest.chunks(options.wave_size)).filter(|wave| !wave.is_empty());

        let mut report = Vec::new();
        let mut halted = false;
        for wave in waves {
            if halted {
                report.extend(wave.iter().map(|customer| (customer.clone(), UpgradeOutcome::Skipped)));
                continue;
            }
            let outcomes = futures_util::future::join_all(wave.iter().map(|customer| self.upgrade_one(customer, image, options))).await;
            for (customer, outcome) in wave.iter().zip(outcomes) {
                halted |= !outcome.is_fine();
                report.push((customer.clone(), outcome));
            }
        }
        Ok(report)
    }

    /// Moves one instance to `image` (see the module `upgrade`). Never an `Err`: whatever went wrong is the outcome.
    pub async fn upgrade_one(&self, customer_id: &str, image: &str, options: &UpgradeOptions) -> UpgradeOutcome {
        match self.try_upgrade(customer_id, image, options).await {
            Ok(outcome) => outcome,
            Err(err) => UpgradeOutcome::Failed(format!("{err:#}")),
        }
    }

    async fn try_upgrade(&self, customer_id: &str, image: &str, options: &UpgradeOptions) -> anyhow::Result<UpgradeOutcome> {
        let instance = self.require(customer_id)?;
        if instance.spec.image == image {
            return Ok(UpgradeOutcome::AlreadyThere);
        }
        if instance.state == State::Suspended {
            // Nothing runs: the stopped container goes and `resume` makes a new one from the recorded image.
            self.runtime.remove(&instance.container_name()).await?;
            self.store.set_image(customer_id, image)?;
            return Ok(UpgradeOutcome::Upgraded { backup: None });
        }
        let old_image = instance.spec.image.clone();
        let backup = if options.backup {
            let path = self.backup_local(customer_id, options.keep).await.context("could not take the backup before the upgrade; nothing was changed")?;
            // The copy on the host is the way back for the customer; one that does not leave the host is not a reason to stop.
            if let Err(err) = self.ship(customer_id, &path).await {
                tracing::warn!(customer = %customer_id, error = format!("{err:#}"), "the backup before the upgrade was not shipped off the host");
            }
            Some(path)
        } else {
            None
        };
        let Err(reason) = self.swap_image(customer_id, image, options.health_timeout, options.soak).await else {
            return Ok(UpgradeOutcome::Upgraded { backup });
        };
        let reason = format!("{reason:#}");
        // The way back: the old image on the same volume, held to the same first check but without the soak.
        Ok(match self.swap_image(customer_id, &old_image, options.health_timeout, Duration::ZERO).await {
            Ok(()) => UpgradeOutcome::RolledBack { reason, backup },
            Err(rollback) => UpgradeOutcome::RollbackFailed { reason, rollback: format!("{rollback:#}"), backup },
        })
    }

    /// Replaces the instance's container with one from `image` on the same volume, and returns once the hub in it has
    /// answered `/health` and gone on answering for `soak`.
    async fn swap_image(&self, customer_id: &str, image: &str, health_timeout: Duration, soak: Duration) -> anyhow::Result<()> {
        let mut instance = self.require(customer_id)?;
        let name = instance.container_name();
        self.runtime.stop(&name).await?;
        self.runtime.remove(&name).await?;
        self.store.set_image(customer_id, image)?;
        instance.spec.image = image.to_string();
        self.create(&instance).await?;
        self.wait_healthy(&name, health_timeout, &format!("the hub on {image}")).await?;
        if !soak.is_zero() {
            tokio::time::sleep(soak).await;
            anyhow::ensure!(self.runtime.is_healthy(&name).await?, "the hub on {image} stopped answering /health during the {}s soak", soak.as_secs());
        }
        Ok(())
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
            if state == State::Active {
                instance.delete_after_ms = None;
            }
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

    const DAY_MS: u64 = 24 * 60 * 60 * 1000;
    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    #[tokio::test]
    async fn scheduling_a_delete_stops_the_container_and_keeps_the_record_and_the_data() {
        let m = manager("schedule-delete");
        m.provision(spec("acme")).await.unwrap();
        let before = now_ms();
        let pending = m.schedule_delete("acme", 30 * DAY).await.unwrap();
        let after = now_ms();

        assert_eq!(pending.state, State::Suspended);
        let due = pending.delete_after_ms.expect("a deadline");
        assert!((before + 30 * DAY_MS..=after + 30 * DAY_MS).contains(&due), "{due}");
        assert!(!m.runtime().container("warden-acme").unwrap().running);
        assert!(m.runtime().has_volume("warden-data-acme"), "nothing is erased by scheduling");
        assert_eq!(m.get("acme").unwrap().unwrap().delete_after_ms, Some(due), "saved, not only returned");
    }

    #[tokio::test]
    async fn scheduling_again_keeps_the_first_deadline_and_an_unknown_customer_is_refused() {
        let m = manager("schedule-twice");
        m.provision(spec("acme")).await.unwrap();
        let first = m.schedule_delete("acme", 30 * DAY).await.unwrap().delete_after_ms;
        let again = m.schedule_delete("acme", DAY).await.unwrap().delete_after_ms;
        assert_eq!(again, first, "asking twice neither extends nor shortens the time to keep the data");

        let err = m.schedule_delete("ghost", DAY).await.unwrap_err().to_string();
        assert!(err.contains("no instance for 'ghost'"), "{err}");
        assert!(m.get("ghost").unwrap().is_none());
    }

    #[tokio::test]
    async fn resuming_before_the_deadline_cancels_the_delete() {
        let m = manager("resume-cancels");
        m.provision(spec("acme")).await.unwrap();
        m.schedule_delete("acme", DAY).await.unwrap();

        let resumed = m.resume("acme").await.unwrap();
        assert_eq!(resumed.state, State::Active);
        assert_eq!(resumed.delete_after_ms, None);
        assert_eq!(m.get("acme").unwrap().unwrap().delete_after_ms, None, "saved, not only returned");
        assert!(m.runtime().container("warden-acme").unwrap().running);
        assert!(m.purge_expired(u64::MAX).await.unwrap().is_empty(), "a resumed instance is never purged");
        assert!(m.runtime().has_volume("warden-data-acme"));
    }

    #[tokio::test]
    async fn cancelling_a_delete_clears_the_deadline_and_leaves_the_instance_suspended() {
        let m = manager("cancel-delete");
        m.provision(spec("acme")).await.unwrap();
        m.schedule_delete("acme", DAY).await.unwrap();

        let cancelled = m.cancel_delete("acme").unwrap();
        assert_eq!(cancelled.delete_after_ms, None);
        assert_eq!(cancelled.state, State::Suspended, "cancelling does not wake it up; resume does");
        assert!(!m.runtime().container("warden-acme").unwrap().running);
        assert_eq!(m.cancel_delete("acme").unwrap().delete_after_ms, None, "again is a no-op");
        assert!(m.cancel_delete("ghost").is_err());
    }

    #[tokio::test]
    async fn only_what_is_past_its_deadline_is_purged_and_only_then() {
        let m = manager("purge");
        for id in ["old", "young", "plain", "live"] {
            m.provision(spec(id)).await.unwrap();
        }
        m.schedule_delete("old", 10 * DAY).await.unwrap();
        m.schedule_delete("young", 40 * DAY).await.unwrap();
        m.suspend("plain").await.unwrap();
        let old_due = m.get("old").unwrap().unwrap().delete_after_ms.unwrap();

        assert!(m.purge_expired(old_due - 1).await.unwrap().is_empty(), "one millisecond early: nothing");
        assert!(m.runtime().has_volume("warden-data-old"));

        let purged = m.purge_expired(old_due).await.unwrap();
        assert_eq!(purged.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["old"]);
        assert!(purged.iter().all(|(_, result)| result.is_ok()));
        assert!(m.get("old").unwrap().is_none());
        assert!(m.runtime().container("warden-old").is_none());
        assert!(!m.runtime().has_volume("warden-data-old"), "past the deadline the data is erased");

        // The others are untouched: a later deadline, a suspend with no deadline, and a running instance.
        assert!(m.runtime().has_volume("warden-data-young"));
        assert!(m.get("young").unwrap().unwrap().delete_after_ms.is_some());
        assert!(m.runtime().has_volume("warden-data-plain"));
        assert_eq!(m.get("plain").unwrap().unwrap().state, State::Suspended);
        assert!(m.runtime().container("warden-live").unwrap().running);
    }

    #[tokio::test]
    async fn listing_what_is_expired_erases_nothing_and_matches_what_a_purge_would_take() {
        let m = manager("expired");
        for id in ["old", "young", "plain"] {
            m.provision(spec(id)).await.unwrap();
        }
        m.schedule_delete("old", 10 * DAY).await.unwrap();
        m.schedule_delete("young", 40 * DAY).await.unwrap();
        m.suspend("plain").await.unwrap();
        let old_due = m.get("old").unwrap().unwrap().delete_after_ms.unwrap();

        assert!(m.expired(old_due - 1).unwrap().is_empty());
        let listed: Vec<String> = m.expired(old_due).unwrap().into_iter().map(|i| i.spec.customer_id).collect();
        assert_eq!(listed, ["old"]);
        assert!(m.get("old").unwrap().is_some() && m.runtime().has_volume("warden-data-old"), "listing erases nothing");

        let purged: Vec<String> = m.purge_expired(old_due).await.unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(purged, listed, "the same ones");
    }

    #[tokio::test]
    async fn one_purge_failing_does_not_stop_the_others_and_the_record_stays_for_a_retry() {
        let m = manager("purge-failure");
        for id in ["bad", "good"] {
            m.provision(spec(id)).await.unwrap();
            m.schedule_delete(id, DAY).await.unwrap();
        }
        m.runtime().break_container("warden-bad");

        let purged = m.purge_expired(u64::MAX).await.unwrap();
        let outcome = |id: &str| purged.iter().find(|(name, _)| name == id).map(|(_, result)| result.is_ok());
        assert_eq!(outcome("bad"), Some(false));
        assert_eq!(outcome("good"), Some(true));
        assert!(m.get("bad").unwrap().is_some(), "still recorded, so the next pass tries again");
        assert!(m.runtime().has_volume("warden-data-bad"));
        assert!(!m.runtime().has_volume("warden-data-good"));
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

    /// A sink that remembers what it was handed (and whether the file was there to be handed over), and can be made to fail.
    #[derive(Default)]
    struct RecordingSink {
        calls: std::sync::Mutex<Vec<(String, PathBuf, bool)>>,
        fails: bool,
    }

    #[async_trait::async_trait]
    impl BackupSink for RecordingSink {
        async fn ship(&self, customer: &str, file: &Path) -> anyhow::Result<()> {
            self.calls.lock().unwrap().push((customer.to_string(), file.to_path_buf(), file.is_file()));
            anyhow::ensure!(!self.fails, "the bucket said no");
            Ok(())
        }
    }

    async fn with_sink(name: &str, customer: &str, fails: bool) -> (InstanceManager<FakeRuntime>, Arc<RecordingSink>) {
        let sink = Arc::new(RecordingSink { fails, ..RecordingSink::default() });
        let m = with_data(name, customer).await.with_sink(sink.clone());
        (m, sink)
    }

    #[tokio::test]
    async fn a_finished_backup_is_handed_to_the_sink_and_only_a_finished_one() {
        let (m, sink) = with_sink("ship", "acme", false).await;
        let path = m.backup("acme", 7).await.unwrap();

        let calls = sink.calls.lock().unwrap().clone();
        assert_eq!(calls, [("acme".to_string(), path, true)], "the right customer and file, already in place");
        assert!(m.runtime().container("warden-acme").unwrap().running);

        m.runtime().fail_archiving("warden-data-acme");
        assert!(m.backup("acme", 7).await.is_err());
        assert_eq!(sink.calls.lock().unwrap().len(), 1, "a backup that failed has nothing to hand over");
    }

    #[tokio::test]
    async fn a_backup_that_could_not_be_shipped_stays_on_the_host_and_says_so() {
        let (m, sink) = with_sink("ship-fails", "acme", true).await;
        let err = m.backup("acme", 7).await.unwrap_err().to_string();

        let file = m.backups("acme").unwrap().pop().expect("the backup is still on the host");
        assert!(err.contains(&file.display().to_string()), "{err}");
        assert!(err.contains("could not be shipped off the host") && err.contains("the bucket said no"), "{err}");
        assert_eq!(sink.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_local_backup_is_not_handed_to_anyone() {
        let (m, sink) = with_sink("ship-local", "acme", false).await;
        m.backup_local("acme", 7).await.unwrap();
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn backing_up_everyone_reports_each_customer_and_ships_each_one() {
        let (m, sink) = with_sink("ship-all", "acme", false).await;
        m.provision(spec("beta")).await.unwrap();
        m.runtime().put_data("warden-data-beta", b"beta");

        let results = m.backup_all(7).await.unwrap();
        assert!(results.iter().all(|(_, result)| result.is_ok()));
        let shipped: Vec<_> = sink.calls.lock().unwrap().iter().map(|(customer, _, _)| customer.clone()).collect();
        assert_eq!(shipped, ["acme", "beta"]);
    }

    #[tokio::test]
    async fn an_upgrade_goes_on_when_its_backup_cannot_be_shipped() {
        let (m, sink) = with_sink("ship-upgrade", "acme", true).await;
        let outcome = m.upgrade_one("acme", "img:2", &quick()).await;

        assert!(matches!(outcome, UpgradeOutcome::Upgraded { backup: Some(_) }), "{outcome:?}");
        assert_eq!(sink.calls.lock().unwrap().len(), 1, "it was tried");
        assert_eq!(image_of(&m, "acme"), ("img:2".into(), "img:2".into()));
    }

    fn quick() -> UpgradeOptions {
        UpgradeOptions { soak: Duration::ZERO, health_timeout: Duration::ZERO, ..UpgradeOptions::default() }
    }

    fn image_of(m: &InstanceManager<FakeRuntime>, customer: &str) -> (String, String) {
        (m.get(customer).unwrap().unwrap().spec.image, m.runtime().container(&format!("warden-{customer}")).unwrap().image)
    }

    #[tokio::test]
    async fn an_upgrade_backs_up_then_runs_the_new_image_on_the_same_volume() {
        let m = with_data("upgrade", "acme").await;
        let outcome = m.upgrade_one("acme", "img:2", &quick()).await;

        let UpgradeOutcome::Upgraded { backup: Some(backup) } = outcome else { panic!("{outcome:?}") };
        assert_eq!(m.backups("acme").unwrap(), [backup], "the backup was taken first");
        assert_eq!(image_of(&m, "acme"), ("img:2".into(), "img:2".into()), "recorded and running");
        assert!(m.runtime().container("warden-acme").unwrap().running);
        assert_eq!(m.runtime().data("warden-data-acme").as_deref(), Some(DATA), "the customer's data stayed");
    }

    #[tokio::test]
    async fn an_upgrade_can_skip_the_backup_and_the_same_image_again_does_nothing() {
        let m = with_data("upgrade-twice", "acme").await;
        let options = UpgradeOptions { backup: false, ..quick() };
        assert!(matches!(m.upgrade_one("acme", "img:2", &options).await, UpgradeOutcome::Upgraded { backup: None }));
        assert!(m.backups("acme").unwrap().is_empty());

        let runs = m.runtime().run_log().len();
        assert_eq!(m.upgrade_one("acme", "img:2", &quick()).await, UpgradeOutcome::AlreadyThere);
        assert_eq!(m.runtime().run_log().len(), runs, "nothing was recreated");
        assert!(m.backups("acme").unwrap().is_empty(), "and no backup was taken");
    }

    #[tokio::test]
    async fn a_suspended_instance_stays_suspended_and_comes_back_on_the_new_image() {
        let m = with_data("upgrade-suspended", "acme").await;
        m.suspend("acme").await.unwrap();
        assert!(matches!(m.upgrade_one("acme", "img:2", &quick()).await, UpgradeOutcome::Upgraded { backup: None }));
        assert!(m.runtime().container("warden-acme").is_none(), "no container runs the old image");
        assert_eq!(m.get("acme").unwrap().unwrap().state, State::Suspended);

        m.resume("acme").await.unwrap();
        assert_eq!(image_of(&m, "acme"), ("img:2".into(), "img:2".into()));
    }

    #[tokio::test]
    async fn a_new_image_that_does_not_come_up_is_rolled_back_to_the_old_one() {
        let m = with_data("upgrade-rollback", "acme").await;
        m.runtime().make_image_unhealthy("img:bad");
        let outcome = m.upgrade_one("acme", "img:bad", &quick()).await;

        let UpgradeOutcome::RolledBack { reason, backup: Some(backup) } = outcome else { panic!("{outcome:?}") };
        assert!(reason.contains("did not answer /health"), "{reason}");
        assert!(backup.is_file(), "the way back for the data is named");
        assert_eq!(image_of(&m, "acme"), ("img:1".into(), "img:1".into()), "the old image is recorded and running again");
        assert_eq!(m.runtime().data("warden-data-acme").as_deref(), Some(DATA));
    }

    #[tokio::test]
    async fn when_the_old_image_does_not_come_back_either_the_outcome_says_so_and_names_the_backup() {
        let m = with_data("upgrade-down", "acme").await;
        m.runtime().make_image_unhealthy("img:1");
        m.runtime().make_image_unhealthy("img:bad");
        let outcome = m.upgrade_one("acme", "img:bad", &quick()).await;

        let UpgradeOutcome::RollbackFailed { backup: Some(backup), .. } = &outcome else { panic!("{outcome:?}") };
        assert!(outcome.to_string().contains(&backup.display().to_string()), "{outcome}");
        assert!(!outcome.is_fine());
    }

    #[tokio::test]
    async fn a_backup_that_fails_before_an_upgrade_changes_nothing() {
        let m = with_data("upgrade-no-backup", "acme").await;
        m.runtime().fail_archiving("warden-data-acme");
        let outcome = m.upgrade_one("acme", "img:2", &quick()).await;

        let UpgradeOutcome::Failed(why) = outcome else { panic!("{outcome:?}") };
        assert!(why.contains("nothing was changed"), "{why}");
        assert_eq!(image_of(&m, "acme"), ("img:1".into(), "img:1".into()));
        assert!(m.runtime().container("warden-acme").unwrap().running);
    }

    #[tokio::test]
    async fn an_upgrade_goes_canary_first_then_in_waves_and_leaves_out_those_already_there() {
        let m = manager("upgrade-waves");
        for customer in ["a", "b", "c", "d", "e"] {
            m.provision(spec(customer)).await.unwrap();
            m.runtime().put_data(&volume_name(customer), b"x");
        }
        m.provision(InstanceSpec { image: "img:2".into(), ..spec("f") }).await.unwrap();
        m.runtime().put_data("warden-data-f", b"x");
        let before = m.runtime().run_log().len();

        let options = UpgradeOptions { canary: 1, wave_size: 2, backup: false, ..quick() };
        let report = m.upgrade("img:2", &options).await.unwrap();

        let customers: Vec<_> = report.iter().map(|(customer, _)| customer.as_str()).collect();
        assert_eq!(customers, ["a", "b", "c", "d", "e"], "'f' is already there and not in the report");
        assert!(report.iter().all(|(_, outcome)| outcome.is_fine()));
        let log = m.runtime().run_log();
        let order: Vec<_> = log[before..].iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(order[0], "warden-a", "the canary is first, alone");
        assert!(order.iter().all(|name| *name != "warden-f"));
    }

    #[tokio::test]
    async fn the_first_rollback_stops_the_waves_that_have_not_begun() {
        let m = manager("upgrade-halt");
        for customer in ["a", "b", "c", "d"] {
            m.provision(spec(customer)).await.unwrap();
            m.runtime().put_data(&volume_name(customer), b"x");
        }
        m.runtime().make_image_unhealthy("img:bad");

        let options = UpgradeOptions { canary: 1, wave_size: 2, backup: false, ..quick() };
        let report = m.upgrade("img:bad", &options).await.unwrap();

        assert!(matches!(report[0], (ref customer, UpgradeOutcome::RolledBack { .. }) if customer == "a"), "{report:?}");
        assert!(report[1..].iter().all(|(_, outcome)| *outcome == UpgradeOutcome::Skipped), "{report:?}");
        for customer in ["a", "b", "c", "d"] {
            assert_eq!(image_of(&m, customer), ("img:1".into(), "img:1".into()), "{customer} stays on the old image");
        }
    }

    #[tokio::test]
    async fn an_upgrade_needs_an_image_and_a_canary() {
        let m = manager("upgrade-bad-args");
        assert!(m.upgrade(" ", &quick()).await.is_err());
        assert!(m.upgrade("img:2", &UpgradeOptions { canary: 0, ..quick() }).await.is_err());
        assert!(m.upgrade("img:2", &UpgradeOptions { wave_size: 0, ..quick() }).await.is_err());
        assert!(matches!(m.upgrade_one("ghost", "img:2", &quick()).await, UpgradeOutcome::Failed(_)));
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

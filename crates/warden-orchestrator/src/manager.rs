//! Provision, suspend, resume and delete, idempotent by customer id (the contract of `HOSPEDAGEM.md` B1).
//!
//! The store says what should be; the runtime says what is. Every call that can find the two apart brings the container to
//! what the store says (`provision` and `resume` recreate one that has vanished).

use std::io::Read;

use anyhow::{bail, Context};

use crate::instance::{validate_customer_id, Instance, InstanceSpec, State};
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
                    "'{}' already exists with a different spec ({:?}); delete it first, or ask for the same spec",
                    candidate.customer_id,
                    existing.spec
                );
                return Ok((existing.clone(), false));
            }
            let instance = Instance { port: free_port(all)?, spec: candidate, state: State::Active, pairing_key: new_key()?, created_at_ms: now_ms() };
            all.push(instance.clone());
            Ok((instance, true))
        })?;
        if created {
            if let Err(err) = self.runtime.run(&instance).await {
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
    /// error; the answer says whether there was anything.
    pub async fn delete(&self, customer_id: &str, purge_data: bool) -> anyhow::Result<bool> {
        let Some(instance) = self.store.get(customer_id)? else {
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

    /// Brings the container to "running" for an instance that should be: creates it if it is gone, starts it if stopped.
    async fn reconcile(&self, instance: &Instance) -> anyhow::Result<()> {
        let name = instance.container_name();
        match self.runtime.inspect(&name).await? {
            None => self.runtime.run(instance).await,
            Some(info) if !info.running => self.runtime.start(&name).await,
            Some(_) => Ok(()),
        }
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
        InstanceSpec { customer_id: id.into(), image: "img:1".into(), memory_mb: None, cpus: None }
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

        assert!(!m.delete("acme", true).await.unwrap(), "already gone");
        // The port of a deleted instance is free again.
        assert_eq!(m.provision(spec("gamma")).await.unwrap().port, 20000);
    }
}

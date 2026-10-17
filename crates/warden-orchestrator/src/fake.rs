//! A container runtime in memory, for the tests of what sits above it. No Docker, no process.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use anyhow::Context;
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::instance::Instance;
use crate::runtime::{ContainerInfo, ContainerRuntime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeContainer {
    pub running: bool,
    /// How many times it has started: one per `run`, `start` and `restart`. Stands in for Docker's `StartedAt`.
    pub starts: u32,
}

#[derive(Default)]
pub struct FakeRuntime {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    containers: BTreeMap<String, FakeContainer>,
    volumes: BTreeSet<String>,
    /// Names for which every call fails, to see one instance's failure not stopping the others.
    broken: BTreeSet<String>,
    restarts: Vec<String>,
    /// Volumes a starting config was put in, in order.
    seeded: Vec<String>,
    /// What each volume holds, as the bytes `archive_volume` hands out and `extract_volume` takes in.
    data: BTreeMap<String, Vec<u8>>,
    /// Volumes whose archiving fails.
    archive_fails: BTreeSet<String>,
    /// For each volume archived, in order, whether its container was running at the time.
    archived_running: Vec<(String, bool)>,
    /// Containers started with `run_isolated`, in order.
    isolated: Vec<String>,
    /// Every hub answers `/health` with a failure while this is set.
    unhealthy: bool,
}

impl FakeRuntime {
    pub fn container(&self, name: &str) -> Option<FakeContainer> {
        self.state.lock().unwrap().containers.get(name).cloned()
    }

    pub fn has_volume(&self, volume: &str) -> bool {
        self.state.lock().unwrap().volumes.contains(volume)
    }

    /// Container names restarted so far, in order.
    pub fn restarts(&self) -> Vec<String> {
        self.state.lock().unwrap().restarts.clone()
    }

    /// Volumes the starting config was put in so far, in order.
    pub fn seeded(&self) -> Vec<String> {
        self.state.lock().unwrap().seeded.clone()
    }

    /// Puts `bytes` in the volume (made if need be): the customer's data, for a backup to carry.
    pub fn put_data(&self, volume: &str, bytes: &[u8]) {
        let mut state = self.state.lock().unwrap();
        state.volumes.insert(volume.to_string());
        state.data.insert(volume.to_string(), bytes.to_vec());
    }

    pub fn data(&self, volume: &str) -> Option<Vec<u8>> {
        self.state.lock().unwrap().data.get(volume).cloned()
    }

    /// Makes archiving `volume` fail from now on.
    pub fn fail_archiving(&self, volume: &str) {
        self.state.lock().unwrap().archive_fails.insert(volume.to_string());
    }

    /// For each volume archived, in order, whether its container was running while it was.
    pub fn archived_running(&self) -> Vec<(String, bool)> {
        self.state.lock().unwrap().archived_running.clone()
    }

    /// From now on no hub answers `/health`.
    pub fn make_unhealthy(&self) {
        self.state.lock().unwrap().unhealthy = true;
    }

    /// Containers started with `run_isolated`, in order.
    pub fn isolated_runs(&self) -> Vec<String> {
        self.state.lock().unwrap().isolated.clone()
    }

    /// Every volume that exists, sorted.
    pub fn volumes(&self) -> Vec<String> {
        self.state.lock().unwrap().volumes.iter().cloned().collect()
    }

    /// Every container that exists, sorted.
    pub fn containers(&self) -> Vec<String> {
        self.state.lock().unwrap().containers.keys().cloned().collect()
    }

    /// A container that vanished behind the orchestrator's back (the host was rebuilt, someone ran `docker rm`).
    pub fn lose_container(&self, name: &str) {
        self.state.lock().unwrap().containers.remove(name);
    }

    /// From now on every call about `name` fails.
    pub fn break_container(&self, name: &str) {
        self.state.lock().unwrap().broken.insert(name.to_string());
    }

    fn check(state: &FakeState, name: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!state.broken.contains(name), "the runtime refuses '{name}'");
        Ok(())
    }
}

#[async_trait]
impl ContainerRuntime for FakeRuntime {
    async fn seed_config(&self, instance: &Instance) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        Self::check(&state, &instance.container_name())?;
        if instance.spec.config.is_some() {
            state.volumes.insert(instance.volume_name());
            state.seeded.push(instance.volume_name());
        }
        Ok(())
    }

    async fn run(&self, instance: &Instance) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        let name = instance.container_name();
        Self::check(&state, &name)?;
        anyhow::ensure!(!state.containers.contains_key(&name), "the name '{name}' is already in use");
        state.volumes.insert(instance.volume_name());
        state.containers.insert(name, FakeContainer { running: true, starts: 1 });
        Ok(())
    }

    async fn stop(&self, name: &str) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        Self::check(&state, name)?;
        if let Some(container) = state.containers.get_mut(name) {
            container.running = false;
        }
        Ok(())
    }

    async fn start(&self, name: &str) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        Self::check(&state, name)?;
        let container = state.containers.get_mut(name).ok_or_else(|| anyhow::anyhow!("No such container: {name}"))?;
        if !container.running {
            container.running = true;
            container.starts += 1;
        }
        Ok(())
    }

    async fn restart(&self, name: &str) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        Self::check(&state, name)?;
        let container = state.containers.get_mut(name).ok_or_else(|| anyhow::anyhow!("No such container: {name}"))?;
        container.running = true;
        container.starts += 1;
        state.restarts.push(name.to_string());
        Ok(())
    }

    async fn remove(&self, name: &str) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        Self::check(&state, name)?;
        state.containers.remove(name);
        Ok(())
    }

    async fn remove_volume(&self, volume: &str) -> anyhow::Result<()> {
        self.state.lock().unwrap().volumes.remove(volume);
        Ok(())
    }

    async fn inspect(&self, name: &str) -> anyhow::Result<Option<ContainerInfo>> {
        let state = self.state.lock().unwrap();
        Self::check(&state, name)?;
        Ok(state.containers.get(name).map(|c| ContainerInfo { running: c.running, started_at: format!("start-{}", c.starts) }))
    }

    async fn run_isolated(&self, instance: &Instance) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        let name = instance.container_name();
        Self::check(&state, &name)?;
        anyhow::ensure!(!state.containers.contains_key(&name), "the name '{name}' is already in use");
        anyhow::ensure!(state.volumes.contains(&instance.volume_name()), "no such volume: {}", instance.volume_name());
        state.isolated.push(name.clone());
        state.containers.insert(name, FakeContainer { running: true, starts: 1 });
        Ok(())
    }

    async fn is_healthy(&self, name: &str) -> anyhow::Result<bool> {
        let state = self.state.lock().unwrap();
        Ok(state.containers.get(name).is_some_and(|c| c.running) && !state.unhealthy)
    }

    async fn volume_exists(&self, volume: &str) -> anyhow::Result<bool> {
        Ok(self.state.lock().unwrap().volumes.contains(volume))
    }

    async fn archive_volume(&self, volume: &str, _image: &str, out: &mut (dyn AsyncWrite + Send + Unpin)) -> anyhow::Result<()> {
        let bytes = {
            let mut state = self.state.lock().unwrap();
            let container = volume.replacen("warden-data-", "warden-", 1);
            let running = state.containers.get(&container).is_some_and(|c| c.running);
            state.archived_running.push((volume.to_string(), running));
            anyhow::ensure!(!state.archive_fails.contains(volume), "could not archive {volume}");
            state.data.get(volume).cloned().with_context(|| format!("no such volume: {volume}"))?
        };
        out.write_all(&bytes).await?;
        Ok(())
    }

    async fn extract_volume(&self, volume: &str, _image: &str, input: &mut (dyn AsyncRead + Send + Unpin)) -> anyhow::Result<()> {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).await?;
        let mut state = self.state.lock().unwrap();
        state.volumes.insert(volume.to_string());
        state.data.insert(volume.to_string(), bytes);
        Ok(())
    }
}

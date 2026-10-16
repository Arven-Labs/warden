//! A container runtime in memory, for the tests of what sits above it. No Docker, no process.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use async_trait::async_trait;

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
}

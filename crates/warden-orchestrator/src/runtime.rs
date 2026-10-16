//! The container runtime: what the orchestrator asks of Docker, behind a trait so the rest runs against a fake.
//!
//! `DockerCli` shells out to the `docker` binary, the way `scripts/image-smoke.sh` does. No daemon client library: the
//! calls are few and a failed one carries Docker's own message.

use anyhow::{bail, Context};
use async_trait::async_trait;
use tokio::process::Command;

use crate::instance::{Instance, HUB_PORT};

/// The environment variable the hub reads its pairing key from. Passed to `docker run` by name only, so the value never
/// shows in the host's process list.
pub const KEY_ENV: &str = "WARDEN_SERVER_AUTH_KEY";

/// How long Docker waits for the hub to stop on SIGTERM before killing it. The hub's own grace is 5 s by default.
const STOP_TIMEOUT_SECS: u32 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub running: bool,
    /// When this run of the container started (Docker's `State.StartedAt`). It changes with every restart.
    pub started_at: String,
}

#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    /// Creates the container (and its volume, the first time) and starts it.
    async fn run(&self, instance: &Instance) -> anyhow::Result<()>;
    /// Stops a running container. Stopping one that is already stopped, or missing, is not an error.
    async fn stop(&self, name: &str) -> anyhow::Result<()>;
    /// Starts a stopped container.
    async fn start(&self, name: &str) -> anyhow::Result<()>;
    /// Stops (SIGTERM, then a kill after the timeout) and starts again, keeping the volume.
    async fn restart(&self, name: &str) -> anyhow::Result<()>;
    /// Removes the container, running or not. A missing one is not an error. The volume stays.
    async fn remove(&self, name: &str) -> anyhow::Result<()>;
    /// Removes a volume, and with it the customer's data. A missing one is not an error.
    async fn remove_volume(&self, volume: &str) -> anyhow::Result<()>;
    /// `None` when there is no such container.
    async fn inspect(&self, name: &str) -> anyhow::Result<Option<ContainerInfo>>;
}

/// The arguments of `docker run` for an instance (everything after `docker`). The shape of the container, kept in one
/// pure function: a read-only root with only `/data` (the volume) and `/tmp` writable, no capabilities, no new
/// privileges, published on the loopback only. The command is the hosted profile of the image (`--no-embedding-warm-up`,
/// P146) with the scheduled tasks on, as `scripts/image-smoke.sh` runs it.
pub fn run_args(instance: &Instance) -> Vec<String> {
    let mut args: Vec<String> = ["run", "-d", "--name"].map(String::from).into();
    args.push(instance.container_name());
    for (flag, value) in [
        ("--label", format!("warden.customer={}", instance.spec.customer_id)),
        ("--restart", "unless-stopped".to_string()),
        ("--tmpfs", "/tmp".to_string()),
        ("-v", format!("{}:/data", instance.volume_name())),
        ("-p", format!("127.0.0.1:{}:{HUB_PORT}", instance.port)),
        ("--cap-drop", "ALL".to_string()),
        ("--security-opt", "no-new-privileges".to_string()),
        ("--pids-limit", "512".to_string()),
    ] {
        args.push(flag.into());
        args.push(value);
    }
    args.push("--read-only".into());
    if let Some(mb) = instance.spec.memory_mb {
        args.push("--memory".into());
        args.push(format!("{mb}m"));
    }
    if let Some(cpus) = instance.spec.cpus {
        args.push("--cpus".into());
        args.push(cpus.to_string());
    }
    args.push("-e".into());
    args.push(KEY_ENV.into());
    args.push(instance.spec.image.clone());
    args.extend(["serve", "--no-embedding-warm-up", "--run-tasks"].map(String::from));
    args
}

pub struct DockerCli {
    binary: String,
}

impl DockerCli {
    pub fn new() -> Self {
        Self { binary: "docker".into() }
    }

    /// Runs `docker <args>`. A non-zero exit is an error carrying Docker's stderr, unless `tolerate` appears in it (the
    /// "no such container" of an already-gone thing).
    async fn docker(&self, args: &[String], key: Option<&str>, tolerate: Option<&str>) -> anyhow::Result<String> {
        let mut command = Command::new(&self.binary);
        command.args(args);
        if let Some(key) = key {
            command.env(KEY_ENV, key);
        }
        let output = command.output().await.with_context(|| format!("could not run `{}` (is Docker installed and on the PATH?)", self.binary))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if tolerate.is_some_and(|text| stderr.contains(text)) {
            return Ok(String::new());
        }
        bail!("`docker {}` failed: {stderr}", args.first().map(String::as_str).unwrap_or(""));
    }
}

impl Default for DockerCli {
    fn default() -> Self {
        Self::new()
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

#[async_trait]
impl ContainerRuntime for DockerCli {
    async fn run(&self, instance: &Instance) -> anyhow::Result<()> {
        self.docker(&run_args(instance), Some(&instance.pairing_key), None).await.map(|_| ())
    }

    async fn stop(&self, name: &str) -> anyhow::Result<()> {
        let timeout = STOP_TIMEOUT_SECS.to_string();
        self.docker(&strings(&["stop", "-t", &timeout, name]), None, Some("No such container")).await.map(|_| ())
    }

    async fn start(&self, name: &str) -> anyhow::Result<()> {
        self.docker(&strings(&["start", name]), None, None).await.map(|_| ())
    }

    async fn restart(&self, name: &str) -> anyhow::Result<()> {
        let timeout = STOP_TIMEOUT_SECS.to_string();
        self.docker(&strings(&["restart", "-t", &timeout, name]), None, None).await.map(|_| ())
    }

    async fn remove(&self, name: &str) -> anyhow::Result<()> {
        self.docker(&strings(&["rm", "-f", name]), None, Some("No such container")).await.map(|_| ())
    }

    async fn remove_volume(&self, volume: &str) -> anyhow::Result<()> {
        self.docker(&strings(&["volume", "rm", volume]), None, Some("No such volume")).await.map(|_| ())
    }

    async fn inspect(&self, name: &str) -> anyhow::Result<Option<ContainerInfo>> {
        let format = "{{.State.Running}}|{{.State.StartedAt}}";
        let out = self.docker(&strings(&["inspect", "-f", format, name]), None, Some("No such object")).await?;
        if out.is_empty() {
            return Ok(None);
        }
        let (running, started_at) = out.split_once('|').with_context(|| format!("unexpected `docker inspect` output: {out}"))?;
        Ok(Some(ContainerInfo { running: running == "true", started_at: started_at.to_string() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::{InstanceSpec, State};

    fn instance(memory_mb: Option<u64>, cpus: Option<f64>) -> Instance {
        Instance {
            spec: InstanceSpec { customer_id: "acme".into(), image: "ghcr.io/arven-labs/warden-server:0.1.0".into(), memory_mb, cpus },
            state: State::Active,
            port: 20001,
            pairing_key: "secret-key-that-must-not-be-an-argument".into(),
            created_at_ms: 0,
        }
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn the_container_is_locked_down_and_published_on_the_loopback_only() {
        let args = run_args(&instance(None, None));
        assert_eq!(&args[..4], ["run", "-d", "--name", "warden-acme"]);
        assert!(args.contains(&"--read-only".to_string()));
        assert!(has_pair(&args, "--tmpfs", "/tmp"));
        assert!(has_pair(&args, "-v", "warden-data-acme:/data"));
        assert!(has_pair(&args, "-p", "127.0.0.1:20001:7420"));
        assert!(has_pair(&args, "--cap-drop", "ALL"));
        assert!(has_pair(&args, "--security-opt", "no-new-privileges"));
        assert!(has_pair(&args, "--restart", "unless-stopped"));
        assert!(has_pair(&args, "--label", "warden.customer=acme"));
        assert!(!args.iter().any(|a| a == "--memory" || a == "--cpus"), "no limits unless asked");
    }

    #[test]
    fn the_image_comes_last_with_the_hosted_command_and_the_key_is_passed_by_name() {
        let args = run_args(&instance(None, None));
        let image = args.iter().position(|a| a == "ghcr.io/arven-labs/warden-server:0.1.0").expect("the image");
        assert_eq!(&args[image + 1..], ["serve", "--no-embedding-warm-up", "--run-tasks"]);
        assert!(has_pair(&args, "-e", KEY_ENV), "the variable is named, with no value");
        assert!(!args.iter().any(|a| a.contains("secret-key")), "the key never becomes an argument");
    }

    #[test]
    fn limits_are_added_when_asked() {
        let args = run_args(&instance(Some(512), Some(1.5)));
        assert!(has_pair(&args, "--memory", "512m"));
        assert!(has_pair(&args, "--cpus", "1.5"));
    }
}

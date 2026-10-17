//! The container runtime: what the orchestrator asks of Docker, behind a trait so the rest runs against a fake.
//!
//! `DockerCli` shells out to the `docker` binary, the way `scripts/image-smoke.sh` does. No daemon client library: the
//! calls are few and a failed one carries Docker's own message.

use anyhow::{bail, Context};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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
    /// Puts the instance's starting `config.toml` (if its spec has one) in its volume, creating the volume if need be. A
    /// config already in the volume is left alone: the customer's own changes win over the spec.
    async fn seed_config(&self, instance: &Instance) -> anyhow::Result<()>;
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
    /// Starts the hub of `instance` with no network, no published port and no scheduled tasks (see `isolated_args`).
    async fn run_isolated(&self, instance: &Instance) -> anyhow::Result<()>;
    /// Whether the hub in the container answers `/health` right now, asked from inside it (the isolated one has no port).
    async fn is_healthy(&self, name: &str) -> anyhow::Result<bool>;
    /// Whether the volume exists.
    async fn volume_exists(&self, volume: &str) -> anyhow::Result<bool>;
    /// Writes the volume's contents to `out` as a `tar.gz`, without `models/` (a cache the hub downloads again). Read from a
    /// throwaway container with the volume mounted read-only; the caller stops the instance first, so the files are still.
    async fn archive_volume(&self, volume: &str, image: &str, out: &mut (dyn AsyncWrite + Send + Unpin)) -> anyhow::Result<()>;
    /// The inverse: unpacks a `tar.gz` read from `input` into the volume (made if it isn't there).
    async fn extract_volume(&self, volume: &str, image: &str, input: &mut (dyn AsyncRead + Send + Unpin)) -> anyhow::Result<()>;
}

/// The arguments of `docker run` for an instance (everything after `docker`). The shape of the container, kept in one
/// pure function: a read-only root with only `/data` (the volume) and `/tmp` writable, no capabilities, no new
/// privileges, published on the loopback only. The command is the hosted profile of the image (`--no-embedding-warm-up`,
/// P146) with the scheduled tasks on, as `scripts/image-smoke.sh` runs it.
pub fn run_args(instance: &Instance) -> Vec<String> {
    container_args(instance, false)
}

/// Like `run_args`, for the hub that proves a restored backup starts (`ContainerRuntime::run_isolated`): no network at all
/// (the data is a customer's, and the hub on it must not sync, call a model or reach anything), so no published port;
/// no scheduled tasks; and it is not restarted by Docker.
pub fn isolated_args(instance: &Instance) -> Vec<String> {
    container_args(instance, true)
}

fn container_args(instance: &Instance, isolated: bool) -> Vec<String> {
    let mut args: Vec<String> = ["run", "-d", "--name"].map(String::from).into();
    args.push(instance.container_name());
    let mut flags = vec![
        ("--label", format!("warden.customer={}", instance.spec.customer_id)),
        ("--tmpfs", "/tmp".to_string()),
        ("-v", format!("{}:/data", instance.volume_name())),
        ("--cap-drop", "ALL".to_string()),
        ("--security-opt", "no-new-privileges".to_string()),
        ("--pids-limit", "512".to_string()),
    ];
    if isolated {
        flags.push(("--network", "none".to_string()));
    } else {
        flags.push(("--restart", "unless-stopped".to_string()));
        flags.push(("-p", format!("127.0.0.1:{}:{HUB_PORT}", instance.port)));
    }
    for (flag, value) in flags {
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
    args.extend(["serve", "--no-embedding-warm-up"].map(String::from));
    if !isolated {
        args.push("--run-tasks".into());
    }
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
        // Docker is not consistent about the case of these ("No such container", but "get <name>: no such volume").
        if tolerate.is_some_and(|text| stderr.to_lowercase().contains(&text.to_lowercase())) {
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

/// The arguments of the throwaway container that writes the starting config into the volume (everything after `docker`):
/// the hub's own image, as the user it runs as, with the shell as entrypoint, no network and nothing writable but `/data`.
/// The file is only written when absent.
pub fn seed_args(instance: &Instance) -> Vec<String> {
    let mut args: Vec<String> = ["run", "--rm", "-i", "--network", "none", "--read-only", "--cap-drop", "ALL", "--entrypoint", "sh", "-v"].map(String::from).into();
    args.push(format!("{}:/data", instance.volume_name()));
    args.push(instance.spec.image.clone());
    args.push("-c".into());
    args.push("[ -e /data/config.toml ] || cat > /data/config.toml".into());
    args
}

/// The arguments of the throwaway container that archives a volume (everything after `docker`): the hub's own image with
/// `tar` as the entrypoint, no network, nothing writable, the volume mounted read-only. The archive goes to stdout.
/// `models/` stays out: it is the embedding models cache, downloaded again on first use, not the customer's data.
pub fn archive_args(volume: &str, image: &str) -> Vec<String> {
    let mut args: Vec<String> = ["run", "--rm", "--network", "none", "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges", "--entrypoint", "tar", "-v"].map(String::from).into();
    args.push(format!("{volume}:/data:ro"));
    args.push(image.to_string());
    args.extend(["-C", "/data", "--exclude=./models", "-czf", "-", "."].map(String::from));
    args
}

/// The arguments of the throwaway container that unpacks an archive read from stdin into a volume. As the user the hub
/// runs as, so the files belong to it; the only writable thing is the volume.
pub fn extract_args(volume: &str, image: &str) -> Vec<String> {
    let mut args: Vec<String> = ["run", "--rm", "-i", "--network", "none", "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges", "--entrypoint", "tar", "-v"].map(String::from).into();
    args.push(format!("{volume}:/data"));
    args.push(image.to_string());
    args.extend(["-C", "/data", "-xzf", "-"].map(String::from));
    args
}

#[async_trait]
impl ContainerRuntime for DockerCli {
    async fn seed_config(&self, instance: &Instance) -> anyhow::Result<()> {
        let Some(config) = &instance.spec.config else {
            return Ok(());
        };
        let mut child = Command::new(&self.binary)
            .args(seed_args(instance))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("could not run `{}` (is Docker installed and on the PATH?)", self.binary))?;
        let mut stdin = child.stdin.take().context("no stdin on the seeding container")?;
        tokio::io::AsyncWriteExt::write_all(&mut stdin, config.as_bytes()).await?;
        drop(stdin); // the end of the input is what lets `cat` finish
        let output = child.wait_with_output().await?;
        if !output.status.success() {
            bail!("could not put the starting config in {}: {}", instance.volume_name(), String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }

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

    async fn run_isolated(&self, instance: &Instance) -> anyhow::Result<()> {
        self.docker(&isolated_args(instance), Some(&instance.pairing_key), None).await.map(|_| ())
    }

    async fn is_healthy(&self, name: &str) -> anyhow::Result<bool> {
        // The image has `curl` (it is its own HEALTHCHECK). A failure to answer is "not yet", not an error.
        let url = format!("http://127.0.0.1:{HUB_PORT}/health");
        let output = Command::new(&self.binary).args(["exec", name, "curl", "-fsS", "-o", "/dev/null", &url]).output().await.with_context(|| format!("could not run `{}`", self.binary))?;
        Ok(output.status.success())
    }

    async fn volume_exists(&self, volume: &str) -> anyhow::Result<bool> {
        let out = self.docker(&strings(&["volume", "inspect", "-f", "{{.Name}}", volume]), None, Some("No such volume")).await?;
        Ok(!out.is_empty())
    }

    async fn archive_volume(&self, volume: &str, image: &str, out: &mut (dyn AsyncWrite + Send + Unpin)) -> anyhow::Result<()> {
        let mut child = Command::new(&self.binary)
            .args(archive_args(volume, image))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("could not run `{}` (is Docker installed and on the PATH?)", self.binary))?;
        let mut stdout = child.stdout.take().context("no stdout on the archiving container")?;
        let mut stderr = child.stderr.take().context("no stderr on the archiving container")?;
        // stderr is read on the side so a chatty `tar` can never fill the pipe while this waits on stdout.
        let errors = tokio::spawn(async move {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text).await;
            text
        });
        let copied = tokio::io::copy(&mut stdout, out).await;
        let status = child.wait().await?;
        let errors = errors.await.unwrap_or_default();
        if !status.success() {
            bail!("could not archive {volume}: {}", errors.trim());
        }
        copied.with_context(|| format!("could not hand on the archive of {volume}"))?;
        out.flush().await?;
        Ok(())
    }

    async fn extract_volume(&self, volume: &str, image: &str, input: &mut (dyn AsyncRead + Send + Unpin)) -> anyhow::Result<()> {
        let mut child = Command::new(&self.binary)
            .args(extract_args(volume, image))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("could not run `{}` (is Docker installed and on the PATH?)", self.binary))?;
        let mut stdin = child.stdin.take().context("no stdin on the unpacking container")?;
        let mut stderr = child.stderr.take().context("no stderr on the unpacking container")?;
        let errors = tokio::spawn(async move {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text).await;
            text
        });
        let copied = tokio::io::copy(input, &mut stdin).await;
        drop(stdin); // the end of the input is what lets `tar` finish
        let status = child.wait().await?;
        let errors = errors.await.unwrap_or_default();
        if !status.success() {
            bail!("could not unpack into {volume}: {}", errors.trim());
        }
        copied.with_context(|| format!("could not hand the archive to {volume}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::{InstanceSpec, State};

    fn instance(memory_mb: Option<u64>, cpus: Option<f64>) -> Instance {
        Instance {
            spec: InstanceSpec { customer_id: "acme".into(), image: "ghcr.io/arven-labs/warden-server:0.1.0".into(), memory_mb, cpus, config: Some("token = \"s3cret\"".into()) },
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
    fn the_seeding_container_has_no_network_writes_only_the_volume_and_never_overwrites() {
        let args = seed_args(&instance(None, None));
        assert!(has_pair(&args, "--network", "none"));
        assert!(has_pair(&args, "--entrypoint", "sh"));
        assert!(has_pair(&args, "-v", "warden-data-acme:/data"));
        assert!(args.contains(&"--read-only".to_string()));
        assert_eq!(args.last().unwrap(), "[ -e /data/config.toml ] || cat > /data/config.toml");
        assert!(!args.iter().any(|a| a.contains("s3cret")), "the config goes in through stdin, not as an argument");
    }

    #[test]
    fn the_archiving_container_reads_the_volume_only_and_leaves_the_models_out() {
        let args = archive_args("warden-data-acme", "img:1");
        assert!(has_pair(&args, "--network", "none"));
        assert!(has_pair(&args, "--entrypoint", "tar"));
        assert!(has_pair(&args, "-v", "warden-data-acme:/data:ro"));
        assert!(args.contains(&"--read-only".to_string()));
        assert!(args.contains(&"--exclude=./models".to_string()));
        assert_eq!(&args[args.len() - 6..], ["-C", "/data", "--exclude=./models", "-czf", "-", "."]);
    }

    #[test]
    fn the_unpacking_container_has_no_network_and_writes_only_the_volume() {
        let args = extract_args("warden-data-acme", "img:1");
        assert!(has_pair(&args, "--network", "none"));
        assert!(has_pair(&args, "-v", "warden-data-acme:/data"));
        assert!(args.contains(&"-i".to_string()) && args.contains(&"--read-only".to_string()));
        assert_eq!(&args[args.len() - 4..], ["-C", "/data", "-xzf", "-"]);
    }

    #[test]
    fn the_container_that_checks_a_restore_has_no_network_no_port_and_runs_no_tasks() {
        let args = isolated_args(&instance(None, None));
        assert!(has_pair(&args, "--network", "none"));
        assert!(!args.iter().any(|a| a == "-p" || a == "--restart" || a == "--run-tasks"), "{args:?}");
        assert!(args.contains(&"--read-only".to_string()) && has_pair(&args, "--cap-drop", "ALL"));
        assert_eq!(&args[args.len() - 2..], ["serve", "--no-embedding-warm-up"]);
    }

    #[test]
    fn limits_are_added_when_asked() {
        let args = run_args(&instance(Some(512), Some(1.5)));
        assert!(has_pair(&args, "--memory", "512m"));
        assert!(has_pair(&args, "--cpus", "1.5"));
    }
}

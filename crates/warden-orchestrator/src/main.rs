//! `warden-orchestrator`: runs the hosted Warden instances on this machine, one Docker container per customer (P138).
//!
//!   warden-orchestrator provision acme --image ghcr.io/arven-labs/warden-server:0.1.0
//!   warden-orchestrator suspend acme      # stops the container, keeps the data
//!   warden-orchestrator resume acme
//!   warden-orchestrator recycle --every 300
//!   warden-orchestrator backup acme            # or --all [--every 86400]; encrypted, in <state-dir>/backups/acme/
//!   warden-orchestrator verify acme            # restores the newest backup into a scratch volume and starts a hub on it
//!   warden-orchestrator delete acme --purge-data
//!   warden-orchestrator restore acme latest    # into a volume that is not there; then `provision` again
//!
//! One orchestrator per state folder. The hubs are published on 127.0.0.1 only; a reverse proxy with TLS is another slice.
//! Keep a copy of `<state-dir>/backup.key` somewhere else: without it the backups cannot be opened.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;
use warden_orchestrator::{
    recycle_pass, DockerCli, HttpStatus, InstanceManager, InstanceSpec, InstanceStore, Outcome, RecyclePolicy,
};

/// The image a customer gets when none is named: the hub published on every tag.
const DEFAULT_IMAGE: &str = "ghcr.io/arven-labs/warden-server:latest";

/// How many backups of a customer stay when `--keep` is not given: a week of daily ones.
const DEFAULT_KEEP: usize = 7;

#[derive(Parser, Debug)]
#[command(name = "warden-orchestrator", about = "Runs hosted Warden instances, one Docker container per customer")]
struct Cli {
    /// Where the orchestrator keeps its record of the instances (`instances.json`, with their pairing keys, mode 0600).
    #[arg(long, env = "WARDEN_ORCHESTRATOR_DIR", default_value = "/var/lib/warden-orchestrator", global = true)]
    state_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Creates a customer's instance, or confirms it is there with the same settings. Asking again changes nothing.
    Provision {
        customer_id: String,
        /// The hub image to run.
        #[arg(long, default_value = DEFAULT_IMAGE)]
        image: String,
        /// Memory limit of the container, in MiB.
        #[arg(long)]
        memory_mb: Option<u64>,
        /// CPU limit of the container, in cores.
        #[arg(long)]
        cpus: Option<f64>,
        /// A `config.toml` for the new instance to start with (it needs a model provider: a hub without one refuses to
        /// start). Put in the volume before the first start, never over a config already there. May hold secrets.
        #[arg(long)]
        config_file: Option<PathBuf>,
    },
    /// Stops the instance and keeps its data.
    Suspend { customer_id: String },
    /// Starts a suspended instance again, on the same data.
    Resume { customer_id: String },
    /// Removes the container and the record. The data volume stays unless --purge-data.
    Delete {
        customer_id: String,
        /// Also deletes the volume: the customer's data, for good.
        #[arg(long)]
        purge_data: bool,
    },
    /// Every instance: customer, state, port and image.
    List,
    /// Prints an instance's pairing key (for the operator: it is how the hub is reached and how its status is read).
    Key { customer_id: String },
    /// Takes a backup of one instance (or, with --all, of every active one): the hub is stopped for the copy, the volume is
    /// archived and encrypted with the orchestrator's `backup.key` into `<state-dir>/backups/<customer>/`, and the hub is
    /// started again. The embedding models cache (`models/`) is left out.
    Backup {
        /// The customer to back up (or --all).
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        customer_id: Option<String>,
        /// Back up every active instance, one after the other.
        #[arg(long)]
        all: bool,
        /// With --all: keep going, one round every this many seconds.
        #[arg(long, requires = "all")]
        every: Option<u64>,
        /// How many backups of a customer to keep; older ones are deleted after a new one succeeds.
        #[arg(long, default_value_t = DEFAULT_KEEP)]
        keep: usize,
    },
    /// A customer's backups, oldest first.
    Backups { customer_id: String },
    /// Puts a backup in the customer's volume, which must not exist (delete the data first with `delete --purge-data`).
    Restore {
        customer_id: String,
        /// A backup file, or `latest`.
        #[arg(default_value = "latest")]
        backup: String,
        /// The hub image to unpack with, when the customer is not an instance any more.
        #[arg(long)]
        image: Option<String>,
    },
    /// Proves a backup is good: restores it into a scratch volume, starts a hub on it with no network and no scheduled tasks,
    /// waits for `/health`, and removes the scratch. Nothing of the customer's own is touched.
    Verify {
        customer_id: String,
        /// A backup file, or `latest`.
        #[arg(default_value = "latest")]
        backup: String,
        /// The hub image, when the customer is not an instance any more.
        #[arg(long)]
        image: Option<String>,
        /// How long the restored hub gets to answer `/health`.
        #[arg(long, default_value_t = 60)]
        timeout_secs: u64,
    },
    /// Restarts the instances that are idle and heavy, so the memory the hub holds comes back (K.6 item 5).
    Recycle {
        /// Keep going, one pass every this many seconds. Without it, one pass and done.
        #[arg(long)]
        every: Option<u64>,
        /// Resident memory, in MiB, at or above which an instance counts as heavy. Default 512.
        #[arg(long)]
        rss_threshold_mb: Option<u64>,
        /// How long nothing must have happened. Default 900 (15 min).
        #[arg(long)]
        min_idle_secs: Option<u64>,
        /// How long the hub must have been up. Default 3600 (1 h).
        #[arg(long)]
        min_uptime_secs: Option<u64>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).without_time().with_target(false).init();

    let cli = Cli::parse();
    let manager = InstanceManager::new(InstanceStore::new(&cli.state_dir), DockerCli::new());
    match cli.command {
        Command::Provision { customer_id, image, memory_mb, cpus, config_file } => {
            let config = config_file
                .map(|path| std::fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display())))
                .transpose()?;
            let instance = manager.provision(InstanceSpec { customer_id, image, memory_mb, cpus, config }).await?;
            println!("{} {:?} 127.0.0.1:{}", instance.spec.customer_id, instance.state, instance.port);
        }
        Command::Suspend { customer_id } => {
            manager.suspend(&customer_id).await?;
            println!("{customer_id} suspended");
        }
        Command::Resume { customer_id } => {
            let instance = manager.resume(&customer_id).await?;
            println!("{} {:?} 127.0.0.1:{}", instance.spec.customer_id, instance.state, instance.port);
        }
        Command::Delete { customer_id, purge_data } => {
            if manager.delete(&customer_id, purge_data).await? {
                println!("{customer_id} deleted{}", if purge_data { " (data too)" } else { ", data kept" });
            } else {
                println!("{customer_id} was not there");
            }
        }
        Command::List => {
            for i in manager.list()? {
                println!("{}\t{:?}\t127.0.0.1:{}\t{}", i.spec.customer_id, i.state, i.port, i.spec.image);
            }
        }
        Command::Key { customer_id } => {
            let instance = manager.get(&customer_id)?.with_context(|| format!("there is no instance for '{customer_id}'"))?;
            println!("{}", instance.pairing_key);
        }
        Command::Backup { customer_id, all, every, keep } => {
            let _ = all;
            match customer_id {
                Some(customer) => println!("{}", manager.backup(&customer, keep).await?.display()),
                None => loop {
                    for (customer, result) in manager.backup_all(keep).await? {
                        match result {
                            Ok(path) => tracing::info!(%customer, path = %path.display(), "backed up"),
                            Err(err) => tracing::warn!(%customer, error = format!("{err:#}"), "could not back up"),
                        }
                    }
                    let Some(secs) = every else { break };
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
                        _ = tokio::signal::ctrl_c() => break,
                    }
                },
            }
        }
        Command::Backups { customer_id } => {
            for path in manager.backups(&customer_id)? {
                println!("{}\t{} bytes", path.display(), std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
            }
        }
        Command::Restore { customer_id, backup, image } => {
            let file = manager.restore(&customer_id, &backup, image.as_deref()).await?;
            println!("{customer_id} restored from {}; `provision` it again to run it", file.display());
        }
        Command::Verify { customer_id, backup, image, timeout_secs } => {
            let file = manager.verify(&customer_id, &backup, image.as_deref(), Duration::from_secs(timeout_secs)).await?;
            println!("{} is good: a hub started on it and answered /health", file.display());
        }
        Command::Recycle { every, rss_threshold_mb, min_idle_secs, min_uptime_secs } => {
            let defaults = RecyclePolicy::default();
            let policy = RecyclePolicy {
                rss_threshold_bytes: rss_threshold_mb.map_or(defaults.rss_threshold_bytes, |mb| mb * 1024 * 1024),
                min_idle_secs: min_idle_secs.unwrap_or(defaults.min_idle_secs),
                min_uptime_secs: min_uptime_secs.unwrap_or(defaults.min_uptime_secs),
            };
            let source = HttpStatus::new();
            loop {
                for (customer, outcome) in recycle_pass(&manager, &source, &policy).await? {
                    match outcome {
                        Outcome::Recycled => tracing::info!(%customer, "recycled"),
                        Outcome::Kept(reason) => tracing::info!(%customer, ?reason, "kept"),
                        Outcome::Failed(message) => tracing::warn!(%customer, %message, "could not recycle"),
                    }
                }
                let Some(secs) = every else { break };
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
        }
    }
    Ok(())
}

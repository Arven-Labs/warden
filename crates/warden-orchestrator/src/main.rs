//! `warden-orchestrator`: runs the hosted Warden instances on this machine, one Docker container per customer (P138).
//!
//!   warden-orchestrator provision acme --image ghcr.io/arven-labs/warden-server:0.1.0
//!   warden-orchestrator suspend acme      # stops the container, keeps the data
//!   warden-orchestrator resume acme
//!   warden-orchestrator recycle --every 300
//!   warden-orchestrator delete acme --purge-data
//!
//! One orchestrator per state folder. The hubs are published on 127.0.0.1 only; a reverse proxy with TLS is another slice.

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
        Command::Provision { customer_id, image, memory_mb, cpus } => {
            let instance = manager.provision(InstanceSpec { customer_id, image, memory_mb, cpus }).await?;
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

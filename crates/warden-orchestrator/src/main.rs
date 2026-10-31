//! `warden-orchestrator`: runs the hosted Warden instances on this machine, one Docker container per customer (P138).
//!
//!   warden-orchestrator provision acme --image ghcr.io/arven-labs/warden-server:0.1.0
//!   warden-orchestrator suspend acme      # stops the container, keeps the data
//!   warden-orchestrator resume acme
//!   warden-orchestrator recycle --every 300
//!   warden-orchestrator backup acme            # or --all [--every 86400]; encrypted, in <state-dir>/backups/acme/
//!   warden-orchestrator upgrade --image ghcr.io/arven-labs/warden-server:0.2.0   # canary, then waves; rolls back what fails
//!   warden-orchestrator verify acme            # restores the newest backup into a scratch volume and starts a hub on it
//!   warden-orchestrator serve                  # the signed HTTP API of the site mãe; needs WARDEN_ORCHESTRATOR_ADMIN_SECRET
//!   warden-orchestrator purge-expired [--yes]  # lists, or with --yes erases, the instances whose delete deadline has passed
//!   warden-orchestrator delete acme --purge-data
//!   warden-orchestrator restore acme latest    # into a volume that is not there; then `provision` again
//!
//! One orchestrator per state folder. The hubs are published on 127.0.0.1 only; a reverse proxy with TLS is another slice.
//! Keep a copy of `<state-dir>/backup.key` somewhere else: without it the backups cannot be opened.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;
use warden_orchestrator::api::{non_loopback_warning, router, validate_secrets, ApiConfig};
use warden_orchestrator::{
    recycle_pass, BackupKey, CommandSink, DockerCli, HttpStatus, InstanceManager, InstanceSpec, InstanceStore, Outcome, RecyclePolicy, UpgradeOptions,
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

    /// A command to run after each backup that `backup` and `upgrade` take, to get the file off this host (rclone, scp, rsync,
    /// aws s3 cp...). Split into arguments first, no shell; `{file}`, `{customer}` and `{name}` are filled in inside each
    /// argument, and `WARDEN_BACKUP_FILE` and `WARDEN_BACKUP_CUSTOMER` are in its environment. The file is already
    /// encrypted. The key is not sent: see `backup-key`.
    #[arg(long, env = "WARDEN_AFTER_BACKUP", global = true)]
    after_backup: Option<String>,

    /// How long the --after-backup command may run before it is stopped.
    #[arg(long, default_value_t = 600, global = true)]
    after_backup_timeout_secs: u64,

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
    /// Moves every instance to a new hub image, a few at a time: the canary first, alone, then waves. Each instance is backed
    /// up, its container replaced by one from the new image on the same volume, and `/health` awaited; one that does not
    /// come up healthy gets the old image back, and the waves not yet begun are skipped. Exits with an error unless every
    /// instance ended on the new image. Running it again after a fix picks up where it stopped.
    Upgrade {
        /// The hub image to move to.
        #[arg(long)]
        image: String,
        /// How many instances go first, alone.
        #[arg(long, default_value_t = 1)]
        canary: usize,
        /// How many go together in each wave after the canary.
        #[arg(long, default_value_t = 5)]
        wave_size: usize,
        /// How long the new hub must go on answering `/health` after it first does.
        #[arg(long, default_value_t = 30)]
        soak_secs: u64,
        /// How long the new hub gets to answer `/health` the first time.
        #[arg(long, default_value_t = 60)]
        health_timeout_secs: u64,
        /// Do not take a backup of each instance before changing its image.
        #[arg(long)]
        no_backup: bool,
        /// How many backups of a customer to keep.
        #[arg(long, default_value_t = DEFAULT_KEEP)]
        keep: usize,
    },
    /// Prints the key the backups are encrypted with (64 hexadecimal characters, the content of `<state-dir>/backup.key`).
    /// Without it no backup can be opened, so keep a copy somewhere other than where the backups go.
    BackupKey,
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
    /// Serves the signed HTTP API of the site mãe (provision, suspend, resume, export, delete). Plain HTTP: keep it on
    /// loopback and put a reverse proxy with TLS in front.
    Serve {
        #[arg(long, env = "WARDEN_ORCHESTRATOR_LISTEN", default_value = "127.0.0.1:7440")]
        listen: std::net::SocketAddr,
        /// The secret the calls are signed with (32 characters at least). Required.
        #[arg(long, env = "WARDEN_ORCHESTRATOR_ADMIN_SECRET", hide_env_values = true)]
        admin_secret: Option<String>,
        /// The secret being replaced, accepted too while the site rotates to the new one. Leave it out otherwise.
        #[arg(long, env = "WARDEN_ORCHESTRATOR_ADMIN_SECRET_PREVIOUS", hide_env_values = true)]
        admin_secret_previous: Option<String>,
        /// Days the data of a deleted instance is kept before `purge-expired` may erase it.
        #[arg(long, env = "WARDEN_ORCHESTRATOR_RETENTION_DAYS", default_value_t = 30)]
        retention_days: u64,
        /// How many backups of a customer stay after an export.
        #[arg(long, env = "WARDEN_ORCHESTRATOR_KEEP_BACKUPS", default_value_t = DEFAULT_KEEP)]
        keep_backups: usize,
    },
    /// Erases the instances whose delete deadline has passed, data included. Without --yes it only lists them.
    PurgeExpired {
        /// Really erase. Irreversible: the volume with the customer's data is removed.
        #[arg(long)]
        yes: bool,
        /// Keep going, one pass every this many seconds (needs --yes). Without it, one pass and done.
        #[arg(long)]
        every: Option<u64>,
    },
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).without_time().with_target(false).init();

    let cli = Cli::parse();
    let mut manager = InstanceManager::new(InstanceStore::new(&cli.state_dir), DockerCli::new());
    if let Some(command) = &cli.after_backup {
        manager = manager.with_sink(Arc::new(CommandSink::new(command, Duration::from_secs(cli.after_backup_timeout_secs))?));
    }
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
        Command::Upgrade { image, canary, wave_size, soak_secs, health_timeout_secs, no_backup, keep } => {
            let options = UpgradeOptions {
                backup: !no_backup,
                keep,
                canary,
                wave_size,
                soak: Duration::from_secs(soak_secs),
                health_timeout: Duration::from_secs(health_timeout_secs),
            };
            let report = manager.upgrade(&image, &options).await?;
            for (customer, outcome) in &report {
                println!("{customer}\t{outcome}");
            }
            if report.is_empty() {
                println!("every instance is already on {image}");
            }
            anyhow::ensure!(report.iter().all(|(_, outcome)| outcome.is_fine()), "not every instance ended on {image}");
        }
        Command::BackupKey => {
            let key = BackupKey::load(&cli.state_dir)?;
            eprintln!(
                "This is the key of {}/backup.key. Without it the backups cannot be opened; with it and a backup, anyone can read the \
                 customer's data. Keep it apart from where the backups go. On a new host, write it back to <state-dir>/backup.key (mode 0600).",
                cli.state_dir.display()
            );
            println!("{}", key.to_hex());
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
        Command::Serve { listen, admin_secret, admin_secret_previous, retention_days, keep_backups } => {
            let secrets = validate_secrets(admin_secret.as_deref().unwrap_or(""), admin_secret_previous.as_deref())?;
            anyhow::ensure!(retention_days >= 1, "--retention-days is at least 1: 0 would let the data be erased the moment a delete is asked for");
            anyhow::ensure!(keep_backups >= 1, "--keep-backups is at least 1: keeping none would delete the backup just taken");
            if let Some(warning) = non_loopback_warning(listen) {
                tracing::warn!("{warning}");
            }
            let config = ApiConfig { secrets, retention: Duration::from_secs(retention_days * 24 * 60 * 60), keep_backups };
            let app = router(Arc::new(manager), Arc::new(HttpStatus::new()), config);
            let listener = tokio::net::TcpListener::bind(listen).await.with_context(|| format!("could not listen on {listen}"))?;
            tracing::info!(%listen, "serving the orchestrator API");
            axum::serve(listener, app).with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; }).await?;
        }
        Command::PurgeExpired { yes, every } => {
            anyhow::ensure!(yes || every.is_none(), "--every needs --yes: repeating a list that erases nothing is no use");
            let mut failed = false;
            loop {
                if yes {
                    for (customer, result) in manager.purge_expired(now_ms()).await? {
                        match result {
                            Ok(()) => println!("{customer} purged"),
                            Err(err) => {
                                failed = true;
                                tracing::warn!(%customer, "could not purge: {err:#}");
                            }
                        }
                    }
                } else {
                    let due = manager.expired(now_ms())?;
                    for instance in &due {
                        println!("{} would be erased (deadline {} ms); run with --yes to erase it", instance.spec.customer_id, instance.delete_after_ms.unwrap_or_default());
                    }
                    if due.is_empty() {
                        println!("nothing is past its deadline");
                    }
                }
                let Some(secs) = every else { break };
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
            anyhow::ensure!(!failed, "some instances could not be purged; they stay recorded, so the next pass tries them again");
        }
    }
    Ok(())
}

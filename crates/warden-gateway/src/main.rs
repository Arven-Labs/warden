//! `warden-gateway`: the operator CLI of the hosted model gateway.
//!
//! `serve` runs the endpoint; the other commands work on the ledger and the catalog directly, which is how an account is
//! made and topped up by hand while there is no site to do it (Session 273: not a commercial product yet).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;
use warden_gateway::http::GatewayServer;
use warden_gateway::money::{format_usd, parse_usd};
use warden_gateway::store::{EntryKind, Micros, Store};
use warden_gateway::upstream::HttpUpstream;
use warden_gateway::webhook::{Dispatcher, HttpSender};
use warden_gateway::{Catalog, Gateway};

#[derive(Parser)]
#[command(name = "warden-gateway", about = "The Warden model gateway: prepaid credits for hosted instances")]
struct Cli {
    /// Where the ledger (`ledger.db`) and, by default, the catalog live.
    #[arg(long, env = "WARDEN_GATEWAY_DIR", default_value = "warden-gateway-data", global = true)]
    data_dir: PathBuf,
    /// The model catalog (TOML). Defaults to `catalog.toml` in the data directory.
    #[arg(long, env = "WARDEN_GATEWAY_CATALOG", global = true)]
    catalog: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the endpoint (`/v1/chat/completions`, `/v1/models`, `/health`).
    Serve {
        #[arg(long, env = "WARDEN_GATEWAY_LISTEN", default_value = "127.0.0.1:7430")]
        listen: SocketAddr,
        /// The secret the site mãe signs its calls with. Without it the account routes (`/v1/accounts/...`) don't exist.
        /// Prefer the environment variable: a flag shows in the process list.
        #[arg(long, env = "WARDEN_GATEWAY_ADMIN_SECRET", hide_env_values = true)]
        admin_secret: Option<String>,
        /// Where to send the webhooks (`credits.low`, `credits.depleted`, `usage.daily_summary`). Needs `--webhook-secret`.
        #[arg(long, env = "WARDEN_GATEWAY_WEBHOOK_URL")]
        webhook_url: Option<String>,
        /// The secret the gateway signs its webhooks with: not the same as `--admin-secret`.
        #[arg(long, env = "WARDEN_GATEWAY_WEBHOOK_SECRET", hide_env_values = true)]
        webhook_secret: Option<String>,
        /// Send `credits.low` when an account's balance falls below this many USD.
        #[arg(long, env = "WARDEN_GATEWAY_LOW_BALANCE_USD")]
        low_balance_usd: Option<String>,
    },
    /// The webhook outbox.
    Webhooks {
        #[command(subcommand)]
        command: WebhooksCommand,
    },
    /// Make, list and change accounts.
    Accounts {
        #[command(subcommand)]
        command: AccountsCommand,
    },
    /// Add credit to an account.
    Credit {
        #[command(subcommand)]
        command: CreditCommand,
    },
    /// An account's history, newest first.
    Ledger {
        account: String,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Check the catalog and show what each model costs and sells for.
    CatalogCheck,
}

#[derive(Subcommand)]
enum WebhooksCommand {
    /// How many webhooks are pending, delivered and given up.
    Status,
    /// The ones not delivered yet, pending and given up.
    Pending {
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
}

#[derive(Subcommand)]
enum AccountsCommand {
    /// Make an account; its instance token is shown once.
    Create {
        name: String,
        /// Daily spend cap in USD.
        #[arg(long)]
        daily_cap: Option<String>,
        /// Monthly (rolling 30 days) spend cap in USD.
        #[arg(long)]
        monthly_cap: Option<String>,
    },
    List,
    Show { account: String },
    /// Refuse the account's calls until it is resumed.
    Suspend { account: String },
    Resume { account: String },
    /// Set the spend caps; a cap left out is removed.
    SetCaps {
        account: String,
        #[arg(long)]
        daily: Option<String>,
        #[arg(long)]
        monthly: Option<String>,
    },
}

#[derive(Subcommand)]
enum CreditCommand {
    /// Top up an account by hand. With `--ref` the same reference is applied once, so a repeated command is harmless.
    Add {
        account: String,
        /// Amount in USD, like 25 or 12.50.
        amount: String,
        #[arg(long = "ref")]
        reference: Option<String>,
        #[arg(long, default_value = "")]
        note: String,
    },
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn open_store(cli: &Cli) -> Result<Store> {
    std::fs::create_dir_all(&cli.data_dir).with_context(|| format!("cannot create {}", cli.data_dir.display()))?;
    Store::open(&cli.data_dir.join("ledger.db")).context("cannot open the ledger")
}

fn load_catalog(cli: &Cli) -> Result<Catalog> {
    let path = cli.catalog.clone().unwrap_or_else(|| cli.data_dir.join("catalog.toml"));
    Catalog::load(&path).map_err(anyhow::Error::msg)
}

fn optional_usd(value: &Option<String>) -> Result<Option<Micros>> {
    value.as_deref().map(|v| parse_usd(v).map_err(anyhow::Error::msg)).transpose()
}

fn show_cap(cap: Option<Micros>) -> String {
    cap.map_or("none".to_string(), format_usd)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))).init();
    let cli = Cli::parse();
    match &cli.command {
        Command::Serve { listen, admin_secret, webhook_url, webhook_secret, low_balance_usd } => {
            let mut store = open_store(&cli)?;
            if let Some(low) = optional_usd(low_balance_usd)? {
                store = store.with_low_threshold(low);
            }
            let store = Arc::new(store);
            let catalog = Arc::new(load_catalog(&cli)?);
            for upstream in &catalog.upstreams {
                if upstream.api_key.is_none() {
                    tracing::warn!(upstream = %upstream.id, "no API key for this upstream (its `api_key_env` is unset or empty)");
                }
            }
            let gateway = Gateway::new(store.clone(), catalog.clone());
            let mut server = GatewayServer::bind(*listen, gateway, Arc::new(HttpUpstream::new())).await.with_context(|| format!("cannot listen on {listen}"))?;
            match admin_secret.as_deref().filter(|s| !s.is_empty()) {
                Some(secret) => server = server.with_admin_secret(secret),
                None => tracing::warn!("no admin secret: the site's account routes (/v1/accounts/...) are off"),
            }
            match (webhook_url.as_deref().filter(|s| !s.is_empty()), webhook_secret.as_deref().filter(|s| !s.is_empty())) {
                (Some(url), Some(secret)) => {
                    let dispatcher = Dispatcher::new(store.clone(), Arc::new(HttpSender::new()), url, secret);
                    tokio::spawn(Arc::new(dispatcher).run());
                    tracing::info!(%url, "webhooks on");
                }
                (None, None) => tracing::warn!("no webhook URL: balance events and daily summaries are queued but not sent"),
                _ => bail!("--webhook-url and --webhook-secret go together"),
            }
            tracing::info!(addr = %server.local_addr()?, models = catalog.models.len(), "gateway listening (plain HTTP: put TLS in front of it)");
            server.serve().await;
            Ok(())
        }
        Command::Accounts { command } => accounts(&cli, command),
        Command::Webhooks { command } => {
            let store = open_store(&cli)?;
            match command {
                WebhooksCommand::Status => {
                    let s = store.outbox_stats()?;
                    println!("pending {}, delivered {}, given up {} (after {} failed attempts)", s.pending, s.delivered, s.given_up, warden_gateway::store::MAX_DELIVERY_ATTEMPTS);
                }
                WebhooksCommand::Pending { limit } => {
                    for e in store.undelivered_events(*limit)? {
                        println!("{}  {:<20} {}  attempts {}", e.id, e.kind, e.account_id, e.attempts);
                    }
                }
            }
            Ok(())
        }
        Command::Credit { command: CreditCommand::Add { account, amount, reference, note } } => {
            let store = open_store(&cli)?;
            let micros = parse_usd(amount).map_err(anyhow::Error::msg)?;
            if micros == 0 {
                bail!("the amount must be more than zero");
            }
            let applied = store.credit(account, EntryKind::Topup, micros, reference.as_deref(), note, now())?;
            if applied {
                println!("added {} to {account}; balance {}", format_usd(micros), format_usd(store.balance(account)?));
            } else {
                println!("reference already applied to {account}; nothing changed (balance {})", format_usd(store.balance(account)?));
            }
            Ok(())
        }
        Command::Ledger { account, limit } => {
            let store = open_store(&cli)?;
            for e in store.entries(account, *limit)? {
                println!("{:>5}  {:<10} {:>14}  {}  {}", e.id, format!("{:?}", e.kind).to_lowercase(), format_usd(e.amount), e.ref_id.unwrap_or_default(), e.note);
            }
            println!("balance {}", format_usd(store.balance(account)?));
            Ok(())
        }
        Command::CatalogCheck => {
            let catalog = load_catalog(&cli)?;
            println!("{} upstream(s), {} model(s)", catalog.upstreams.len(), catalog.models.len());
            for m in &catalog.models {
                println!(
                    "{:<20} -> {}/{}   cost {}/{}   sale {}/{}   (USD per million tokens, in/out)   max {} tokens",
                    m.name, m.upstream, m.upstream_model, format_usd(m.cost_in), format_usd(m.cost_out), format_usd(m.sale_in), format_usd(m.sale_out), m.max_tokens
                );
            }
            Ok(())
        }
    }
}

fn accounts(cli: &Cli, command: &AccountsCommand) -> Result<()> {
    let store = open_store(cli)?;
    match command {
        AccountsCommand::Create { name, daily_cap, monthly_cap } => {
            let (daily, monthly) = (optional_usd(daily_cap)?, optional_usd(monthly_cap)?);
            let (account, token) = store.create_account(name, now())?;
            if daily.is_some() || monthly.is_some() {
                store.set_caps(&account.id, daily, monthly)?;
            }
            println!("account  {}", account.id);
            println!("token    {token}");
            println!("(the token is shown once: only a hash is kept. Use it as the API key of a `cloud` provider.)");
        }
        AccountsCommand::List => {
            for a in store.list_accounts()? {
                let state = if a.active { "active" } else { "suspended" };
                println!("{}  {:<20} {:<9} balance {}", a.id, a.name, state, format_usd(store.balance(&a.id)?));
            }
        }
        AccountsCommand::Show { account } => {
            let a = store.account(account)?.with_context(|| format!("no account {account}"))?;
            println!("account   {}", a.id);
            println!("name      {}", a.name);
            println!("state     {}", if a.active { "active" } else { "suspended" });
            println!("balance   {}", format_usd(store.balance(&a.id)?));
            println!("available {}", format_usd(store.available(&a.id, now())?));
            println!("caps      daily {}, monthly {}", show_cap(a.daily_cap), show_cap(a.monthly_cap));
        }
        AccountsCommand::Suspend { account } => {
            store.set_active(account, false)?;
            println!("{account} suspended");
        }
        AccountsCommand::Resume { account } => {
            store.set_active(account, true)?;
            println!("{account} resumed");
        }
        AccountsCommand::SetCaps { account, daily, monthly } => {
            store.set_caps(account, optional_usd(daily)?, optional_usd(monthly)?)?;
            println!("caps of {account}: daily {}, monthly {}", show_cap(optional_usd(daily)?), show_cap(optional_usd(monthly)?));
        }
    }
    Ok(())
}

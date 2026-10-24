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
use warden_gateway::tls::GatewayTls;
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
        /// While rotating `--admin-secret`: the old one, still accepted next to the new one. Take it out once the site signs
        /// with the new secret. Needs `--admin-secret`. (The webhook secret rotates on the site's side: it first accepts both,
        /// then the gateway switches `--webhook-secret`, then the site drops the old one.)
        #[arg(long, env = "WARDEN_GATEWAY_ADMIN_SECRET_PREVIOUS", hide_env_values = true)]
        admin_secret_previous: Option<String>,
        /// Where to send the webhooks (`credits.low`, `credits.depleted`, `usage.daily_summary`, `account.suspended`). Needs `--webhook-secret`.
        #[arg(long, env = "WARDEN_GATEWAY_WEBHOOK_URL")]
        webhook_url: Option<String>,
        /// The secret the gateway signs its webhooks with: not the same as `--admin-secret`.
        #[arg(long, env = "WARDEN_GATEWAY_WEBHOOK_SECRET", hide_env_values = true)]
        webhook_secret: Option<String>,
        /// Send `credits.low` when an account's balance falls below this many USD.
        #[arg(long, env = "WARDEN_GATEWAY_LOW_BALANCE_USD")]
        low_balance_usd: Option<String>,
        /// Requests a minute allowed from one client address (`0`: no limit). The site's signed routes and `/health` are exempt.
        #[arg(long, env = "WARDEN_GATEWAY_IP_RATE_PER_MIN", default_value_t = 600)]
        ip_rate_per_min: u32,
        /// Requests a minute allowed with one instance token (`0`: no limit).
        #[arg(long, env = "WARDEN_GATEWAY_TOKEN_RATE_PER_MIN", default_value_t = 120)]
        token_rate_per_min: u32,
        /// Behind a reverse proxy that sets `X-Forwarded-For`, take the client's address from its last entry. Leave it off
        /// without a proxy: then anyone could write the header and pick their own limit.
        #[arg(long, env = "WARDEN_GATEWAY_TRUST_FORWARDED_FOR")]
        trust_forwarded_for: bool,
        /// Connections open at once, in all (`0`: no cap). One over the cap is closed without an answer.
        #[arg(long, env = "WARDEN_GATEWAY_MAX_CONNECTIONS", default_value_t = 1024)]
        max_connections: usize,
        /// Connections open at once from one address (`0`: no cap). Not applied with `--trust-forwarded-for`: behind the proxy
        /// every connection comes from the proxy's address.
        #[arg(long, env = "WARDEN_GATEWAY_MAX_CONNECTIONS_PER_IP", default_value_t = 64)]
        max_connections_per_ip: usize,
        /// Chat calls in flight with one instance token (`0`: no cap). Over it the answer is a 429.
        #[arg(long, env = "WARDEN_GATEWAY_MAX_INFLIGHT_PER_TOKEN", default_value_t = 8)]
        max_inflight_per_token: usize,
        /// The longest a streamed answer may run, in seconds (`0`: no limit). Past it the stream is cut and settled with the
        /// usage seen so far.
        #[arg(long, env = "WARDEN_GATEWAY_MAX_STREAM_SECS", default_value_t = 600)]
        max_stream_secs: u64,
        /// Suspend an account that is charged more than this many USD within an hour, and send `account.suspended`.
        /// Resume it with `accounts resume`. Without it there is no hourly cap.
        #[arg(long, env = "WARDEN_GATEWAY_HOURLY_SPEND_CAP_USD")]
        hourly_spend_cap_usd: Option<String>,
        /// Serve TLS on the socket itself: the certificate chain, PEM, leaf first. Read again on the next handshake when the
        /// file changes, so a renewal needs no restart. Without `--tls-cert` and `--tls-key` the endpoint is plain HTTP, for a
        /// reverse proxy that does the TLS.
        #[arg(long, env = "WARDEN_GATEWAY_TLS_CERT", requires = "tls_key")]
        tls_cert: Option<PathBuf>,
        /// The private key of `--tls-cert`, PEM.
        #[arg(long, env = "WARDEN_GATEWAY_TLS_KEY", requires = "tls_cert")]
        tls_key: Option<PathBuf>,
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
    /// Delete the old rows now (the running gateway already does it once an hour).
    Prune {
        /// Keep delivered rows this many days; it must be longer than the daily summaries' window, or they would be sent twice.
        #[arg(long, default_value_t = warden_gateway::webhook::DELIVERED_KEEP_DAYS)]
        delivered_days: i64,
        /// Keep the rows given up on this many days.
        #[arg(long, default_value_t = warden_gateway::webhook::GIVEN_UP_KEEP_DAYS)]
        given_up_days: i64,
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

/// Resolves when the process is asked to stop: SIGTERM (what systemd and `docker stop` send) or SIGINT (Ctrl+C).
async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = terminate.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
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
        Command::Serve {
            listen,
            admin_secret,
            admin_secret_previous,
            webhook_url,
            webhook_secret,
            low_balance_usd,
            ip_rate_per_min,
            token_rate_per_min,
            trust_forwarded_for,
            max_connections,
            max_connections_per_ip,
            max_inflight_per_token,
            max_stream_secs,
            hourly_spend_cap_usd,
            tls_cert,
            tls_key,
        } => {
            let mut store = open_store(&cli)?;
            if let Some(low) = optional_usd(low_balance_usd)? {
                store = store.with_low_threshold(low);
            }
            if let Some(cap) = optional_usd(hourly_spend_cap_usd)? {
                store = store.with_hourly_spend_cap(cap);
            }
            let store = Arc::new(store);
            let catalog = Arc::new(load_catalog(&cli)?);
            for upstream in &catalog.upstreams {
                if upstream.api_key.get().is_none() {
                    tracing::warn!(
                        upstream = %upstream.id,
                        key = %upstream.api_key.origin(),
                        "no API key for this upstream (set `api_key_env` to a variable that is set, or `api_key_file` to a file that is not empty)"
                    );
                }
            }
            let gateway = Gateway::new(store.clone(), catalog.clone());
            let mut server = GatewayServer::bind(*listen, gateway, Arc::new(HttpUpstream::new())).await.with_context(|| format!("cannot listen on {listen}"))?;
            server = server.with_rate_limits(*ip_rate_per_min, *token_rate_per_min).with_trust_forwarded_for(*trust_forwarded_for);
            server = server.with_connection_limits(*max_connections, *max_connections_per_ip).with_inflight_limit(*max_inflight_per_token);
            if *max_stream_secs > 0 {
                server = server.with_max_stream(std::time::Duration::from_secs(*max_stream_secs));
            }
            // The two go together (clap's `requires`), so one is enough to tell. A bad file stops the start: a gateway that
            // meant to serve TLS must never come up answering in the clear.
            let secure = match (tls_cert, tls_key) {
                (Some(cert), Some(key)) => {
                    let tls = GatewayTls::from_pem_files(cert, key).with_context(|| format!("cannot load the TLS certificate {}", cert.display()))?;
                    server = server.with_tls(tls);
                    true
                }
                _ => false,
            };
            if *trust_forwarded_for {
                tracing::info!("trusting the last X-Forwarded-For entry as the client address: keep the gateway reachable only through the proxy");
            }
            let previous = admin_secret_previous.as_deref().filter(|s| !s.is_empty());
            match admin_secret.as_deref().filter(|s| !s.is_empty()) {
                Some(secret) => {
                    server = server.with_admin_secret(secret);
                    if let Some(previous) = previous {
                        server = server.with_previous_admin_secret(previous);
                        tracing::info!("rotating the admin secret: the previous one is still accepted");
                    }
                }
                None if previous.is_some() => bail!("--admin-secret-previous needs --admin-secret: it is the old one of a rotation"),
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
            if secure {
                tracing::info!(addr = %server.local_addr()?, models = catalog.models.len(), "gateway listening (TLS only)");
            } else {
                tracing::info!(
                    addr = %server.local_addr()?,
                    models = catalog.models.len(),
                    "gateway listening (plain HTTP: use --tls-cert and --tls-key, or put TLS in front of it)"
                );
            }
            // A cut mid-call is safe: the ledger is SQLite in WAL, a hold left behind expires, and a webhook still queued is sent
            // again by the next run (delivery is at least once).
            tokio::select! {
                _ = server.serve() => {}
                _ = stop_signal() => {}
            }
            tracing::info!("gateway stopped");
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
                WebhooksCommand::Prune { delivered_days, given_up_days } => {
                    if *delivered_days <= warden_gateway::webhook::SUMMARY_DAYS_BACK {
                        bail!("--delivered-days must be more than {} (the daily summaries' window), or they would be sent again", warden_gateway::webhook::SUMMARY_DAYS_BACK);
                    }
                    if *given_up_days < 0 {
                        bail!("--given-up-days cannot be negative");
                    }
                    let gone = store.prune_outbox(now(), delivered_days * 86_400, given_up_days * 86_400)?;
                    println!("deleted {gone} old webhook rows");
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
            for u in &catalog.upstreams {
                let state = if u.api_key.get().is_some() { "key found" } else { "NO KEY" };
                let budget = u.daily_budget.map_or("no daily budget".to_string(), |b| format!("daily budget {} USD", format_usd(b)));
                println!("upstream {:<16} {}   key: {} ({state})   {budget}", u.id, u.base_url, u.api_key.origin());
            }
            for m in &catalog.models {
                println!(
                    "{:<20} -> {}/{}   cost {}/{}   sale {}/{}   (USD per million tokens, in/out)   max {} tokens",
                    m.name, m.upstream, m.upstream_model, format_usd(m.cost_in), format_usd(m.cost_out), format_usd(m.sale_in), format_usd(m.sale_out), m.max_tokens
                );
                let weighted = m.strategy == warden_gateway::catalog::Strategy::Weighted;
                if !m.fallbacks.is_empty() {
                    let main_weight = if weighted { format!("   weight {}", m.weight) } else { String::new() };
                    println!("{:<20}    strategy {}{main_weight}", "", m.strategy.name());
                }
                for (n, f) in m.fallbacks.iter().enumerate() {
                    let weight = if weighted { format!("   weight {}", f.weight) } else { String::new() };
                    println!(
                        "{:<20}    fallback {} -> {}/{}   cost {}/{}   sale {}/{}{weight}",
                        "", n + 1, f.upstream, f.upstream_model, format_usd(f.cost_in), format_usd(f.cost_out), format_usd(f.sale_in), format_usd(f.sale_out)
                    );
                }
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
                let state = if a.active { "active".to_string() } else { format!("suspended ({})", a.suspended_reason.as_deref().unwrap_or("unknown")) };
                println!("{}  {:<20} {:<26} balance {}", a.id, a.name, state, format_usd(store.balance(&a.id)?));
            }
        }
        AccountsCommand::Show { account } => {
            let a = store.account(account)?.with_context(|| format!("no account {account}"))?;
            println!("account   {}", a.id);
            println!("name      {}", a.name);
            if a.active {
                println!("state     active");
            } else {
                // An account suspended before the reason was kept has neither.
                let reason = a.suspended_reason.as_deref().unwrap_or("unknown");
                match a.suspended_at {
                    Some(at) => println!("state     suspended ({reason}, since unix {at})"),
                    None => println!("state     suspended ({reason})"),
                }
            }
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

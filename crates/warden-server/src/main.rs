use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use warden_bootstrap::auto_sync::{SyncBackend, SyncRunner, AUTO_SYNC_INTERVAL, PAIRING_PORTS, PAIRING_TIMEOUT};
use warden_bootstrap::tasks::{check_tasks, next_run, run_task, task_status, TaskStore, Zone};
use warden_bootstrap::webhooks::{upsert_webhook, WebhookAuth, WebhookConfig};
use warden_bootstrap::{bootstrap, load_config_from_path, save_config, Overrides, TaskConfig};
use warden_core::tool::code_task::CodeEngineSlot;
use warden_server::chat_input::WhisperTranscriber;
use warden_server::{resolve_server_name, EmbeddedWebUi, HubTls, PairingStore, Server, WebAssets};

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Provider {
    Gemini,
    Openai,
}

impl From<Provider> for warden_bootstrap::Provider {
    fn from(p: Provider) -> Self {
        match p {
            Provider::Gemini => warden_bootstrap::Provider::Gemini,
            Provider::Openai => warden_bootstrap::Provider::Openai,
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "warden-server", version, about)]
struct Cli {
    /// Keep every Warden file (config, devices, API keys, members, conversations, the downloaded models and, unless
    /// `--vault-path` says otherwise, the vault) in this one folder instead of the OS config folder (P143): what a
    /// service or a container wants, so one volume holds all of it. Falls back to WARDEN_DATA_DIR. Applies to every
    /// command, since `devices`, `api-keys` and `sync` read the same files `serve` writes.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Starts listening for client connections (the long-running hub process).
    Serve(ServeArgs),
    /// Manage the persistent device pairing registry (Phase 9.3) — `CallDeviceTool` routing only
    /// works between devices that have been `approve`d here; a running `serve` process picks up
    /// changes made this way without needing a restart.
    Devices {
        #[command(subcommand)]
        action: DevicesAction,
    },
    /// The Warden API's keys (P12): what an OpenAI-compatible client presents as its bearer token
    /// to `http(s)://<hub>/v1`. A running `serve` sees a change on the next request.
    ApiKeys {
        #[command(subcommand)]
        action: ApiKeysAction,
    },
    /// Prints a fresh random pairing key (64 hex chars) for `serve --auth-key` /
    /// WARDEN_SERVER_AUTH_KEY.
    GenKey,
    /// Signed skill packages (P160): make a signing key, sign a package file with it, and see who a package says signed it. Others
    /// trust what you sign by listing your public key under `[[trusted_publishers]]` in their config.
    Package {
        #[command(subcommand)]
        action: PackageAction,
    },
    /// Vault sync (P61) — what `serve` does every 5 minutes on its own, and how a hub with no
    /// screen gets its vault key. Safe while `serve` runs: a round started here waits for the
    /// hub's to finish (one sync at a time per vault, across processes).
    Sync(SyncArgs),
    /// Scheduled tasks (P92): prompts an agent runs on its own, kept as `[[tasks]]` in the config
    /// file. Only a hub started with `serve --run-tasks` runs them; each run lands in the task's
    /// conversation, which every device lists. A running `serve` sees changes made here within
    /// half a minute.
    Tasks(TasksArgs),
    /// Incoming webhooks (P105): a prompt an agent runs when something `POST`s to `/hooks/<id>` on this hub with
    /// the webhook's token — kept as `[[webhooks]]` in the config file, with the tokens outside it (they don't
    /// sync). Each call lands in the webhook's conversation, which every device lists. A running `serve` sees
    /// changes made here on the next call.
    Webhooks(WebhooksArgs),
    /// Makes this machine a node (P93): it connects to a hub and lends its shell and/or a folder of
    /// files to the hub's agents. The hub still decides who may use it (`warden-server nodes`).
    Node(NodeArgs),
    /// On the hub: what agents may do with each node (P93) — the `[[nodes]]` entries.
    Nodes {
        #[command(subcommand)]
        action: NodesAction,
        /// Path to the config file (TOML), as in `serve`.
        #[arg(long, global = true)]
        config: Option<String>,
    },
    /// The people of this workspace besides you (P84), kept as `[[users]]` in the config file. Each
    /// signs in with their username and password (the web, the phone) and has their own vault and
    /// conversations on this hub. You stay the owner: whoever holds the pairing key.
    Users {
        #[command(subcommand)]
        action: UsersAction,
        /// Path to the config file (TOML), as in `serve`.
        #[arg(long, global = true)]
        config: Option<String>,
    },
    /// Folders of your vault shared with members (P84), kept as `[[spaces]]` in the config file. A
    /// member sees one at `compartilhado/<name>/` in their own vault, and never the rest of yours.
    Spaces {
        #[command(subcommand)]
        action: SpacesAction,
        /// Path to the config file (TOML), as in `serve`.
        #[arg(long, global = true)]
        config: Option<String>,
    },
    /// Who besides a person may open their encrypted data (P84): the workspace's recovery policy, and
    /// the owner's part in it. `private` (the default) means nobody; `consent` means you and the
    /// person's recovery code together; `company` means you alone, recorded and told to the person.
    Recovery {
        #[command(subcommand)]
        action: RecoveryAction,
        /// Path to the config file (TOML), as in `serve`.
        #[arg(long, global = true)]
        config: Option<String>,
    },
    /// Copies the members' data (P84) into a folder. It's encrypted with each member's own key, so
    /// you can keep the copy anywhere and still can't read it; with it, a member you removed by
    /// mistake, or a hub you lost, comes back and opens with their password or recovery code.
    Backup {
        /// An empty folder to write the backup into.
        #[arg(long)]
        out: PathBuf,
        /// Only this member; every member by default.
        #[arg(long)]
        user: Option<String>,
        /// Path to the config file (TOML), as in `serve`.
        #[arg(long)]
        config: Option<String>,
    },
    /// Puts members back from a backup made by `backup`: their data, and their entry in the config
    /// file if it's gone. Restart the hub afterwards if it's running.
    Restore {
        /// The folder `backup` wrote.
        from: PathBuf,
        /// Only this member; everyone in the backup by default.
        #[arg(long)]
        user: Option<String>,
        /// Replace data this hub already has, or a member entry with another key.
        #[arg(long)]
        force: bool,
        /// Path to the config file (TOML), as in `serve`.
        #[arg(long)]
        config: Option<String>,
    },
    /// Copies everything that is yours (P141) into a folder: the config, the vault, the conversations, the members
    /// (still encrypted), the MCP logins and the scheduled tasks. What belongs to the operator or the destination stays out:
    /// paired devices, API keys, webhook tokens, the sync key, the spend ledger and the audit logs. Use it to leave a hosted
    /// instance for a self-hosted one, or the other way round. Stop the hub first, so no file changes while it's copied.
    Export {
        /// An empty folder (or one that doesn't exist yet) to write the export into. `tar czf` makes it one file.
        out: PathBuf,
        /// Take the providers' API keys along (`config.secrets.toml`). Not the hosted gateway's, ever. Without it you
        /// enter your own keys at the destination. The config can hold other tokens (bots, git sync, MCP variables);
        /// this doesn't remove them, so keep the export private.
        #[arg(long)]
        with_secrets: bool,
    },
    /// Puts an export made by `export` into this hub's folder (`--data-dir`), checking its version and every checksum first.
    /// Refuses a folder that already has a config or a vault unless `--force`. Stop the hub first.
    Import {
        /// The folder `export` wrote.
        package: PathBuf,
        /// Write over a folder that already has a config or a vault.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
enum SpacesAction {
    /// Every space, its folder and who reads and writes in it.
    List,
    /// Shares a folder of your vault, or changes who is in a space that already exists.
    Add {
        /// Its name, which members see it under: lowercase letters, digits, - or _.
        id: String,
        /// The folder in your vault, relative to its root (`casa`, `viagens/2026`).
        #[arg(long)]
        folder: String,
        /// A member who reads it (repeat for more), or `*` for everyone.
        #[arg(long = "reader")]
        readers: Vec<String>,
        /// A member who also writes in it (repeat for more), or `*` for everyone.
        #[arg(long = "writer")]
        writers: Vec<String>,
    },
    /// Stops sharing a folder. The folder and its notes stay in your vault.
    Remove { id: String },
}

#[derive(Subcommand, Debug)]
enum RecoveryAction {
    /// Shows the workspace's recovery policy, or sets it (`private`, `consent` or `company`). A policy
    /// that needs your recovery key makes one the first time and shows it **once**: write it down, it is
    /// never kept on the hub. Each member's data follows at their next sign-in, and a change to a weaker
    /// policy waits for their yes.
    Policy {
        /// `private`, `consent` or `company`. Left out, it only shows the current one.
        policy: Option<String>,
        /// Replaces your recovery key with a new one, shown once.
        #[arg(long)]
        new_key: bool,
    },
    /// Opens a member's data with your recovery key and gives them a new provisional password (shown
    /// once). Under `consent` their recovery code is needed too; under `company` it isn't. It is recorded,
    /// and they are told at their next sign-in.
    Recover {
        id: String,
        /// Your recovery key, as it was shown when it was made.
        #[arg(long)]
        key: String,
        /// The member's recovery code (`consent` only).
        #[arg(long)]
        code: Option<String>,
    },
    /// The recoveries made so far, per member.
    Log,
}

#[derive(Subcommand, Debug)]
enum UsersAction {
    /// Every member, and whether they still have to pick their own password.
    List,
    /// Adds a member with a provisional password (printed once) they change when they first sign in.
    Add {
        /// Their username: lowercase letters, digits, - or _.
        id: String,
        /// How they're shown.
        #[arg(long)]
        name: String,
    },
    /// Gives a member a new provisional password (printed once), for a forgotten one.
    ResetPassword { id: String },
    /// Makes an invite (printed once) for a member to link their TruthID: they sign in on the web, open
    /// "Link TruthID" and give the code and their TruthID username. Good for 7 days, single use.
    Invite { id: String },
    /// Unties a member's TruthID and cancels an open invite.
    UnlinkTruthid { id: String },
    /// Removes a member and revokes their devices. Their vault and conversations stay on disk. If
    /// their data is encrypted, the member is kept under `users removed` — with the key that opens
    /// it — so `users restore` can bring them back.
    Remove { id: String },
    /// Members you removed whose encrypted data is still on disk.
    Removed,
    /// Brings a removed member back, with the same password and all their data.
    Restore { id: String },
    /// Deletes a removed member's data for good — their vault and conversations — and the key with it.
    Purge {
        id: String,
        /// Confirms it: this can't be undone.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(clap::Args, Debug)]
struct NodeArgs {
    /// The hub to join, e.g. wss://hub.tailnet.ts.net:7420 (or ws:// on a trusted network).
    #[arg(long)]
    hub: String,
    /// The hub's pairing key — only the first time; the node keeps the token the hub gives it.
    /// Falls back to WARDEN_SERVER_AUTH_KEY.
    #[arg(long)]
    auth_key: Option<String>,
    /// How this node shows up on the hub. Defaults to the machine's host name.
    #[arg(long)]
    name: Option<String>,
    /// One line for the agents: what this machine is and what it's good for.
    #[arg(long, default_value = "")]
    description: String,
    /// A label agents can pick nodes by, like gpu or home. Repeatable.
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// Lend this machine's shell: agents allowed on the hub can run any command as this user.
    #[arg(long)]
    shell: bool,
    /// Share this folder: agents allowed on the hub can read and write text files inside it (never
    /// outside). Also where --shell commands start.
    #[arg(long)]
    files: Option<PathBuf>,
    /// Lend one of this machine's MCP servers, by its name in `[[mcp_servers]]` of this machine's
    /// config file. Repeatable; a server not named here isn't lent.
    #[arg(long = "mcp")]
    mcp: Vec<String>,
    /// Lend one of this machine's model providers (its local Ollama, say), by its id in
    /// `[[providers]]` of this machine's config file. Repeatable. On the hub, a `[[providers]]` entry
    /// with `kind = "node"`, this node's id and that provider's id uses it.
    #[arg(long = "model")]
    models: Vec<String>,
    /// This machine's config file (TOML), where --mcp looks the servers up. Defaults to the OS config dir.
    #[arg(long)]
    config: Option<String>,
}

#[derive(Subcommand, Debug)]
enum NodesAction {
    /// Every node in `[[nodes]]` and whether it's on, for which agents, and whether it asks first.
    List,
    /// Lets agents use this node (its device id, as `devices list` shows it). It must also be approved
    /// there (`devices approve`).
    Allow {
        id: String,
        /// Only these agents (repeatable). Without it, every agent.
        #[arg(long = "agent")]
        agents: Vec<String>,
        /// Ask a person before every command or file operation on it.
        #[arg(long)]
        approval: bool,
    },
    /// Stops agents from using this node (keeps its entry).
    Deny { id: String },
}

#[derive(clap::Args, Debug)]
struct TasksArgs {
    #[command(subcommand)]
    action: TasksCommand,

    /// Path to the config file (TOML), as in `serve`.
    #[arg(long, global = true)]
    config: Option<String>,
}

#[derive(Subcommand, Debug)]
enum TasksCommand {
    /// Every task: its schedule, agent, whether it's on, the last run and the next one.
    List,
    /// Adds a task. Give exactly one of --every, --cron or --once.
    #[command(group(clap::ArgGroup::new("schedule").required(true).args(["every", "cron", "once"])))]
    Add {
        /// 1-59 letters, digits, '-' or '_'.
        id: String,
        /// What the agent is asked on every run.
        #[arg(long)]
        prompt: String,
        /// The agent that runs it (none: no persona).
        #[arg(long)]
        agent: Option<String>,
        /// An interval counted from the last run: 30m, 2h, 1d.
        #[arg(long)]
        every: Option<String>,
        /// Five-field cron, e.g. "0 8 * * 1-5" for 8:00 on weekdays.
        #[arg(long)]
        cron: Option<String>,
        /// Once, at this local date and time: 2026-10-01T09:00.
        #[arg(long)]
        once: Option<String>,
        /// IANA time zone for --cron and --once (e.g. America/Sao_Paulo). Default: this machine's.
        #[arg(long)]
        timezone: Option<String>,
    },
    /// Stops a task from running until `resume`.
    Pause { id: String },
    /// Switches a paused task back on; it counts from now, without making up for what it skipped.
    Resume { id: String },
    /// Removes a task. Its conversation stays, for whoever wants to read it.
    Remove { id: String },
    /// Runs a task now, in this process, and prints the answer. Devices connected to a running
    /// `serve` see it when they next reload their conversation list.
    Run {
        id: String,
        /// Path to the markdown vault, as in `serve`.
        #[arg(long)]
        vault_path: Option<String>,
    },
}

#[derive(clap::Args, Debug)]
struct WebhooksArgs {
    #[command(subcommand)]
    action: WebhooksCommand,

    /// Path to the config file (TOML), as in `serve`.
    #[arg(long, global = true)]
    config: Option<String>,
}

#[derive(Subcommand, Debug)]
enum WebhooksCommand {
    /// Every webhook: whether it's on, its agent and whether it has a token.
    List,
    /// Adds a webhook. It takes no calls until it has a credential (`webhooks token <id>`).
    Add {
        /// 1-54 letters, digits, '-' or '_'. It is the end of the URL: /hooks/<id>.
        id: String,
        /// What the agent is asked on every call; the request body is added after it, as data.
        #[arg(long)]
        prompt: String,
        /// The agent that runs it (none: no persona). Give it few tools: the body of a call comes from outside.
        #[arg(long)]
        agent: Option<String>,
        /// How the caller proves itself: `token` (a bearer token, the default) or `hmac` (a signature of the body, made with
        /// a secret the service is given: GitHub's X-Hub-Signature-256, Stripe's Stripe-Signature or Slack's X-Slack-Signature).
        #[arg(long, default_value = "token")]
        auth: String,
    },
    /// Refuses calls to a webhook until `resume`.
    Pause { id: String },
    /// Takes calls again.
    Resume { id: String },
    /// Removes a webhook and its token. Its conversation stays, for whoever wants to read it.
    Remove { id: String },
    /// A new credential for a webhook — a token, or a signing secret for an `--auth hmac` one — printed once. It replaces
    /// the old one, which stops working at once.
    Token { id: String },
    /// Takes a webhook's credential away: calls get 401 from the next one on, and the webhook stays.
    Revoke { id: String },
}

#[derive(clap::Args, Debug)]
struct SyncArgs {
    #[command(subcommand)]
    action: SyncCommand,

    /// Path to the config file (TOML), as in `serve`.
    #[arg(long, global = true)]
    config: Option<String>,

    /// Path to the markdown vault, as in `serve`.
    #[arg(long, global = true)]
    vault_path: Option<String>,
}

#[derive(Subcommand, Debug)]
enum SyncCommand {
    /// Where the vault syncs to, what's waiting to go, and when it last synced.
    Status,
    /// Runs one round now: git pulls then pushes, Arweave only pulls.
    Now,
    /// Makes this hub the first device of a sync group (a fresh vault key).
    Init,
    /// Receives the vault key from a device showing a pairing code (the desktop's Sync screen, or
    /// `/sync pair` in the CLI).
    Pair {
        code: String,
        /// That device's IPv4 address (a Tailscale one works) — needed whenever it isn't on this
        /// hub's LAN, which the default sweep covers.
        #[arg(long)]
        host: Option<std::net::Ipv4Addr>,
    },
    /// Shows a pairing code another device joins this hub's sync group with, and waits for it.
    /// Safe while `serve` runs: it only reads the vault key.
    Host,
}

#[derive(Subcommand, Debug)]
enum DevicesAction {
    /// List every device that has ever said `Hello`, with its pairing status.
    List,
    /// Approve a device for routing — it must have connected at least once already.
    Approve { device_id: String },
    /// Revoke a device — its token stops working (so does re-pairing under the same id), and a
    /// running `serve` closes its open connection within a few seconds. To also keep it from
    /// pairing again under a *new* id, change the pairing key too: pass `--rotate-key` (or run
    /// `devices rotate-key`), no restart needed.
    Revoke {
        device_id: String,
        /// Also change the pairing key, as `devices rotate-key` does, and print the new one.
        #[arg(long)]
        rotate_key: bool,
    },
    /// Change the pairing key — new devices need the new one to pair, and an old key stops opening the hub's
    /// administration. A running `serve` takes it on the next use, with no restart; devices that already
    /// hold a token keep working. The new key is written beside the device registry and printed once.
    RotateKey,
    /// Make a one-use code that pairs one device without the pairing key. It is shown once and expires, so a device
    /// that was revoked cannot come back under a new id with it.
    Invite {
        /// Who it is for; shown by `devices list` once the device has paired.
        #[arg(long)]
        name: String,
        /// How long the code works.
        #[arg(long, default_value_t = 15)]
        ttl_minutes: u64,
    },
    /// List the invites not used or expired yet.
    Invites,
    /// Cancel an unused invite by its name.
    Uninvite { name: String },
}

#[derive(Subcommand, Debug)]
enum PackageAction {
    /// Makes a signing key: writes the private half to FILE (readable only by you; never overwritten, back it up) and prints the
    /// public half to give to the people who should trust you.
    Keygen { file: PathBuf },
    /// Signs the package PACKAGE with the key in --key. Replaces the file unless --out names another one.
    Sign {
        package: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Says whether PACKAGE is signed and by whom, against the `[[trusted_publishers]]` of the config. A signature that does not
    /// check out is an error.
    Verify {
        package: PathBuf,
        /// The config file to read `[[trusted_publishers]]` from (this machine's by default).
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum ApiKeysAction {
    /// Every key: id, name, its first characters, when it was created and last used.
    List,
    /// A new key named NAME — printed once, never stored.
    Create {
        name: String,
        /// Binds the key to this agent: it then only speaks as it (`warden` and `warden/<this
        /// agent>` both mean it, any other agent is refused). Without it the key is general.
        #[arg(long)]
        agent: Option<String>,
        /// Makes the key a member's (P84): a call with it runs in their space, with their vault and conversations. The
        /// member has to exist (`users add`).
        #[arg(long)]
        user: Option<String>,
        /// The config file the agent and the member are checked against, as in `serve`.
        #[arg(long)]
        config: Option<String>,
    },
    /// Sets which providers and combos key ID may call straight, with no agent in between (P131): the model is then
    /// `provider/<id>` or `combo/<id>`. Replaces the list; with no MODELS the key can't call any. `*` stands for every provider
    /// and combo that doesn't reach another machine (a `node` or `cloud` one has to be named). Only a general key of the owner can.
    Raw {
        id: String,
        /// Provider or combo ids from the config, or `*`.
        models: Vec<String>,
        /// The config file the ids are checked against, as in `serve`.
        #[arg(long)]
        config: Option<String>,
    },
    /// Removes a key; clients using it get 401 from the next request on.
    Revoke { id: String },
}

/// Warden's server-side WebSocket endpoint (Phase 9/7.3): hosts a real `Orchestrator` (same
/// `bootstrap()` every other channel uses) and answers chat over `ws://`, or only over `wss://`
/// once a certificate is configured (P36 — `--tailscale-cert`, or `--tls-cert`/`--tls-key`).
#[derive(Parser, Debug)]
struct ServeArgs {
    /// Address to listen on.
    #[arg(long, default_value = "0.0.0.0:7420")]
    listen: SocketAddr,

    /// Serve only wss://, with a certificate for this machine's Tailscale MagicDNS name, fetched
    /// via `tailscale cert` at startup and renewed daily (no restart needed). Needs HTTPS
    /// certificates enabled for the tailnet; clients then connect to wss://<name>.<tailnet>.ts.net.
    #[arg(long, conflicts_with_all = ["tls_cert", "tls_key"])]
    tailscale_cert: bool,

    /// Serve only wss://, with this PEM certificate chain (leaf first). Re-read whenever the file
    /// changes, so renewing it needs no restart. Requires --tls-key.
    #[arg(long, requires = "tls_key")]
    tls_cert: Option<PathBuf>,

    /// PEM private key for --tls-cert.
    #[arg(long, requires = "tls_cert")]
    tls_key: Option<PathBuf>,

    /// Host name --tls-cert is valid for, advertised to LAN discovery so clients know which
    /// wss:// URL to use. Optional; --tailscale-cert fills it in by itself.
    #[arg(long, requires = "tls_cert")]
    tls_host: Option<String>,

    /// Don't serve the web interface (P78). By default, opening this hub's address in a browser
    /// (http://, or https:// with TLS) shows the Warden web UI, which pairs as one more device.
    #[arg(long)]
    no_web_ui: bool,

    /// Pairing key — what a new client presents in its first Hello to get its own device token
    /// (P36). Devices already holding a token keep working if this changes, so rotating it only
    /// affects new pairings. Falls back to WARDEN_SERVER_AUTH_KEY if not passed (env wins if both
    /// are set). Must be at least 32 characters; `warden-server gen-key` prints one.
    #[arg(long)]
    auth_key: Option<String>,

    /// Path to the markdown vault (memory). Overrides the config file; defaults to
    /// ~/Warden/vault (a background process has no predictable cwd, same as warden-telegram).
    #[arg(long)]
    vault_path: Option<String>,

    /// Which model provider to talk to. Overrides the config file; defaults to gemini.
    #[arg(long, value_enum)]
    provider: Option<Provider>,

    /// Model name passed to the provider. Overrides the config file; provider-specific default otherwise.
    #[arg(long)]
    model: Option<String>,

    /// Path to the config file (TOML). Defaults to the OS config dir (e.g. ~/.config/warden/config.toml on Linux).
    #[arg(long)]
    config: Option<String>,

    /// Display name this hub answers with in HelloAck and to a LAN discovery sweep (Phase 9.1,
    /// redefined — see `discover_hubs`). Falls back to WARDEN_SERVER_NAME, then the OS hostname.
    #[arg(long)]
    server_name: Option<String>,

    /// Run the scheduled tasks (`warden-server tasks`, P92) on this hub. Off by default: the config
    /// file syncs, so turn it on in exactly one hub — the one that's always up.
    #[arg(long)]
    run_tasks: bool,

    /// Let the web settings change what reaches this machine (P119): the shell tool, MCP servers (a
    /// process this hub starts), SSH hosts, the vault and generated-files folders and the embedded hub.
    /// Off by default. Even on, a save needs the pairing key and an encrypted (https://) or local
    /// connection, and the hub logs each change. Turn it on only for a hub you reach over TLS.
    #[arg(long)]
    allow_machine_settings: bool,

    /// Don't load the two search models (the vault's and the history's) when the hub starts: each loads at its first
    /// search instead. An idle hub then weighs ~25 MB rather than ~960 MB (P146), which is what a hub hosting many
    /// instances wants; the cost is that the first turn that searches waits for the model.
    #[arg(long)]
    no_embedding_warm_up: bool,

    /// How the hub writes its log to stderr (P143): `text` for a person, `json` for one object per line, which a
    /// collector reads. `RUST_LOG` filters either one (`RUST_LOG=warn`); without it, `info` and above. Falls back to
    /// WARDEN_LOG_FORMAT.
    #[arg(long, value_enum, env = "WARDEN_LOG_FORMAT", default_value = "text")]
    log_format: warden_server::logging::LogFormat,

    /// On SIGTERM or SIGINT (Ctrl+C) the hub stops accepting connections and, for this many seconds, lets the ones open
    /// finish what they are doing before it exits (P143); anything still running then is cut. 0 exits at once.
    #[arg(long, default_value_t = 5)]
    shutdown_grace_secs: u64,

    /// Let go of the search models (the vault's and the history's) after this many seconds without a search (P146, P156): an
    /// idle hub then weighs ~76 MB instead of ~475 MB (vault) or ~830 MB (history), and the next search loads the model again
    /// (~1.5 s of CPU, the history's ~0.7 s). 0 keeps them for good.
    #[arg(long, default_value_t = 600)]
    embedding_idle_secs: u64,
}

/// Same fallback warden-telegram/the desktop app use — a background process launched by a
/// terminal, systemd unit, etc. has no cwd a user would recognize either.
fn default_vault_path() -> PathBuf {
    // With a data dir (P143) the vault lives in it too, so one volume holds everything.
    if let Some(dir) = warden_core::paths::data_dir() {
        return dir.join("vault");
    }
    dirs::home_dir().unwrap_or_default().join("Warden").join("vault")
}


/// The runner `serve` and `sync` share: this hub's config file and the vault it resolves to.
fn sync_runner(config: Option<&str>, vault_path: Option<String>) -> anyhow::Result<SyncRunner> {
    let config_path = config.map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let file = warden_bootstrap::load_config_from_path(&config_path, config.is_some())?;
    let overrides = Overrides { vault_path, ..Default::default() };
    let vault_path = warden_bootstrap::resolve_vault_path(&overrides, &file, default_vault_path());
    Ok(SyncRunner::with_default_paths(vault_path, config_path))
}

async fn run_sync_command(args: SyncArgs) -> anyhow::Result<()> {
    let runner = sync_runner(args.config.as_deref(), args.vault_path)?;
    match args.action {
        SyncCommand::Status => {}
        SyncCommand::Now => {
            let report = runner.run_once().await;
            if let Some(p) = &report.pulled {
                println!("pulled: {} written, {} removed{}", p.files_written, p.files_deleted, if p.config_updated { ", config.toml updated" } else { "" });
            }
            if let Some(p) = &report.pushed {
                println!("pushed: commit {} ({} file(s))", &p.commit_sha[..12.min(p.commit_sha.len())], p.files_changed);
            }
            if let Some(err) = report.error {
                anyhow::bail!(err);
            }
            if report.pulled.is_none() && report.pushed.is_none() {
                println!("nothing to sync");
            }
        }
        SyncCommand::Init => {
            runner.init_fresh().await?;
            println!("vault key created — other devices pair with this one to get it");
        }
        SyncCommand::Pair { code, host } => {
            runner.pair_join(&code, host).await?;
            println!("paired — this hub now has the vault key");
        }
        SyncCommand::Host => {
            let host = runner.pairing_host().await?;
            let port = host.local_addr()?.port();
            println!("pairing code: {}", host.code());
            println!(
                "type it on the other device (desktop Sync screen, or `/sync pair <code> <this hub's IP>` in the terminal); \
                 listening on port {port} for {} min",
                PAIRING_TIMEOUT.as_secs() / 60
            );
            println!("from outside this hub's LAN, give the other device this hub's IP (a Tailscale one works) and allow ports {}-{}", PAIRING_PORTS[0], PAIRING_PORTS[PAIRING_PORTS.len() - 1]);
            host.wait_for_join().await?;
            println!("paired — the other device now has the vault key");
        }
    }
    let state = runner.state()?;
    let backend = match state.backend {
        SyncBackend::NotSetUp => "not set up (no vault key yet: `warden-server sync init` or `sync pair <code>`)",
        SyncBackend::Git => "git",
        SyncBackend::Arweave => "Arweave (pull only here; pushing needs the TruthID phone)",
    };
    println!("backend: {backend}");
    if let Some(remote) = &state.git_remote {
        println!("git remote: {remote}");
    }
    println!(
        "pending: {} file(s){}{}",
        state.pending_vault_changes,
        if state.pending_config_changed { " + config.toml" } else { "" },
        if state.pending_oauth_changes > 0 { format!(" + {} MCP login(s)", state.pending_oauth_changes) } else { String::new() }
    );
    match state.last_synced_at_ms {
        Some(ms) => {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(ms);
            println!("last synced: {} min ago", (now - ms).max(0) / 60_000);
        }
        None => println!("last synced: never"),
    }
    if state.backend == SyncBackend::Arweave {
        println!("no [git_sync] in the config: set one to sync both ways from this hub");
    }
    Ok(())
}

/// Settings over the network (P78) for this process: the same config file and the same flags it
/// started with, so a reload after a save builds exactly what a restart would.
struct ServeSettings {
    config_path: PathBuf,
    explicit_config: Option<String>,
    overrides: Overrides,
    /// Where the hub's code engine goes once it exists, for the `code_task` of every orchestrator built here.
    code_engine: CodeEngineSlot,
    /// The models a code task may name, refilled by every build so the engine's route follows the settings.
    engine_models: warden_server::openai_api::EngineModelSlot,
}

/// What `bootstrap` builds, plus the tools that need the hub (the code engine, which needs the hub's model route).
async fn bootstrap_hub(config: Option<&str>, overrides: Overrides, code_engine: &CodeEngineSlot, engine_models: &warden_server::openai_api::EngineModelSlot) -> anyhow::Result<warden_core::orchestrator::Orchestrator> {
    let mut orchestrator = bootstrap(config, overrides, default_vault_path()).await?;
    // P89 — an agent given `code_task` hands tasks to the engine of a code project; nobody has it until a person lists it.
    let models = warden_bootstrap::load_config(config).ok().and_then(|config| warden_bootstrap::model_choices(&config));
    *engine_models.write().unwrap_or_else(|e| e.into_inner()) = models.clone();
    warden_server::code_turns::register_code_task(&mut orchestrator, code_engine, models);
    Ok(orchestrator)
}

#[async_trait::async_trait]
impl warden_server::SettingsHost for ServeSettings {
    fn config_path(&self) -> PathBuf {
        self.config_path.clone()
    }

    async fn build(&self) -> anyhow::Result<warden_core::orchestrator::Orchestrator> {
        bootstrap_hub(self.explicit_config.as_deref(), self.overrides.clone(), &self.code_engine, &self.engine_models).await
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if let Some(model) = &self.overrides.model {
            notes.push(format!("This hub was started with --model {model}, which wins over the active provider's model."));
        }
        if let Some(provider) = self.overrides.provider {
            notes.push(format!("This hub was started with --provider {provider:?}, which only applies while no providers are saved."));
        }
        notes
    }
}

fn devices_path() -> anyhow::Result<PathBuf> {
    warden_bootstrap::default_server_devices_path().context("could not determine the OS config directory for the device registry")
}

fn run_devices_command(action: DevicesAction) -> anyhow::Result<()> {
    let store = PairingStore::new(devices_path()?);
    match action {
        DevicesAction::List => {
            let devices = store.list()?;
            if devices.is_empty() {
                println!("no devices have connected to this server yet");
                return Ok(());
            }
            for (device_id, device) in devices {
                let owner = device.user.as_deref().map(|user| format!("member {user}")).unwrap_or_else(|| "root".into());
                let via = device.invite_name.as_deref().map(|name| format!("invite '{name}'")).unwrap_or_default();
                println!(
                    "{device_id}\t{}\t{}\t{owner}\t{via}\tfirst seen {}\tlast seen {}",
                    device.device_name,
                    device.status,
                    ago(device.first_seen_ms),
                    ago(device.last_seen_ms)
                );
            }
        }
        DevicesAction::Invite { name, ttl_minutes } => {
            anyhow::ensure!(ttl_minutes > 0, "--ttl-minutes must be at least 1");
            let code = store.create_invite(&name, std::time::Duration::from_secs(ttl_minutes * 60))?;
            println!("invite '{name}' (works once, for {ttl_minutes} minutes; shown once):\n{code}");
            println!("the device puts it where it would put the pairing key; a running server takes it with no restart");
        }
        DevicesAction::Invites => {
            let invites = store.list_invites()?;
            if invites.is_empty() {
                println!("no invites waiting");
            }
            for invite in invites {
                println!("{}\texpires in {}", invite.name, until(invite.expires_ms));
            }
        }
        DevicesAction::Uninvite { name } => {
            if store.cancel_invite(&name)? {
                println!("invite '{name}' cancelled");
            } else {
                println!("no unused invite called '{name}'");
            }
        }
        DevicesAction::Approve { device_id } => {
            store.approve(&device_id)?;
            println!("device '{device_id}' approved — it can now be used as a CallDeviceTool caller or target");
        }
        DevicesAction::Revoke { device_id, rotate_key } => {
            store.revoke(&device_id)?;
            println!("device '{device_id}' revoked — its token no longer works and a running server closes its connection shortly");
            if rotate_key {
                rotate_pairing_key()?;
            } else {
                println!("it cannot pair again without a new invite (the pairing key only pairs a hub's first device), but whoever knows the pairing key still administers the hub — `devices rotate-key` changes it");
            }
        }
        DevicesAction::RotateKey => rotate_pairing_key()?,
    }
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

fn span(ms: i64) -> String {
    match ms.max(0) / 1000 {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn ago(ms: i64) -> String {
    format!("{} ago", span(now_ms() - ms))
}

fn until(ms: i64) -> String {
    span(ms - now_ms())
}

/// Where the pairing key can be changed while the hub runs: beside the device registry (P36 c).
fn pairing_key_path() -> anyhow::Result<PathBuf> {
    Ok(warden_server::pairing_key::pairing_key_path_beside(&devices_path()?))
}

fn rotate_pairing_key() -> anyhow::Result<()> {
    let path = pairing_key_path()?;
    let key = warden_server::pairing_key::rotate_at(&path)?;
    println!("new pairing key (shown once, also saved in {}):\n{key}", path.display());
    println!("a running server takes it on its next use, with no restart; devices that already hold a token keep working, and new ones need this key");
    Ok(())
}

fn tasks_store() -> anyhow::Result<TaskStore> {
    let dir = warden_bootstrap::default_server_tasks_dir().context("could not determine the OS config directory for scheduled tasks")?;
    Ok(TaskStore::new(dir))
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default()
}

async fn run_node_command(args: NodeArgs) -> anyhow::Result<()> {
    // The node's reconnection notes are `tracing` events, which go nowhere without a subscriber (P143 d).
    warden_bootstrap::logging::init_compact();
    anyhow::ensure!(
        args.shell || args.files.is_some() || !args.mcp.is_empty() || !args.models.is_empty(),
        "a node has to lend something — pass --shell, --files <folder>, --mcp <server>, --model <provider>, or a mix"
    );
    if let Some(dir) = &args.files {
        anyhow::ensure!(dir.is_dir(), "--files {} is not a folder", dir.display());
    }
    let name = args.name.clone().unwrap_or_else(|| resolve_server_name(None));
    let identity_path = warden_server::node_client::default_node_identity_path().context("could not determine the OS config directory")?;
    let identity = warden_server::node_client::NodeIdentity::load_or_create(&identity_path, &name)?;
    let config_path = args.config.as_deref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let mcp_tools = warden_server::node_client::lend_mcp_servers(&args.mcp, &config_path, args.config.is_some()).await?;
    let models = warden_server::node_client::lend_models(&args.models, &config_path, args.config.is_some())?;
    let local = std::sync::Arc::new(warden_server::node_client::LocalNode::new(args.shell, args.files.clone()).with_mcp_tools(mcp_tools).with_models(models));
    let session = warden_server::node_client::NodeSession {
        hub_url: args.hub.clone(),
        name,
        auth_key: std::env::var("WARDEN_SERVER_AUTH_KEY").ok().or(args.auth_key.clone()).unwrap_or_default(),
        offer: local.offer(args.description.clone(), args.tags.clone()),
        identity_path: Some(identity_path),
    };
    let mut lent = Vec::new();
    if args.shell {
        lent.push("its shell".to_string());
    }
    if let Some(dir) = &args.files {
        lent.push(format!("the folder {}", dir.display()));
    }
    if !args.mcp.is_empty() {
        lent.push(format!("{} MCP tool(s) from {}", session_tool_count(&local), args.mcp.join(", ")));
    }
    if !args.models.is_empty() {
        lent.push(format!("the model(s) {}", args.models.join(", ")));
    }
    let lends = lent.join(", ");
    eprintln!("warden-server node: '{}' ({}) lends {lends}", session.name, identity.device_id);
    warden_server::node_client::run_node(session, identity, local, None).await
}

fn session_tool_count(local: &warden_server::node_client::LocalNode) -> usize {
    local.offer(String::new(), Vec::new()).mcp_tools.len()
}

fn run_users_command(action: UsersAction, config: Option<String>) -> anyhow::Result<()> {
    use warden_bootstrap::users::{add_user, generate_temp_password, purge_removed_user, remove_user, reset_password, restore_user};
    let config_path = config.as_deref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let mut file = load_config_from_path(&config_path, config.is_some())?;
    match action {
        UsersAction::List => {
            if file.users.is_empty() {
                println!("no one else yet — add someone with `warden-server users add <username> --name \"Their Name\"`");
            }
            for user in &file.users {
                let password = if user.must_change_password { "provisional password" } else { "own password" };
                let data = match (&user.key, user.key_needs_recovery) {
                    (None, _) => "not encrypted yet",
                    (Some(_), true) => "encrypted, needs the recovery code",
                    (Some(_), false) => "encrypted",
                };
                let truthid = match (&user.truthid, &user.invite) {
                    (Some(link), _) => format!("TruthID @{}", link.username),
                    (None, Some(_)) => "invite open".to_string(),
                    (None, None) => "no TruthID".to_string(),
                };
                println!("{}\t{}\t{password}\t{data}\t{truthid}", user.id, user.name);
            }
        }
        UsersAction::Add { id, name } => {
            let password = generate_temp_password();
            add_user(&mut file, &id, &name, &password)?;
            save_config(&config_path, &file)?;
            let id = &file.users.last().expect("just added").id;
            println!("'{id}' added. Provisional password (shown only now): {password}");
            println!("They sign in on the web or the phone with the username '{id}' and this password, then pick their own.");
        }
        UsersAction::Invite { id } => {
            let code = warden_bootstrap::users::create_invite(&mut file, &id, warden_server::people::unix_now())?;
            save_config(&config_path, &file)?;
            println!("Invite for '{id}' (shown only now, good for 7 days, single use): {code}");
            println!("They sign in, open \"Link TruthID\" and give this code and their TruthID username.");
        }
        UsersAction::UnlinkTruthid { id } => {
            warden_bootstrap::users::unlink_truthid(&mut file, &id)?;
            save_config(&config_path, &file)?;
            println!("'{id}' has no TruthID linked now, and no open invite");
        }
        UsersAction::ResetPassword { id } => {
            let password = generate_temp_password();
            reset_password(&mut file, &id, &password)?;
            save_config(&config_path, &file)?;
            println!("New provisional password for '{id}' (shown only now): {password}");
        }
        UsersAction::Remove { id } => {
            remove_user(&mut file, &id)?;
            save_config(&config_path, &file)?;
            let revoked = PairingStore::new(devices_path()?).revoke_user_devices(&id)?;
            if file.removed_users.iter().any(|u| u.id == id) {
                println!("'{id}' removed and {revoked} device(s) of theirs revoked — their encrypted vault and conversations stay on disk, and `warden-server users restore {id}` brings them back");
            } else {
                println!("'{id}' removed and {revoked} device(s) of theirs revoked — their vault and conversations stay on disk");
            }
        }
        UsersAction::Removed => {
            if file.removed_users.is_empty() {
                println!("no removed members with data on disk");
            }
            for user in &file.removed_users {
                println!("{}\t{}", user.id, user.name);
            }
        }
        UsersAction::Restore { id } => {
            restore_user(&mut file, &id)?;
            save_config(&config_path, &file)?;
            println!("'{id}' is back, with the password they had. Their devices were revoked when they were removed: they pair again.");
        }
        UsersAction::Purge { id, yes } => {
            anyhow::ensure!(yes, "this deletes '{id}'s vault and conversations for good — run it again with --yes");
            purge_removed_user(&mut file, &id)?;
            let (users_dir, conversations) = member_data_dirs()?;
            for dir in [users_dir.join(&id), warden_bootstrap::users::user_conversations_dir(&conversations, &id)] {
                if dir.exists() {
                    std::fs::remove_dir_all(&dir).with_context(|| format!("failed to delete {}", dir.display()))?;
                }
            }
            save_config(&config_path, &file)?;
            println!("'{id}' and their data are gone for good");
        }
    }
    Ok(())
}

fn run_spaces_command(action: SpacesAction, config: Option<String>) -> anyhow::Result<()> {
    use warden_bootstrap::users::{remove_space, save_space, SpaceConfig};
    let config_path = config.as_deref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let mut file = load_config_from_path(&config_path, config.is_some())?;
    let people = |list: &[String]| if list.is_empty() { "no one".to_string() } else { list.join(", ") };
    match action {
        SpacesAction::List => {
            if file.spaces.is_empty() {
                println!("nothing shared yet — share a folder with `warden-server spaces add <name> --folder <folder> --reader <username>`");
            }
            for space in &file.spaces {
                println!("{}\t{}\treaders: {}\twriters: {}", space.id, space.folder, people(&space.readers), people(&space.writers));
            }
        }
        SpacesAction::Add { id, folder, readers, writers } => {
            let wanted = id.trim().to_ascii_lowercase();
            let existing = file.spaces.iter().any(|s| s.id == wanted).then_some(wanted.as_str());
            save_space(&mut file, existing, SpaceConfig { id: id.clone(), folder, readers, writers })?;
            save_config(&config_path, &file)?;
            let space = file.spaces.iter().find(|s| s.id == wanted).expect("just saved");
            let verb = if existing.is_some() { "updated" } else { "shared" };
            println!("'{}' {verb}: {} — readers: {}; writers: {}", space.id, space.folder, people(&space.readers), people(&space.writers));
            println!("Members see it at {}/{}/ in their vault, from their next message.", warden_core::memory::MOUNTS_DIR, space.id);
        }
        SpacesAction::Remove { id } => {
            remove_space(&mut file, &id)?;
            save_config(&config_path, &file)?;
            println!("'{id}' is no longer shared — the folder and its notes stay in your vault");
        }
    }
    Ok(())
}

fn run_recovery_command(action: RecoveryAction, config: Option<String>) -> anyhow::Result<()> {
    use warden_bootstrap::recovery::RecoveryPolicy;
    use warden_bootstrap::users::{recover_member, set_recovery_policy, workspace_policy};
    let config_path = config.as_deref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let mut file = load_config_from_path(&config_path, config.is_some())?;
    match action {
        RecoveryAction::Policy { policy: None, new_key: false } => {
            let now = workspace_policy(&file);
            println!("recovery policy: {}{}", now.as_str(), if file.recovery_public_key.is_some() { " (a recovery key is set)" } else { "" });
            println!("private: nobody recovers a member's data · consent: you and the member's recovery code together · company: you alone, recorded and told to them");
        }
        RecoveryAction::Policy { policy, new_key } => {
            let policy = match policy {
                Some(text) => RecoveryPolicy::parse(&text)?,
                None => file.recovery_policy,
            };
            let secret = set_recovery_policy(&mut file, policy, new_key);
            save_config(&config_path, &file)?;
            println!("recovery policy: {}", policy.as_str());
            if let Some(secret) = secret {
                println!("Your recovery key (shown only now — it is never kept on the hub):");
                println!("  {secret}");
                println!("Write it down somewhere safe, apart from this machine. Without it, `recovery recover` can't open anyone's data.");
            }
            if !policy.is_private() {
                println!("Each member's data follows at their next sign-in; a change to a weaker policy waits for their yes.");
            }
        }
        RecoveryAction::Recover { id, key, code } => {
            let recovered = recover_member(&mut file, &id, &key, code.as_deref())?;
            save_config(&config_path, &file)?;
            println!("'{id}' recovered under the {} policy. Provisional password (shown only now): {}", recovered.kind.as_str(), recovered.temp_password);
            println!("They sign in with it and pick their own; their data is intact. It's recorded, and they're told at their next sign-in.");
        }
        RecoveryAction::Log => {
            let mut any = false;
            for user in &file.users {
                for event in &user.recoveries {
                    any = true;
                    println!("{}\t{}\t{}\t{}", user.id, event.at_ms, event.kind.as_str(), if event.seen { "seen" } else { "not seen yet" });
                }
            }
            if !any {
                println!("no recoveries yet");
            }
        }
    }
    Ok(())
}

/// Where the hub keeps the members' data, from the OS config directory as `serve` does.
fn member_data_dirs() -> anyhow::Result<(PathBuf, PathBuf)> {
    let users_dir = warden_bootstrap::users::default_users_dir().context("could not determine the OS config directory")?;
    let conversations = warden_bootstrap::default_server_conversations_dir().context("could not determine the OS config directory")?;
    Ok((users_dir, conversations))
}

fn run_backup_command(out: &std::path::Path, user: Option<&str>, config: Option<String>) -> anyhow::Result<()> {
    let config_path = config.as_deref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let (users_dir, conversations) = member_data_dirs()?;
    let report = warden_server::member_backup::backup_members(&config_path, &users_dir, &conversations, out, user)?;
    for id in &report.backed_up {
        println!("'{id}' backed up");
    }
    for (id, reason) in &report.skipped {
        println!("'{id}' left out: {reason}");
    }
    println!("{} member(s) in {}. What's in it stays unreadable without each member's password or recovery code.", report.backed_up.len(), out.display());
    Ok(())
}

fn run_restore_command(from: &std::path::Path, user: Option<&str>, force: bool, config: Option<String>) -> anyhow::Result<()> {
    let config_path = config.as_deref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let (users_dir, conversations) = member_data_dirs()?;
    let restored = warden_server::member_backup::restore_members(&config_path, &users_dir, &conversations, from, user, force)?;
    println!("restored: {}. If the hub is running, restart it so it opens their data.", restored.join(", "));
    Ok(())
}

fn run_export_command(out: &std::path::Path, with_secrets: bool) -> anyhow::Result<()> {
    let data_dir = warden_core::paths::warden_dir().context("could not determine the OS config directory")?;
    let options = warden_bootstrap::instance_export::ExportOptions { with_secrets };
    let report = warden_bootstrap::instance_export::export_instance(&data_dir, out, &options)?;
    println!("exported {} file(s), {} bytes, from {} to {}", report.files, report.bytes, data_dir.display(), out.display());
    if report.with_secrets {
        println!("the providers' API keys, the MCP servers' tokens and the other keys are in it (config.secrets.toml): keep the export private");
    } else {
        println!("the providers' API keys, the MCP servers' tokens and the search, voice and bot keys are not in it: enter your own at the destination");
    }
    if !report.needs_keys.is_empty() {
        println!("needs a key of your own at the destination: {}", report.needs_keys.join(", "));
    }
    println!("the config can still hold other tokens (git sync, for one): keep the export private");
    Ok(())
}

fn run_import_command(package: &std::path::Path, force: bool) -> anyhow::Result<()> {
    let data_dir = warden_core::paths::warden_dir().context("could not determine the OS config directory")?;
    let report = warden_bootstrap::instance_export::import_instance(package, &data_dir, &warden_bootstrap::instance_export::ImportOptions { force })?;
    println!("imported {} file(s) into {}", report.files, data_dir.display());
    if !report.needs_keys.is_empty() {
        println!("these need a key or token of your own before they work (providers, `mcp:<server>`, `api_keys:<name>`): {}", report.needs_keys.join(", "));
    }
    println!("paired devices, API keys and webhook tokens are not carried over: pair the devices again and create new keys");
    Ok(())
}

fn run_nodes_command(action: NodesAction, config: Option<String>) -> anyhow::Result<()> {
    let config_path = config.as_ref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    match action {
        NodesAction::List => {
            let config = load_config_from_path(&config_path, config.is_some())?;
            if config.nodes.is_empty() {
                println!("no nodes allowed yet — run `warden-server node` on a machine, approve it (`devices approve <id>`), then `nodes allow <id>`");
            }
            for node in &config.nodes {
                let who = if node.agents.is_empty() { "every agent".to_string() } else { node.agents.join(", ") };
                println!("{}\t{}\t{who}\t{}", node.id, if node.enabled { "on" } else { "off" }, if node.require_approval { "asks first" } else { "no approval" });
            }
        }
        NodesAction::Allow { id, agents, approval } => {
            warden_server::nodes::set_node_access(&config_path, warden_bootstrap::NodeAccessConfig { id: id.clone(), enabled: true, agents, require_approval: approval })?;
            let store = PairingStore::new(devices_path()?);
            match store.status(&id)? {
                Some(warden_server::PairingStatus::Approved) => println!("node '{id}' allowed"),
                _ => println!("node '{id}' allowed — it also has to be approved in the device list: `warden-server devices approve {id}`"),
            }
        }
        NodesAction::Deny { id } => {
            let mut config = load_config_from_path(&config_path, config.is_some())?;
            let node = config.nodes.iter_mut().find(|n| n.id == id).ok_or_else(|| anyhow::anyhow!("no node '{id}' in the config"))?;
            node.enabled = false;
            save_config(&config_path, &config)?;
            println!("node '{id}' denied — agents can't use it any more");
        }
    }
    Ok(())
}

async fn run_tasks_command(args: TasksArgs) -> anyhow::Result<()> {
    let config_path = args.config.as_ref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let mut config = load_config_from_path(&config_path, args.config.is_some())?;
    let store = tasks_store()?;
    let position = |config: &warden_bootstrap::FileConfig, id: &str| {
        config.tasks.iter().position(|t| t.id == id).ok_or_else(|| anyhow::anyhow!("no task named '{id}' — `warden-server tasks list` shows them"))
    };
    match args.action {
        TasksCommand::List => {
            if config.tasks.is_empty() {
                println!("no scheduled tasks yet — add one with `warden-server tasks add <id> --prompt ... --every 1d`");
                return Ok(());
            }
            let states = store.states()?;
            let now = now_millis();
            for task in &config.tasks {
                let zone = Zone::parse(task.timezone.as_deref()).unwrap_or(Zone::Local);
                let status = task_status(task, states.get(&task.id), now);
                let on = if task.enabled { "on" } else { "paused" };
                let agent = task.agent.as_deref().unwrap_or("(no agent)");
                let last = match (status.last_run_at_ms, status.running, status.last_error.as_deref()) {
                    (None, _, _) => "never ran".to_string(),
                    (Some(at), true, _) => format!("running since {}", zone.format(at)),
                    (Some(at), false, None) => format!("last ran {}", zone.format(at)),
                    (Some(at), false, Some(err)) => format!("last ran {} and failed: {err}", zone.format(at)),
                };
                let next = match (&status.schedule_error, status.next_run_at_ms) {
                    (Some(err), _) => format!("invalid: {err}"),
                    (None, Some(at)) => format!("next {}", zone.format(at)),
                    (None, None) if !task.enabled => "not scheduled".to_string(),
                    (None, None) => "done".to_string(),
                };
                println!("{}\t{on}\t{}\t{agent}\t{last}\t{next}", task.id, task.schedule_label());
            }
        }
        TasksCommand::Add { id, prompt, agent, every, cron, once, timezone } => {
            anyhow::ensure!(!config.tasks.iter().any(|t| t.id == id), "there's already a task named '{id}'");
            let task = TaskConfig { id: id.clone(), agent, prompt, every, cron, once, timezone, enabled: true };
            config.tasks.push(task.clone());
            check_tasks(&config.tasks, &config.agents)?;
            warden_bootstrap::webhooks::check_task_clashes(&config.tasks, &config.webhooks)?;
            save_config(&config_path, &config)?;
            let zone = Zone::parse(task.timezone.as_deref()).unwrap_or(Zone::Local);
            match next_run(&task, None, now_millis()) {
                Some(at) => println!("task '{id}' added — first run {} (on the hub started with `serve --run-tasks`)", zone.format(at)),
                None => println!("task '{id}' added"),
            }
        }
        TasksCommand::Pause { id } => {
            let i = position(&config, &id)?;
            config.tasks[i].enabled = false;
            save_config(&config_path, &config)?;
            println!("task '{id}' paused");
        }
        TasksCommand::Resume { id } => {
            let i = position(&config, &id)?;
            config.tasks[i].enabled = true;
            save_config(&config_path, &config)?;
            println!("task '{id}' is on again — it counts from now");
        }
        TasksCommand::Remove { id } => {
            let i = position(&config, &id)?;
            config.tasks.remove(i);
            save_config(&config_path, &config)?;
            println!("task '{id}' removed — its conversation stays on the hub");
        }
        TasksCommand::Run { id, vault_path } => {
            let task = config.tasks[position(&config, &id)?].clone();
            task.schedule()?;
            let overrides = Overrides { vault_path, ..Default::default() };
            let orchestrator = bootstrap(args.config.as_deref(), overrides, default_vault_path()).await?;
            let now = now_millis();
            store.mark_started(&task, now)?;
            let result = run_task(&orchestrator, &config, Some(&config_path), &task, &store.conversations_dir(), now).await;
            store.record_finish(&id, now_millis(), result.as_ref().err().map(|e| format!("{e:#}")))?;
            println!("{}", result?.content);
        }
    }
    Ok(())
}

fn api_keys_path() -> anyhow::Result<PathBuf> {
    warden_bootstrap::default_api_keys_path().context("could not determine the OS config directory for the API keys")
}

fn webhook_tokens_path() -> anyhow::Result<PathBuf> {
    warden_bootstrap::default_webhook_tokens_path().context("could not determine the OS config directory for the webhook tokens")
}

fn run_webhooks_command(args: WebhooksArgs) -> anyhow::Result<()> {
    let config_path = args.config.as_ref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
    let mut config = load_config_from_path(&config_path, args.config.is_some())?;
    let tokens = warden_server::webhook_tokens::WebhookTokenStore::new(webhook_tokens_path()?);
    let position = |config: &warden_bootstrap::FileConfig, id: &str| {
        config.webhooks.iter().position(|h| h.id == id).ok_or_else(|| anyhow::anyhow!("no webhook named '{id}' — `warden-server webhooks list` shows them"))
    };
    match args.action {
        WebhooksCommand::List => {
            if config.webhooks.is_empty() {
                println!("no webhooks yet — add one with `warden-server webhooks add <id> --prompt ...`");
                return Ok(());
            }
            let with_token = tokens.list()?;
            for hook in &config.webhooks {
                let on = if hook.enabled { "on" } else { "paused" };
                let agent = hook.agent.as_deref().unwrap_or("(no agent)");
                let credential = match with_token.iter().find(|t| t.webhook == hook.id) {
                    Some(t) if t.kind != hook.auth => format!("a {} but the webhook wants {} — `warden-server webhooks token {}` makes the right one", t.kind.as_str(), hook.auth.as_str(), hook.id),
                    Some(t) => format!("{} {}… created {}{}", t.kind.as_str(), t.shown, t.created_at_ms, t.last_used_at_ms.map_or(", never used".to_string(), |ms| format!(", last used {ms}"))),
                    None => "no credential (takes no calls)".to_string(),
                };
                println!("{}\t{on}\t{agent}\t{}\t/hooks/{}\t{credential}", hook.id, hook.auth.as_str(), hook.id);
            }
        }
        WebhooksCommand::Add { id, prompt, agent, auth } => {
            anyhow::ensure!(!config.webhooks.iter().any(|h| h.id == id), "there's already a webhook named '{id}'");
            let auth = WebhookAuth::parse(&auth)?;
            upsert_webhook(&mut config, WebhookConfig { id: id.clone(), agent, prompt, enabled: true, auth })?;
            save_config(&config_path, &config)?;
            println!("webhook '{id}' added ({}) — it takes no calls until it has a credential: `warden-server webhooks token {id}`", auth.as_str());
        }
        WebhooksCommand::Pause { id } => {
            let i = position(&config, &id)?;
            config.webhooks[i].enabled = false;
            save_config(&config_path, &config)?;
            println!("webhook '{id}' paused — calls are refused until `resume`");
        }
        WebhooksCommand::Resume { id } => {
            let i = position(&config, &id)?;
            config.webhooks[i].enabled = true;
            save_config(&config_path, &config)?;
            println!("webhook '{id}' takes calls again");
        }
        WebhooksCommand::Remove { id } => {
            let i = position(&config, &id)?;
            config.webhooks.remove(i);
            save_config(&config_path, &config)?;
            tokens.revoke(&id)?;
            println!("webhook '{id}' and its token removed — its conversation stays on the hub");
        }
        WebhooksCommand::Token { id } => {
            let i = position(&config, &id)?;
            // A token or a signing secret, whichever the webhook was made for.
            let auth = config.webhooks[i].auth;
            let created = tokens.create_credential(&id, auth)?;
            println!("{}", created.token);
            match auth {
                WebhookAuth::Token => {
                    eprintln!("token for webhook '{id}' created — copy it now, it isn't shown again; any earlier credential for it stopped working");
                    eprintln!("call it with: curl -X POST -H 'Authorization: Bearer <token>' --data-binary @body.json http(s)://<this hub>:<port>/hooks/{id}");
                    eprintln!("(a service that can't set that header can send the token as X-Warden-Token instead)");
                }
                WebhookAuth::Hmac => {
                    eprintln!("signing secret for webhook '{id}' created — copy it now, it isn't shown again; any earlier credential for it stopped working");
                    eprintln!("give it to the service that calls http(s)://<this hub>:<port>/hooks/{id} (GitHub: the webhook's \"Secret\"; Stripe: the endpoint's signing secret; Slack: the app's signing secret)");
                    eprintln!("it is signed with HMAC-SHA256 in X-Hub-Signature-256 (GitHub, Gitea, Forgejo), Stripe-Signature or X-Slack-Signature; a bearer token is not accepted");
                    eprintln!("note: unlike a token, the secret is kept in the clear in webhook_tokens.json (mode 0600): checking a signature means making one");
                }
            }
            if !config.webhooks[i].enabled {
                eprintln!("note: the webhook is paused — `warden-server webhooks resume {id}` to take calls");
            }
        }
        WebhooksCommand::Revoke { id } => {
            anyhow::ensure!(tokens.revoke(&id)?, "webhook '{id}' has no credential");
            println!("the credential of webhook '{id}' is revoked — calls get 401 from the next one on");
        }
    }
    Ok(())
}

fn run_package_command(action: PackageAction) -> anyhow::Result<()> {
    use warden_server::package_cmd;
    match action {
        PackageAction::Keygen { file } => {
            let public = package_cmd::keygen(&file)?;
            println!("wrote the private key to {} — keep it secret and back it up; it cannot be made again", file.display());
            println!("public key (give this to the people who should trust you): {public}");
        }
        PackageAction::Sign { package, key, out } => {
            let public = package_cmd::sign(&package, &key, out.as_deref())?;
            println!("signed {} with the key {public}", out.as_deref().unwrap_or(&package).display());
        }
        PackageAction::Verify { package, config } => {
            let trusted = config
                .or_else(warden_bootstrap::default_config_path)
                .and_then(|path| load_config_from_path(&path, false).ok())
                .map(|config| config.trusted_publishers)
                .unwrap_or_default();
            println!("{}", package_cmd::verify(&package, &trusted)?);
        }
    }
    Ok(())
}

fn run_api_keys_command(action: ApiKeysAction) -> anyhow::Result<()> {
    let store = warden_server::api_keys::ApiKeyStore::new(api_keys_path()?);
    match action {
        ApiKeysAction::List => {
            let keys = store.list()?;
            if keys.is_empty() {
                println!("no API keys yet — create one with `warden-server api-keys create <name>`");
            }
            for key in keys {
                let used = key.last_used_at_ms.map_or("never used".to_string(), |ms| format!("last used {ms}"));
                let scope = key.agent_id.as_deref().map_or("any agent".to_string(), |agent| format!("only agent {agent}"));
                let raw = if key.raw.is_empty() { String::new() } else { format!("\tdirect: {}", key.raw.join(",")) };
                println!("{}\t{}\t{}…\t{scope}\tcreated {}\t{used}{raw}", key.id, key.name, key.shown, key.created_at_ms);
            }
        }
        ApiKeysAction::Create { name, agent, user, config } => {
            let user = user.as_deref().map(str::trim).filter(|u| !u.is_empty());
            if agent.as_deref().is_some_and(|a| !a.trim().is_empty()) || user.is_some() {
                let config_path = config.map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
                if let Some(user) = user {
                    warden_server::api_key_admin::check_member_exists(&config_path, user)?;
                }
                warden_server::api_key_admin::check_agent_exists(&config_path, agent.as_deref(), user)?;
            }
            let created = store.create_for(&name, agent.as_deref(), user)?;
            println!("{}", created.key);
            eprintln!("key '{}' created (id {}) — copy it now, it isn't shown again", created.info.name, created.info.id);
            eprintln!("use it as the bearer token with base URL http(s)://<this hub>:<port>/v1");
            match &created.info.agent_id {
                Some(agent) => eprintln!("it only speaks as agent '{agent}' (model \"warden\" or \"warden/{agent}\"; any other agent is refused)"),
                None => eprintln!("a general key: model \"warden\", or \"warden/<agent>\" to speak as a configured agent"),
            }
        }
        ApiKeysAction::Raw { id, models, config } => {
            if !models.is_empty() {
                let config_path = config.map(PathBuf::from).or_else(warden_bootstrap::default_config_path).context("could not determine the OS config directory")?;
                warden_server::api_key_admin::check_raw_ids(&config_path, &models)?;
            }
            anyhow::ensure!(store.set_raw(&id, models.clone())?, "no API key with id '{id}'");
            if models.is_empty() {
                println!("key '{id}' can no longer call a model directly");
            } else {
                println!("key '{id}' may call: {}", models.join(", "));
                eprintln!("use the model \"provider/<id>\" or \"combo/<id>\"; the spend goes to the `api-raw` channel, so `[[limits]]` with scope channel `api-raw` covers it alone");
            }
        }
        ApiKeysAction::Revoke { id } => {
            anyhow::ensure!(store.revoke(&id)?, "no API key with id '{id}'");
            println!("key '{id}' revoked");
        }
    }
    Ok(())
}

async fn run_serve(args: ServeArgs) -> anyhow::Result<()> {
    warden_server::logging::init(args.log_format);
    let auth_key = std::env::var("WARDEN_SERVER_AUTH_KEY")
        .ok()
        .or(args.auth_key.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no auth key configured — set WARDEN_SERVER_AUTH_KEY or pass --auth-key"
            )
        })?;
    // P83 — the pairing key also guards saving settings from the web, so a short one is refused.
    if !warden_bootstrap::is_strong_auth_key(&auth_key) {
        anyhow::bail!(
            "the auth key must be at least {} characters — generate one with `warden-server gen-key` and use it in \
             WARDEN_SERVER_AUTH_KEY or --auth-key (paired devices keep working; only new pairings need the new key)",
            warden_bootstrap::MIN_AUTH_KEY_LEN
        );
    }

    let overrides = Overrides { provider: args.provider.map(Into::into), model: args.model.clone(), vault_path: args.vault_path.clone(), ..Default::default() };
    let code_engine = CodeEngineSlot::default();
    let engine_model_slot = warden_server::openai_api::EngineModelSlot::default();
    let orchestrator = bootstrap_hub(args.config.as_deref(), overrides.clone(), &code_engine, &engine_model_slot).await?;
    if args.no_embedding_warm_up {
        tracing::info!("search models are not preloaded (--no-embedding-warm-up); each loads at its first search");
    } else {
        warm_embedding_models(orchestrator.vault().clone());
    }
    let settings =args.config.as_ref().map(PathBuf::from).or_else(warden_bootstrap::default_config_path).map(|config_path| ServeSettings {
        config_path,
        explicit_config: args.config.clone(),
        overrides,
        code_engine: code_engine.clone(),
        engine_models: engine_model_slot.clone(),
    });

    // P122 — an "always" a device gave is kept in a file of its own beside the config (and lapses unused), unless `persist_grants = false`.
    let grant_policy = settings.as_ref().and_then(|s| warden_bootstrap::grant_policy_at(&s.config_path, s.explicit_config.is_some()));

    // P61 — the vault syncs on its own every few minutes, and the web's Sync screen drives it too.
    let runner = Arc::new(sync_runner(args.config.as_deref(), args.vault_path.clone())?);

    let conversations_dir = warden_bootstrap::default_server_conversations_dir()
        .context("could not determine the OS config directory for conversations")?;

    let tls = resolve_tls(&args).await?;

    let server_name = resolve_server_name(args.server_name.clone());
    let shared = warden_server::SharedOrchestrator::new(orchestrator);
    // P103 b — the opencode's door to the hub's model, on the loopback.
    let engine_models = warden_server::engine_models::EngineModels::start(shared.clone(), engine_model_slot).await?;
    let opencode = warden_server::code_turns::opencode_engine(&engine_models);
    // The same engine serves the code projects' conversations and the agents' `code_task`.
    code_engine.set(opencode.clone()).ok();
    // P36 c: a key in this file (`devices rotate-key`) wins over the one given above while the hub runs.
    let key_file = pairing_key_path()?;
    if warden_server::PairingKey::new(auth_key.as_str()).with_file(key_file.clone()).from_file() {
        println!("pairing key: taken from {} (changed with `devices rotate-key`), not from --auth-key", key_file.display());
    }
    println!("pairing: the pairing key pairs only this hub's first device; every other device needs an invite (`warden-server devices invite`)");
    let mut server = Server::bind(args.listen, auth_key, server_name.clone(), shared, conversations_dir, devices_path()?)
        .await?
        .with_pairing_key_file(key_file)
        // P78 — voice input from the web UI, with the Whisper key from the same config file.
        .with_transcriber(Arc::new(WhisperTranscriber::new(args.config.as_ref().map(PathBuf::from))))
        .with_sync(runner, Some(AUTO_SYNC_INTERVAL))
        .with_api(api_keys_path()?)
        // P103 b — the conversations of code projects are tasks for the opencode, started per project folder.
        .with_code_engine(opencode)
        // P92 — every device lists the tasks' conversations; only `--run-tasks` runs them.
        .with_tasks(tasks_store()?, args.run_tasks)
        // P105 — `POST /hooks/<id>` with a webhook's token; the conversations go next to the tasks' ones.
        .with_webhooks(webhook_tokens_path()?)
        // P123 — the tasks agents delegate to each other, written by the orchestrator and listed to the devices.
        .with_agent_tasks(warden_bootstrap::resolve_agent_tasks_path(std::env::var("WARDEN_AGENT_TASKS").ok()))
        // P122 — where each device's "always" live.
        .with_grants(grant_policy)
        // P119 — whether the web settings may change the shell, MCP servers, SSH hosts, folders and the embedded hub.
        .with_machine_settings(args.allow_machine_settings);
    // P146 c, P147 — the search models of the owner's, the shared folders' and the members' vaults go after a pause.
    if args.embedding_idle_secs > 0 {
        server = server.with_embedding_idle(std::time::Duration::from_secs(args.embedding_idle_secs));
    }
    if args.allow_machine_settings {
        tracing::warn!("--allow-machine-settings is on: whoever holds the pairing key can change this machine's shell, MCP servers, SSH hosts and folders from the web (encrypted or local connections only)");
    }
    let task_count = args
        .config
        .as_ref()
        .map(PathBuf::from)
        .or_else(warden_bootstrap::default_config_path)
        .and_then(|path| load_config_from_path(&path, false).ok())
        .map_or(0, |config| config.tasks.len());
    match (args.run_tasks, task_count) {
        (true, n) => tracing::info!(configured = n, "running scheduled tasks on this hub"),
        (false, 0) => {}
        (false, n) => tracing::warn!(configured = n, "scheduled task(s) configured, but this hub doesn't run them (start it with --run-tasks)"),
    }
    if let Some(settings) = settings {
        server = server.with_settings(Arc::new(settings));
        // P84 — the members in `[[users]]` sign in with their password, each with their own vault.
        if let Some(users_dir) = warden_bootstrap::users::default_users_dir() {
            server = server.with_users_dir(users_dir);
        }
    }
    let addr = server.local_addr()?;
    let page_url = match tls.as_ref().and_then(|tls| tls.secure_url(addr.port())) {
        Some(url) => url.replacen("wss://", "https://", 1),
        None if tls.is_some() => format!("https://<this hub's TLS name>:{}", addr.port()),
        None => format!("http://{addr}"),
    };
    match tls {
        Some(tls) => {
            match tls.secure_url(addr.port()) {
                Some(url) => tracing::info!(%addr, %server_name, "listening — TLS only, clients connect to {url}"),
                None => tracing::info!(%addr, %server_name, "listening — TLS only"),
            }
            server = server.with_tls(tls);
        }
        None => tracing::warn!(%addr, %server_name, "listening — plain ws://, not encrypted (see --tailscale-cert)"),
    }
    if !args.no_web_ui {
        if EmbeddedWebUi.get("index.html").is_some() {
            tracing::info!(url = %page_url, "web interface");
        } else {
            tracing::warn!("this build has no web interface (run `npm run build` in web/ and rebuild) — pages answer 503");
        }
        server = server.with_web_ui(Arc::new(EmbeddedWebUi));
    }
    tracing::info!("Warden API (OpenAI-compatible) at {}/v1 — keys: `warden-server api-keys create <name>`", page_url.trim_end_matches('/'));
    // P143: a stop signal ends the accept loop and frees the port; the open connections get a grace period, and the
    // children (`kill_on_drop`) are collected when the process exits normally rather than being cut by the signal.
    server.serve_until(stop_signal()).await?;
    if args.shutdown_grace_secs > 0 {
        tracing::info!(grace_secs = args.shutdown_grace_secs, "not accepting connections; exiting after the grace period (or now, with a second signal)");
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(args.shutdown_grace_secs)) => {}
            _ = stop_signal() => {}
        }
    }
    tracing::info!("stopped");
    Ok(())
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

/// Loads the two embedding models (the vault's and the history search's) in the background, so the first turn after the hub
/// starts doesn't wait for them, nor for the download on a machine that never had them. Failing is only logged: offline with no
/// model yet, the first search tries again and falls back to words, as it always did.
fn warm_embedding_models(vault: Arc<warden_core::memory::Vault>) {
    // Two tasks, not one after the other: they don't share a lock, and a first turn may wait for either.
    tokio::task::spawn_blocking(move || {
        if let Err(err) = vault.warm_up() {
            tracing::warn!("could not load the vault's search model ({err:#}); searching by meaning will try again when it is used");
        }
    });
    tokio::task::spawn_blocking(|| {
        if std::env::var_os(warden_core::memory::embed::OFF_SWITCH).is_none_or(|v| v.is_empty()) {
            if let Err(err) = warden_core::memory::embed::warm_up() {
                tracing::warn!("could not load the history search model ({err:#}); it will try again when it is used");
            }
        }
    });
}

/// `--tailscale-cert` fetches the cert first (and spawns its daily renewal); `--tls-cert`/
/// `--tls-key` just load what's there. `None` = plain `ws://`, as before P36's second slice.
async fn resolve_tls(args: &ServeArgs) -> anyhow::Result<Option<HubTls>> {
    if args.tailscale_cert {
        let dir = warden_bootstrap::default_tls_dir().context("could not determine the OS config directory for the TLS certificate")?;
        let (hub_tls, cert) = HubTls::from_tailscale(&dir).await?;
        tokio::spawn(cert.renewal());
        return Ok(Some(hub_tls));
    }
    match (&args.tls_cert, &args.tls_key) {
        (Some(cert), Some(key)) => Ok(Some(HubTls::from_pem_files(cert, key, args.tls_host.clone())?)),
        _ => Ok(None),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // Before anything asks where a file lives (P143).
    if let Some(dir) = cli.data_dir.or_else(|| std::env::var_os("WARDEN_DATA_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)) {
        warden_core::paths::set_data_dir(dir);
    }
    match cli.command {
        Command::Serve(args) => run_serve(args).await,
        Command::Devices { action } => run_devices_command(action),
        Command::ApiKeys { action } => run_api_keys_command(action),
        Command::Package { action } => run_package_command(action),
        Command::Sync(args) => run_sync_command(args).await,
        Command::Tasks(args) => run_tasks_command(args).await,
        Command::Webhooks(args) => run_webhooks_command(args),
        Command::Node(args) => run_node_command(args).await,
        Command::Nodes { action, config } => run_nodes_command(action, config),
        Command::Users { action, config } => run_users_command(action, config),
        Command::Spaces { action, config } => run_spaces_command(action, config),
        Command::Recovery { action, config } => run_recovery_command(action, config),
        Command::Backup { out, user, config } => run_backup_command(&out, user.as_deref(), config),
        Command::Restore { from, user, force, config } => run_restore_command(&from, user.as_deref(), force, config),
        Command::Export { out, with_secrets } => run_export_command(&out, with_secrets),
        Command::Import { package, force } => run_import_command(&package, force),
        Command::GenKey => {
            println!("{}", warden_bootstrap::generate_auth_key());
            Ok(())
        }
    }
}

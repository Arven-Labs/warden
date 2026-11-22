//! Arca mode for the phone, the minimum the key swap needs (P-49, slice 1): turn the mode on, run one cycle by hand, and see (and
//! acknowledge) the newer key epochs that stop the vault. Mirrors what `desktop/src-tauri/src/arca_mode_cmds.rs` exposes, minus the
//! automatic loop, the MCP logins (`sync_logins` is always `false` here: they need their own consent, P-36), the conflict losers
//! and the saved hubs: the phone has one hub, the address and token the Dart side already keeps, so `HubStore::new` takes them
//! directly.
//!
//! Like `sync.rs`, every function takes the four sync paths explicitly and rebuilds the `SyncEngine` per call (it is cheap and pure).
//! The Rust text of a failure is English and for the log; the screen translates `ArcaModeErrorKind`.
//!
//! `http` to another machine is refused unless the screen passes `allow_insecure_http` (only after warning the person that the
//! token then travels in the clear: `HubStore::allow_insecure_http`, the name is on purpose). `https` and loopback never need it.

use warden_sync::arca_sync::{is_auth_rejected, HubError, HubStore, PendingEpoch};
use warden_sync::SyncEngine;

use super::sync::{push_is_pending, rt};

/// Why a mode call did not go through, as something the screen can translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcaModeErrorKind {
    /// The vault is not in arca mode (a cycle, or an acknowledge, needs it on).
    NotInArcaMode,
    /// The mode is already on.
    AlreadyOn,
    /// This phone has no vault key yet: pair it first.
    NoKey,
    /// A Send is waiting for the phone's scan: finish or cancel it before turning the mode on.
    SendPending,
    /// `http` to a host that is not this phone: the token would travel in the clear.
    InsecureUrl,
    /// Not an address the hub client can use.
    BadUrl,
    /// The token cannot be used (empty, or not a header value).
    BadToken,
    /// The hub does not accept this device (401, or pending/revoked, 403).
    Unauthorized,
    /// Anything else (network, a hub answer that is not documented, the disk).
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaModeFailure {
    pub kind: ArcaModeErrorKind,
    /// English, for logs. Never the token, and never shown as the main text.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaModeStatusDto {
    pub on: bool,
    /// A Send is waiting for the phone's scan: turning the mode on is refused until it ends.
    pub send_pending: bool,
}

/// What one cycle did, in the shape the screen gets (counts only: no file names, and never a login).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaSyncDto {
    pub snapshot_created: bool,
    pub pulled: u32,
    pub merges: u32,
    pub conflicts: u32,
    pub files_changed: u32,
    pub pushed: u32,
    /// Every file of the vault is gone from this phone: the cycle left the vault out instead of sending the removal.
    pub vault_emptied: bool,
}

/// A newer key epoch that stops the vault, for the person to look at before acknowledging. `id` is the text of the manifest id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEpochDto {
    pub epoch: u32,
    pub id: String,
}

fn engine(vault_root: &str, config_path: &str, secrets_path: &str, manifest_path: &str) -> SyncEngine {
    SyncEngine::new(vault_root.into(), config_path.into(), secrets_path.into(), manifest_path.into())
}

fn failure(kind: ArcaModeErrorKind, detail: impl std::fmt::Display) -> ArcaModeFailure {
    ArcaModeFailure { kind, detail: detail.to_string() }
}

fn other(error: anyhow::Error) -> ArcaModeFailure {
    failure(ArcaModeErrorKind::Other, format!("{error:#}"))
}

impl From<&PendingEpoch> for PendingEpochDto {
    fn from(pending: &PendingEpoch) -> Self {
        Self { epoch: pending.epoch, id: pending.id.to_string() }
    }
}

/// Whether the mode is on, and whether a Send is waiting.
pub fn arca_mode_status(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<ArcaModeStatusDto, ArcaModeFailure> {
    let on = engine(&vault_root, &config_path, &secrets_path, &manifest_path).arca_mode_on().map_err(other)?;
    Ok(ArcaModeStatusDto { on, send_pending: push_is_pending() })
}

/// What turning the mode on does with the answer about the Send (the seam the test uses): refused while one waits. The refusals come
/// first, one by one, so each has its own kind. The engine repeats the key and the "already on" ones under its lock; the Send is NOT
/// rechecked there (`arca_enable_after_pull` reads it before the Pull and the engine never sees it again), so a Send begun during a
/// long Pull can still get in: `finish_push` then fails cleanly (it checks the mode under the lock before it asks for the pin), but
/// the mode is already on. The desktop has the same shape (limit, not a regression).
#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn enable_with(engine: &SyncEngine, pending_send: bool, now_ms: u64) -> Result<(), ArcaModeFailure> {
    if !engine.is_initialized() {
        return Err(failure(ArcaModeErrorKind::NoKey, "this phone has no vault key yet: pair it first"));
    }
    if pending_send {
        return Err(failure(ArcaModeErrorKind::SendPending, "a Send is waiting for the phone: finish or cancel it first"));
    }
    if engine.arca_mode_on().map_err(other)? {
        return Err(failure(ArcaModeErrorKind::AlreadyOn, "this vault is already in arca mode"));
    }
    // The logins never travel from the phone in this slice (`sync_logins` false); the Pull before it only happens for a vault paired on Arweave.
    rt().block_on(engine.arca_enable_after_pull(pending_send, false, now_ms)).map(|_needs_reload| ()).map_err(other)
}

/// Turns the mode on (warden ADR 0012, step a): `pending_send` comes from the Send this process has in flight.
pub fn arca_mode_enable(vault_root: String, config_path: String, secrets_path: String, manifest_path: String, now_ms: u64) -> Result<(), ArcaModeFailure> {
    enable_with(&engine(&vault_root, &config_path, &secrets_path, &manifest_path), push_is_pending(), now_ms)
}

/// Whether `host` (as written in a URL, brackets and all) is a machine on the person's own network: a private, link-local or
/// carrier-grade-NAT (Tailscale's 100.64.0.0/10) address, a unique-local or link-local IPv6 one, loopback, `localhost` or an mDNS
/// `*.local` name. A public address or name is not: the token must not cross the internet in the clear.
fn is_own_network(host: &str) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local() || (u32::from(ip) & 0xffc0_0000) == u32::from(Ipv4Addr::new(100, 64, 0, 0)),
        Ok(IpAddr::V6(ip)) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80,
        Err(_) => {
            let name = bare.to_ascii_lowercase();
            name == "localhost" || name.ends_with(".local")
        }
    }
}

/// The host of a plain `http://` address, or `None` when the address is not one (the hub client judges everything else).
fn plain_http_host(hub_url: &str) -> Option<&str> {
    let rest = hub_url.get(..7).filter(|scheme| scheme.eq_ignore_ascii_case("http://")).map(|_| &hub_url[7..])?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Credentials in the address are the hub client's to refuse; they must not shift what counts as the host here.
    if authority.contains('@') {
        return None;
    }
    let host = if authority.starts_with('[') { authority.split_inclusive(']').next().unwrap_or(authority) } else { authority.split(':').next().unwrap_or(authority) };
    Some(host)
}

/// The hub client for `hub_url`. Plain `http` to another machine is refused unless `allow_insecure_http` (the person was warned), and
/// even then only to a machine on the person's own network (`is_own_network`): the permission is a bool with no origin attached, so
/// the host is checked here and a screen that passes `true` for a public address (by mistake or by a stale "already warned") is refused.
#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn hub_store_for(hub_url: &str, device_token: &str, allow_insecure_http: bool) -> Result<HubStore, ArcaModeFailure> {
    if allow_insecure_http && plain_http_host(hub_url).is_some_and(|host| !is_own_network(host)) {
        return Err(failure(ArcaModeErrorKind::InsecureUrl, "plain http is only accepted to a machine on your own network, never to a public address"));
    }
    let built = if allow_insecure_http { HubStore::allow_insecure_http(hub_url, device_token) } else { HubStore::new(hub_url, device_token) };
    built.map_err(|error| match error {
        HubError::InsecureUrl => failure(ArcaModeErrorKind::InsecureUrl, "plain http to another machine: the token would travel in the clear"),
        HubError::InvalidUrl => failure(ArcaModeErrorKind::BadUrl, "not a hub address the sync can use"),
        HubError::InvalidToken => failure(ArcaModeErrorKind::BadToken, "the token cannot be used"),
        other_error => failure(ArcaModeErrorKind::Other, other_error),
    })
}

/// `detail` with every copy of `token` taken out (literal only: another encoding of it is not covered, the layers below never print it).
#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn redact(detail: &str, token: &str) -> String {
    if token.is_empty() {
        detail.to_string()
    } else {
        detail.replace(token, "***")
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// One cycle against the hub at `hub_url` with this phone's `device_token`. `allow_insecure_http` only after the screen warned the person.
#[allow(clippy::too_many_arguments)]
pub fn arca_sync_run(
    vault_root: String,
    config_path: String,
    secrets_path: String,
    manifest_path: String,
    hub_url: String,
    device_token: String,
    allow_insecure_http: bool,
    now_ms: u64,
) -> Result<ArcaSyncDto, ArcaModeFailure> {
    let engine = engine(&vault_root, &config_path, &secrets_path, &manifest_path);
    if !engine.arca_mode_on().map_err(other)? {
        return Err(failure(ArcaModeErrorKind::NotInArcaMode, "this vault is not in arca mode: turn it on first"));
    }
    let hub = hub_store_for(&hub_url, &device_token, allow_insecure_http)?;
    match rt().block_on(engine.arca_sync(&hub, now_ms)) {
        Ok(report) => Ok(ArcaSyncDto {
            snapshot_created: report.snapshot_created,
            pulled: count(report.pulled),
            merges: count(report.merges),
            conflicts: count(report.conflicts),
            files_changed: count(report.files_changed),
            pushed: count(report.pushed),
            vault_emptied: report.vault_emptied,
        }),
        Err(error) => {
            // The token is never in the engine's text; this is for the day a lower layer puts a header in an error.
            let detail = redact(&format!("{error:#}"), &device_token);
            let kind = if is_auth_rejected(&error) { ArcaModeErrorKind::Unauthorized } else { ArcaModeErrorKind::Other };
            Err(failure(kind, detail))
        }
    }
}

/// The newer key epochs that stop the vault. Empty with the mode off.
pub fn arca_epochs_pending(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<Vec<PendingEpochDto>, ArcaModeFailure> {
    let engine = engine(&vault_root, &config_path, &secrets_path, &manifest_path);
    rt().block_on(engine.arca_pending_epochs()).map(|list| list.iter().map(PendingEpochDto::from).collect()).map_err(other)
}

/// Acknowledges the pairs the person saw. They are only looked up in a fresh list; nothing from `seen` reaches the engine as an id.
/// What shows up after the person looked stays pending. Asked even with nothing to acknowledge, so the mode off is told. Returns how
/// many pairs were acknowledged: 0 means the list changed under the person (or nothing was seen), so the vault is still stopped and
/// the screen must list the epochs again instead of saying "acknowledged".
pub fn arca_epochs_acknowledge(vault_root: String, config_path: String, secrets_path: String, manifest_path: String, seen: Vec<PendingEpochDto>) -> Result<u32, ArcaModeFailure> {
    let engine = engine(&vault_root, &config_path, &secrets_path, &manifest_path);
    if !engine.arca_mode_on().map_err(other)? {
        return Err(failure(ArcaModeErrorKind::NotInArcaMode, "this vault is not in arca mode: turn it on first"));
    }
    let fresh = rt().block_on(engine.arca_pending_epochs()).map_err(other)?;
    let listed: Vec<PendingEpochDto> = fresh.iter().map(PendingEpochDto::from).collect();
    let to_acknowledge: Vec<PendingEpoch> = positions_seen(&listed, &seen).into_iter().map(|position| fresh[position]).collect();
    rt().block_on(engine.arca_acknowledge_epochs(&to_acknowledge)).map_err(other)?;
    Ok(count(to_acknowledge.len()))
}

/// Where, in the fresh list, are the pairs the screen handed back: the whole pair (epoch and id text), since the next epoch is
/// predictable and the epoch alone would let a forged object hide the real rotation. Each position once, in the fresh list's order.
#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn positions_seen(fresh: &[PendingEpochDto], seen: &[PendingEpochDto]) -> Vec<usize> {
    fresh.iter().enumerate().filter(|(_, pending)| seen.contains(pending)).map(|(position, _)| position).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use axum::http::StatusCode;
    use axum::Router;

    fn unique_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("warden-bridge-arca-mode-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The phone's four sync paths, with the vault folder created (a phone always has one).
    fn phone(name: &str) -> [String; 4] {
        let base = unique_dir(name);
        std::fs::create_dir_all(base.join("vault")).unwrap();
        ["vault", "config.toml", "sync_secrets.json", "sync_manifest.json"].map(|p| base.join(p).to_string_lossy().into_owned())
    }

    fn with_key(paths: &[String; 4]) {
        engine(&paths[0], &paths[1], &paths[2], &paths[3]).init_fresh().unwrap();
    }

    fn status(paths: &[String; 4]) -> ArcaModeStatusDto {
        arca_mode_status(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone()).unwrap()
    }

    fn enable(paths: &[String; 4]) -> Result<(), ArcaModeFailure> {
        arca_mode_enable(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), 1_000)
    }

    fn sync_run(paths: &[String; 4], hub_url: &str, token: &str) -> Result<ArcaSyncDto, ArcaModeFailure> {
        arca_sync_run(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), hub_url.to_string(), token.to_string(), false, 2_000)
    }

    fn pending(paths: &[String; 4]) -> Result<Vec<PendingEpochDto>, ArcaModeFailure> {
        arca_epochs_pending(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone())
    }

    fn epoch(number: u32, id: &str) -> PendingEpochDto {
        PendingEpochDto { epoch: number, id: format!("manifest/{id:0>64}") }
    }

    /// A hub that refuses everyone (401), on a runtime of its own: a token it does not know.
    fn hub_that_refuses() -> String {
        let (tx, rx) = std::sync::mpsc::channel::<SocketAddr>();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let router = Router::new().fallback(|| async { StatusCode::UNAUTHORIZED });
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, router).await.unwrap();
            });
        });
        format!("http://{}", rx.recv().unwrap())
    }

    // ---------- status ----------

    #[test]
    fn a_vault_that_never_turned_the_mode_on_says_off_and_no_send_waiting() {
        let paths = phone("status-off");
        with_key(&paths);
        assert_eq!(status(&paths), ArcaModeStatusDto { on: false, send_pending: false });
    }

    // ---------- enable ----------

    #[test]
    fn a_phone_without_a_key_cannot_turn_the_mode_on_and_nothing_is_written() {
        let paths = phone("enable-no-key");
        let failure = enable(&paths).unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::NoKey, "{}", failure.detail);
        assert!(!status(&paths).on);
        assert!(!std::path::Path::new(&paths[3]).with_file_name("arca_mode.json").exists(), "no switch file");
    }

    #[test]
    fn with_a_key_the_mode_turns_on_and_the_switch_file_exists() {
        let paths = phone("enable-ok");
        with_key(&paths);
        enable(&paths).unwrap();
        assert!(status(&paths).on);
        assert!(std::path::Path::new(&paths[3]).with_file_name("arca_mode.json").exists());
    }

    #[test]
    fn turning_it_on_twice_is_refused_as_already_on() {
        let paths = phone("enable-twice");
        with_key(&paths);
        enable(&paths).unwrap();
        assert_eq!(enable(&paths).unwrap_err().kind, ArcaModeErrorKind::AlreadyOn);
    }

    #[test]
    fn a_send_waiting_for_the_phone_blocks_the_mode_and_nothing_is_written() {
        // The seam: the public call reads the Send this process has in flight; this is what it does with the answer.
        let paths = phone("enable-send-pending");
        with_key(&paths);
        let failure = enable_with(&engine(&paths[0], &paths[1], &paths[2], &paths[3]), true, 1_000).unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::SendPending, "{}", failure.detail);
        assert!(!status(&paths).on);
    }

    // ---------- one cycle ----------

    #[test]
    fn a_cycle_with_the_mode_off_is_refused_before_any_network_call() {
        let paths = phone("sync-off");
        with_key(&paths);
        // An address nothing listens on: if the code reached the network the kind would be `Other`, not `NotInArcaMode`.
        let failure = sync_run(&paths, "http://127.0.0.1:9", "token").unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::NotInArcaMode, "{}", failure.detail);
    }

    #[test]
    fn a_hub_that_does_not_know_the_token_is_told_as_unauthorized_and_the_token_is_not_in_the_detail() {
        let paths = phone("sync-401");
        with_key(&paths);
        enable(&paths).unwrap();
        let failure = sync_run(&paths, &hub_that_refuses(), "SECRET-TOKEN-123").unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::Unauthorized, "{}", failure.detail);
        assert!(!failure.detail.contains("SECRET-TOKEN-123"), "{}", failure.detail);
    }

    #[test]
    fn plain_http_to_another_machine_is_refused_before_the_token_leaves_the_phone() {
        let paths = phone("sync-insecure");
        with_key(&paths);
        enable(&paths).unwrap();
        assert_eq!(sync_run(&paths, "http://192.168.0.10:7000", "token").unwrap_err().kind, ArcaModeErrorKind::InsecureUrl);
        assert_eq!(sync_run(&paths, "https://hub.example/some/path", "token").unwrap_err().kind, ArcaModeErrorKind::BadUrl);
    }

    #[test]
    fn plain_http_to_the_lan_is_only_accepted_when_the_person_was_warned_and_the_other_refusals_stay() {
        // Building the client opens no connection, so a LAN address is enough to see what is accepted.
        let refused = hub_store_for("http://192.168.0.10:7000", "token", false).err().unwrap();
        assert_eq!(refused.kind, ArcaModeErrorKind::InsecureUrl);
        assert!(hub_store_for("http://192.168.0.10:7000", "token", true).is_ok(), "warned: http to the LAN is accepted");
        assert!(hub_store_for("https://hub.example", "token", false).is_ok(), "https never needs the permission");
        // The permission is for plain http only: a bad address or token stays refused with it.
        assert_eq!(hub_store_for("https://hub.example/some/path", "token", true).err().unwrap().kind, ArcaModeErrorKind::BadUrl);
        assert_eq!(hub_store_for("ftp://hub.example", "token", true).err().unwrap().kind, ArcaModeErrorKind::BadUrl);
        assert_eq!(hub_store_for("http://192.168.0.10:7000", "", true).err().unwrap().kind, ArcaModeErrorKind::BadToken);
    }

    #[test]
    fn the_permission_for_plain_http_is_only_for_the_own_network_never_for_a_public_address() {
        // A screen that passes `true` by mistake (or from a stale "already warned") must not send the token across the internet.
        for public in ["http://hub.example.com", "http://203.0.113.7:7000", "http://8.8.8.8", "http://[2001:db8::1]:7000", "http://172.32.0.1", "http://100.128.0.1", "http://192.169.0.1", "http://localhost.evil.com", "http://HUB.EXAMPLE.COM"] {
            let refused = hub_store_for(public, "token", true).err().unwrap_or_else(|| panic!("{public} was accepted"));
            assert_eq!(refused.kind, ArcaModeErrorKind::InsecureUrl, "{public}");
        }
        for own in ["http://192.168.0.10:7000", "http://10.0.0.5", "http://172.16.4.2", "http://172.31.255.255", "http://169.254.1.1", "http://100.64.0.1", "http://100.127.255.255", "http://127.0.0.1:7000", "http://localhost:7000", "http://[::1]:7000", "http://[fd12:3456::1]:7000", "http://[fe80::1]", "http://my-hub.local:7000", "HTTP://192.168.0.10:7000"] {
            assert!(hub_store_for(own, "token", true).is_ok(), "{own} should be accepted once the person was warned");
        }
    }

    #[test]
    fn the_permission_does_not_relax_credentials_a_path_or_a_query_in_the_address() {
        for odd in ["http://user:pass@192.168.0.10:7000", "http://192.168.0.10:7000/some/path", "http://192.168.0.10:7000/?x=1", "http://192.168.0.10:7000#frag", "http://public.example.com@192.168.0.10:7000", "http://192.168.0.10@public.example.com"] {
            let refused = hub_store_for(odd, "token", true).err().unwrap_or_else(|| panic!("{odd} was accepted"));
            assert!(matches!(refused.kind, ArcaModeErrorKind::BadUrl | ArcaModeErrorKind::InsecureUrl), "{odd}: {:?}", refused.kind);
        }
    }

    #[test]
    fn the_token_is_taken_out_of_a_failure_text_wherever_it_appears() {
        assert_eq!(redact("hub said no to SECRET-TOKEN-123 (SECRET-TOKEN-123)", "SECRET-TOKEN-123"), "hub said no to *** (***)");
        assert_eq!(redact("nothing to hide", ""), "nothing to hide", "an empty token must not turn every gap into stars");
    }

    #[test]
    fn a_warned_cycle_over_plain_http_reaches_the_hub_instead_of_being_refused() {
        let paths = phone("sync-insecure-allowed");
        with_key(&paths);
        enable(&paths).unwrap();
        // Loopback needs no permission, so this also shows the flag does not change what a reachable hub answers.
        let result = arca_sync_run(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), hub_that_refuses(), "token".into(), true, 2_000);
        assert_eq!(result.unwrap_err().kind, ArcaModeErrorKind::Unauthorized);
    }

    // ---------- epochs ----------

    #[test]
    fn with_the_mode_off_no_epoch_is_pending_and_acknowledging_says_the_mode_is_off() {
        let paths = phone("epochs-off");
        with_key(&paths);
        assert_eq!(pending(&paths).unwrap(), Vec::<PendingEpochDto>::new());
        let failure = arca_epochs_acknowledge(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), vec![epoch(1, "a")]).unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::NotInArcaMode, "{}", failure.detail);
    }

    #[test]
    fn only_the_pairs_the_person_saw_and_that_are_still_pending_are_acknowledged() {
        let fresh = [epoch(1, "a"), epoch(1, "b"), epoch(2, "c")];
        assert_eq!(positions_seen(&fresh, &[epoch(1, "b")]), vec![1], "the one that was shown");
        assert_eq!(positions_seen(&fresh, &[epoch(2, "c"), epoch(1, "a")]), vec![0, 2], "in the order of the fresh list, each once");
        assert_eq!(positions_seen(&fresh, &[epoch(1, "b"), epoch(1, "b")]), vec![1], "a pair repeated is still one");
    }

    #[test]
    fn the_epoch_alone_never_matches_a_forged_object_cannot_hide_the_real_rotation() {
        let fresh = [epoch(1, "real")];
        assert_eq!(positions_seen(&fresh, &[epoch(1, "forged")]), Vec::<usize>::new(), "same epoch, another id");
        assert_eq!(positions_seen(&fresh, &[epoch(2, "real")]), Vec::<usize>::new(), "same id, another epoch");
        assert_eq!(positions_seen(&fresh, &[]), Vec::<usize>::new(), "nothing seen");
    }
}

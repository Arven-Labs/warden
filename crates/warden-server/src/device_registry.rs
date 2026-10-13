//! Fase 9.3 — persistent device pairing, the piece 9.4/9.5's routing (`CallDeviceTool`) was
//! missing: knowing the shared `auth_key` is enough to `Hello` and chat, but routing a tool call
//! to (or from) a device now also requires that device to have been explicitly `approve`d by the
//! operator (via `warden-server devices approve <id>`, `main.rs`) — not just present and using the
//! right key. See `server.rs`'s `Hello`/`CallDeviceTool` handling and `project/PENDING.md` P61.
//!
//! As of P36, the shared `auth_key` is only a *pairing* key: the first `Hello` with it gets a
//! per-device token back (`authenticate`), and that token is what the device presents from then
//! on. That's what makes `revoke` stick (a revoked device's token stops working, and the server
//! closes its open connection) and lets the operator rotate the pairing key without re-pairing
//! every device already holding a token.
//!
//! `PairingStore` is deliberately stateless between calls — each method re-reads the JSON file
//! from disk, mutates if needed, and writes it back. The long-running `warden-server` process and
//! a one-shot `warden-server devices approve <id>` invocation are two separate processes sharing
//! this file as their only coordination; an approval made while the server is already up must take
//! effect on the *next* `CallDeviceTool` without a restart, which a cached in-memory copy
//! wouldn't give us. The file is tiny and touched at most once per `Hello`/`CallDeviceTool`, so the
//! read-mutate-write cost is a non-issue. Known limitation, accepted for this MVP: two writes
//! racing (e.g. an operator's `approve` landing at the exact instant a `Hello` updates
//! `last_seen_ms`) can lose one of them — a single-operator CLI tool at this call frequency doesn't
//! warrant real file locking yet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingStatus {
    Pending,
    Approved,
    Revoked,
}

impl std::fmt::Display for PairingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairingStatus::Pending => write!(f, "pending"),
            PairingStatus::Approved => write!(f, "approved"),
            PairingStatus::Revoked => write!(f, "revoked"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedDevice {
    pub device_name: String,
    pub status: PairingStatus,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    /// SHA-256 (hex) of the per-device token issued in `HelloAck` (P36) — never the token itself,
    /// so a leaked `devices.json` doesn't hand out working credentials. `None` for a device
    /// recorded before tokens existed, until its next `Hello` issues one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_hash: Option<String>,
    /// P84: the member this device belongs to, set when it paired with their password. `None` is
    /// the root's — every device paired with the pairing key, and every one from before P84.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// P36: the label of the invite this device paired with, so `devices list` shows who it was for.
    /// `None` for a device that paired with the pairing key or a member's password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite_name: Option<String>,
}

/// A one-use code that pairs one device (P36), made with `devices invite`. Only its hash is stored (the key of
/// `RegistryFile.invites`), like a device token's.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub name: String,
    pub created_ms: i64,
    pub expires_ms: i64,
}

/// What a `Hello` proved besides a token (P84).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingProof {
    Nothing,
    /// The code of an invite (P36), as the device sent it: the device is the root's, and the invite is used up.
    Invite(String),
    /// The hub's pairing key: the device is the root's.
    PairingKey,
    /// A member's password, already checked: the device is theirs.
    Member(String),
}

/// What a successful `PairingStore::authenticate` hands back to `server.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloOutcome {
    /// Status right after this `Hello` — see `authenticate` for when a re-pair resets it.
    pub status: PairingStatus,
    /// A freshly issued device token, to send back in `HelloAck` — `None` when the device
    /// authenticated with its existing token, which stays valid.
    pub issued_token: Option<String>,
    /// Whose device it is now — `None` for the root.
    pub user: Option<String>,
}

/// Why `PairingStore::authenticate` turned a `Hello` down — `Display` is the `AuthError.reason`
/// the client sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthRejection {
    Revoked,
    InvalidCredentials,
}

impl std::fmt::Display for AuthRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthRejection::Revoked => write!(f, "device revoked"),
            AuthRejection::InvalidCredentials => write!(f, "invalid auth key or invite (the pairing key only pairs the first device — ask for an invite)"),
        }
    }
}

fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    devices: HashMap<String, PairedDevice>,
    /// Invites not used yet, by the hash of their code (P36).
    #[serde(default)]
    invites: HashMap<String, Invite>,
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

fn load(path: &Path) -> anyhow::Result<RegistryFile> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(serde_json::from_str(&contents)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(RegistryFile::default()),
        Err(err) => Err(err.into()),
    }
}

fn save(path: &Path, file: &RegistryFile) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(file)?)?;
    Ok(())
}

/// Handle onto the on-disk registry at `path` — see module docs for why this holds no in-memory
/// state beyond the path itself.
pub struct PairingStore {
    path: PathBuf,
    /// Test hubs only: the pairing key pairs more than the first device (`PairingKey::with_open_pairing`).
    open_key: bool,
}

impl PairingStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), open_key: false }
    }

    pub(crate) fn with_open_key_pairing(mut self, open: bool) -> Self {
        self.open_key = open;
        self
    }

    /// Decides a `Hello` (P36) and records the device — the shared auth key is only a *pairing*
    /// key; what a device keeps using afterwards is its own token, issued here. In order:
    ///
    /// - a `Revoked` device is always turned away, token or key;
    /// - a `device_token` matching the stored hash is accepted as-is, status untouched — and
    ///   keeps working after the operator rotates the pairing key;
    /// - otherwise, `pairing_key_ok` issues a new token. If this `device_id` already held a token,
    ///   its status goes back to `Pending`: knowing the pairing key must not be enough to take over
    ///   another device's `Approved` routing rights just by claiming its id. A record from before
    ///   tokens existed (`token_hash: None`) keeps its status, so upgrading doesn't un-approve
    ///   everyone;
    /// - anything else is `InvalidCredentials`.
    ///
    /// A never-seen device starts `Pending`, visible to the operator via `devices list`.
    pub fn authenticate(
        &self,
        device_id: &str,
        device_name: &str,
        device_token: Option<&str>,
        pairing_key_ok: bool,
    ) -> anyhow::Result<Result<HelloOutcome, AuthRejection>> {
        let proof = if pairing_key_ok { PairingProof::PairingKey } else { PairingProof::Nothing };
        self.authenticate_as(device_id, device_name, device_token, proof)
    }

    /// `authenticate`, with a member's password as a way to pair too (P84). A token keeps the
    /// device's owner; pairing afresh makes it the prover's — the root's for the key, the member's
    /// for their password.
    pub fn authenticate_as(
        &self,
        device_id: &str,
        device_name: &str,
        device_token: Option<&str>,
        proof: PairingProof,
    ) -> anyhow::Result<Result<HelloOutcome, AuthRejection>> {
        let mut file = load(&self.path)?;
        let existing = file.devices.get(device_id);
        if existing.is_some_and(|d| d.status == PairingStatus::Revoked) {
            return Ok(Err(AuthRejection::Revoked));
        }
        let token_ok = match (existing.and_then(|d| d.token_hash.as_deref()), device_token) {
            (Some(stored), Some(token)) => stored == hash_token(token),
            _ => false,
        };
        if !token_ok && proof == PairingProof::Nothing {
            return Ok(Err(AuthRejection::InvalidCredentials));
        }
        // P36: the shared key pairs only the very first device of a hub; after that a new device (or one that
        // lost its token) needs an invite. A revoked device still counts, so revoking everyone doesn't reopen it.
        if !token_ok && proof == PairingProof::PairingKey && !file.devices.is_empty() && !self.open_key {
            return Ok(Err(AuthRejection::InvalidCredentials));
        }

        let now = now_millis();
        // P36: an invite is used up only when it really pairs a device — never by one that came back with its
        // token, and never by a revoked one (turned away above).
        let mut used_invite = None;
        if let (false, PairingProof::Invite(code)) = (token_ok, &proof) {
            file.invites.retain(|_, invite| invite.expires_ms > now);
            match file.invites.remove(&hash_token(code)) {
                Some(invite) => used_invite = Some(invite.name),
                None => return Ok(Err(AuthRejection::InvalidCredentials)),
            }
        }
        let issued_token = (!token_ok).then(warden_bootstrap::generate_auth_key);
        let device = file.devices.entry(device_id.to_string()).or_insert_with(|| PairedDevice {
            device_name: device_name.to_string(),
            status: PairingStatus::Pending,
            first_seen_ms: now,
            last_seen_ms: now,
            token_hash: None,
            user: None,
            invite_name: None,
        });
        device.device_name = device_name.to_string();
        device.last_seen_ms = now;
        if let Some(token) = &issued_token {
            if device.token_hash.is_some() {
                device.status = PairingStatus::Pending;
            }
            device.token_hash = Some(hash_token(token));
            device.user = match proof {
                PairingProof::Member(user) => Some(user),
                _ => None,
            };
            device.invite_name = used_invite;
        }
        let (status, user) = (device.status, device.user.clone());
        save(&self.path, &file)?;
        Ok(Ok(HelloOutcome { status, issued_token, user }))
    }

    pub fn status(&self, device_id: &str) -> anyhow::Result<Option<PairingStatus>> {
        Ok(load(&self.path)?.devices.get(device_id).map(|d| d.status))
    }

    pub fn approve(&self, device_id: &str) -> anyhow::Result<()> {
        self.set_status(device_id, PairingStatus::Approved)
    }

    pub fn revoke(&self, device_id: &str) -> anyhow::Result<()> {
        self.set_status(device_id, PairingStatus::Revoked)
    }

    fn set_status(&self, device_id: &str, status: PairingStatus) -> anyhow::Result<()> {
        let mut file = load(&self.path)?;
        let device = file
            .devices
            .get_mut(device_id)
            .ok_or_else(|| anyhow::anyhow!("device '{device_id}' has never connected to this server — it needs to Hello at least once before it can be {status}"))?;
        device.status = status;
        save(&self.path, &file)
    }

    /// Revokes every device of member `user` (P84: removing them). Returns how many.
    pub fn revoke_user_devices(&self, user: &str) -> anyhow::Result<usize> {
        let mut file = load(&self.path)?;
        let mut revoked = 0;
        for device in file.devices.values_mut().filter(|d| d.user.as_deref() == Some(user) && d.status != PairingStatus::Revoked) {
            device.status = PairingStatus::Revoked;
            revoked += 1;
        }
        save(&self.path, &file)?;
        Ok(revoked)
    }

    /// Makes a one-use invite called `name` that pairs a device for `ttl` (P36) and returns its code, which is
    /// not kept anywhere (only its hash is): the caller shows it once. Another invite with the same name is replaced,
    /// so a lost code is cancelled by asking for a new one.
    pub fn create_invite(&self, name: &str, ttl: std::time::Duration) -> anyhow::Result<String> {
        let mut file = load(&self.path)?;
        let now = now_millis();
        file.invites.retain(|_, invite| invite.expires_ms > now && invite.name != name);
        let code = warden_bootstrap::generate_auth_key();
        file.invites.insert(hash_token(&code), Invite { name: name.to_string(), created_ms: now, expires_ms: now + ttl.as_millis() as i64 });
        save(&self.path, &file)?;
        Ok(code)
    }

    /// The invites not used or expired yet, by name.
    pub fn list_invites(&self) -> anyhow::Result<Vec<Invite>> {
        let now = now_millis();
        let mut invites: Vec<_> = load(&self.path)?.invites.into_values().filter(|invite| invite.expires_ms > now).collect();
        invites.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(invites)
    }

    /// Cancels the unused invite called `name`; whether there was one.
    pub fn cancel_invite(&self, name: &str) -> anyhow::Result<bool> {
        let mut file = load(&self.path)?;
        let before = file.invites.len();
        file.invites.retain(|_, invite| invite.name != name);
        let cancelled = file.invites.len() != before;
        if cancelled {
            save(&self.path, &file)?;
        }
        Ok(cancelled)
    }

    pub fn list(&self) -> anyhow::Result<Vec<(String, PairedDevice)>> {
        let mut devices: Vec<_> = load(&self.path)?.devices.into_iter().collect();
        devices.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(devices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "warden-device-registry-test-{}.json",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    /// A first `Hello` with no token: the pairing key for the hub's very first device, an invite for the rest (P36).
    fn pair(store: &PairingStore, device_id: &str, device_name: &str) -> HelloOutcome {
        if store.list().unwrap().is_empty() {
            return store.authenticate(device_id, device_name, None, true).unwrap().unwrap();
        }
        let code = store.create_invite("test", std::time::Duration::from_secs(60)).unwrap();
        store.authenticate_as(device_id, device_name, None, PairingProof::Invite(code)).unwrap().unwrap()
    }

    #[test]
    fn the_pairing_key_pairs_only_the_first_device() {
        let store = PairingStore::new(temp_path());
        pair(&store, "dev-a", "A");
        assert!(matches!(store.authenticate("dev-b", "B", None, true).unwrap(), Err(AuthRejection::InvalidCredentials)));
        // A device that lost its token is no exception, and a token keeps working with the key beside it.
        assert!(matches!(store.authenticate("dev-a", "A", None, true).unwrap(), Err(AuthRejection::InvalidCredentials)));
        let token = pair(&store, "dev-c", "C").issued_token.unwrap();
        assert!(store.authenticate("dev-c", "C", Some(&token), true).unwrap().is_ok());
    }

    #[test]
    fn revoking_the_only_device_does_not_reopen_the_key() {
        let store = PairingStore::new(temp_path());
        pair(&store, "dev-a", "A");
        store.revoke("dev-a").unwrap();
        assert!(matches!(store.authenticate("dev-b", "B", None, true).unwrap(), Err(AuthRejection::InvalidCredentials)));
    }

    #[test]
    fn a_never_seen_device_starts_pending_and_gets_a_token() {
        let store = PairingStore::new(temp_path());
        let outcome = pair(&store, "dev-a", "Device A");
        assert_eq!(outcome.status, PairingStatus::Pending);
        assert_eq!(outcome.issued_token.as_ref().map(String::len), Some(64));
        assert_eq!(store.status("dev-a").unwrap(), Some(PairingStatus::Pending));
    }

    #[test]
    fn the_token_itself_is_never_written_to_disk() {
        let path = temp_path();
        let token = pair(&PairingStore::new(&path), "dev-a", "Device A").issued_token.unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(!contents.contains(&token));
        assert!(contents.contains(&hash_token(&token)));
    }

    #[test]
    fn reconnecting_with_the_token_refreshes_name_but_not_an_existing_approved_status() {
        let store = PairingStore::new(temp_path());
        let token = pair(&store, "dev-a", "Old Name").issued_token.unwrap();
        store.approve("dev-a").unwrap();

        let outcome = store.authenticate("dev-a", "New Name", Some(&token), false).unwrap().unwrap();
        assert_eq!(outcome, HelloOutcome { status: PairingStatus::Approved, issued_token: None, user: None });

        let devices = store.list().unwrap();
        assert_eq!(devices[0].1.device_name, "New Name");
        assert_eq!(devices[0].1.status, PairingStatus::Approved);
    }

    #[test]
    fn a_wrong_token_without_the_pairing_key_is_rejected() {
        let store = PairingStore::new(temp_path());
        pair(&store, "dev-a", "Device A");
        let result = store.authenticate("dev-a", "Device A", Some("not-the-token"), false).unwrap();
        assert_eq!(result, Err(AuthRejection::InvalidCredentials));
    }

    #[test]
    fn no_token_and_a_wrong_pairing_key_is_rejected() {
        let store = PairingStore::new(temp_path());
        let result = store.authenticate("dev-a", "Device A", None, false).unwrap();
        assert_eq!(result, Err(AuthRejection::InvalidCredentials));
        assert_eq!(store.status("dev-a").unwrap(), None);
    }

    #[test]
    fn re_pairing_an_id_that_already_had_a_token_resets_it_to_pending_and_invalidates_the_old_token() {
        let store = PairingStore::new(temp_path());
        let old_token = pair(&store, "dev-a", "Device A").issued_token.unwrap();
        store.approve("dev-a").unwrap();

        let outcome = pair(&store, "dev-a", "Impostor");
        assert_eq!(outcome.status, PairingStatus::Pending);
        assert!(outcome.issued_token.is_some());
        assert_eq!(store.authenticate("dev-a", "Device A", Some(&old_token), false).unwrap(), Err(AuthRejection::InvalidCredentials));
    }

    #[test]
    fn a_record_from_before_tokens_keeps_its_status_when_it_first_gets_one() {
        let path = temp_path();
        std::fs::write(
            &path,
            r#"{"devices":{"dev-a":{"device_name":"Device A","status":"approved","first_seen_ms":1,"last_seen_ms":2}}}"#,
        )
        .unwrap();
        let outcome = pair(&PairingStore::new(&path), "dev-a", "Device A");
        assert_eq!(outcome.status, PairingStatus::Approved);
        assert!(outcome.issued_token.is_some());
    }

    #[test]
    fn a_revoked_device_is_rejected_with_its_token_or_the_pairing_key() {
        let store = PairingStore::new(temp_path());
        let token = pair(&store, "dev-a", "Device A").issued_token.unwrap();
        store.revoke("dev-a").unwrap();

        assert_eq!(store.authenticate("dev-a", "Device A", Some(&token), false).unwrap(), Err(AuthRejection::Revoked));
        assert_eq!(store.authenticate("dev-a", "Device A", None, true).unwrap(), Err(AuthRejection::Revoked));
    }

    #[test]
    fn approve_and_revoke_change_status() {
        let store = PairingStore::new(temp_path());
        pair(&store, "dev-a", "Device A");

        store.approve("dev-a").unwrap();
        assert_eq!(store.status("dev-a").unwrap(), Some(PairingStatus::Approved));

        store.revoke("dev-a").unwrap();
        assert_eq!(store.status("dev-a").unwrap(), Some(PairingStatus::Revoked));
    }

    #[test]
    fn approving_or_revoking_an_unknown_device_errors() {
        let store = PairingStore::new(temp_path());
        assert!(store.approve("dev-ghost").is_err());
        assert!(store.revoke("dev-ghost").is_err());
    }

    #[test]
    fn status_of_an_unknown_device_is_none() {
        let store = PairingStore::new(temp_path());
        assert_eq!(store.status("dev-ghost").unwrap(), None);
    }

    #[test]
    fn state_survives_a_reload_from_a_fresh_store_at_the_same_path() {
        let path = temp_path();
        pair(&PairingStore::new(&path), "dev-a", "Device A");
        PairingStore::new(&path).approve("dev-a").unwrap();

        // A brand new `PairingStore` (simulating a separate `devices approve` CLI invocation, or
        // the server process restarting) sees the same state purely from disk.
        let reloaded = PairingStore::new(&path);
        assert_eq!(reloaded.status("dev-a").unwrap(), Some(PairingStatus::Approved));
    }

    #[test]
    fn list_is_sorted_by_device_id() {
        let store = PairingStore::new(temp_path());
        pair(&store, "dev-b", "B");
        pair(&store, "dev-a", "A");

        let ids: Vec<_> = store.list().unwrap().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["dev-a".to_string(), "dev-b".to_string()]);
    }

    /// P84: a member's password pairs the device as theirs; its token keeps it theirs; the pairing
    /// key takes it back to the root; removing the member revokes all of theirs.
    #[test]
    fn a_device_belongs_to_whoever_paired_it() {
        let store = PairingStore::new(temp_path());
        let paired = store.authenticate_as("phone", "Ana's phone", None, PairingProof::Member("ana".into())).unwrap().unwrap();
        assert_eq!(paired.user.as_deref(), Some("ana"));
        let token = paired.issued_token.unwrap();
        let again = store.authenticate_as("phone", "Ana's phone", Some(&token), PairingProof::Nothing).unwrap().unwrap();
        assert_eq!((again.user.as_deref(), again.issued_token), (Some("ana"), None));
        assert!(matches!(store.authenticate_as("tablet", "x", None, PairingProof::Nothing).unwrap(), Err(AuthRejection::InvalidCredentials)));

        let code = store.create_invite("laptop", std::time::Duration::from_secs(60)).unwrap();
        let root = store.authenticate_as("laptop", "Laptop", None, PairingProof::Invite(code)).unwrap().unwrap();
        assert_eq!(root.user, None);
        store.authenticate_as("tablet", "Ana's tablet", None, PairingProof::Member("ana".into())).unwrap().unwrap();
        assert_eq!(store.revoke_user_devices("ana").unwrap(), 2);
        assert_eq!(store.status("phone").unwrap(), Some(PairingStatus::Revoked));
        assert_eq!(store.status("laptop").unwrap(), Some(PairingStatus::Pending), "the root's device stays");
        assert!(matches!(store.authenticate_as("phone", "x", Some(&token), PairingProof::Nothing).unwrap(), Err(AuthRejection::Revoked)));
    }

    const HOUR: std::time::Duration = std::time::Duration::from_secs(3600);

    fn pair_with_invite(store: &PairingStore, device_id: &str, code: &str) -> Result<HelloOutcome, AuthRejection> {
        store.authenticate_as(device_id, device_id, None, PairingProof::Invite(code.to_string())).unwrap()
    }

    /// P36: an invite pairs one device as the root's, remembers who it was for, and is gone after.
    #[test]
    fn an_invite_pairs_one_device_once() {
        let store = PairingStore::new(temp_path());
        let code = store.create_invite("Ana's phone", HOUR).unwrap();
        let paired = pair_with_invite(&store, "phone", &code).unwrap();
        assert_eq!((paired.status, paired.user), (PairingStatus::Pending, None));
        assert!(paired.issued_token.is_some());
        let device = store.list().unwrap().into_iter().find(|(id, _)| id == "phone").unwrap().1;
        assert_eq!(device.invite_name.as_deref(), Some("Ana's phone"));

        assert!(matches!(pair_with_invite(&store, "tablet", &code), Err(AuthRejection::InvalidCredentials)), "used up");
        assert!(store.list_invites().unwrap().is_empty());
    }

    #[test]
    fn an_expired_or_unknown_invite_pairs_nothing() {
        let store = PairingStore::new(temp_path());
        let code = store.create_invite("late", std::time::Duration::ZERO).unwrap();
        assert!(matches!(pair_with_invite(&store, "phone", &code), Err(AuthRejection::InvalidCredentials)));
        assert!(matches!(pair_with_invite(&store, "phone", "not-an-invite"), Err(AuthRejection::InvalidCredentials)));
        assert_eq!(store.status("phone").unwrap(), None, "a refused Hello records nothing");
    }

    /// A device that comes back with its token, or a revoked one, must not burn an invite meant for someone else.
    #[test]
    fn an_invite_is_used_only_by_a_device_that_really_pairs() {
        let store = PairingStore::new(temp_path());
        let token = pair(&store, "laptop", "Laptop").issued_token.unwrap();
        pair(&store, "gone", "Gone");
        store.revoke("gone").unwrap();

        let code = store.create_invite("new", HOUR).unwrap();
        let back = store.authenticate_as("laptop", "Laptop", Some(&token), PairingProof::Invite(code.clone())).unwrap().unwrap();
        assert_eq!(back.issued_token, None);
        assert!(matches!(pair_with_invite(&store, "gone", &code), Err(AuthRejection::Revoked)));
        assert_eq!(store.list_invites().unwrap().len(), 1, "still there");
        pair_with_invite(&store, "fresh", &code).unwrap();
    }

    #[test]
    fn invites_can_be_replaced_and_cancelled_by_name() {
        let store = PairingStore::new(temp_path());
        let first = store.create_invite("phone", HOUR).unwrap();
        let second = store.create_invite("phone", HOUR).unwrap();
        assert_eq!(store.list_invites().unwrap().len(), 1);
        assert!(matches!(pair_with_invite(&store, "a", &first), Err(AuthRejection::InvalidCredentials)), "replaced");
        assert!(store.cancel_invite("phone").unwrap());
        assert!(!store.cancel_invite("phone").unwrap());
        assert!(matches!(pair_with_invite(&store, "a", &second), Err(AuthRejection::InvalidCredentials)), "cancelled");
    }

    /// A registry written before invites existed still reads.
    #[test]
    fn a_registry_without_invites_reads() {
        let path = temp_path();
        std::fs::write(&path, r#"{"devices":{"old":{"device_name":"Old","status":"approved","first_seen_ms":1,"last_seen_ms":2}}}"#).unwrap();
        let store = PairingStore::new(path);
        assert_eq!(store.status("old").unwrap(), Some(PairingStatus::Approved));
        assert!(store.list_invites().unwrap().is_empty());
    }
}

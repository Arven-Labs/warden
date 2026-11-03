//! Handing the arca `vault_key` to a new device through the hub (arca's phase 2b-3).
//!
//! The new device (B) publishes a public key (an offer); an approved device of the same owner (A) encrypts the `vault_key` to
//! it (ECIES) and leaves the result here (a grant); B reads it once. The hub only ever holds public keys and ciphertext.
//!
//! Two things a hub (or a device with no key yet) could do, and the check that answers each, both done by a person comparing
//! four groups of hex on the two devices:
//! - the hub chooses which public key A is shown, so it could swap B's for its own: A and B each compute
//!   `verification_code(device_id, public_key)` before A grants;
//! - ECIES does not say who encrypted, so the hub could hand B a `vault_key` of its own making: A and B each compute
//!   `key_fingerprint(vault_key)` after B has the key.
//!
//! Routes (all behind the device-token check of `arca_api`, so only an approved device gets here), under `/arca/v1/pairing/`:
//! - `PUT offer`, body `{"public_key": "<secp256k1 SEC1 hex>"}`: B's offer, kept as the compressed lowercase form, replacing its
//!   previous one (`204`);
//! - `GET offers`: `{"offers": [{"device_id", "public_key"}, ...]}`, the live offers of the caller's owner whose device is
//!   still approved;
//! - `PUT grant/{device_id}?key=<public key A saw>`, body = the ECIES blob (`204`; `400` for no key, a bad one, or oneself;
//!   `404` without a live offer from that device; `409` when the offer now holds another key, which it keeps). The offer is
//!   used up;
//! - `GET grant`: B's grant, once (`404` after, or when there is none).
//!
//! Offers and grants live in files next to, not inside, the owner's object folder, and expire after `ArcaLimits::pairing_ttl`
//! (by modification time; an expired one, and any `tmp-`/`taken-` leftover of a crashed call, is deleted the next time anyone
//! looks). One lock per owner covers every call, so counting offers, checking one and consuming it cannot interleave.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::device_registry::{PairingStatus, PairingStore};

/// The biggest body of an offer or a grant. An offer is about 100 bytes; a grant is ECIES of 32 bytes: 33 + 12 + 32 + 16.
pub(crate) const MAX_BODY_BYTES: u64 = 4096;

/// Longest device id taken in a path; a longer one cannot be a real device's.
const MAX_DEVICE_ID_BYTES: usize = 128;

// The two codes a person compares live in `warden-truthid`: the client (`warden-sync`) must compute exactly what the hub's
// documentation promises, and `warden-sync` cannot depend on this crate. Re-exported so the path stays the same.
pub use warden_truthid::crypto::{key_fingerprint, verification_code};

/// Which pairing call a request is.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Offer,
    Offers,
    /// The device it is for and the key the giver says it saw (`None`: the query was missing or not `key=<value>`).
    Grant(String, Option<String>),
    TakeGrant,
}

/// `None`: not under `/arca/v1/pairing/`. `Some(Err(status))`: it is, but not a call that exists (`404`, or `405` for another
/// method on a route that does).
pub(crate) fn route(method: &str, target: &str) -> Option<Result<Route, u16>> {
    let rest = target.strip_prefix("/arca/v1/pairing/")?;
    let (route, wanted) = match rest {
        "offer" => (Route::Offer, "PUT"),
        "offers" => (Route::Offers, "GET"),
        "grant" => (Route::TakeGrant, "GET"),
        _ => match rest.strip_prefix("grant/") {
            Some(tail) => {
                let (id, query) = tail.split_once('?').map_or((tail, None), |(id, query)| (id, Some(query)));
                if id.is_empty() || id.len() > MAX_DEVICE_ID_BYTES || id.contains('/') {
                    return Some(Err(404));
                }
                let key = query.and_then(|q| q.strip_prefix("key=")).filter(|k| !k.is_empty() && !k.contains(['&', '=', '?'])).map(str::to_owned);
                (Route::Grant(id.to_string(), key), "PUT")
            }
            None => return Some(Err(404)),
        },
    };
    Some(if method == wanted { Ok(route) } else { Err(405) })
}

/// What a pairing call answers.
pub(crate) struct Done {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Done {
    fn text(status: u16, message: &'static str) -> Self {
        Self { status, content_type: "text/plain; charset=utf-8", body: message.as_bytes().to_vec() }
    }

    fn empty() -> Self {
        Self { status: 204, content_type: "text/plain; charset=utf-8", body: Vec::new() }
    }

    fn unavailable() -> Self {
        Self::text(503, "pairing storage unavailable")
    }
}

/// One owner's pairing files.
pub(crate) struct Pairing {
    dir: PathBuf,
    ttl: Duration,
    max_offers: usize,
    devices: PairingStore,
    /// The owner's lock, held for the whole call.
    lock: Arc<Mutex<()>>,
}

impl Pairing {
    pub(crate) fn new(dir: PathBuf, ttl: Duration, max_offers: usize, devices: PairingStore, lock: Arc<Mutex<()>>) -> Self {
        Self { dir, ttl, max_offers, devices, lock }
    }

    /// B's offer. `device_id` comes from the token, never from the body.
    pub(crate) fn put_offer(&self, device_id: &str, body: &[u8]) -> Done {
        let Some(public_key) = serde_json::from_slice::<serde_json::Value>(body).ok().and_then(|v| v["public_key"].as_str().map(str::to_owned)) else {
            return Done::text(400, "the offer is {\"public_key\": \"<hex>\"}");
        };
        let Ok(public_key) = warden_truthid::crypto::canonical_public_key(&public_key) else {
            return Done::text(400, "the public key is not a secp256k1 key");
        };
        let _lock = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(others) = self.live_offers().map(|offers| offers.into_iter().filter(|(id, _)| id != device_id).count()) else {
            return Done::unavailable();
        };
        if others >= self.max_offers {
            return Done::text(507, "too many offers are waiting");
        }
        let offer = serde_json::json!({ "device_id": device_id, "public_key": public_key });
        // A grant left for an older key of this device is for a key nobody holds any more.
        let _ = fs::remove_file(self.grant_file(device_id));
        match self.write(&self.offer_file(device_id), offer.to_string().as_bytes()) {
            Ok(()) => Done::empty(),
            Err(_) => Done::unavailable(),
        }
    }

    pub(crate) fn list_offers(&self) -> Done {
        let _lock = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(offers) = self.live_offers() else {
            return Done::unavailable();
        };
        let offers: Vec<_> = offers.into_iter().map(|(device_id, public_key)| serde_json::json!({ "device_id": device_id, "public_key": public_key })).collect();
        Done { status: 200, content_type: "application/json", body: serde_json::json!({ "offers": offers }).to_string().into_bytes() }
    }

    /// A's grant for `to`, for the offer that holds `saw`. The offer is used up when the grant is kept.
    pub(crate) fn put_grant(&self, from: &str, to: &str, saw: Option<&str>, blob: &[u8]) -> Done {
        if from == to {
            return Done::text(400, "a device does not grant to itself");
        }
        let Some(saw) = saw.and_then(|key| warden_truthid::crypto::canonical_public_key(key).ok()) else {
            return Done::text(400, "name the key you saw: ?key=<public key hex>");
        };
        let _lock = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let offers = match self.live_offers() {
            Ok(offers) => offers,
            Err(_) => return Done::unavailable(),
        };
        match offers.iter().find(|(id, _)| id == to) {
            None => return Done::text(404, "no offer from that device"),
            Some((_, offered)) if *offered != saw => return Done::text(409, "that device's offer holds another key now"),
            Some(_) => {}
        }
        match self.write(&self.grant_file(to), blob) {
            Ok(()) => {
                // The offer is what makes a grant single: if it cannot be used up, take the grant back rather than leave room
                // for a second one over a grant nobody has read.
                if fs::remove_file(self.offer_file(to)).is_err() {
                    let _ = fs::remove_file(self.grant_file(to));
                    return Done::unavailable();
                }
                Done::empty()
            }
            Err(_) => Done::unavailable(),
        }
    }

    /// B's grant, once: the file is renamed to a name only this call knows, so two readers cannot both get it.
    pub(crate) fn take_grant(&self, device_id: &str) -> Done {
        let _lock = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let taken = self.dir.join(format!("taken-{}", unique()));
        if fs::rename(self.grant_file(device_id), &taken).is_err() {
            return Done::text(404, "no grant for this device");
        }
        let fresh = self.is_fresh(&taken);
        let blob = fs::read(&taken);
        let _ = fs::remove_file(&taken);
        match (fresh, blob) {
            (true, Ok(blob)) => Done { status: 200, content_type: "application/octet-stream", body: blob },
            _ => Done::text(404, "no grant for this device"),
        }
    }

    fn offer_file(&self, device_id: &str) -> PathBuf {
        self.dir.join(format!("offer-{}", name_of(device_id)))
    }

    fn grant_file(&self, device_id: &str) -> PathBuf {
        self.dir.join(format!("grant-{}", name_of(device_id)))
    }

    fn is_fresh(&self, file: &Path) -> bool {
        fs::metadata(file).and_then(|m| m.modified()).ok().and_then(|modified| modified.elapsed().ok()).is_some_and(|age| age <= self.ttl)
    }

    fn is_approved(&self, device_id: &str) -> bool {
        matches!(self.devices.status(device_id), Ok(Some(PairingStatus::Approved)))
    }

    /// `(device_id, public_key)` of every offer not yet expired whose device is still approved, by device id. Expired offers and
    /// grants, and `tmp-`/`taken-` leftovers older than the ttl, are deleted. The caller holds the lock.
    fn live_offers(&self) -> std::io::Result<Vec<(String, String)>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut live = Vec::new();
        for entry in entries {
            let path = entry?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if !["offer-", "grant-", "tmp-", "taken-"].iter().any(|prefix| name.starts_with(prefix)) {
                continue;
            }
            if !self.is_fresh(&path) {
                let _ = fs::remove_file(&path);
            } else if name.starts_with("offer-") {
                let parsed = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
                if let Some((id, key)) = parsed.and_then(|v| Some((v["device_id"].as_str()?.to_owned(), v["public_key"].as_str()?.to_owned()))) {
                    if self.is_approved(&id) {
                        live.push((id, key));
                    }
                }
            }
        }
        live.sort();
        Ok(live)
    }

    /// Written whole or not at all (a temporary file renamed over the name), readable by the hub's user only.
    fn write(&self, file: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let mut dir = fs::DirBuilder::new();
        dir.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dir, 0o700);
        dir.create(&self.dir)?;
        let temporary = self.dir.join(format!("tmp-{}", unique()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let result = options.open(&temporary).and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all())).and_then(|()| fs::rename(&temporary, file));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

/// A file name for a device id: fixed shape, so what a device calls itself is never part of a path.
fn name_of(device_id: &str) -> String {
    Sha256::digest(device_id.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn unique() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    format!("{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst))
}

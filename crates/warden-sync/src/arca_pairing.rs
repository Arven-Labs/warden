//! The client of the hub's arca pairing routes (`/arca/v1/pairing/`, see `warden-server`'s `arca_pairing.rs` and arca's ADR 0016):
//! hands the `vault_key` from a device that has it (A) to a new one (B), through the hub, encrypted to B (ECIES).
//!
//! The hub is not trusted with this. It chooses which public key A is shown, and ECIES does not say who encrypted, so a person
//! has to compare two codes, and this API makes that unskippable by type:
//! - A cannot grant without the **typed** `verification_code` of the offer it is granting to (`grant` compares it, and a
//!   mismatch fails before any request is made);
//! - B cannot adopt the key without the **typed** `key_fingerprint` A shows (`Received::confirm` compares it, and a mismatch
//!   writes nothing).
//!
//! Typed means what the person typed, compared whole: case, spaces and dashes are forgiven, a single wrong digit is not.
//!
//! The requests go out over `reqwest` (the workspace's, `ring`), never following a redirect and never through a proxy from the
//! environment: the device token would go with them. What comes back is read up to a cap, so a hub cannot make a device hold
//! more than it documents.

use std::time::Duration;

use reqwest::{Client, Method, StatusCode, Url};
use warden_truthid::crypto::{canonical_public_key, ecies_decrypt, ecies_encrypt, generate_ecies_keypair, EciesKeyPair};

use crate::SyncEngine;

pub use warden_truthid::crypto::{key_fingerprint, verification_code};

/// The most a grant can be (ECIES of 32 bytes is 93) and a little more; anything bigger is not what the hub documents.
const MAX_GRANT_BYTES: usize = 8 * 1024;
/// A list of offers: at most a handful of small objects.
const MAX_LIST_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a pairing call did not go through. `Display` is meant for the person.
#[derive(Debug)]
pub enum PairingError {
    /// `http` to a host that is not this computer, without having said that is fine: the device token would travel in the clear.
    InsecureUrl,
    /// Not a hub address (no host, credentials in it, a path or a query).
    BadUrl,
    /// The hub does not offer the arca routes (`404`): it was not started with `--arca`.
    NotAvailable,
    /// `401`: the token is not one this hub issued.
    Unauthorized,
    /// `403`: this device is not approved on the hub (pending or revoked).
    Forbidden,
    /// `409`: the offer holds another key now.
    Conflict,
    /// `404` on a grant: that device has no live offer.
    NoOffer,
    /// The typed verification code is not the offer's.
    CodeMismatch,
    /// The typed fingerprint is not the key's.
    FingerprintMismatch,
    /// The key could not be stored on this device (it already has one, or the disk refused).
    Adopt(String),
    /// Anything else: network, an answer that is not what the hub documents, a hub that is full or unavailable.
    Hub(String),
}

impl std::fmt::Display for PairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairingError::InsecureUrl => write!(f, "this hub address is plain http to another computer: the device token would travel unencrypted"),
            PairingError::BadUrl => write!(f, "that is not a hub address (expected https://host[:port], with no user, path or query)"),
            PairingError::NotAvailable => write!(f, "this hub does not offer the arca routes (start it with --arca)"),
            PairingError::Unauthorized => write!(f, "the hub does not know this device's token"),
            PairingError::Forbidden => write!(f, "this device is not approved on the hub"),
            PairingError::Conflict => write!(f, "the other device made a new offer in the meantime; list the offers again"),
            PairingError::NoOffer => write!(f, "that device has no offer waiting (it expired, or was used)"),
            PairingError::CodeMismatch => write!(f, "that is not the code of this offer"),
            PairingError::FingerprintMismatch => write!(f, "that is not the fingerprint of the key that arrived; nothing was stored"),
            PairingError::Adopt(why) => write!(f, "could not store the key on this device: {why}"),
            PairingError::Hub(why) => write!(f, "the hub call failed: {why}"),
        }
    }
}

impl std::error::Error for PairingError {}

/// Where the hub is and the token it issued to this device. Never shows the token in `Debug`.
#[derive(Clone)]
pub struct HubAccess {
    base: Url,
    token: String,
}

impl HubAccess {
    /// `https` to any host, `http` only to this computer (`127.0.0.1`, `::1`, `localhost`).
    pub fn new(url: &str, device_token: &str) -> Result<Self, PairingError> {
        Self::parse(url, device_token, false)
    }

    /// Same, but also `http` to another host: the token travels in the clear (the name is on purpose).
    pub fn new_allowing_insecure_http(url: &str, device_token: &str) -> Result<Self, PairingError> {
        Self::parse(url, device_token, true)
    }

    fn parse(url: &str, device_token: &str, allow_insecure: bool) -> Result<Self, PairingError> {
        let parsed = Url::parse(url.trim()).map_err(|_| PairingError::BadUrl)?;
        let plain = match parsed.scheme() {
            "https" => false,
            "http" => true,
            _ => return Err(PairingError::BadUrl),
        };
        if parsed.host().is_none() || !parsed.username().is_empty() || parsed.password().is_some() || !matches!(parsed.path(), "" | "/") || parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(PairingError::BadUrl);
        }
        if plain && !allow_insecure && !is_this_computer(&parsed) {
            return Err(PairingError::InsecureUrl);
        }
        Ok(Self { base: parsed, token: device_token.to_string() })
    }

    /// `https://hub:port/arca/v1/pairing/<tail...>`, each part percent-encoded as one path segment.
    fn url(&self, tail: &[&str]) -> Url {
        let mut url = self.base.clone();
        {
            let mut segments = url.path_segments_mut().expect("an http(s) address can have a path");
            segments.clear().extend(["arca", "v1", "pairing"]).extend(tail);
        }
        url
    }

    async fn send(&self, method: Method, url: Url, body: Option<Vec<u8>>, content_type: &str, cap: usize) -> Result<(StatusCode, Vec<u8>), PairingError> {
        let client = Client::builder().redirect(reqwest::redirect::Policy::none()).no_proxy().timeout(REQUEST_TIMEOUT).build().map_err(|e| PairingError::Hub(e.to_string()))?;
        let mut request = client.request(method, url).bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.header("Content-Type", content_type).body(body);
        }
        let mut response = request.send().await.map_err(|e| PairingError::Hub(e.without_url().to_string()))?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| PairingError::Hub(e.without_url().to_string()))? {
            if body.len() + chunk.len() > cap {
                return Err(PairingError::Hub("the hub's answer is bigger than it documents".into()));
            }
            body.extend_from_slice(&chunk);
        }
        Ok((status, body))
    }
}

fn is_this_computer(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

impl std::fmt::Debug for HubAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubAccess").field("base", &self.base.as_str()).field("token", &"<redacted>").finish()
    }
}

/// What the statuses every route can answer mean; `None`: the caller decides (a `404` means something different per route).
fn common_failure(status: StatusCode) -> Option<PairingError> {
    match status.as_u16() {
        401 => Some(PairingError::Unauthorized),
        403 => Some(PairingError::Forbidden),
        409 => Some(PairingError::Conflict),
        507 => Some(PairingError::Hub("the hub has too many offers waiting for this owner".into())),
        503 => Some(PairingError::Hub("the hub is busy or its storage is unavailable; try again".into())),
        s if s >= 400 => Some(PairingError::Hub(format!("the hub refused the call ({s})"))),
        _ => None,
    }
}

/// The one status a route documents for success. A redirect (not followed), a bare `200` where a `204` is documented, or a `202` from
/// a proxy in front of the hub are not "stored": the person must not be told it worked.
fn require(status: StatusCode, documented: u16) -> Result<(), PairingError> {
    if status.as_u16() == documented {
        Ok(())
    } else {
        Err(PairingError::Hub(format!("the hub answered with an unexpected status ({})", status.as_u16())))
    }
}

/// What the person typed, as it is compared: no spaces, no dashes, lowercase.
fn normalized(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace() && *c != '-').flat_map(char::to_lowercase).collect()
}

/// B's side: the offer this device made, and the key pair that lives only here.
pub struct Offer {
    device_id: String,
    pair: EciesKeyPair,
}

impl Offer {
    /// Makes a key pair and publishes its public half as this device's offer, replacing the one it had.
    pub async fn publish(access: &HubAccess, device_id: &str) -> Result<Offer, PairingError> {
        let pair = generate_ecies_keypair();
        let body = serde_json::to_vec(&serde_json::json!({ "public_key": pair.public_hex })).map_err(|e| PairingError::Hub(e.to_string()))?;
        let (status, _) = access.send(Method::PUT, access.url(&["offer"]), Some(body), "application/json", MAX_GRANT_BYTES).await?;
        if status == StatusCode::NOT_FOUND {
            return Err(PairingError::NotAvailable);
        }
        if let Some(failure) = common_failure(status) {
            return Err(failure);
        }
        require(status, 204)?;
        Ok(Offer { device_id: device_id.to_string(), pair })
    }

    /// What B shows, for the person to type on A.
    pub fn code(&self) -> String {
        verification_code(&self.device_id, &self.pair.public_hex)
    }

    /// One try at reading the grant: `None` while there is none (or after it was read, or expired).
    pub async fn receive(&self, access: &HubAccess) -> Result<Option<Received>, PairingError> {
        let (status, body) = access.send(Method::GET, access.url(&["grant"]), None, "", MAX_GRANT_BYTES).await?;
        if status == StatusCode::NOT_FOUND {
            // The hub says "no grant" when the route exists; any other 404 is a hub without the routes.
            return if String::from_utf8_lossy(&body).contains("no grant") { Ok(None) } else { Err(PairingError::NotAvailable) };
        }
        if let Some(failure) = common_failure(status) {
            return Err(failure);
        }
        require(status, 200)?;
        let plain = ecies_decrypt(&body, &self.pair.secret).map_err(|_| PairingError::Hub("the grant could not be decrypted with this device's key".into()))?;
        let key: [u8; 32] = plain.try_into().map_err(|_| PairingError::Hub("the grant is not a 32-byte key".into()))?;
        Ok(Some(Received { key }))
    }
}

impl std::fmt::Debug for Offer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Offer").field("device_id", &self.device_id).field("secret", &"<redacted>").finish()
    }
}

/// A's side: an offer the hub lists. What is in it is what the hub says; the person is the one who checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOffer {
    pub device_id: String,
    pub public_key: String,
}

impl PendingOffer {
    /// What this device computes from what the hub gave it, for A to show (and compare with what B shows).
    pub fn code(&self) -> String {
        verification_code(&self.device_id, &self.public_key)
    }
}

/// The offers waiting for the caller's owner, by device id.
pub async fn list_offers(access: &HubAccess) -> Result<Vec<PendingOffer>, PairingError> {
    let (status, body) = access.send(Method::GET, access.url(&["offers"]), None, "", MAX_LIST_BYTES).await?;
    if status == StatusCode::NOT_FOUND {
        return Err(PairingError::NotAvailable);
    }
    if let Some(failure) = common_failure(status) {
        return Err(failure);
    }
    require(status, 200)?;
    let json: serde_json::Value = serde_json::from_slice(&body).map_err(|_| PairingError::Hub("the list of offers is not what the hub documents".into()))?;
    let offers = json["offers"].as_array().ok_or_else(|| PairingError::Hub("the list of offers is not what the hub documents".into()))?;
    offers
        .iter()
        .map(|offer| match (offer["device_id"].as_str(), offer["public_key"].as_str()) {
            (Some(device_id), Some(public_key)) => Ok(PendingOffer { device_id: device_id.to_string(), public_key: public_key.to_string() }),
            _ => Err(PairingError::Hub("an offer is not what the hub documents".into())),
        })
        .collect()
}

/// Encrypts `vault_key` to the offer's key and leaves it on the hub. `typed_code` is what the person typed after reading it on
/// B; if it is not this offer's `verification_code` the call fails with `CodeMismatch` **before any request**.
pub async fn grant(access: &HubAccess, offer: &PendingOffer, typed_code: &str, vault_key: &[u8; 32]) -> Result<(), PairingError> {
    if normalized(typed_code) != normalized(&offer.code()) {
        return Err(PairingError::CodeMismatch);
    }
    let public_key = canonical_public_key(&offer.public_key).map_err(|_| PairingError::Hub("the offer's public key is not a valid key".into()))?;
    let blob = ecies_encrypt(vault_key, &public_key).map_err(|_| PairingError::Hub("could not encrypt the key to the offer".into()))?;
    let mut url = access.url(&["grant", &offer.device_id]);
    url.query_pairs_mut().append_pair("key", &public_key);
    let (status, body) = access.send(Method::PUT, url, Some(blob), "application/octet-stream", MAX_GRANT_BYTES).await?;
    if status == StatusCode::NOT_FOUND {
        return if String::from_utf8_lossy(&body).contains("no offer") { Err(PairingError::NoOffer) } else { Err(PairingError::NotAvailable) };
    }
    if let Some(failure) = common_failure(status) {
        return Err(failure);
    }
    require(status, 204)
}

/// B's side: a key that came through the hub and is not trusted yet. Nothing is written until `confirm`.
pub struct Received {
    key: [u8; 32],
}

impl Received {
    /// Adopts the key on `engine` if `typed_fingerprint` (what the person typed after reading it on A) is this key's
    /// `key_fingerprint`; otherwise `FingerprintMismatch` and nothing is written.
    pub fn confirm(self, typed_fingerprint: &str, engine: &SyncEngine) -> Result<(), PairingError> {
        if normalized(typed_fingerprint) != normalized(&key_fingerprint(&self.key)) {
            return Err(PairingError::FingerprintMismatch);
        }
        engine.adopt_vault_key(self.key).map_err(|e| PairingError::Adopt(e.to_string()))
    }
}

impl std::fmt::Debug for Received {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Received(<redacted>)")
    }
}

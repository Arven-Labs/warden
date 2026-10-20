//! Tauri commands backing the "Workspace" screen (Fase 9.6) — lists/approves/revokes devices
//! paired with a `warden-server` hub (Fase 9.3, `warden_server::PairingStore`). No `AppState`
//! needed: `PairingStore` is stateless by design (just a `PathBuf`, rereads the JSON file on every
//! call — see `device_registry.rs`'s module docs), so a fresh one is built per command.
//!
//! **Scope decision**: this assumes the desktop app runs on the *same machine* as the
//! `warden-server` hub whose registry it's reading — it opens `devices.json` straight off disk via
//! `default_server_devices_path()`, the exact path `warden-server devices list/approve/revoke`
//! (the CLI) already reads/writes. A hub running on a different machine needs a real admin
//! surface over the WS protocol, which doesn't exist yet (no `ClientMessage`/`ServerMessage`
//! variant for it, and no answer yet to "which credential is allowed to approve a device" beyond
//! the one shared `auth_key`) — deliberately out of scope for this slice.

use serde::{Deserialize, Serialize};
use warden_bootstrap::HubPairingConfig;
use warden_server::{DiscoveredHub, PairedDevice, PairingStore};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDeviceInfo {
    device_id: String,
    device_name: String,
    status: String,
    first_seen_ms: i64,
    last_seen_ms: i64,
}

fn to_info(device_id: String, device: PairedDevice) -> PairedDeviceInfo {
    PairedDeviceInfo {
        device_id,
        device_name: device.device_name,
        status: device.status.to_string(),
        first_seen_ms: device.first_seen_ms,
        last_seen_ms: device.last_seen_ms,
    }
}

fn store() -> Result<PairingStore, String> {
    warden_bootstrap::default_server_devices_path()
        .map(PairingStore::new)
        .ok_or_else(|| "could not determine the OS config directory".to_string())
}

#[tauri::command]
pub fn list_paired_devices() -> Result<Vec<PairedDeviceInfo>, String> {
    Ok(store()?.list().map_err(|e| format!("{e:#}"))?.into_iter().map(|(id, device)| to_info(id, device)).collect())
}

#[tauri::command]
pub fn approve_paired_device(device_id: String) -> Result<(), String> {
    store()?.approve(&device_id).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn revoke_paired_device(device_id: String) -> Result<(), String> {
    store()?.revoke(&device_id).map_err(|e| format!("{e:#}"))
}

/// An invite waiting to be used (P36) — never its code, which the hub keeps only as a hash.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInviteInfo {
    name: String,
    expires_ms: i64,
}

/// A freshly made invite, with the code shown once.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDeviceInvite {
    name: String,
    code: String,
    expires_ms: i64,
}

#[tauri::command]
pub fn list_device_invites() -> Result<Vec<DeviceInviteInfo>, String> {
    Ok(store()?.list_invites().map_err(|e| format!("{e:#}"))?.into_iter().map(|i| DeviceInviteInfo { name: i.name, expires_ms: i.expires_ms }).collect())
}

/// Same limits as the hub's `CreateDeviceInvite`, so the two screens agree.
fn make_invite(store: &PairingStore, name: &str, ttl_minutes: u64) -> Result<NewDeviceInvite, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("an invite needs a name, so the device list can say who it was for".to_string());
    }
    if !(1..=warden_server::devices::MAX_INVITE_MINUTES).contains(&ttl_minutes) {
        return Err(format!("an invite works for 1 to {} minutes", warden_server::devices::MAX_INVITE_MINUTES));
    }
    let ttl = std::time::Duration::from_secs(ttl_minutes * 60);
    let code = store.create_invite(name, ttl).map_err(|e| format!("{e:#}"))?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
    Ok(NewDeviceInvite { name: name.to_string(), code, expires_ms: now + ttl.as_millis() as i64 })
}

#[tauri::command]
pub fn create_device_invite(name: String, ttl_minutes: u64) -> Result<NewDeviceInvite, String> {
    make_invite(&store()?, &name, ttl_minutes)
}

#[tauri::command]
pub fn cancel_device_invite(name: String) -> Result<bool, String> {
    store()?.cancel_invite(&name).map_err(|e| format!("{e:#}"))
}

/// Changes the pairing key of the hub on this machine (P36): the embedded one and a `warden-server serve` both read the
/// `pairing_key` file beside the registry, so a running one takes it on its next use. Returns the new key, shown once.
#[tauri::command]
pub fn rotate_pairing_key() -> Result<String, String> {
    let devices_path = warden_bootstrap::default_server_devices_path().ok_or_else(|| "could not determine the OS config directory".to_string())?;
    warden_server::pairing_key::rotate_at(&warden_server::pairing_key::pairing_key_path_beside(&devices_path)).map_err(|e| format!("{e:#}"))
}

/// What the Workspace screen keeps of the QR section: only the hub's address (P36 — the secret in the QR is an invite made
/// when the QR is drawn, so nothing secret is saved).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubPairingConfigPayload {
    server_url: String,
}

impl From<HubPairingConfig> for HubPairingConfigPayload {
    fn from(c: HubPairingConfig) -> Self {
        HubPairingConfigPayload { server_url: c.server_url }
    }
}

/// The JSON drawn in the pairing QR: the shape `mobile/lib/services/hub_pairing_qr.dart::parseHubPairingQr` reads. `auth_key`
/// is the code of a one-use invite (the hub takes any code it does not know as the key for an invite), not the pairing key.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingQrPayload<'a> {
    server_url: &'a str,
    auth_key: &'a str,
}

/// A drawn pairing QR and when the invite inside it stops working.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubPairingQr {
    svg: String,
    expires_ms: i64,
}

/// How long the invite in a QR works: the time to pick up the phone and scan it.
const QR_INVITE_MINUTES: u64 = 10;
/// The invite's name. A fixed one means drawing a new QR replaces the last one instead of leaving invites behind.
const QR_INVITE_NAME: &str = "QR pairing";

fn hub_pairing_config_path() -> Result<std::path::PathBuf, String> {
    warden_bootstrap::default_hub_pairing_config_path().ok_or_else(|| "could not determine the OS config directory".to_string())
}

/// What the Workspace screen's "Pareamento por QR" section pre-fills its Server URL/Auth key
/// fields from (Fase 9.7) — `None` until the operator has saved it at least once.
#[tauri::command]
pub fn get_hub_pairing_config() -> Result<Option<HubPairingConfigPayload>, String> {
    let path = hub_pairing_config_path()?;
    warden_bootstrap::load_hub_pairing_config(&path).map(|opt| opt.map(Into::into)).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn save_hub_pairing_config(server_url: String) -> Result<(), String> {
    let server_url = server_url.trim();
    if server_url.is_empty() {
        return Err("The Server URL is required".to_string());
    }
    let path = hub_pairing_config_path()?;
    let config = HubPairingConfig { server_url: server_url.to_string() };
    warden_bootstrap::save_hub_pairing_config(&path, &config).map_err(|e| format!("{e:#}"))
}

/// Draws the pairing QR (Fase 9.7) from the address saved with `save_hub_pairing_config` — a new client (today, the mobile app's
/// `ConnectionScreen`) scans this to skip typing the address and the key by hand. The key in it is a one-use invite made now, on
/// this machine's registry (P36: the hub's pairing key pairs only its first device), valid for `QR_INVITE_MINUTES`. `device_id`/
/// `device_name` stay client-chosen, and approving the device for `CallDeviceTool` routing (Fase 9.3/9.6) still happens separately.
#[tauri::command]
pub fn hub_pairing_qr_svg() -> Result<HubPairingQr, String> {
    let path = hub_pairing_config_path()?;
    let config = warden_bootstrap::load_hub_pairing_config(&path)
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(|| "Preencha o Server URL antes de gerar o QR".to_string())?;
    draw_pairing_qr(&store()?, &config.server_url)
}

/// `hub_pairing_qr_svg` without the files: makes the invite in `store` and draws it with `server_url`.
fn draw_pairing_qr(store: &PairingStore, server_url: &str) -> Result<HubPairingQr, String> {
    let invite = make_invite(store, QR_INVITE_NAME, QR_INVITE_MINUTES)?;
    let json = serde_json::to_string(&PairingQrPayload { server_url, auth_key: &invite.code }).map_err(|e| format!("failed to serialize pairing payload: {e}"))?;
    Ok(HubPairingQr { svg: crate::qr::render_qr_svg(&json)?, expires_ms: invite.expires_ms })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredHubPayload {
    host: String,
    port: u16,
    server_name: String,
    /// The `wss://` URL to pair with instead of `ws://host:port` — TLS-only hubs (P36).
    secure_url: Option<String>,
}

impl From<DiscoveredHub> for DiscoveredHubPayload {
    fn from(hub: DiscoveredHub) -> Self {
        DiscoveredHubPayload { host: hub.host.to_string(), port: hub.port, server_name: hub.server_name, secure_url: hub.secure_url }
    }
}

/// Sweeps the local network for `warden-server` hubs (Fase 9.1, redefined — see
/// `warden_server_protocol::discovery`) and lets the "Pareamento por QR" section's "Server URL"
/// field be filled by picking one instead of typing an IP by hand. Never reveals or needs the
/// auth key — that stays manual, same security boundary the discovery probe itself keeps. `port`
/// comes from the frontend's own input (defaulted to 7420 there) so a hub started with
/// `--listen` on a non-default port is still discoverable.
#[tauri::command]
pub async fn discover_hubs(port: u16) -> Result<Vec<DiscoveredHubPayload>, String> {
    warden_server::discover_hubs(port)
        .await
        .map(|hubs| hubs.into_iter().map(Into::into).collect())
        .map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use warden_server::PairingStatus;

    #[test]
    fn an_invite_from_the_desktop_follows_the_hubs_limits_and_pairs_a_device() {
        let path = std::env::temp_dir().join(format!("warden-desktop-invite-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = PairingStore::new(&path);

        assert!(make_invite(&store, "  ", 15).is_err(), "no name");
        assert!(make_invite(&store, "x", 0).is_err(), "no time");
        assert!(make_invite(&store, "x", warden_server::devices::MAX_INVITE_MINUTES + 1).is_err(), "too long");
        assert!(store.list_invites().unwrap().is_empty(), "a refused one leaves nothing");

        let made = make_invite(&store, " Ana's phone ", 15).unwrap();
        assert_eq!(made.name, "Ana's phone");
        assert_eq!(serde_json::to_value(&made).unwrap()["expiresMs"], made.expires_ms);
        assert_eq!(store.list_invites().unwrap().len(), 1);
        store.authenticate_as("phone", "Phone", None, warden_server::PairingProof::Invite(made.code)).unwrap().expect("the code pairs");
        std::fs::remove_file(path).unwrap();
    }

    // Locks in the exact camelCase JSON shape `desktop/src/types.ts`'s `PairedDevice` expects —
    // a mismatch here (e.g. someone reverting the `rename_all`) would silently break the frontend
    // without any Rust-side error, since these command results aren't otherwise typechecked
    // against the TS side.
    #[test]
    fn paired_device_info_serializes_as_camel_case() {
        let device = PairedDevice { device_name: "Desktop A".to_string(), status: PairingStatus::Approved, first_seen_ms: 1, last_seen_ms: 2, token_hash: None, user: None, invite_name: None };
        let info = to_info("dev-a".to_string(), device);
        assert_eq!(
            serde_json::to_string(&info).unwrap(),
            r#"{"deviceId":"dev-a","deviceName":"Desktop A","status":"approved","firstSeenMs":1,"lastSeenMs":2}"#
        );
    }

    // Locks in the exact camelCase JSON shape embedded in the pairing QR (Fase 9.7) — this is
    // what `mobile/lib/services/hub_pairing_qr.dart::parseHubPairingQr` decodes on the other end,
    // so the two sides must agree on field names without either one importing the other's types.
    #[test]
    fn the_pairing_qr_payload_serializes_as_camel_case() {
        let payload = PairingQrPayload { server_url: "ws://192.168.1.10:7420", auth_key: "secret" };
        assert_eq!(serde_json::to_string(&payload).unwrap(), r#"{"serverUrl":"ws://192.168.1.10:7420","authKey":"secret"}"#);
    }

    // What the Workspace screen keeps and reads back is the address alone: no secret sits in the config file (P36).
    #[test]
    fn the_saved_qr_config_holds_the_address_and_nothing_secret() {
        let payload = HubPairingConfigPayload::from(HubPairingConfig { server_url: "ws://192.168.1.10:7420".to_string() });
        assert_eq!(serde_json::to_string(&payload).unwrap(), r#"{"serverUrl":"ws://192.168.1.10:7420"}"#);
    }

    // P36: the QR carries a one-use invite made when it is drawn, short-lived, and a new QR replaces the last one's invite.
    #[test]
    fn drawing_the_qr_makes_one_short_invite_and_a_second_qr_replaces_it() {
        let path = std::env::temp_dir().join(format!("warden-desktop-qr-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = PairingStore::new(&path);

        let first = draw_pairing_qr(&store, "ws://192.168.1.10:7420").unwrap();
        assert!(first.svg.contains("<svg"));
        let invites = store.list_invites().unwrap();
        assert_eq!(invites.len(), 1);
        assert_eq!(invites[0].name, QR_INVITE_NAME);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        assert!(first.expires_ms > now && first.expires_ms <= now + (QR_INVITE_MINUTES as i64) * 60_000, "valid for the next few minutes only");
        assert_eq!(serde_json::to_value(&first).unwrap()["expiresMs"], first.expires_ms);

        draw_pairing_qr(&store, "ws://192.168.1.10:7420").unwrap();
        assert_eq!(store.list_invites().unwrap().len(), 1, "the new QR took the place of the old one");
        std::fs::remove_file(path).unwrap();
    }

    // Locks in the exact camelCase JSON shape `desktop/src/types.ts`'s `DiscoveredHub` expects.
    #[test]
    fn discovered_hub_payload_serializes_as_camel_case() {
        let payload = DiscoveredHubPayload::from(DiscoveredHub {
            host: "192.168.1.10".parse().unwrap(),
            port: 7420,
            server_name: "Fabio's Desktop".to_string(),
            secure_url: Some("wss://desktop.tail1234.ts.net:7420".to_string()),
        });
        assert_eq!(
            serde_json::to_string(&payload).unwrap(),
            r#"{"host":"192.168.1.10","port":7420,"serverName":"Fabio's Desktop","secureUrl":"wss://desktop.tail1234.ts.net:7420"}"#
        );
    }
}

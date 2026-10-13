//! The pairing registry from a client (Sessão 103): the same list/approve/revoke as
//! `warden-server devices`, so a hub with no screen can be managed from the web. Listing is open
//! to any paired device, like reading settings; approving or revoking asks for the pairing key
//! again, with the same 1 s wait and the same per-hub lock as a settings save, so both share one
//! guessing rate.

use warden_server_protocol::protocol::{DeviceAction, DeviceDto, DeviceStatusDto};
use warden_server_protocol::ServerMessage;

use crate::device_registry::{PairingStatus, PairingStore};
use crate::settings::{keys_match, WRONG_KEY_DELAY};

/// Answers `ListDevices`. `you` is the asking connection's device id.
pub fn handle_list_devices(store: &PairingStore, you: &str, request_id: u64) -> ServerMessage {
    match store.list() {
        Ok(devices) => ServerMessage::DeviceList {
            request_id,
            devices: devices
                .into_iter()
                .map(|(device_id, device)| DeviceDto {
                    device_id,
                    device_name: device.device_name,
                    status: match device.status {
                        PairingStatus::Pending => DeviceStatusDto::Pending,
                        PairingStatus::Approved => DeviceStatusDto::Approved,
                        PairingStatus::Revoked => DeviceStatusDto::Revoked,
                    },
                    first_seen_ms: device.first_seen_ms,
                    last_seen_ms: device.last_seen_ms,
                    user: device.user,
                })
                .collect(),
            you: you.to_string(),
        },
        Err(err) => device_error(request_id, format!("failed to read the device registry: {err:#}"), false),
    }
}

/// Answers `SetDeviceStatus` with the updated list. A revoked device's open connection is closed
/// by the watcher `server.rs` already runs, within a few seconds — this one's too, if it revokes
/// itself.
#[allow(clippy::too_many_arguments)]
pub async fn handle_set_device_status(
    store: &PairingStore,
    lock: &tokio::sync::Mutex<()>,
    auth_key: &str,
    you: &str,
    request_id: u64,
    pairing_key: &str,
    device_id: &str,
    action: DeviceAction,
) -> ServerMessage {
    let _serialized = lock.lock().await;
    if !keys_match(pairing_key, auth_key) {
        tokio::time::sleep(WRONG_KEY_DELAY).await;
        return device_error(request_id, "wrong pairing key".to_string(), true);
    }
    let result = match action {
        DeviceAction::Approve => store.approve(device_id),
        DeviceAction::Revoke => store.revoke(device_id),
    };
    match result {
        Ok(()) => handle_list_devices(store, you, request_id),
        Err(err) => device_error(request_id, format!("{err:#}"), false),
    }
}

/// The longest an invite may wait to be used (P36): a day. A code left around for longer is a door left open.
pub const MAX_INVITE_MINUTES: u64 = 24 * 60;

/// Answers `CreateDeviceInvite` (P36) with the code, shown once. Asks for the pairing key like `handle_set_device_status`
/// (same lock, same wrong-key wait), and an invite never opens the administration itself.
pub async fn handle_create_device_invite(
    store: &PairingStore,
    lock: &tokio::sync::Mutex<()>,
    auth_key: &str,
    request_id: u64,
    pairing_key: &str,
    name: &str,
    ttl_minutes: u64,
) -> ServerMessage {
    let _serialized = lock.lock().await;
    if !keys_match(pairing_key, auth_key) {
        tokio::time::sleep(WRONG_KEY_DELAY).await;
        return device_error(request_id, "wrong pairing key".to_string(), true);
    }
    let name = name.trim();
    if name.is_empty() {
        return device_error(request_id, "an invite needs a name, so the device list can say who it was for".to_string(), false);
    }
    if !(1..=MAX_INVITE_MINUTES).contains(&ttl_minutes) {
        return device_error(request_id, format!("an invite works for 1 to {MAX_INVITE_MINUTES} minutes"), false);
    }
    let ttl = std::time::Duration::from_secs(ttl_minutes * 60);
    match store.create_invite(name, ttl) {
        Ok(code) => {
            let expires_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64 + ttl.as_millis() as i64;
            ServerMessage::DeviceInvite { request_id, name: name.to_string(), code, expires_ms }
        }
        Err(err) => device_error(request_id, format!("{err:#}"), false),
    }
}

/// Answers `RotatePairingKey` (P36) with the new key, shown once. The hub needs the key file to do it: one started without
/// (an embedded hub of a build that never had it) says so instead of pretending.
pub async fn handle_rotate_pairing_key(
    key_file: Option<&std::path::Path>,
    lock: &tokio::sync::Mutex<()>,
    auth_key: &str,
    request_id: u64,
    pairing_key: &str,
) -> ServerMessage {
    let _serialized = lock.lock().await;
    if !keys_match(pairing_key, auth_key) {
        tokio::time::sleep(WRONG_KEY_DELAY).await;
        return device_error(request_id, "wrong pairing key".to_string(), true);
    }
    let Some(path) = key_file else {
        return device_error(request_id, "this hub's pairing key cannot be changed while it runs".to_string(), false);
    };
    match crate::pairing_key::rotate_at(path) {
        Ok(new_key) => ServerMessage::PairingKeyRotated { request_id, new_key },
        Err(err) => device_error(request_id, format!("{err:#}"), false),
    }
}

fn device_error(request_id: u64, message: String, auth_rejected: bool) -> ServerMessage {
    ServerMessage::DeviceError { request_id, message, auth_rejected }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "pairing-key-0123456789-0123456789";

    fn store(name: &str) -> (PairingStore, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("warden-hub-devices-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = PairingStore::new(&path);
        store.authenticate("phone", "Phone", None, true).unwrap().unwrap();
        store.authenticate("web-1", "Browser", None, true).unwrap().unwrap();
        (store, path)
    }

    fn statuses(reply: &ServerMessage) -> Vec<(String, DeviceStatusDto)> {
        let ServerMessage::DeviceList { devices, you, .. } = reply else { panic!("{reply:?}") };
        assert_eq!(you, "web-1");
        devices.iter().map(|d| (d.device_id.clone(), d.status)).collect()
    }

    #[tokio::test]
    async fn listing_shows_every_device_and_never_a_token() {
        let (store, path) = store("list");
        let reply = handle_list_devices(&store, "web-1", 1);
        assert_eq!(statuses(&reply), vec![("phone".into(), DeviceStatusDto::Pending), ("web-1".into(), DeviceStatusDto::Pending)]);
        let json = serde_json::to_string(&reply).unwrap();
        assert!(!json.contains("token"), "{json}");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn approve_and_revoke_with_the_pairing_key() {
        let (store, path) = store("set");
        let lock = tokio::sync::Mutex::new(());
        let reply = handle_set_device_status(&store, &lock, KEY, "web-1", 2, KEY, "phone", DeviceAction::Approve).await;
        assert_eq!(statuses(&reply)[0], ("phone".into(), DeviceStatusDto::Approved));
        let reply = handle_set_device_status(&store, &lock, KEY, "web-1", 3, KEY, "phone", DeviceAction::Revoke).await;
        assert_eq!(statuses(&reply)[0], ("phone".into(), DeviceStatusDto::Revoked));
        assert_eq!(store.status("phone").unwrap(), Some(PairingStatus::Revoked));
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn a_wrong_key_or_an_unknown_device_changes_nothing() {
        let (store, path) = store("refused");
        let lock = tokio::sync::Mutex::new(());
        let started = std::time::Instant::now();
        let reply = handle_set_device_status(&store, &lock, KEY, "web-1", 4, "wrong", "phone", DeviceAction::Approve).await;
        assert!(matches!(reply, ServerMessage::DeviceError { auth_rejected: true, .. }), "{reply:?}");
        assert!(started.elapsed() >= WRONG_KEY_DELAY);
        assert_eq!(store.status("phone").unwrap(), Some(PairingStatus::Pending));

        let reply = handle_set_device_status(&store, &lock, KEY, "web-1", 5, KEY, "ghost", DeviceAction::Approve).await;
        assert!(matches!(reply, ServerMessage::DeviceError { auth_rejected: false, .. }), "{reply:?}");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn an_invite_is_made_with_the_pairing_key_and_a_sane_name_and_time() {
        let (store, path) = store("invite");
        let lock = tokio::sync::Mutex::new(());

        let reply = handle_create_device_invite(&store, &lock, KEY, 6, KEY, "  Ana's phone ", 15).await;
        let ServerMessage::DeviceInvite { name, code, expires_ms, .. } = reply else { panic!("{reply:?}") };
        assert_eq!(name, "Ana's phone");
        assert!(expires_ms > 0);
        assert!(store.authenticate_as("new", "New", None, crate::device_registry::PairingProof::Invite(code)).unwrap().is_ok());

        for (key, name, ttl) in [(KEY, "  ", 15), (KEY, "x", 0), (KEY, "x", MAX_INVITE_MINUTES + 1)] {
            let reply = handle_create_device_invite(&store, &lock, KEY, 7, key, name, ttl).await;
            assert!(matches!(reply, ServerMessage::DeviceError { auth_rejected: false, .. }), "{name:?} {ttl}: {reply:?}");
        }
        assert!(store.list_invites().unwrap().is_empty(), "a refused request left no invite");

        let started = std::time::Instant::now();
        let reply = handle_create_device_invite(&store, &lock, KEY, 8, "wrong", "x", 15).await;
        assert!(matches!(reply, ServerMessage::DeviceError { auth_rejected: true, .. }), "{reply:?}");
        assert!(started.elapsed() >= WRONG_KEY_DELAY);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn rotating_needs_the_key_and_a_key_file() {
        let lock = tokio::sync::Mutex::new(());
        let file = std::env::temp_dir().join(format!("warden-hub-rotate-{}", std::process::id()));
        let _ = std::fs::remove_file(&file);

        let reply = handle_rotate_pairing_key(Some(&file), &lock, KEY, 9, "wrong").await;
        assert!(matches!(reply, ServerMessage::DeviceError { auth_rejected: true, .. }), "{reply:?}");
        assert!(!file.exists(), "a wrong key changes nothing");

        let reply = handle_rotate_pairing_key(None, &lock, KEY, 10, KEY).await;
        assert!(matches!(reply, ServerMessage::DeviceError { auth_rejected: false, .. }), "{reply:?}");

        let reply = handle_rotate_pairing_key(Some(&file), &lock, KEY, 11, KEY).await;
        let ServerMessage::PairingKeyRotated { new_key, .. } = reply else { panic!("{reply:?}") };
        assert_eq!(std::fs::read_to_string(&file).unwrap().trim(), new_key);
        std::fs::remove_file(file).unwrap();
    }
}

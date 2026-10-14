//! P36: a one-use invite pairs a device without the pairing key (`warden-server devices invite`), against a real hub.

mod support;

use std::time::Duration;

use support::{spin_up_server_with_pairing_key_file, MockProvider};
use warden_server::{ClientMessage, PairingStore, ServerConnection, ServerMessage};
use warden_server_protocol::protocol::DeviceAction;

const HOUR: Duration = Duration::from_secs(3600);

fn url(addr: std::net::SocketAddr) -> String {
    format!("ws://{addr}")
}

async fn pair(addr: std::net::SocketAddr, device_id: &str, key: &str) -> anyhow::Result<(ServerConnection, Option<String>)> {
    ServerConnection::handshake(&url(addr), device_id, device_id, key, None, Vec::new()).await
}

#[tokio::test]
async fn an_invite_pairs_one_device_and_only_once() {
    let (addr, devices, _) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let code = PairingStore::new(&devices).create_invite("Ana's phone", HOUR).unwrap();

    let (_conn, token) = pair(addr, "phone", &code).await.expect("the invite pairs");
    assert!(token.is_some(), "the device is given its own token");

    let err = pair(addr, "tablet", &code).await.err().expect("used up").to_string();
    assert!(err.contains("authentication rejected"), "{err}");
}

#[tokio::test]
async fn a_revoked_device_cannot_come_back_under_a_new_id_without_a_new_invite() {
    let (addr, devices, _) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let registry = PairingStore::new(&devices);
    let code = registry.create_invite("phone", HOUR).unwrap();
    let (first, _) = pair(addr, "phone", &code).await.unwrap();
    drop(first);
    registry.revoke("phone").unwrap();

    // It knows nothing but its used-up code: there is no key to pair again with, and no rotation was needed.
    let err = pair(addr, "phone-again", &code).await.err().expect("the code is spent").to_string();
    assert!(err.contains("authentication rejected"), "{err}");

    let fresh = registry.create_invite("phone (again)", HOUR).unwrap();
    pair(addr, "phone-again", &fresh).await.expect("the person can let it back in on purpose");
}

#[tokio::test]
async fn an_expired_invite_pairs_nothing() {
    let (addr, devices, _) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let code = PairingStore::new(&devices).create_invite("late", Duration::ZERO).unwrap();

    assert!(pair(addr, "phone", &code).await.is_err());
}

#[tokio::test]
async fn an_invite_does_not_open_the_administration() {
    let (addr, devices, _) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let registry = PairingStore::new(&devices);
    pair(addr, "dev-a", "test-key").await.unwrap();
    let code = registry.create_invite("guest", HOUR).unwrap();
    let (mut guest, _) = pair(addr, "guest", &code).await.unwrap();

    guest.send(&ClientMessage::SetDeviceStatus { request_id: 1, pairing_key: code, device_id: "dev-a".into(), action: DeviceAction::Approve }).await.unwrap();

    loop {
        match tokio::time::timeout(Duration::from_secs(10), guest.recv()).await.expect("no reply").unwrap().expect("connection closed") {
            ServerMessage::ConversationsChanged { .. } | ServerMessage::Pong { .. } => continue,
            ServerMessage::DeviceError { auth_rejected, .. } => {
                assert!(auth_rejected);
                break;
            }
            other => panic!("the administration opened for an invite: {other:?}"),
        }
    }
}

async fn reply(conn: &mut ServerConnection) -> ServerMessage {
    loop {
        match tokio::time::timeout(Duration::from_secs(10), conn.recv()).await.expect("no reply").unwrap().expect("connection closed") {
            ServerMessage::ConversationsChanged { .. } | ServerMessage::Pong { .. } => continue,
            msg => return msg,
        }
    }
}

/// The Devices screen's way: the invite is made over the protocol by a paired device that knows the pairing key.
#[tokio::test]
async fn an_invite_made_over_the_protocol_pairs_a_device() {
    let (addr, _, _) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let (mut admin, _) = pair(addr, "admin", "test-key").await.unwrap();

    admin.send(&ClientMessage::CreateDeviceInvite { request_id: 1, pairing_key: "test-key".into(), name: "Ana's phone".into(), ttl_minutes: 15 }).await.unwrap();
    let ServerMessage::DeviceInvite { name, code, .. } = reply(&mut admin).await else { panic!("no invite") };
    assert_eq!(name, "Ana's phone");

    pair(addr, "phone", &code).await.expect("the code from the screen pairs");
    assert!(pair(addr, "tablet", &code).await.is_err(), "once");

    admin.send(&ClientMessage::CreateDeviceInvite { request_id: 2, pairing_key: "wrong".into(), name: "x".into(), ttl_minutes: 15 }).await.unwrap();
    assert!(matches!(reply(&mut admin).await, ServerMessage::DeviceError { auth_rejected: true, .. }));
}

/// The invites that wait, from any client: made over the protocol, listed without a code, cancelled with the key, and a cancelled
/// code pairs no one.
#[tokio::test]
async fn a_waiting_invite_is_listed_and_a_cancelled_one_pairs_no_one() {
    let (addr, _, _) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let (mut admin, _) = pair(addr, "admin", "test-key").await.unwrap();

    admin.send(&ClientMessage::CreateDeviceInvite { request_id: 1, pairing_key: "test-key".into(), name: "Ana's phone".into(), ttl_minutes: 15 }).await.unwrap();
    let ServerMessage::DeviceInvite { code, .. } = reply(&mut admin).await else { panic!("no invite") };

    admin.send(&ClientMessage::ListDeviceInvites { request_id: 2 }).await.unwrap();
    let ServerMessage::DeviceInviteList { invites, .. } = reply(&mut admin).await else { panic!("no list") };
    assert_eq!(invites.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Ana's phone"]);

    admin.send(&ClientMessage::CancelDeviceInvite { request_id: 3, pairing_key: "wrong".into(), name: "Ana's phone".into() }).await.unwrap();
    assert!(matches!(reply(&mut admin).await, ServerMessage::DeviceError { auth_rejected: true, .. }));

    admin.send(&ClientMessage::CancelDeviceInvite { request_id: 4, pairing_key: "test-key".into(), name: "Ana's phone".into() }).await.unwrap();
    let ServerMessage::DeviceInviteList { invites, .. } = reply(&mut admin).await else { panic!("no list") };
    assert!(invites.is_empty());

    assert!(pair(addr, "phone", &code).await.is_err(), "a cancelled invite pairs no one");
}

/// And the key: changed over the protocol, it counts for the very next `Hello`, and the one that changed it is held to the new one.
#[tokio::test]
async fn the_pairing_key_rotated_over_the_protocol_counts_at_once() {
    let (addr, _, key_file) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let (mut admin, _) = pair(addr, "admin", "test-key").await.unwrap();

    admin.send(&ClientMessage::RotatePairingKey { request_id: 1, pairing_key: "wrong".into() }).await.unwrap();
    assert!(matches!(reply(&mut admin).await, ServerMessage::DeviceError { auth_rejected: true, .. }));
    assert!(!key_file.exists(), "a wrong key changes nothing");

    admin.send(&ClientMessage::RotatePairingKey { request_id: 2, pairing_key: "test-key".into() }).await.unwrap();
    let ServerMessage::PairingKeyRotated { new_key, .. } = reply(&mut admin).await else { panic!("not rotated") };

    assert!(pair(addr, "dev-b", "test-key").await.is_err(), "the old key pairs no one");
    admin.send(&ClientMessage::CreateDeviceInvite { request_id: 4, pairing_key: new_key.clone(), name: "x".into(), ttl_minutes: 15 }).await.unwrap();
    assert!(matches!(reply(&mut admin).await, ServerMessage::DeviceInvite { .. }), "the new one opens the administration");
    admin.send(&ClientMessage::RotatePairingKey { request_id: 3, pairing_key: "test-key".into() }).await.unwrap();
    assert!(matches!(reply(&mut admin).await, ServerMessage::DeviceError { auth_rejected: true, .. }), "the old key no longer opens the administration");
}

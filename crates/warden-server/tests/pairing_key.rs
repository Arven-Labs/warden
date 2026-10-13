//! P36 (c): the pairing key changes while the hub runs (`warden-server devices rotate-key`), against a real hub.

mod support;

use std::time::Duration;

use support::{spin_up_server_with_pairing_key_file, MockProvider};
use warden_server::pairing_key::rotate_at;
use warden_server::{ClientMessage, PairingStatus, PairingStore, ServerConnection, ServerMessage};
use warden_server_protocol::protocol::DeviceAction;

fn url(addr: std::net::SocketAddr) -> String {
    format!("ws://{addr}")
}

/// Pairs `device_id` with `key` and hands back the connection and the token the hub issued.
async fn pair(addr: std::net::SocketAddr, device_id: &str, key: &str) -> anyhow::Result<(ServerConnection, String)> {
    let (conn, token) = ServerConnection::handshake(&url(addr), device_id, device_id, key, None, Vec::new()).await?;
    Ok((conn, token.expect("a first pairing is given a token")))
}

async fn reply(conn: &mut ServerConnection) -> ServerMessage {
    loop {
        match tokio::time::timeout(Duration::from_secs(10), conn.recv()).await.expect("no reply").unwrap().expect("connection closed") {
            ServerMessage::ConversationsChanged { .. } | ServerMessage::Pong { .. } => continue,
            msg => return msg,
        }
    }
}

#[tokio::test]
async fn after_a_rotation_the_old_key_pairs_no_one_and_the_new_one_does() {
    let (addr, _, key_file) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let new_key = rotate_at(&key_file).unwrap();

    let err = pair(addr, "dev-a", "test-key").await.err().expect("the old key is turned away").to_string();
    assert!(err.contains("authentication rejected"), "{err}");
    pair(addr, "dev-a", &new_key).await.expect("the new key pairs the hub's first device, with no restart");
}

/// P36: the key is only for the hub's first device; everyone after it needs an invite, however the key was never changed.
#[tokio::test]
async fn the_pairing_key_pairs_only_the_first_device() {
    let (addr, devices, _) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    pair(addr, "dev-a", "test-key").await.expect("the first device pairs with the key");

    let err = pair(addr, "dev-b", "test-key").await.err().expect("the second one does not").to_string();
    assert!(err.contains("authentication rejected"), "{err}");

    let code = PairingStore::new(&devices).create_invite("dev-b", Duration::from_secs(60)).unwrap();
    pair(addr, "dev-b", &code).await.expect("an invite does");
}

#[tokio::test]
async fn a_device_that_already_holds_a_token_keeps_working_through_a_rotation() {
    let (addr, devices, key_file) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let (first, token) = pair(addr, "dev-a", "test-key").await.unwrap();
    drop(first);
    PairingStore::new(&devices).approve("dev-a").unwrap();

    rotate_at(&key_file).unwrap();

    // It comes back with its token alone, no key at all, and keeps its approval.
    let (mut back, issued) = ServerConnection::handshake(&url(addr), "dev-a", "dev-a", "", Some(token), Vec::new()).await.expect("the token outlives the key");
    assert_eq!(issued, None, "no new token: the old one is still good");
    back.ping(7).await.unwrap();
    assert!(matches!(back.recv().await.unwrap(), Some(ServerMessage::Pong { nonce: 7 })));
    assert_eq!(PairingStore::new(&devices).status("dev-a").unwrap(), Some(PairingStatus::Approved));
}

#[tokio::test]
async fn a_revoked_device_cannot_come_back_under_a_new_id_with_the_key_it_knew() {
    let (addr, devices, key_file) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let (first, token) = pair(addr, "dev-a", "test-key").await.unwrap();
    drop(first);
    PairingStore::new(&devices).revoke("dev-a").unwrap();

    // Revoking the only device does not reopen the key for a new id…
    let err = pair(addr, "dev-a-again", "test-key").await.err().expect("the key pairs the first device only").to_string();
    assert!(err.contains("authentication rejected"), "{err}");

    rotate_at(&key_file).unwrap();

    // …and its own id stays revoked after a rotation too.
    let err = ServerConnection::handshake(&url(addr), "dev-a", "dev-a", "", Some(token), Vec::new()).await.err().expect("still revoked").to_string();
    assert!(err.contains("revoked"), "{err}");
}

#[tokio::test]
async fn an_open_connection_is_held_to_the_key_of_the_moment_for_the_administration() {
    let (addr, devices, key_file) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    let (mut owner, _) = pair(addr, "owner", "test-key").await.unwrap();
    let code = PairingStore::new(&devices).create_invite("dev-a", Duration::from_secs(60)).unwrap();
    pair(addr, "dev-a", &code).await.unwrap();
    let set = |key: &str, request_id: u64| ClientMessage::SetDeviceStatus { request_id, pairing_key: key.to_string(), device_id: "dev-a".into(), action: DeviceAction::Approve };

    owner.send(&set("test-key", 1)).await.unwrap();
    assert!(matches!(reply(&mut owner).await, ServerMessage::DeviceList { .. }), "the key it paired with opens the administration");

    let new_key = rotate_at(&key_file).unwrap();

    // The same connection, never closed: the old key stops working the very next time it is sent…
    owner.send(&set("test-key", 2)).await.unwrap();
    assert!(matches!(reply(&mut owner).await, ServerMessage::DeviceError { auth_rejected: true, .. }));
    // …and the new one opens it.
    owner.send(&set(&new_key, 3)).await.unwrap();
    assert!(matches!(reply(&mut owner).await, ServerMessage::DeviceList { .. }));
    assert_eq!(PairingStore::new(&devices).status("dev-a").unwrap(), Some(PairingStatus::Approved));
}

#[tokio::test]
async fn deleting_the_file_brings_the_start_up_key_back() {
    let (addr, _, key_file) = spin_up_server_with_pairing_key_file(MockProvider::replying("unused")).await;
    rotate_at(&key_file).unwrap();
    assert!(pair(addr, "dev-a", "test-key").await.is_err());

    std::fs::remove_file(&key_file).unwrap();

    pair(addr, "dev-a", "test-key").await.expect("with no file the key the hub was started with counts again");
}

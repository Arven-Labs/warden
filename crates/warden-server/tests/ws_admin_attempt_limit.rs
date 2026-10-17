//! P142: the owner's administration over the WebSocket asks for the pairing key on every message, so each one is a chance
//! to guess it. The wrong ones are counted per address, drawing on the same count as the sign-in and the HTTP endpoints,
//! and an address that used its attempts up is turned away whatever key it sends.

mod support;

use std::time::Duration;

use support::{spin_up_server_with_attempt_limit, MockProvider};
use warden_server::attempt_limit::AttemptLimit;
use warden_server::{ClientMessage, ServerConnection, ServerMessage};
use warden_server_protocol::protocol::DeviceAction;

fn small_limit() -> AttemptLimit {
    AttemptLimit { max_attempts: 3, window: Duration::from_secs(60), lockout: Duration::from_secs(60) }
}

fn approve(request_id: u64, key: &str) -> ClientMessage {
    ClientMessage::SetDeviceStatus { request_id, pairing_key: key.to_string(), device_id: "owner".into(), action: DeviceAction::Approve }
}

async fn reply(conn: &mut ServerConnection) -> ServerMessage {
    loop {
        match tokio::time::timeout(Duration::from_secs(15), conn.recv()).await.expect("no reply").unwrap().expect("connection closed") {
            ServerMessage::ConversationsChanged { .. } | ServerMessage::Pong { .. } => continue,
            msg => return msg,
        }
    }
}

#[tokio::test]
async fn the_administration_turns_an_address_away_after_too_many_wrong_keys_even_with_the_right_one() {
    let addr = spin_up_server_with_attempt_limit(MockProvider::replying("unused"), small_limit()).await;
    let mut owner = ServerConnection::connect(&format!("ws://{addr}"), "owner", "Owner", "test-key").await.unwrap();

    for id in 1..=3 {
        owner.send(&approve(id, "not-the-key")).await.unwrap();
        match reply(&mut owner).await {
            ServerMessage::DeviceError { auth_rejected: true, message, .. } => assert!(message.contains("wrong pairing key"), "{message}"),
            other => panic!("expected a wrong-key refusal, got {other:?}"),
        }
    }

    owner.send(&approve(4, "test-key")).await.unwrap();
    match reply(&mut owner).await {
        ServerMessage::DeviceError { auth_rejected: true, message, .. } => assert!(message.contains("too many attempts"), "{message}"),
        other => panic!("expected the address to be turned away, got {other:?}"),
    }
}

#[tokio::test]
async fn the_administration_with_the_right_key_never_uses_the_attempts_up() {
    let addr = spin_up_server_with_attempt_limit(MockProvider::replying("unused"), small_limit()).await;
    let mut owner = ServerConnection::connect(&format!("ws://{addr}"), "owner", "Owner", "test-key").await.unwrap();

    for id in 1..=10 {
        owner.send(&approve(id, "test-key")).await.unwrap();
        assert!(matches!(reply(&mut owner).await, ServerMessage::DeviceList { .. }));
    }
}

#[tokio::test]
async fn a_burst_of_wrong_keys_is_capped_at_the_attempts_allowed() {
    let addr = spin_up_server_with_attempt_limit(MockProvider::replying("unused"), small_limit()).await;
    let mut owner = ServerConnection::connect(&format!("ws://{addr}"), "owner", "Owner", "test-key").await.unwrap();

    for id in 1..=10 {
        owner.send(&approve(id, "not-the-key")).await.unwrap();
    }
    let (mut checked, mut turned_away) = (0, 0);
    for _ in 0..10 {
        match reply(&mut owner).await {
            ServerMessage::DeviceError { message, .. } if message.contains("too many attempts") => turned_away += 1,
            ServerMessage::DeviceError { message, .. } if message.contains("wrong pairing key") => checked += 1,
            other => panic!("unexpected answer: {other:?}"),
        }
    }
    assert_eq!((checked, turned_away), (3, 7), "only the attempts allowed had their key checked");
}

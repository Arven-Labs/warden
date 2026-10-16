mod support;

use support::{spin_up_server_with_shutdown, MockProvider};

/// P143: the stop signal only ends the accept loop; a connection that was already open goes on being answered, which is
/// what the grace period before the process exits relies on.
#[tokio::test]
async fn a_connection_open_at_shutdown_is_still_answered_while_new_ones_are_refused() {
    let (addr, shutdown_tx) = spin_up_server_with_shutdown(MockProvider::replying("unused")).await;
    let mut conn = warden_server::ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key")
        .await
        .unwrap();

    shutdown_tx.send(()).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    conn.ping(7).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(warden_server::ServerMessage::Pong { nonce: 7 })));
    let refused = warden_server::ServerConnection::connect(&format!("ws://{addr}"), "dev-2", "Test Device", "test-key").await;
    assert!(refused.is_err(), "a new connection is refused once the signal came");
}

#[tokio::test]
async fn shutdown_stops_accepting_and_frees_the_port() {
    let (addr, shutdown_tx) = spin_up_server_with_shutdown(MockProvider::replying("unused")).await;

    // A connection made before shutdown still works normally — this isn't a hard kill.
    let mut conn = warden_server::ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key")
        .await
        .unwrap();
    conn.ping(1).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(warden_server::ServerMessage::Pong { nonce: 1 })));

    shutdown_tx.send(()).unwrap();

    // Give the accept loop a moment to observe the shutdown signal and drop the listener.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let result = warden_server::ServerConnection::connect(&format!("ws://{addr}"), "dev-2", "Test Device", "test-key").await;
    assert!(result.is_err(), "expected the port to be free (nothing listening) after shutdown");
}

use axum::extract::Path as AxumPath;
use axum::routing::{get, post};
use axum::Router;
use warden_sync::ArweaveClient;

async fn spawn_fake_gateway() -> String {
    let router = Router::new()
        .route(
            "/graphql",
            post(|body: axum::Json<serde_json::Value>| async move {
                let query = body["query"].as_str().unwrap_or_default();
                if query.contains("transactions(") {
                    axum::Json(serde_json::json!({
                        "data": { "transactions": { "edges": [ { "node": { "id": "real-tx-id" } } ] } }
                    }))
                } else {
                    axum::Json(serde_json::json!({
                        "data": { "transaction": { "owner": { "address": "real-owner-address" } } }
                    }))
                }
            }),
        )
        .route("/{tx_id}", get(|AxumPath(_tx_id): AxumPath<String>| async { b"encrypted-bytes".to_vec() }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn latest_tx_by_owner_parses_a_real_http_response() {
    let base_url = spawn_fake_gateway().await;
    let client = ArweaveClient::new(format!("{base_url}/graphql"), base_url);

    let tx_id = client.latest_tx_by_owner("some-owner").await.unwrap();
    assert_eq!(tx_id.as_deref(), Some("real-tx-id"));
}

#[tokio::test]
async fn owner_of_tx_parses_a_real_http_response() {
    let base_url = spawn_fake_gateway().await;
    let client = ArweaveClient::new(format!("{base_url}/graphql"), base_url);

    let owner = client.owner_of_tx("real-tx-id").await.unwrap();
    assert_eq!(owner.as_deref(), Some("real-owner-address"));
}

#[tokio::test]
async fn fetch_tx_data_returns_the_raw_bytes() {
    let base_url = spawn_fake_gateway().await;
    let client = ArweaveClient::new(format!("{base_url}/graphql"), base_url);

    let bytes = client.fetch_tx_data("real-tx-id").await.unwrap();
    assert_eq!(bytes, b"encrypted-bytes".to_vec());
}

/// A gateway that serves what the capped fetch has to be careful with: a body of the right size, one that is too big (with its
/// size announced and without), a missing id and a redirect to another site.
async fn spawn_gateway_of_odd_bodies() -> String {
    let router = Router::new()
        .route("/small-tx", get(|| async { b"hello".to_vec() }))
        .route("/big-tx", get(|| async { vec![b'x'; 5000] }))
        .route(
            "/unannounced-tx",
            get(|| async {
                // A body with no `Content-Length`, sent in pieces, as a server that does not say how much is coming would.
                let pieces = futures_util::stream::iter((0..50).map(|_| Ok::<_, std::convert::Infallible>(vec![b'y'; 100])));
                axum::body::Body::from_stream(pieces)
            }),
        )
        .route("/elsewhere-tx", get(|| async { axum::response::Redirect::temporary("http://example.invalid/stolen") }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn a_capped_fetch_returns_a_body_that_fits() {
    let base_url = spawn_gateway_of_odd_bodies().await;
    let client = ArweaveClient::new(format!("{base_url}/graphql"), base_url);
    assert_eq!(client.fetch_tx_data_capped("small-tx", 1024).await.unwrap(), b"hello".to_vec());
}

#[tokio::test]
async fn a_capped_fetch_refuses_a_body_over_the_cap_announced_or_not() {
    let base_url = spawn_gateway_of_odd_bodies().await;
    let client = ArweaveClient::new(format!("{base_url}/graphql"), base_url);
    for tx in ["big-tx", "unannounced-tx"] {
        let err = client.fetch_tx_data_capped(tx, 1024).await.unwrap_err().to_string();
        assert!(err.contains("bigger than the 1 KiB allowed"), "{tx}: {err}");
    }
}

#[tokio::test]
async fn a_capped_fetch_says_when_nothing_is_there_yet_and_follows_no_redirect_to_another_site() {
    let base_url = spawn_gateway_of_odd_bodies().await;
    let client = ArweaveClient::new(format!("{base_url}/graphql"), base_url);
    let missing = client.fetch_tx_data_capped("no-such-tx", 1024).await.unwrap_err().to_string();
    assert!(missing.contains("a few minutes"), "{missing}");

    let err = client.fetch_tx_data_capped("elsewhere-tx", 1024).await.unwrap_err();
    assert!(format!("{err:#}").contains("another site"), "{err:#}");
}

#[tokio::test]
async fn a_capped_fetch_takes_only_an_id() {
    let client = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
    for bad in ["", "../etc", "a/b", "a?b"] {
        assert!(client.fetch_tx_data_capped(bad, 1024).await.unwrap_err().to_string().contains("not an Arweave transaction id"), "{bad:?}");
    }
}

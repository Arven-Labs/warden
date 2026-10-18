//! Proves `publish::begin_publish`/`publish::run_publish_with_hosts` end to end against a scripted "phone", the same idiom as
//! `fake_phone_push.rs`: a tiny `axum` server with the two single-shot `pin()` endpoints. This one also decrypts what the
//! requester pushed and keeps it, so the test can check that what would be pinned is exactly the package, in plain text.

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::Path;
use axum::routing::{get, put};
use axum::Router;
use warden_core::skill::package::Package;
use warden_core::skill::Skill;
use warden_sync::publish;
use warden_truthid::crypto;

/// The requester sweeps every `LAN_PORTS`, so the fake phone can serve on whichever is free.
async fn bind_fake_phone() -> tokio::net::TcpListener {
    for port in warden_truthid::protocol::LAN_PORTS {
        if let Ok(listener) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            return listener;
        }
    }
    panic!("none of LAN_PORTS is free for the fake phone");
}

/// Accepts one `PUT .../content`, decrypts it with the phase-1 cipher and keeps the plaintext in `received`, then serves
/// `result_json` on `GET`, ECIES-encrypted against the pubkey from the requester's QR payload.
async fn spawn_fake_phone(requester_pub_hex: String, result_json: &'static str, received: Arc<Mutex<Option<Vec<u8>>>>) {
    let stored = received.clone();
    let push_route = put(move |Path(session_id): Path<String>, body: axum::body::Bytes| {
        let stored = stored.clone();
        async move {
            let key = crypto::derive_pin_content_key(&session_id).unwrap();
            *stored.lock().unwrap() = Some(crypto::decrypt_pin_content(&body, &key).unwrap());
            axum::http::StatusCode::OK
        }
    });

    let get_route = get(move |Path(_session_id): Path<String>| {
        let received = received.clone();
        let requester_pub_hex = requester_pub_hex.clone();
        async move {
            loop {
                if received.lock().unwrap().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let encrypted = crypto::ecies_encrypt(result_json.as_bytes(), &requester_pub_hex).unwrap();
            let blob_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &encrypted);
            axum::Json(serde_json::json!({ "blob": blob_b64 }))
        }
    });

    let router = Router::new().route("/session/{id}/content", push_route).route("/session/{id}", get_route);
    let listener = bind_fake_phone().await;
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
}

fn package_json() -> String {
    let skill = Skill {
        name: "greet".into(),
        description: "Says hello.".into(),
        body: "Say hello warmly.".into(),
        agents: Vec::new(),
        proposed: false,
        source: None,
        proposed_at: None,
        revises: None,
    };
    Package::export_skill(&skill, "1.2.0", Some("Ana"), "CC0-1.0").unwrap().to_json().unwrap()
}

async fn publish_with(result_json: &'static str) -> (anyhow::Result<publish::PublishOutcome>, Option<Vec<u8>>) {
    let begin = publish::begin_publish(&package_json()).unwrap();
    let qr: serde_json::Value = serde_json::from_str(&begin.pending.qr_payload_json().unwrap()).unwrap();
    assert_eq!(qr["action"], "truthid-pin");
    let requester_pub_hex = qr["ephemeralPubKey"].as_str().unwrap().to_string();

    let received = Arc::new(Mutex::new(None));
    spawn_fake_phone(requester_pub_hex, result_json, received.clone()).await;
    let outcome = publish::run_publish_with_hosts(begin, vec![Ipv4Addr::LOCALHOST]).await;
    let pushed = received.lock().unwrap().clone();
    (outcome, pushed)
}

#[tokio::test]
async fn publishing_pins_the_package_in_plain_text_and_gives_back_its_arweave_id() {
    let (outcome, pushed) = publish_with(r#"{"status":"pinned","cid":"ar://fake-skill-tx","contentHash":null,"providersOk":["arweave"],"providersFailed":[]}"#).await;

    let outcome = outcome.unwrap();
    assert_eq!((outcome.tx_id.as_str(), outcome.id.as_str(), outcome.version.as_str()), ("fake-skill-tx", "greet", "1.2.0"));

    // What reached the phone is the package itself, readable by anyone, and it still reads back clean.
    let pushed = String::from_utf8(pushed.expect("the phone received the content")).unwrap();
    assert_eq!(pushed, package_json());
    assert_eq!(Package::parse(&pushed).unwrap().hash, outcome.hash);
}

#[tokio::test]
async fn a_phone_that_turns_the_pin_down_is_an_error_with_its_reason() {
    let (outcome, _) = publish_with(r#"{"status":"rejected","error":"the person said no"}"#).await;
    assert_eq!(outcome.unwrap_err().to_string(), "the person said no");
}

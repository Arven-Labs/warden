//! P160 on the hub: the keys whose skill packages the owner trusts (`[[trusted_publishers]]`). `ListTrustedPublishers` and
//! `SaveTrustedPublishers` are the owner's (a pairing key for the save, like any change); a package preview then says `trusted` for a
//! signature by a listed key.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use warden_bootstrap::{load_config_from_path, save_config, FileConfig};
use warden_core::memory::Vault;
use warden_core::model::{ChatStream, Message, ModelProvider};
use warden_core::orchestrator::Orchestrator;
use warden_core::skill::package::Package;
use warden_core::skill::signing::generate_keypair;
use warden_core::skill::Skill;
use warden_core::tool::ToolSpec;
use warden_server::{ClientMessage, Server, ServerConnection, ServerMessage, SettingsHost};
use warden_server_protocol::protocol::TrustedPublisherDto;

const KEY: &str = "test-key";
const TEMP: &str = "provisional-1";

struct NoModel;

#[async_trait]
impl ModelProvider for NoModel {
    async fn chat_stream(&self, _messages: Vec<Message>, _tools: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
        anyhow::bail!("these tests never run a turn")
    }
}

struct Host {
    path: PathBuf,
    dir: PathBuf,
}

#[async_trait]
impl SettingsHost for Host {
    fn config_path(&self) -> PathBuf {
        self.path.clone()
    }

    async fn build(&self) -> anyhow::Result<Orchestrator> {
        Ok(Orchestrator::new(Arc::new(NoModel), Arc::new(Vault::new(self.dir.join("vault")))))
    }
}

/// A hub whose config has a member, Ana, and one unrelated setting that a save of the list must leave alone.
async fn spin_up() -> (String, PathBuf) {
    let dir = std::env::temp_dir().join(format!("warden-server-trusted-publishers-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let mut config = FileConfig { topup_url: Some("https://example.com/credits".into()), ..FileConfig::default() };
    warden_bootstrap::users::add_user(&mut config, "ana", "Ana", TEMP).unwrap();
    save_config(&path, &config).unwrap();
    let orchestrator = Orchestrator::new(Arc::new(NoModel), Arc::new(Vault::new(dir.join("vault"))));
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), KEY, "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_open_key_pairing()
        .with_settings(Arc::new(Host { path: path.clone(), dir: dir.clone() }))
        .with_users_dir(dir.join("users"));
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    (format!("ws://{addr}"), path)
}

async fn next(conn: &mut ServerConnection) -> ServerMessage {
    loop {
        match tokio::time::timeout(Duration::from_secs(10), conn.recv()).await.expect("no reply").unwrap().expect("connection closed") {
            ServerMessage::ConversationsChanged { .. } | ServerMessage::Pong { .. } | ServerMessage::OrgAccessChanged { .. } => continue,
            message => return message,
        }
    }
}

fn publisher(key: &str, name: &str) -> TrustedPublisherDto {
    TrustedPublisherDto { key: key.into(), name: name.into() }
}

async fn list(conn: &mut ServerConnection) -> ServerMessage {
    conn.send(&ClientMessage::ListTrustedPublishers { request_id: 1 }).await.unwrap();
    next(conn).await
}

async fn save(conn: &mut ServerConnection, key: &str, publishers: Vec<TrustedPublisherDto>) -> ServerMessage {
    conn.send(&ClientMessage::SaveTrustedPublishers { request_id: 2, pairing_key: key.into(), publishers }).await.unwrap();
    next(conn).await
}

fn listed(message: ServerMessage) -> Vec<TrustedPublisherDto> {
    match message {
        ServerMessage::TrustedPublishers { publishers, .. } => publishers,
        other => panic!("expected the list, got {other:?}"),
    }
}

#[tokio::test]
async fn the_owner_saves_the_list_and_reads_it_back_and_the_rest_of_the_config_stays() {
    let (url, path) = spin_up().await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    assert!(listed(list(&mut owner).await).is_empty());

    let saved = listed(save(&mut owner, KEY, vec![publisher(&"AB".repeat(32), "  Fabio "), publisher(&"cd".repeat(32), "")]).await);
    assert_eq!(saved, vec![publisher(&"ab".repeat(32), "Fabio"), publisher(&"cd".repeat(32), "")], "lowercase keys, trimmed names");
    assert_eq!(listed(list(&mut owner).await), saved);

    let config = load_config_from_path(&path, false).unwrap();
    assert_eq!(config.trusted_publishers.len(), 2);
    assert_eq!(config.topup_url.as_deref(), Some("https://example.com/credits"));
    assert!(config.users.iter().any(|u| u.id == "ana"), "the members are as they were");

    // Saving an empty list clears it.
    assert!(listed(save(&mut owner, KEY, vec![]).await).is_empty());
    assert!(load_config_from_path(&path, false).unwrap().trusted_publishers.is_empty());
}

#[tokio::test]
async fn a_wrong_pairing_key_or_a_bad_key_in_the_list_changes_nothing() {
    let (url, path) = spin_up().await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    let good = listed(save(&mut owner, KEY, vec![publisher(&"ab".repeat(32), "Fabio")]).await);

    match save(&mut owner, "not-the-key", vec![]).await {
        ServerMessage::SettingsError { auth_rejected, .. } => assert!(auth_rejected),
        other => panic!("expected a refusal, got {other:?}"),
    }
    for (bad, why) in [
        (vec![publisher("abcd", "Short")], "64 hex"),
        (vec![publisher(&"ab".repeat(32), "A"), publisher(&"AB".repeat(32), "B")], "twice"),
        (vec![publisher(&"ab".repeat(32), "two\nlines")], "one line"),
    ] {
        match save(&mut owner, KEY, bad).await {
            ServerMessage::SettingsError { message, auth_rejected, .. } => assert!(message.contains(why) && message.contains("nothing was saved") && !auth_rejected, "{message}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }
    assert_eq!(listed(list(&mut owner).await), good, "none of the refused saves touched the list");
    assert_eq!(load_config_from_path(&path, false).unwrap().trusted_publishers.len(), 1);
}

#[tokio::test]
async fn a_member_neither_reads_nor_changes_the_list() {
    let (url, _) = spin_up().await;
    let (mut ana, _, _) = ServerConnection::handshake_as_member(&url, "ana-phone", "Ana's phone", "ana", TEMP, None, warden_server_protocol::tls::default_client_config()).await.unwrap();
    ana.send(&ClientMessage::ChangePassword { request_id: 9, old_password: TEMP.into(), new_password: "anas-own-pass".into(), recovery_code: None }).await.unwrap();
    assert!(matches!(next(&mut ana).await, ServerMessage::PasswordChanged { .. }));

    assert!(matches!(list(&mut ana).await, ServerMessage::SettingsError { auth_rejected: true, .. }));
    // Even with the owner's key in hand a member's session is not the owner's.
    assert!(matches!(save(&mut ana, KEY, vec![publisher(&"ab".repeat(32), "Ana")]).await, ServerMessage::SettingsError { auth_rejected: true, .. }));

    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    assert!(listed(list(&mut owner).await).is_empty());
}

#[tokio::test]
async fn a_package_signed_by_a_listed_key_previews_as_trusted() {
    let (url, _) = spin_up().await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();

    let skill = Skill { name: "greet".into(), description: "Greets.".into(), body: "Say hello.".into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None };
    let (private, public) = generate_keypair().unwrap();
    let mut package = Package::export_skill(&skill, "1.0.0", Some("Ana"), "MIT").unwrap();
    package.sign(&private).unwrap();
    let content = package.to_json().unwrap();

    let preview = |content: String| ClientMessage::PreviewSkillPackage { request_id: 7, content };
    let signature = |message: ServerMessage| match message {
        ServerMessage::SkillPackagePreview { preview, .. } => (preview.signature, preview.signer_name),
        other => panic!("expected a preview, got {other:?}"),
    };

    owner.send(&preview(content.clone())).await.unwrap();
    assert_eq!(signature(next(&mut owner).await), ("unknown".to_string(), None), "signed, but the owner has not listed the key");

    save(&mut owner, KEY, vec![publisher(&public, "Fabio")]).await;
    owner.send(&preview(content.clone())).await.unwrap();
    assert_eq!(signature(next(&mut owner).await), ("trusted".to_string(), Some("Fabio".to_string())));

    // Take the key off the list and it is unknown again.
    save(&mut owner, KEY, vec![]).await;
    owner.send(&preview(content)).await.unwrap();
    assert_eq!(signature(next(&mut owner).await).0, "unknown");
}

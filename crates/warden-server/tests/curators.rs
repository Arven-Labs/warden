//! P160 on the hub: the curators the owner takes skill packages from (`[[curators]]`). `ListCurators` is for anyone signed in and
//! `SaveCurators` is the owner's (a pairing key, like any change); `FetchCuratorIndex` and `FetchCuratorPackage` read what a curator
//! published, checked against the curator's key, from a server of our own that the hub may reach only because it was started with
//! `with_private_curators`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use warden_bootstrap::{load_config_from_path, save_config, FileConfig};
use warden_core::memory::Vault;
use warden_core::model::{ChatStream, Message, ModelProvider};
use warden_core::orchestrator::Orchestrator;
use warden_core::skill::index::{Index, IndexEntry, Revoked};
use warden_core::skill::package::Package;
use warden_core::skill::signing::generate_keypair;
use warden_core::skill::Skill;
use warden_core::tool::ToolSpec;
use warden_server::{ClientMessage, Server, ServerConnection, ServerMessage, SettingsHost};
use warden_server_protocol::protocol::CuratorDto;

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
async fn spin_up(private_curators: bool) -> (String, PathBuf) {
    let dir = std::env::temp_dir().join(format!("warden-server-curators-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
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
        .with_users_dir(dir.join("users"))
        .with_private_curators(private_curators);
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

fn curator(name: &str, key: &str, url: &str) -> CuratorDto {
    CuratorDto { name: name.into(), key: key.into(), url: url.into() }
}

async fn list(conn: &mut ServerConnection) -> ServerMessage {
    conn.send(&ClientMessage::ListCurators { request_id: 1 }).await.unwrap();
    next(conn).await
}

async fn save(conn: &mut ServerConnection, key: &str, curators: Vec<CuratorDto>) -> ServerMessage {
    conn.send(&ClientMessage::SaveCurators { request_id: 2, pairing_key: key.into(), curators }).await.unwrap();
    next(conn).await
}

fn listed(message: ServerMessage) -> Vec<CuratorDto> {
    match message {
        ServerMessage::Curators { curators, .. } => curators,
        other => panic!("expected the list, got {other:?}"),
    }
}

async fn fetch_index(conn: &mut ServerConnection, curator: &str) -> ServerMessage {
    conn.send(&ClientMessage::FetchCuratorIndex { request_id: 3, curator: curator.into() }).await.unwrap();
    next(conn).await
}

async fn fetch_package(conn: &mut ServerConnection, curator: &str, hash: &str) -> ServerMessage {
    conn.send(&ClientMessage::FetchCuratorPackage { request_id: 4, curator: curator.into(), hash: hash.into() }).await.unwrap();
    next(conn).await
}

fn error_of(message: ServerMessage) -> String {
    match message {
        ServerMessage::SkillError { message, .. } => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// What a curator publishes, served from this machine: the index (which a test may swap for another) and the one package it lists.
struct Published {
    base: String,
    public: String,
    private: Vec<u8>,
    package: Package,
    other: Package,
    index: Arc<Mutex<String>>,
}

fn skill(body: &str) -> Skill {
    Skill { name: "greet".into(), description: "Greets.".into(), body: body.into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None }
}

impl Published {
    fn index_text(&self, sequence: u64, revoked: Vec<Revoked>, key: &[u8]) -> String {
        let entry = IndexEntry::from_package(&self.package, &format!("{}/package.json", self.base)).unwrap();
        Index::build("Ana's picks", sequence, vec![entry], revoked, key).unwrap().to_json().unwrap()
    }

    fn url(&self) -> String {
        format!("{}/index.json", self.base)
    }

    fn swap_index(&self, text: String) {
        *self.index.lock().unwrap() = text;
    }
}

async fn publish(sequence: u64, revoked: bool) -> Published {
    let package = Package::export_skill(&skill("Say hello warmly."), "1.0.0", Some("Ana"), "MIT").unwrap();
    let other = Package::export_skill(&skill("Say something else."), "1.0.0", Some("Ana"), "MIT").unwrap();
    let (private, public) = generate_keypair().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let index = Arc::new(Mutex::new(String::new()));
    let published = Published { base, public, private, package: package.clone(), other, index: index.clone() };
    let revoked = if revoked { vec![Revoked { hash: package.hash.clone(), reason: Some("it asked for a password".into()) }] } else { vec![] };
    published.swap_index(published.index_text(sequence, revoked, &published.private));
    let package_text = package.to_json().unwrap();
    let router = axum::Router::new()
        .route("/index.json", axum::routing::get(move || {
            let index = index.clone();
            async move { index.lock().unwrap().clone() }
        }))
        .route("/package.json", axum::routing::get(move || {
            let text = package_text.clone();
            async move { text }
        }));
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    published
}

#[tokio::test]
async fn the_owner_saves_the_list_and_reads_it_back_and_the_rest_of_the_config_stays() {
    let (url, path) = spin_up(false).await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    assert!(listed(list(&mut owner).await).is_empty());

    let saved = listed(save(&mut owner, KEY, vec![curator(" Ana ", &"AB".repeat(32), "https://example.org/index.json"), curator("Bia", &"cd".repeat(32), "ar://abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN-_0")]).await);
    assert_eq!(saved[0], curator("Ana", &"ab".repeat(32), "https://example.org/index.json"), "trimmed, with the key in lowercase");
    assert_eq!(saved.len(), 2);
    assert_eq!(listed(list(&mut owner).await), saved);

    let config = load_config_from_path(&path, false).unwrap();
    assert_eq!(config.curators.len(), 2);
    assert_eq!(config.topup_url.as_deref(), Some("https://example.com/credits"));
    assert!(config.users.iter().any(|u| u.id == "ana"), "the members are as they were");

    assert!(listed(save(&mut owner, KEY, vec![]).await).is_empty());
    assert!(load_config_from_path(&path, false).unwrap().curators.is_empty());
}

#[tokio::test]
async fn a_wrong_pairing_key_or_a_bad_entry_changes_nothing() {
    let (url, path) = spin_up(false).await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    let good = listed(save(&mut owner, KEY, vec![curator("Ana", &"ab".repeat(32), "https://example.org/index.json")]).await);

    match save(&mut owner, "not-the-key", vec![]).await {
        ServerMessage::SettingsError { auth_rejected, .. } => assert!(auth_rejected),
        other => panic!("expected a refusal, got {other:?}"),
    }
    for (bad, why) in [
        (vec![curator("Ana", "abcd", "https://example.org/i.json")], "64 hex"),
        (vec![curator("Ana", &"ab".repeat(32), "https://example.org/i.json"), curator("ana", &"cd".repeat(32), "https://example.org/j.json")], "twice"),
        (vec![curator("Ana", &"ab".repeat(32), "http://example.org/i.json")], "https:// address or an ar:// one"),
        (vec![curator("Ana", &"ab".repeat(32), "file:///etc/passwd")], "https:// address or an ar:// one"),
        (vec![curator("", &"ab".repeat(32), "https://example.org/i.json")], "needs a name"),
    ] {
        match save(&mut owner, KEY, bad).await {
            ServerMessage::SettingsError { message, auth_rejected, .. } => assert!(message.contains(why) && message.contains("nothing was saved") && !auth_rejected, "{message}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }
    assert_eq!(listed(list(&mut owner).await), good, "none of the refused saves touched the list");
    assert_eq!(load_config_from_path(&path, false).unwrap().curators.len(), 1);
}

#[tokio::test]
async fn a_member_reads_the_list_and_the_index_but_does_not_change_the_list() {
    let (url, _) = spin_up(true).await;
    let published = publish(1, false).await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    save(&mut owner, KEY, vec![curator("Ana", &published.public, &published.url())]).await;

    let (mut ana, _, _) = ServerConnection::handshake_as_member(&url, "ana-phone", "Ana's phone", "ana", TEMP, None, warden_server_protocol::tls::default_client_config()).await.unwrap();
    ana.send(&ClientMessage::ChangePassword { request_id: 9, old_password: TEMP.into(), new_password: "anas-own-pass".into(), recovery_code: None }).await.unwrap();
    assert!(matches!(next(&mut ana).await, ServerMessage::PasswordChanged { .. }));

    assert_eq!(listed(list(&mut ana).await).len(), 1);
    assert!(matches!(fetch_index(&mut ana, "Ana").await, ServerMessage::CuratorIndex { .. }));
    // Even with the owner's key in hand a member's session is not the owner's.
    assert!(matches!(save(&mut ana, KEY, vec![]).await, ServerMessage::SettingsError { auth_rejected: true, .. }));
    assert_eq!(listed(list(&mut owner).await).len(), 1);
}

#[tokio::test]
async fn the_index_lists_what_the_curator_published_and_its_package_comes_back() {
    let (url, _) = spin_up(true).await;
    let published = publish(7, false).await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    save(&mut owner, KEY, vec![curator("Ana", &published.public, &published.url())]).await;

    match fetch_index(&mut owner, "ana").await {
        ServerMessage::CuratorIndex { curator, name, sequence, entries, .. } => {
            assert_eq!((curator.as_str(), name.as_str(), sequence), ("Ana", "Ana's picks", 7));
            assert_eq!(entries.len(), 1);
            assert_eq!((entries[0].id.as_str(), entries[0].hash.as_str(), entries[0].revoked), ("greet", published.package.hash.as_str(), false));
        }
        other => panic!("expected the index, got {other:?}"),
    }
    match fetch_package(&mut owner, "Ana", &published.package.hash).await {
        ServerMessage::SkillPackageFile { content, .. } => assert_eq!(Package::parse(&content).unwrap(), published.package),
        other => panic!("expected the package, got {other:?}"),
    }
    // And what comes back is an ordinary package: the preview reads it.
    owner.send(&ClientMessage::PreviewSkillPackage { request_id: 5, content: published.package.to_json().unwrap() }).await.unwrap();
    assert!(matches!(next(&mut owner).await, ServerMessage::SkillPackagePreview { .. }));
}

#[tokio::test]
async fn an_entry_says_it_updates_a_skill_installed_from_the_same_curator_once_the_curator_lists_another_package() {
    let (url, _) = spin_up(true).await;
    let published = publish(7, false).await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    save(&mut owner, KEY, vec![curator("Ana", &published.public, &published.url())]).await;

    // Nothing installed yet: nothing to update.
    match fetch_index(&mut owner, "Ana").await {
        ServerMessage::CuratorIndex { entries, .. } => assert!(!entries[0].update),
        other => panic!("expected the index, got {other:?}"),
    }

    // Installed from the curator's package, signed with the curator's key: the listed package is what is installed.
    let mut signed = published.package.clone();
    signed.sign(&published.private).unwrap();
    owner.send(&ClientMessage::InstallSkillPackage { request_id: 6, content: signed.to_json().unwrap(), overwrite: false, allow_flagged: false }).await.unwrap();
    assert!(matches!(next(&mut owner).await, ServerMessage::SkillOk { .. }));
    match fetch_index(&mut owner, "Ana").await {
        ServerMessage::CuratorIndex { entries, .. } => assert!(!entries[0].update, "the listed package is the installed one"),
        other => panic!("expected the index, got {other:?}"),
    }

    // The curator republishes `greet` with another text (a newer index number, or the hub refuses it as a replay).
    let entry = IndexEntry::from_package(&published.other, &format!("{}/package.json", published.base)).unwrap();
    published.swap_index(Index::build("Ana's picks", 8, vec![entry], vec![], &published.private).unwrap().to_json().unwrap());
    match fetch_index(&mut owner, "Ana").await {
        ServerMessage::CuratorIndex { entries, .. } => {
            assert_eq!((entries[0].id.as_str(), entries[0].hash.as_str(), entries[0].update), ("greet", published.other.hash.as_str(), true));
        }
        other => panic!("expected the index, got {other:?}"),
    }
}

#[tokio::test]
async fn a_package_the_curator_took_back_or_that_is_not_listed_is_not_given() {
    let (url, _) = spin_up(true).await;
    let published = publish(1, true).await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    save(&mut owner, KEY, vec![curator("Ana", &published.public, &published.url())]).await;

    match fetch_index(&mut owner, "Ana").await {
        ServerMessage::CuratorIndex { entries, .. } => {
            assert!(entries[0].revoked);
            assert_eq!(entries[0].revoked_reason.as_deref(), Some("it asked for a password"));
        }
        other => panic!("expected the index, got {other:?}"),
    }
    assert_eq!(error_of(fetch_package(&mut owner, "Ana", &published.package.hash).await), "the curator took 'greet' back: it asked for a password");
    assert!(error_of(fetch_package(&mut owner, "Ana", &published.other.hash).await).contains("not in the curator's index"));
    assert!(error_of(fetch_index(&mut owner, "Nobody").await).contains("no curator called 'Nobody'"));
}

#[tokio::test]
async fn an_index_signed_by_another_key_or_older_than_one_seen_is_refused() {
    let (url, path) = spin_up(true).await;
    let published = publish(10, false).await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();

    // The owner listed a different key than the one that signs what is published there.
    save(&mut owner, KEY, vec![curator("Ana", &"ab".repeat(32), &published.url())]).await;
    assert!(error_of(fetch_index(&mut owner, "Ana").await).contains("another key"));

    save(&mut owner, KEY, vec![curator("Ana", &published.public, &published.url())]).await;
    assert!(matches!(fetch_index(&mut owner, "Ana").await, ServerMessage::CuratorIndex { sequence: 10, .. }));
    assert!(path.with_file_name("curators_seen.json").exists(), "the newest number seen is kept beside the config");

    // Someone puts an older (but really signed) index back: it is refused, and a newer one is taken.
    published.swap_index(published.index_text(4, vec![], &published.private));
    assert!(error_of(fetch_index(&mut owner, "Ana").await).contains("older than one already seen"));
    published.swap_index(published.index_text(11, vec![], &published.private));
    assert!(matches!(fetch_index(&mut owner, "Ana").await, ServerMessage::CuratorIndex { sequence: 11, .. }));
}

#[tokio::test]
async fn by_default_the_hub_does_not_reach_a_curator_on_a_private_address() {
    let (url, _) = spin_up(false).await;
    let published = publish(1, false).await;
    let mut owner = ServerConnection::connect(&url, "laptop", "Laptop", KEY).await.unwrap();
    save(&mut owner, KEY, vec![curator("Ana", &published.public, &published.url())]).await;
    assert!(error_of(fetch_index(&mut owner, "Ana").await).contains("only https://"));
    assert!(error_of(fetch_package(&mut owner, "Ana", &published.package.hash).await).contains("only https://"));
}

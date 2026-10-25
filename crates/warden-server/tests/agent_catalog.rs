//! P140 on the hub: the gallery of ready-made agents. `ListAgentCatalog` shows each with its state among the owner's agents, and
//! `EditAgentOrg` with `InstallCatalogAgent` adds one (careful by default), puts the skills it works with in the vault without
//! replacing any the person has, and answers like a settings save.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use warden_bootstrap::agent_catalog;
use warden_bootstrap::{load_config_from_path, save_config, FileConfig};
use warden_core::memory::Vault;
use warden_core::model::{ChatStream, Message, ModelProvider};
use warden_core::orchestrator::Orchestrator;
use warden_core::skill::{Skill, SkillStore};
use warden_core::tool::ToolSpec;
use warden_server::{ClientMessage, Server, ServerConnection, ServerMessage, SettingsHost};
use warden_server_protocol::protocol::{AgentOrgEdit, CatalogAgentDto};

const KEY: &str = "test-key";

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

/// A hub with an empty config; returns its address, the config path and the skills of its vault.
async fn spin_up() -> (String, PathBuf, SkillStore) {
    let dir = support::unique_temp_dir("warden-server-agent-catalog");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    save_config(&path, &FileConfig::default()).unwrap();
    let vault = Arc::new(Vault::new(dir.join("vault")));
    let orchestrator = Orchestrator::new(Arc::new(NoModel), vault.clone());
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), KEY, "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_open_key_pairing()
        .with_settings(Arc::new(Host { path: path.clone(), dir }));
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    (format!("ws://{addr}"), path, SkillStore::new(vault))
}

async fn list(conn: &mut ServerConnection) -> Vec<CatalogAgentDto> {
    conn.send(&ClientMessage::ListAgentCatalog { request_id: 1 }).await.unwrap();
    loop {
        match conn.recv().await.unwrap().expect("connection closed") {
            ServerMessage::AgentCatalog { entries, .. } => return entries,
            ServerMessage::SettingsError { message, .. } => panic!("the gallery was refused: {message}"),
            _ => continue,
        }
    }
}

async fn install(conn: &mut ServerConnection, key: &str, id: &str, overwrite: bool) -> ServerMessage {
    conn.send(&ClientMessage::EditAgentOrg { request_id: 2, pairing_key: key.into(), edit: AgentOrgEdit::InstallCatalogAgent { id: id.into(), overwrite } }).await.unwrap();
    loop {
        match conn.recv().await.unwrap().expect("connection closed") {
            reply @ (ServerMessage::SettingsSaved { .. } | ServerMessage::SettingsError { .. }) => return reply,
            _ => continue,
        }
    }
}

#[tokio::test]
async fn the_gallery_lists_every_agent_and_installing_one_adds_it_careful_with_its_skills() {
    let (url, path, skills) = spin_up().await;
    let mut conn = ServerConnection::connect(&url, "web-1", "Browser", KEY).await.unwrap();

    let before = list(&mut conn).await;
    assert_eq!(before.len(), agent_catalog::entries().len());
    assert!(before.iter().all(|e| e.status == "notInstalled" && !e.skills.is_empty() && !e.role.is_empty() && !e.description.is_empty()));

    let entry = agent_catalog::entries().iter().find(|e| e.skills.len() > 1).expect("an agent with several skills");
    match install(&mut conn, KEY, &entry.id, false).await {
        ServerMessage::SettingsSaved { settings, .. } => {
            let added = settings.agents.iter().find(|a| a.id == entry.id).expect("the new settings carry the agent");
            assert_eq!(added.role.as_deref(), Some(entry.role.as_str()));
        }
        other => panic!("expected the new settings, got {other:?}"),
    }

    let config = load_config_from_path(&path, false).unwrap();
    let agent = config.agents.iter().find(|a| a.id == entry.id).unwrap();
    assert_eq!((agent.autonomy, agent.provider_id.as_deref(), agent.owner.as_deref()), (3, None, None));
    assert!(!agent.can_delegate_to_agents && !agent.can_manage_agents && !agent.can_message_agents && !agent.can_manage_tasks);
    assert_eq!(agent.persona, entry.persona);
    for skill in &entry.skills {
        assert!(skills.exists(skill), "the skill '{skill}' it works with is in the vault");
    }

    let after = list(&mut conn).await;
    assert_eq!(after.iter().find(|e| e.id == entry.id).unwrap().status, "installed");
    assert!(after.iter().filter(|e| e.id != entry.id).all(|e| e.status == "notInstalled"));
}

#[tokio::test]
async fn a_skill_the_person_has_is_kept_a_changed_agent_needs_overwrite_and_a_wrong_key_changes_nothing() {
    let (url, path, skills) = spin_up().await;
    let mut conn = ServerConnection::connect(&url, "web-1", "Browser", KEY).await.unwrap();
    let entry = agent_catalog::entries().iter().find(|e| e.skills.len() > 1).unwrap();
    let mine = &entry.skills[0];
    skills.save(&Skill::parse(mine, "---\ndescription: Mine\n---\nMy own steps.")).unwrap();

    // A wrong key: refused, nothing written, no skill installed.
    match install(&mut conn, "not-the-key", &entry.id, false).await {
        ServerMessage::SettingsError { auth_rejected, .. } => assert!(auth_rejected),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(load_config_from_path(&path, false).unwrap().agents.is_empty());
    assert!(!skills.exists(&entry.skills[1]));

    // The right key: the agent comes, the other skills with it, and the person's own skill is as it was.
    assert!(matches!(install(&mut conn, KEY, &entry.id, false).await, ServerMessage::SettingsSaved { .. }));
    assert_eq!(skills.get(mine).unwrap().body, "My own steps.");
    assert!(skills.exists(&entry.skills[1]));

    // Edit the agent's text by hand: the gallery says "modified", and installing again needs `overwrite`.
    let mut config = load_config_from_path(&path, false).unwrap();
    config.agents[0].persona = "My own words.".into();
    save_config(&path, &config).unwrap();
    assert_eq!(list(&mut conn).await.iter().find(|e| e.id == entry.id).unwrap().status, "modified");

    match install(&mut conn, KEY, &entry.id, false).await {
        ServerMessage::SettingsError { message, .. } => assert!(message.contains("differs"), "{message}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(load_config_from_path(&path, false).unwrap().agents[0].persona, "My own words.");

    assert!(matches!(install(&mut conn, KEY, &entry.id, true).await, ServerMessage::SettingsSaved { .. }));
    assert_eq!(load_config_from_path(&path, false).unwrap().agents[0].persona, entry.persona);

    // An id the gallery does not have is an error.
    assert!(matches!(install(&mut conn, KEY, "nope", false).await, ServerMessage::SettingsError { .. }));
}

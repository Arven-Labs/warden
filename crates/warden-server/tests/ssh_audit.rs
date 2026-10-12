//! P47 on the hub: `ListSshAudit` shows the calls the SSH tools made, read from the log they write, to the owner.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use warden_core::memory::Vault;
use warden_core::orchestrator::Orchestrator;
use warden_core::tool::ssh::AuditLog;
use warden_server::{ClientMessage, Server, ServerConnection, ServerMessage};

struct NoModel;

#[async_trait::async_trait]
impl warden_core::model::ModelProvider for NoModel {
    async fn chat_stream(&self, _messages: Vec<warden_core::model::Message>, _tools: Vec<warden_core::tool::ToolSpec>) -> anyhow::Result<warden_core::model::ChatStream> {
        anyhow::bail!("these tests never run a turn")
    }
}

fn temp_log(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("warden-ssh-audit-hub-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())).join("ssh_audit.jsonl")
}

async fn spin_up(log: Option<PathBuf>) -> String {
    let dir = std::env::temp_dir().join(format!("warden-server-ssh-audit-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let orchestrator = Orchestrator::new(Arc::new(NoModel), Arc::new(Vault::new(dir.join("vault"))));
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_ssh_audit(log);
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    format!("ws://{addr}")
}

async fn list(url: &str) -> Vec<warden_server_protocol::protocol::SshAuditEntryDto> {
    let mut conn = ServerConnection::connect(url, "web-1", "Browser", "test-key").await.unwrap();
    conn.send(&ClientMessage::ListSshAudit { request_id: 7 }).await.unwrap();
    loop {
        match conn.recv().await.unwrap().expect("connection closed") {
            ServerMessage::SshAuditList { request_id, entries } => {
                assert_eq!(request_id, 7);
                return entries;
            }
            _ => continue,
        }
    }
}

#[tokio::test]
async fn the_calls_in_the_log_are_listed_newest_first_with_how_they_ended() {
    let log = temp_log("listed");
    let audit = AuditLog::new(&log);
    audit.record(&json!({ "ts": 1000, "host_id": "prod", "agent": "ops", "action": "exec", "approval": "approved", "command": "uptime", "outcome": { "exit_code": 0 } })).unwrap();
    audit
        .record(&json!({ "ts": 2000, "host_id": "prod", "agent": null, "action": "upload", "approval": "not_required", "local_path": "/tmp/a", "remote_path": "/srv/a", "overwrite": true, "outcome": { "bytes": 2048 } }))
        .unwrap();
    audit.record(&json!({ "ts": 3000, "host_id": "prod", "agent": "ops", "action": "exec", "approval": "denied", "command": "reboot", "outcome": { "error": "refused: not approved" } })).unwrap();

    let entries = list(&spin_up(Some(log)).await).await;

    assert_eq!(entries.iter().map(|e| (e.at_ms, e.action.as_str(), e.approval.as_str())).collect::<Vec<_>>(), [(3000, "exec", "denied"), (2000, "upload", "not_required"), (1000, "exec", "approved")]);
    assert_eq!(entries[0].error.as_deref(), Some("refused: not approved"));
    assert_eq!((entries[1].local_path.as_deref(), entries[1].remote_path.as_deref(), entries[1].overwrite, entries[1].bytes), (Some("/tmp/a"), Some("/srv/a"), true, Some(2048)));
    assert_eq!((entries[2].command.as_deref(), entries[2].agent.as_deref(), entries[2].exit_code), (Some("uptime"), Some("ops"), Some(0)));
}

#[tokio::test]
async fn a_hub_with_no_log_lists_nothing() {
    assert!(list(&spin_up(None).await).await.is_empty());
    assert!(list(&spin_up(Some(temp_log("never-written"))).await).await.is_empty());
}

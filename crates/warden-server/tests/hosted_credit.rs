//! The hosted credit in the usage report: a hub with a `cloud` provider asks its gateway for the balance when a client asks for
//! usage, and the report carries it. A real `Server` and a real `Provider::Cloud` config, with a tiny local HTTP server standing
//! in for the gateway.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use warden_bootstrap::{save_config, FileConfig, Provider, ProviderConfig};
use warden_core::memory::Vault;
use warden_core::model::{ChatStream, Message, ModelProvider};
use warden_core::orchestrator::Orchestrator;
use warden_core::tool::ToolSpec;
use warden_server::{ClientMessage, Server, ServerConnection, ServerMessage, SettingsHost};
use warden_server_protocol::protocol::{CreditDto, UsageReportDto};

const KEY: &str = "test-key-0123456789-0123456789-ab";

/// Answers every request with `body` and `status`, then closes; returns its `/v1` address.
async fn stub_gateway(status: u16, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                let reply = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{addr}/v1")
}

struct Unused;

#[async_trait]
impl ModelProvider for Unused {
    async fn chat_stream(&self, _messages: Vec<Message>, _tools: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
        anyhow::bail!("not used by this test")
    }
}

struct Host {
    path: PathBuf,
}

#[async_trait]
impl SettingsHost for Host {
    fn config_path(&self) -> PathBuf {
        self.path.clone()
    }

    async fn build(&self) -> anyhow::Result<Orchestrator> {
        anyhow::bail!("not used by this test")
    }
}

fn cloud(base_url: String) -> ProviderConfig {
    ProviderConfig { id: "hosted".into(), kind: Provider::Cloud, api_key: Some("gwy_test".into()), base_url: Some(base_url), model: Some("warden-fast".into()), node: None }
}

/// A hub whose config holds `providers`; returns the address to connect to.
async fn hub(providers: Vec<ProviderConfig>) -> String {
    let dir = std::env::temp_dir().join(format!("warden-hosted-credit-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    save_config(&config_path, &FileConfig { providers, ..FileConfig::default() }).unwrap();
    let orchestrator = Orchestrator::new(Arc::new(Unused), Arc::new(Vault::new(dir.join("vault"))));
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), KEY, "Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_settings(Arc::new(Host { path: config_path }));
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    format!("ws://{addr}")
}

async fn usage(url: &str) -> UsageReportDto {
    let mut conn = ServerConnection::connect(url, "dev-1", "Device", KEY).await.unwrap();
    conn.send(&ClientMessage::RequestUsage { request_id: 1, tz_offset_minutes: 0 }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::UsageReport { report, .. }) => report,
        other => panic!("expected a UsageReport, got {other:?}"),
    }
}

#[tokio::test]
async fn the_report_carries_the_credit_the_gateway_reports() {
    let gateway = stub_gateway(200, r#"{"balance_micro_usd":25000000,"available_micro_usd":24000000}"#).await;
    let report = usage(&hub(vec![cloud(gateway)]).await).await;
    assert_eq!(report.credit, Some(CreditDto { balance_micro_usd: 25_000_000, available_micro_usd: 24_000_000 }));
}

#[tokio::test]
async fn a_hub_with_no_cloud_provider_has_no_credit_in_its_report() {
    let local = ProviderConfig { id: "local".into(), kind: Provider::OpenaiCompatible, api_key: None, base_url: Some("http://127.0.0.1:1/v1".into()), model: Some("m".into()), node: None };
    let report = usage(&hub(vec![local]).await).await;
    assert_eq!(report.credit, None);
}

#[tokio::test]
async fn a_gateway_that_refuses_or_is_down_leaves_the_report_without_credit_but_whole() {
    let refusing = stub_gateway(401, r#"{"error":{"code":"invalid_api_key"}}"#).await;
    let report = usage(&hub(vec![cloud(refusing)]).await).await;
    // `usage` only returns if a whole `UsageReport` arrived, so the report was not held back by the refusing gateway.
    assert_eq!(report.credit, None);

    // Nothing is listening here.
    let unused = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead = format!("http://{}/v1", unused.local_addr().unwrap());
    drop(unused);
    assert_eq!(usage(&hub(vec![cloud(dead)]).await).await.credit, None);
}

//! Asking a hub how loaded it is (`GET /status`), behind a trait so the recycler is tested without a network.

use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use warden_server_protocol::status::STATUS_PATH;
use warden_server_protocol::HubStatus;

use crate::instance::Instance;

#[async_trait]
pub trait StatusSource: Send + Sync {
    async fn fetch(&self, instance: &Instance) -> anyhow::Result<HubStatus>;
}

/// Reads the status over HTTP from the port the instance is published on, with its pairing key as the bearer token. The
/// hub answers 401 (after a short wait) to a wrong key and says nothing else, so a failure here is "could not tell".
pub struct HttpStatus {
    client: reqwest::Client,
}

impl HttpStatus {
    pub fn new() -> Self {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().expect("a plain HTTP client always builds");
        Self { client }
    }
}

impl Default for HttpStatus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatusSource for HttpStatus {
    async fn fetch(&self, instance: &Instance) -> anyhow::Result<HubStatus> {
        let url = format!("http://127.0.0.1:{}{STATUS_PATH}", instance.port);
        let response = self.client.get(&url).bearer_auth(&instance.pairing_key).send().await.with_context(|| format!("could not reach {url}"))?;
        anyhow::ensure!(response.status().is_success(), "{url} answered {}", response.status());
        response.json::<HubStatus>().await.with_context(|| format!("{url} did not answer with a status"))
    }
}

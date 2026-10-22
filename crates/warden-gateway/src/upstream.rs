//! The call to the provider, behind a trait so the rest is tested with `fake::FakeUpstream`.
//!
//! `HttpUpstream` is the real one: a POST to `{base_url}/chat/completions` with the provider's key. A non-2xx answer is an
//! error for both calls, so a failure never looks like a reply. The gateway's own key is the only thing that goes upstream:
//! the instance token stays here.

use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use serde_json::Value;

/// One call to a provider: where, with which key, and the (already rewritten) request body.
#[derive(Debug, Clone)]
pub struct UpstreamCall {
    pub base_url: String,
    pub api_key: Option<String>,
    pub body: Value,
}

#[derive(Debug)]
pub enum UpstreamError {
    /// The provider could not be reached, or what it sent back was not what an OpenAI-compatible server sends.
    Unreachable(String),
    /// The provider answered with an error status.
    Status { status: u16, body: String },
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpstreamError::Unreachable(why) => write!(f, "the model provider is unreachable: {why}"),
            UpstreamError::Status { status, .. } => write!(f, "the model provider answered with status {status}"),
        }
    }
}

impl std::error::Error for UpstreamError {}

/// The chunks of a streamed answer, as the provider sent them.
#[async_trait]
pub trait ChunkStream: Send {
    /// `None` once the stream ended; an `Err` means it broke.
    async fn next(&mut self) -> Option<Result<Vec<u8>, UpstreamError>>;
}

#[async_trait]
pub trait Upstream: Send + Sync {
    /// A non-streaming call; the JSON answer.
    async fn complete(&self, call: &UpstreamCall) -> Result<Value, UpstreamError>;
    /// A streaming call (`"stream": true` in the body); the chunks.
    async fn stream(&self, call: &UpstreamCall) -> Result<Box<dyn ChunkStream>, UpstreamError>;
}

pub struct HttpUpstream {
    client: reqwest::Client,
}

impl HttpUpstream {
    pub fn new() -> HttpUpstream {
        // No overall timeout: a stream may run for minutes. The connect and the first-byte waits are what bound a dead provider.
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(120))
            .build()
            .expect("a reqwest client with plain timeouts builds");
        HttpUpstream { client }
    }

    async fn post(&self, call: &UpstreamCall) -> Result<reqwest::Response, UpstreamError> {
        let url = format!("{}/chat/completions", call.base_url.trim_end_matches('/'));
        let mut request = self.client.post(url).json(&call.body);
        if let Some(key) = &call.api_key {
            request = request.bearer_auth(key);
        }
        let response = request.send().await.map_err(|e| UpstreamError::Unreachable(e.without_url().to_string()))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        // The provider's own message helps the operator; cap it so a huge error page can't fill the log.
        let mut body = response.text().await.unwrap_or_default();
        body.truncate(2048);
        Err(UpstreamError::Status { status: status.as_u16(), body })
    }
}

impl Default for HttpUpstream {
    fn default() -> Self {
        HttpUpstream::new()
    }
}

struct BodyChunks {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Vec<u8>>> + Send>>,
}

#[async_trait]
impl ChunkStream for BodyChunks {
    async fn next(&mut self) -> Option<Result<Vec<u8>, UpstreamError>> {
        let item = self.inner.next().await?;
        Some(item.map_err(|e| UpstreamError::Unreachable(e.without_url().to_string())))
    }
}

#[async_trait]
impl Upstream for HttpUpstream {
    async fn complete(&self, call: &UpstreamCall) -> Result<Value, UpstreamError> {
        let response = self.post(call).await?;
        response.json::<Value>().await.map_err(|e| UpstreamError::Unreachable(format!("the answer is not JSON: {e}")))
    }

    async fn stream(&self, call: &UpstreamCall) -> Result<Box<dyn ChunkStream>, UpstreamError> {
        let response = self.post(call).await?;
        let chunks = response.bytes_stream().map(|r| r.map(|b| b.to_vec()));
        Ok(Box::new(BodyChunks { inner: Box::pin(chunks) }))
    }
}

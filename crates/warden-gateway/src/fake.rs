//! A scripted `Upstream` for tests, here (and not under `#[cfg(test)]`) so the integration tests can use it too.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;

use crate::upstream::{ChunkStream, Upstream, UpstreamCall, UpstreamError};
use crate::webhook::WebhookSender;

/// A webhook the fake site mãe received.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Delivery {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// A `WebhookSender` that records what it was asked to send. `scripted` makes its first answers fail (or whatever the
/// script says); once the script is used up it answers 200.
pub struct FakeSender {
    script: Mutex<VecDeque<Result<u16, String>>>,
    deliveries: Mutex<Vec<Delivery>>,
}

impl FakeSender {
    pub fn new() -> FakeSender {
        FakeSender::scripted([])
    }

    pub fn scripted(answers: impl IntoIterator<Item = Result<u16, String>>) -> FakeSender {
        FakeSender { script: Mutex::new(answers.into_iter().collect()), deliveries: Mutex::new(Vec::new()) }
    }

    /// Every attempt so far, failed ones included, in order.
    pub fn deliveries(&self) -> Vec<Delivery> {
        self.deliveries.lock().unwrap().clone()
    }
}

impl Default for FakeSender {
    fn default() -> Self {
        FakeSender::new()
    }
}

#[async_trait]
impl WebhookSender for FakeSender {
    async fn send(&self, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<u16, String> {
        self.deliveries.lock().unwrap().push(Delivery { url: url.to_string(), headers: headers.to_vec(), body: body.to_vec() });
        self.script.lock().unwrap().pop_front().unwrap_or(Ok(200))
    }
}

/// What the fake does on a call.
pub enum Script {
    /// `complete` answers this JSON.
    Reply(Value),
    /// `stream` sends these chunks, then ends; an `Err` item breaks the stream there.
    Chunks(Vec<Result<Vec<u8>, String>>),
    /// `stream` sends this chunk, then never another and never ends: a provider that stalls mid-answer.
    Stalls(Vec<u8>),
    /// Both fail with this provider status.
    Status(u16),
    /// Both fail as if the provider were down.
    Down,
    /// One script per upstream, found by the `base_url` of the call: a catalog whose first provider fails and whose second
    /// answers. A `base_url` that isn't listed is down.
    ByBaseUrl(Vec<(String, Script)>),
}

static DOWN: Script = Script::Down;

pub struct FakeUpstream {
    script: Script,
    calls: Mutex<Vec<UpstreamCall>>,
}

impl FakeUpstream {
    pub fn new(script: Script) -> FakeUpstream {
        FakeUpstream { script, calls: Mutex::new(Vec::new()) }
    }

    /// The calls made so far, in order.
    pub fn calls(&self) -> Vec<UpstreamCall> {
        self.calls.lock().unwrap().clone()
    }

    /// The script that applies to this call: the whole one, or the one listed for the call's `base_url`.
    fn script_for(&self, call: &UpstreamCall) -> &Script {
        match &self.script {
            Script::ByBaseUrl(list) => list.iter().find(|(url, _)| *url == call.base_url).map_or(&DOWN, |(_, script)| script),
            script => script,
        }
    }

    fn fail(&self, call: &UpstreamCall) -> Option<UpstreamError> {
        match self.script_for(call) {
            Script::Status(status) => Some(UpstreamError::Status { status: *status, body: "upstream says no".into() }),
            Script::Down => Some(UpstreamError::Unreachable("connection refused".into())),
            _ => None,
        }
    }
}

struct Chunks(std::vec::IntoIter<Result<Vec<u8>, String>>);

#[async_trait]
impl ChunkStream for Chunks {
    async fn next(&mut self) -> Option<Result<Vec<u8>, UpstreamError>> {
        self.0.next().map(|item| item.map_err(UpstreamError::Unreachable))
    }
}

struct Stalled(Option<Vec<u8>>);

#[async_trait]
impl ChunkStream for Stalled {
    async fn next(&mut self) -> Option<Result<Vec<u8>, UpstreamError>> {
        match self.0.take() {
            Some(chunk) => Some(Ok(chunk)),
            None => std::future::pending().await,
        }
    }
}

#[async_trait]
impl Upstream for FakeUpstream {
    async fn complete(&self, call: &UpstreamCall) -> Result<Value, UpstreamError> {
        self.calls.lock().unwrap().push(call.clone());
        if let Some(err) = self.fail(call) {
            return Err(err);
        }
        match self.script_for(call) {
            Script::Reply(body) => Ok(body.clone()),
            _ => Err(UpstreamError::Unreachable("the fake has no reply scripted".into())),
        }
    }

    async fn stream(&self, call: &UpstreamCall) -> Result<Box<dyn ChunkStream>, UpstreamError> {
        self.calls.lock().unwrap().push(call.clone());
        if let Some(err) = self.fail(call) {
            return Err(err);
        }
        match self.script_for(call) {
            Script::Chunks(chunks) => Ok(Box::new(Chunks(chunks.clone().into_iter()))),
            Script::Stalls(first) => Ok(Box::new(Stalled(Some(first.clone())))),
            _ => Err(UpstreamError::Unreachable("the fake has no stream scripted".into())),
        }
    }
}

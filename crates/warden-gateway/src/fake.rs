//! A scripted `Upstream` for tests, here (and not under `#[cfg(test)]`) so the integration tests can use it too.

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;

use crate::upstream::{ChunkStream, Upstream, UpstreamCall, UpstreamError};

/// What the fake does on a call.
pub enum Script {
    /// `complete` answers this JSON.
    Reply(Value),
    /// `stream` sends these chunks, then ends; an `Err` item breaks the stream there.
    Chunks(Vec<Result<Vec<u8>, String>>),
    /// Both fail with this provider status.
    Status(u16),
    /// Both fail as if the provider were down.
    Down,
}

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

    fn fail(&self) -> Option<UpstreamError> {
        match &self.script {
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

#[async_trait]
impl Upstream for FakeUpstream {
    async fn complete(&self, call: &UpstreamCall) -> Result<Value, UpstreamError> {
        self.calls.lock().unwrap().push(call.clone());
        if let Some(err) = self.fail() {
            return Err(err);
        }
        match &self.script {
            Script::Reply(body) => Ok(body.clone()),
            _ => Err(UpstreamError::Unreachable("the fake has no reply scripted".into())),
        }
    }

    async fn stream(&self, call: &UpstreamCall) -> Result<Box<dyn ChunkStream>, UpstreamError> {
        self.calls.lock().unwrap().push(call.clone());
        if let Some(err) = self.fail() {
            return Err(err);
        }
        match &self.script {
            Script::Chunks(chunks) => Ok(Box::new(Chunks(chunks.clone().into_iter()))),
            _ => Err(UpstreamError::Unreachable("the fake has no stream scripted".into())),
        }
    }
}

//! The endpoint: `POST /v1/chat/completions`, `GET /v1/models` and `GET /health`, with the instance token as the bearer key.
//!
//! Hand-written over a TCP stream (`httparse`), like the Warden API in `warden-server`: one request per connection, the
//! connection closed after the answer, plain HTTP (TLS is the reverse proxy's job for now). A call goes through
//! `Gateway::reserve`, the upstream provider and `Gateway::settle`; the caller sees the provider's answer with the
//! catalog's model name, and a streamed answer is forwarded as it comes.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::catalog::Tokens;
use crate::flow::{Gateway, GatewayError, Outcome, Reservation, ReserveRequest};
use crate::sse::{usage_of, UsageScanner};
use crate::upstream::{Upstream, UpstreamCall, UpstreamError};

const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 8 * 1024 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// An error answer in the OpenAI shape: `{"error": {"message", "type", "code"}}`.
struct ApiError {
    status: &'static str,
    kind: &'static str,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: &'static str, kind: &'static str, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { status, kind, code, message: message.into() }
    }

    fn bad_request(message: impl Into<String>) -> ApiError {
        ApiError::new("400 Bad Request", "invalid_request_error", "invalid_request", message)
    }

    fn body(&self) -> Vec<u8> {
        json!({ "error": { "message": self.message, "type": self.kind, "code": self.code } }).to_string().into_bytes()
    }
}

impl From<GatewayError> for ApiError {
    fn from(err: GatewayError) -> ApiError {
        let message = err.to_string();
        match err {
            GatewayError::Unauthorized => ApiError::new("401 Unauthorized", "invalid_request_error", "invalid_api_key", message),
            GatewayError::AccountInactive => ApiError::new("403 Forbidden", "invalid_request_error", "account_suspended", message),
            GatewayError::UnknownModel(_) => ApiError::new("404 Not Found", "invalid_request_error", "model_not_found", message),
            GatewayError::InsufficientBalance { .. } => {
                ApiError::new("402 Payment Required", "insufficient_credit", "insufficient_credit", message)
            }
            GatewayError::QuotaExceeded(_) => ApiError::new("429 Too Many Requests", "quota_exceeded", "quota_exceeded", message),
            GatewayError::DuplicateRequest => ApiError::new("409 Conflict", "invalid_request_error", "duplicate_request", message),
            GatewayError::Store(e) => {
                tracing::error!("ledger error: {e}");
                ApiError::new("500 Internal Server Error", "server_error", "internal_error", "the gateway failed; try again")
            }
        }
    }
}

fn upstream_error(err: &UpstreamError) -> ApiError {
    // The provider's own words stay in the gateway's log: they can name the gateway's account with the provider.
    tracing::warn!("upstream failed: {err}");
    ApiError::new("502 Bad Gateway", "server_error", "upstream_error", "the model provider could not answer; try again")
}

struct Request {
    method: String,
    path: String,
    token: Option<String>,
    body: Vec<u8>,
}

struct State {
    gateway: Gateway,
    upstream: Arc<dyn Upstream>,
}

pub struct GatewayServer {
    listener: TcpListener,
    state: Arc<State>,
}

impl GatewayServer {
    pub async fn bind(addr: SocketAddr, gateway: Gateway, upstream: Arc<dyn Upstream>) -> std::io::Result<GatewayServer> {
        Ok(GatewayServer { listener: TcpListener::bind(addr).await?, state: Arc::new(State { gateway, upstream }) })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Accepts connections until the task is dropped.
    pub async fn serve(self) {
        loop {
            let Ok((stream, _)) = self.listener.accept().await else { continue };
            let state = self.state.clone();
            tokio::spawn(async move {
                let mut stream = stream;
                if let Err(e) = handle(&mut stream, &state).await {
                    tracing::debug!("connection ended: {e}");
                }
            });
        }
    }
}

async fn handle(stream: &mut TcpStream, state: &State) -> std::io::Result<()> {
    let request = match tokio::time::timeout(HEAD_TIMEOUT, read_request(stream)).await {
        Ok(Ok(request)) => request,
        Ok(Err(err)) => return write_json(stream, err.status, &err.body(), &[]).await,
        Err(_) => return Ok(()),
    };
    match route(stream, state, &request).await {
        Ok(()) => Ok(()),
        Err(err) => write_json(stream, err.status, &err.body(), &[]).await,
    }
}

async fn route<S: AsyncWrite + Unpin>(stream: &mut S, state: &State, request: &Request) -> Result<(), ApiError> {
    let path = request.path.split('?').next().unwrap_or_default();
    match (request.method.as_str(), path) {
        ("GET", "/health") => write_json(stream, "200 OK", br#"{"status":"ok"}"#, &[]).await.map_err(io_error),
        ("GET", "/v1/models") => {
            state.gateway.authenticate(request.token.as_deref().unwrap_or_default()).await?;
            let data: Vec<Value> = state
                .gateway
                .catalog()
                .models
                .iter()
                .map(|m| json!({ "id": m.name, "object": "model", "created": 0, "owned_by": "warden" }))
                .collect();
            let body = json!({ "object": "list", "data": data });
            write_json(stream, "200 OK", body.to_string().as_bytes(), &[]).await.map_err(io_error)
        }
        ("POST", "/v1/chat/completions") => chat(stream, state, request).await,
        (_, "/health" | "/v1/models" | "/v1/chat/completions") => {
            Err(ApiError::new("405 Method Not Allowed", "invalid_request_error", "method_not_allowed", "this method isn't allowed here"))
        }
        _ => Err(ApiError::new("404 Not Found", "invalid_request_error", "not_found", "this gateway serves /v1/models and /v1/chat/completions")),
    }
}

/// The caller's own ceiling on the answer: `max_tokens`, or the newer `max_completion_tokens`.
fn requested_max_tokens(request: &Value) -> Option<u32> {
    ["max_tokens", "max_completion_tokens"]
        .iter()
        .find_map(|key| request.get(*key).and_then(Value::as_u64))
        .map(|n| n.min(u32::MAX as u64) as u32)
}

/// The request as the provider gets it: its own model name, the answer ceiling the hold was sized for, and (for a stream)
/// the flag that makes it report usage.
fn upstream_body(request: &Value, res: &Reservation, stream: bool) -> Value {
    let mut body = request.clone();
    body["model"] = json!(res.model.upstream_model);
    body["max_tokens"] = json!(res.max_tokens);
    if let Some(map) = body.as_object_mut() {
        map.remove("max_completion_tokens");
    }
    if stream {
        body["stream_options"] = json!({ "include_usage": true });
    }
    body
}

async fn chat<S: AsyncWrite + Unpin>(stream: &mut S, state: &State, request: &Request) -> Result<(), ApiError> {
    let body: Value = serde_json::from_slice(&request.body).map_err(|e| ApiError::bad_request(format!("the body isn't valid JSON: {e}")))?;
    let model = body.get("model").and_then(Value::as_str).ok_or_else(|| ApiError::bad_request("the request has no `model`"))?;
    // The prompt is the messages and the tools: both go upstream and both are billed as input.
    let prompt_chars = ["messages", "tools"].iter().filter_map(|k| body.get(*k)).map(|v| v.to_string().len()).sum();

    let res = state
        .gateway
        .reserve(ReserveRequest {
            token: request.token.clone().unwrap_or_default(),
            model: model.to_string(),
            prompt_chars,
            max_tokens: requested_max_tokens(&body),
            now: now(),
        })
        .await?;

    let streaming = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let upstream = state.gateway.catalog().upstream(&res.model.upstream).expect("a catalog's models name existing upstreams");
    let call = UpstreamCall { base_url: upstream.base_url.clone(), api_key: upstream.api_key.clone(), body: upstream_body(&body, &res, streaming) };

    if streaming {
        stream_answer(stream, state, &res, &call).await
    } else {
        whole_answer(stream, state, &res, &call).await
    }
}

async fn settle(state: &State, res: &Reservation, outcome: Outcome) -> Option<i64> {
    match state.gateway.settle(res, outcome, now()).await {
        Ok(priced) => Some(priced.charged),
        Err(e) => {
            // The customer already has the answer; a failed settle is for the operator to see, not for the caller.
            tracing::error!(request = %res.request_id, "settling a call failed: {e}");
            None
        }
    }
}

async fn whole_answer<S: AsyncWrite + Unpin>(stream: &mut S, state: &State, res: &Reservation, call: &UpstreamCall) -> Result<(), ApiError> {
    match state.upstream.complete(call).await {
        Ok(mut answer) => {
            let outcome = usage_of(&answer).map_or(Outcome::Unmetered, Outcome::Completed);
            let charged = settle(state, res, outcome).await;
            answer["model"] = json!(res.model.name);
            let header = charged.map(|c| ("X-Warden-Charged-Micros", c.to_string()));
            let headers: Vec<(&str, String)> = header.into_iter().collect();
            write_json(stream, "200 OK", answer.to_string().as_bytes(), &headers).await.map_err(io_error)
        }
        Err(err) => {
            settle(state, res, Outcome::Failed(Tokens::default())).await;
            Err(upstream_error(&err))
        }
    }
}

async fn stream_answer<S: AsyncWrite + Unpin>(stream: &mut S, state: &State, res: &Reservation, call: &UpstreamCall) -> Result<(), ApiError> {
    let mut chunks = match state.upstream.stream(call).await {
        Ok(chunks) => chunks,
        Err(err) => {
            settle(state, res, Outcome::Failed(Tokens::default())).await;
            return Err(upstream_error(&err));
        }
    };
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    let mut broken = stream.write_all(head.as_bytes()).await.is_err();
    let mut scan = UsageScanner::default();
    let mut forwarded = false;
    while !broken {
        match chunks.next().await {
            Some(Ok(chunk)) => {
                scan.push(&chunk);
                broken = stream.write_all(&chunk).await.is_err() || stream.flush().await.is_err();
                forwarded = true;
            }
            Some(Err(err)) => {
                tracing::warn!("the stream broke: {err}");
                broken = true;
            }
            None => break,
        }
    }
    let outcome = match (broken, scan.usage()) {
        (false, Some(used)) => Outcome::Completed(used),
        (true, Some(used)) => Outcome::Failed(used),
        // It ended or broke with no usage seen: charge the hold if anything was sent, nothing if the caller got nothing.
        (_, None) if forwarded => Outcome::Unmetered,
        (_, None) => Outcome::Failed(Tokens::default()),
    };
    settle(state, res, outcome).await;
    Ok(())
}

fn io_error(err: std::io::Error) -> ApiError {
    ApiError::new("500 Internal Server Error", "server_error", "io_error", err.to_string())
}

async fn read_request<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Result<Request, ApiError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let (method, path, token, content_length, expect_continue, head_len) = loop {
        let n = stream.read(&mut chunk).await.map_err(io_error)?;
        if n == 0 {
            return Err(ApiError::bad_request("the connection closed before the request was complete"));
        }
        buf.extend_from_slice(&chunk[..n]);
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut parsed = httparse::Request::new(&mut headers);
        match parsed.parse(&buf) {
            Ok(httparse::Status::Complete(head_len)) => {
                let header = |name: &str| {
                    parsed.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).and_then(|h| std::str::from_utf8(h.value).ok())
                };
                let token = header("authorization")
                    .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
                    .map(|t| t.trim().to_string());
                let content_length = header("content-length").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                let expect_continue = header("expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue"));
                break (
                    parsed.method.unwrap_or_default().to_string(),
                    parsed.path.unwrap_or_default().to_string(),
                    token,
                    content_length,
                    expect_continue,
                    head_len,
                );
            }
            Ok(httparse::Status::Partial) if buf.len() < MAX_HEAD => continue,
            Ok(httparse::Status::Partial) => return Err(ApiError::bad_request("the request head is too large")),
            Err(e) => return Err(ApiError::bad_request(format!("not an HTTP request: {e}"))),
        }
    };
    if content_length > MAX_BODY {
        return Err(ApiError::new("413 Payload Too Large", "invalid_request_error", "payload_too_large", "the request body is too large"));
    }
    let mut body = buf.split_off(head_len);
    if expect_continue && body.len() < content_length {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await.map_err(io_error)?;
    }
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await.map_err(io_error)?;
        if n == 0 {
            return Err(ApiError::bad_request("the connection closed before the body was complete"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Ok(Request { method, path, token, body })
}

async fn write_json<S: AsyncWrite + Unpin>(stream: &mut S, status: &str, body: &[u8], headers: &[(&str, String)]) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

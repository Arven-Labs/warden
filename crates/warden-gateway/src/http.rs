//! The endpoint: `POST /v1/chat/completions`, `GET /v1/models`, `GET /v1/balance` and `GET /health`, with the instance token as the bearer key.
//!
//! Hand-written over a TCP stream (`httparse`), like the Warden API in `warden-server`: one request per connection, the
//! connection closed after the answer. Plain HTTP by default, with TLS in front of it by a reverse proxy; or TLS on the
//! socket itself when `GatewayServer::with_tls` is given the certificate (P168 b). A call goes through
//! `Gateway::reserve`, the upstream provider and `Gateway::settle`; the caller sees the provider's answer with the
//! catalog's model name (in a streamed answer too, line by line), and a streamed answer is forwarded as it comes.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

use warden_hosted_protocol::{
    verify, BalanceResponse, TopUpRequest, TopUpResponse, UsageResponse, SIGNATURE_HEADER,
};

use crate::catalog::Tokens;
use crate::flow::{Gateway, GatewayError, Outcome, Reservation, ReserveRequest};
use crate::limits::Slots;
use crate::quota::{day_start, DAY_SECS};
use crate::ratelimit::{client_key, retry_after_secs, token_key, RateLimiter};
use crate::sse::{usage_of, ModelRewriter, UsageScanner};
use crate::store::{EntryKind, StoreError};
use crate::tls::GatewayTls;
use crate::upstream::{Upstream, UpstreamCall, UpstreamError};

const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 8 * 1024 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a client has to send the body once the limits let its request in.
const BODY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a client has to finish the TLS handshake before the connection is dropped.
const TLS_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// An error answer in the OpenAI shape: `{"error": {"message", "type", "code"}}`.
struct ApiError {
    status: &'static str,
    kind: &'static str,
    code: &'static str,
    message: String,
    /// Seconds for the `Retry-After` header, on a rate-limit refusal.
    retry_after: Option<u64>,
}

impl ApiError {
    fn new(status: &'static str, kind: &'static str, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { status, kind, code, message: message.into(), retry_after: None }
    }

    /// The rate limit said no: `wait` is when to try again. Not the spending cap, which is `quota_exceeded`.
    fn rate_limited(wait: Duration) -> ApiError {
        let mut err = ApiError::new("429 Too Many Requests", "rate_limit_error", "rate_limit_exceeded", "too many requests; slow down and try again");
        err.retry_after = Some(retry_after_secs(wait));
        err
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        self.retry_after.map(|secs| ("Retry-After", secs.to_string())).into_iter().collect()
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
    /// The request target as received, query included: it is what the site mãe signed.
    path: String,
    token: Option<String>,
    signature: Option<String>,
    body: Vec<u8>,
}

struct State {
    gateway: Gateway,
    upstream: Arc<dyn Upstream>,
    /// The secret the site mãe's calls are verified with. `None`: the account routes don't exist (404).
    admin_secret: Option<String>,
    ip_limit: RateLimiter,
    token_limit: RateLimiter,
    trust_forwarded_for: bool,
    /// Chat calls in flight, per instance token.
    inflight: Arc<Slots>,
    /// The longest a streamed answer may run; `None`: no limit.
    max_stream: Option<Duration>,
}

pub struct GatewayServer {
    listener: TcpListener,
    gateway: Gateway,
    upstream: Arc<dyn Upstream>,
    admin_secret: Option<String>,
    ip_per_min: u32,
    token_per_min: u32,
    trust_forwarded_for: bool,
    tls: Option<GatewayTls>,
    max_connections: usize,
    max_connections_per_ip: usize,
    max_inflight_per_token: usize,
    max_stream: Option<Duration>,
}

impl GatewayServer {
    /// No rate limit until `with_rate_limits` says so: the library default is off, the `serve` command turns it on.
    pub async fn bind(addr: SocketAddr, gateway: Gateway, upstream: Arc<dyn Upstream>) -> std::io::Result<GatewayServer> {
        Ok(GatewayServer {
            listener: TcpListener::bind(addr).await?,
            gateway,
            upstream,
            admin_secret: None,
            ip_per_min: 0,
            token_per_min: 0,
            trust_forwarded_for: false,
            tls: None,
            max_connections: 0,
            max_connections_per_ip: 0,
            max_inflight_per_token: 0,
            max_stream: None,
        })
    }

    /// Caps the connections open at once, in all and per client address (`0` leaves that one off). A connection over a cap is
    /// closed without an answer. The address is the connection's own, read before any header: behind a reverse proxy it is
    /// always the proxy's, so with `with_trust_forwarded_for` the per-address cap is not applied (only the total).
    pub fn with_connection_limits(mut self, total: usize, per_ip: usize) -> GatewayServer {
        self.max_connections = total;
        self.max_connections_per_ip = per_ip;
        self
    }

    /// Caps the chat calls one instance token may have in flight (`0`: no cap). Over it: `429`.
    pub fn with_inflight_limit(mut self, per_token: usize) -> GatewayServer {
        self.max_inflight_per_token = per_token;
        self
    }

    /// Cuts a streamed answer that runs longer than `max`, and settles it with the usage seen so far.
    pub fn with_max_stream(mut self, max: Duration) -> GatewayServer {
        self.max_stream = Some(max);
        self
    }

    /// Serves TLS on the socket itself: a client that doesn't open with a TLS handshake gets no answer. Without this the
    /// endpoint is plain HTTP, for a reverse proxy that does the TLS.
    pub fn with_tls(mut self, tls: GatewayTls) -> GatewayServer {
        self.tls = Some(tls);
        self
    }

    /// Limits requests a minute per client IP and per instance token (`0` leaves that one off). The site mãe's signed
    /// routes and `/health` are never limited. Over the limit: `429` with `Retry-After`.
    pub fn with_rate_limits(mut self, ip_per_min: u32, token_per_min: u32) -> GatewayServer {
        self.ip_per_min = ip_per_min;
        self.token_per_min = token_per_min;
        self
    }

    /// Takes the client's address from the last `X-Forwarded-For` entry instead of the connection. Turn it on only behind a
    /// reverse proxy that sets the header: otherwise anyone can write it and pick their own limit bucket.
    pub fn with_trust_forwarded_for(mut self, trust: bool) -> GatewayServer {
        self.trust_forwarded_for = trust;
        self
    }

    /// Turns on the signed account routes (`/v1/accounts/{id}/credits`, `/balance`, `/usage`) for the site mãe.
    pub fn with_admin_secret(mut self, secret: &str) -> GatewayServer {
        self.admin_secret = Some(secret.to_string());
        self
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Accepts connections until the task is dropped.
    pub async fn serve(self) {
        let state = Arc::new(State {
            gateway: self.gateway,
            upstream: self.upstream,
            admin_secret: self.admin_secret,
            ip_limit: RateLimiter::new(self.ip_per_min),
            token_limit: RateLimiter::new(self.token_per_min),
            trust_forwarded_for: self.trust_forwarded_for,
            inflight: Slots::new(0, self.max_inflight_per_token),
            max_stream: self.max_stream,
        });
        let per_ip = if self.trust_forwarded_for { 0 } else { self.max_connections_per_ip };
        let connections = Slots::new(self.max_connections, per_ip);
        let tls = self.tls;
        loop {
            let Ok((stream, peer)) = self.listener.accept().await else { continue };
            let Some(slot) = connections.try_acquire(&client_key(peer.ip(), None, false)) else {
                tracing::debug!(%peer, "too many open connections; closing this one");
                continue;
            };
            let state = state.clone();
            let tls = tls.clone();
            // The handshake is in the connection's own task, so a client that is slow at it holds up nobody else's accept.
            tokio::spawn(async move {
                let _slot = slot;
                let result = match tls {
                    Some(tls) => match tokio::time::timeout(TLS_ACCEPT_TIMEOUT, tls.acceptor.accept(stream)).await {
                        Ok(Ok(mut secure)) => {
                            let result = handle(&mut secure, &state, peer.ip()).await;
                            // `close_notify`, so the client can tell the end of the answer from a cut connection.
                            let _ = secure.shutdown().await;
                            result
                        }
                        Ok(Err(e)) => {
                            tracing::debug!(%peer, "TLS handshake failed: {e}");
                            return;
                        }
                        Err(_) => {
                            tracing::debug!(%peer, "TLS handshake timed out");
                            return;
                        }
                    },
                    None => {
                        let mut stream = stream;
                        handle(&mut stream, &state, peer.ip()).await
                    }
                };
                if let Err(e) = result {
                    tracing::debug!("connection ended: {e}");
                }
            });
        }
    }
}

async fn handle<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, state: &State, peer: IpAddr) -> std::io::Result<()> {
    let head = match tokio::time::timeout(HEAD_TIMEOUT, read_head(stream)).await {
        Ok(Ok(head)) => head,
        Ok(Err(err)) => return write_json(stream, err.status, &err.body(), &err.headers()).await,
        Err(_) => return Ok(()),
    };
    // The limits see the head alone: a refused request's body is never read (and with `Expect: 100-continue` never sent).
    if let Err(err) = limit(state, &head, peer) {
        return write_json(stream, err.status, &err.body(), &err.headers()).await;
    }
    let request = match tokio::time::timeout(BODY_TIMEOUT, read_body(stream, head)).await {
        Ok(Ok(request)) => request,
        Ok(Err(err)) => return write_json(stream, err.status, &err.body(), &err.headers()).await,
        Err(_) => return Ok(()),
    };
    match route(stream, state, &request).await {
        Ok(()) => Ok(()),
        Err(err) => write_json(stream, err.status, &err.body(), &err.headers()).await,
    }
}

/// The rate limits: by client address first (a refusal there costs the token's bucket nothing), then by token. `/health` and
/// the site mãe's signed account routes are exempt: the site calls from one address, and a monitor must always get an answer.
fn limit(state: &State, head: &Head, peer: IpAddr) -> Result<(), ApiError> {
    let path = head.path.split('?').next().unwrap_or_default();
    if path == "/health" || path.starts_with("/v1/accounts/") {
        return Ok(());
    }
    let now = now_ms();
    let ip = client_key(peer, head.forwarded_for.as_deref(), state.trust_forwarded_for);
    state.ip_limit.check(&ip, now).map_err(ApiError::rate_limited)?;
    if let Some(token) = &head.token {
        state.token_limit.check(&token_key(token), now).map_err(ApiError::rate_limited)?;
    }
    Ok(())
}

async fn route<S: AsyncWrite + Unpin>(stream: &mut S, state: &State, request: &Request) -> Result<(), ApiError> {
    let path = request.path.split('?').next().unwrap_or_default();
    if let Some(rest) = path.strip_prefix("/v1/accounts/") {
        return account_route(stream, state, request, rest).await;
    }
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
        ("GET", "/v1/balance") => {
            // The instance's own balance, for its usage screen: the instance token is all it holds.
            let account = state.gateway.authenticate(request.token.as_deref().unwrap_or_default()).await?;
            let id = account.id;
            let (balance, available) = on_store(state, move |store| Ok((store.balance(&id)?, store.available(&id, now())?))).await?;
            let body = serde_json::to_vec(&BalanceResponse { balance_micro_usd: balance, available_micro_usd: available }).expect("plain data");
            write_json(stream, "200 OK", &body, &[]).await.map_err(io_error)
        }
        ("POST", "/v1/chat/completions") => chat(stream, state, request).await,
        (_, "/health" | "/v1/models" | "/v1/balance" | "/v1/chat/completions") => {
            Err(ApiError::new("405 Method Not Allowed", "invalid_request_error", "method_not_allowed", "this method isn't allowed here"))
        }
        _ => Err(ApiError::new("404 Not Found", "invalid_request_error", "not_found", "this gateway serves /v1/models and /v1/chat/completions")),
    }
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> ApiError {
        match err {
            StoreError::UnknownAccount => ApiError::new("404 Not Found", "invalid_request_error", "account_not_found", "no such account"),
            other => {
                tracing::error!("ledger error: {other}");
                ApiError::new("500 Internal Server Error", "server_error", "internal_error", "the gateway failed; try again")
            }
        }
    }
}

/// Runs a ledger call off the async workers.
async fn on_store<T: Send + 'static>(
    state: &State,
    f: impl FnOnce(&crate::store::Store) -> Result<T, StoreError> + Send + 'static,
) -> Result<T, ApiError> {
    let store = state.gateway.store().clone();
    match tokio::task::spawn_blocking(move || f(&store)).await {
        Ok(result) => result.map_err(ApiError::from),
        Err(_) => Err(ApiError::new("500 Internal Server Error", "server_error", "internal_error", "the gateway failed; try again")),
    }
}

/// The most days a usage query may span.
const MAX_USAGE_DAYS: i64 = 92;

/// `YYYY-MM-DD` → the unix second at the start of that UTC day.
fn parse_day(text: &str) -> Option<i64> {
    let mut parts = text.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * DAY_SECS)
}

fn query_param<'a>(path: &'a str, name: &str) -> Option<&'a str> {
    path.split_once('?')?.1.split('&').find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

/// `/v1/accounts/{id}/credits`, `/balance` and `/usage`: the site mãe's calls, signed with the admin secret.
async fn account_route<S: AsyncWrite + Unpin>(stream: &mut S, state: &State, request: &Request, rest: &str) -> Result<(), ApiError> {
    let not_found = || ApiError::new("404 Not Found", "invalid_request_error", "not_found", "this gateway serves /v1/models and /v1/chat/completions");
    let Some(secret) = &state.admin_secret else { return Err(not_found()) };
    verify(secret, request.signature.as_deref(), &request.method, &request.path, &request.body, now()).map_err(|e| {
        ApiError::new("401 Unauthorized", "invalid_request_error", "invalid_signature", format!("{e} ({SIGNATURE_HEADER})"))
    })?;

    let Some((account, action)) = rest.split_once('/') else { return Err(not_found()) };
    let account = account.to_string();
    let reply = |body: Vec<u8>| async move { write_json(stream, "200 OK", &body, &[]).await.map_err(io_error) };
    match (request.method.as_str(), action) {
        ("POST", "credits") => {
            let top_up: TopUpRequest = serde_json::from_slice(&request.body).map_err(|e| ApiError::bad_request(format!("the body isn't a top-up: {e}")))?;
            if top_up.amount_micro_usd <= 0 {
                return Err(ApiError::bad_request("`amount_micro_usd` must be more than zero"));
            }
            if top_up.payment_ref.is_empty() || top_up.payment_ref.len() > 128 {
                return Err(ApiError::bad_request("`payment_ref` must be 1 to 128 characters"));
            }
            let (applied, balance) = on_store(state, move |store| {
                let applied = store.credit(&account, EntryKind::Topup, top_up.amount_micro_usd, Some(&top_up.payment_ref), "site top-up", now())?;
                Ok((applied, store.balance(&account)?))
            })
            .await?;
            reply(serde_json::to_vec(&TopUpResponse { applied, balance_micro_usd: balance }).expect("plain data")).await
        }
        ("GET", "balance") => {
            let (balance, available) = on_store(state, move |store| Ok((store.balance(&account)?, store.available(&account, now())?))).await?;
            reply(serde_json::to_vec(&BalanceResponse { balance_micro_usd: balance, available_micro_usd: available }).expect("plain data")).await
        }
        ("GET", "usage") => {
            let (Some(from), Some(to)) = (query_param(&request.path, "from").and_then(parse_day), query_param(&request.path, "to").and_then(parse_day)) else {
                return Err(ApiError::bad_request("`from` and `to` are required, as YYYY-MM-DD"));
            };
            if to < from || (to - from) / DAY_SECS >= MAX_USAGE_DAYS {
                return Err(ApiError::bad_request(format!("`from` must not be after `to`, and the range at most {MAX_USAGE_DAYS} days")));
            }
            let days = on_store(state, move |store| {
                store.balance(&account)?; // unknown account → 404
                store.usage_by_day(&account, from, day_start(to) + DAY_SECS)
            })
            .await?;
            reply(serde_json::to_vec(&UsageResponse { days }).expect("plain data")).await
        }
        (_, "credits" | "balance" | "usage") => {
            Err(ApiError::new("405 Method Not Allowed", "invalid_request_error", "method_not_allowed", "this method isn't allowed here"))
        }
        _ => Err(not_found()),
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
    // Before the reservation, so a refusal costs the account nothing. Held until the answer is done.
    let _inflight = state
        .inflight
        .try_acquire(&token_key(request.token.as_deref().unwrap_or_default()))
        .ok_or_else(|| ApiError::rate_limited(Duration::from_secs(1)))?;
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
    let mut rewriter = ModelRewriter::new(&res.model.name);
    let mut forwarded = false;
    let deadline = state.max_stream.map(|max| tokio::time::Instant::now() + max);
    while !broken {
        let next = match deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, chunks.next()).await {
                Ok(next) => next,
                Err(_) => {
                    tracing::warn!("the stream ran past its time limit; cutting it");
                    broken = true;
                    break;
                }
            },
            None => chunks.next().await,
        };
        match next {
            Some(Ok(chunk)) => {
                // The usage is read off the provider's own bytes; what goes to the caller carries the catalog's model name.
                scan.push(&chunk);
                let out = rewriter.push(&chunk);
                if !out.is_empty() {
                    broken = stream.write_all(&out).await.is_err() || stream.flush().await.is_err();
                    forwarded = true;
                }
            }
            Some(Err(err)) => {
                tracing::warn!("the stream broke: {err}");
                broken = true;
            }
            None => {
                // A last line the provider ended without a newline.
                let tail = rewriter.finish();
                if !tail.is_empty() {
                    broken = stream.write_all(&tail).await.is_err() || stream.flush().await.is_err();
                    forwarded = true;
                }
                break;
            }
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

/// The request line and headers, read before the body so the limits can refuse a request without taking its body in.
struct Head {
    method: String,
    path: String,
    token: Option<String>,
    signature: Option<String>,
    /// The `X-Forwarded-For` header as received, for the client's address behind a reverse proxy.
    forwarded_for: Option<String>,
    content_length: usize,
    expect_continue: bool,
    /// The body bytes that arrived in the same reads as the head.
    leftover: Vec<u8>,
}

async fn read_head<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Result<Head, ApiError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let (method, path, token, signature, forwarded_for, content_length, expect_continue, head_len) = loop {
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
                let signature = header(SIGNATURE_HEADER).map(str::to_string);
                let forwarded_for = header("x-forwarded-for").map(str::to_string);
                let content_length = header("content-length").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                let expect_continue = header("expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue"));
                break (
                    parsed.method.unwrap_or_default().to_string(),
                    parsed.path.unwrap_or_default().to_string(),
                    token,
                    signature,
                    forwarded_for,
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
    let leftover = buf.split_off(head_len);
    Ok(Head { method, path, token, signature, forwarded_for, content_length, expect_continue, leftover })
}

/// Reads the body the head promised. Only called for a request the limits let through: with `Expect: 100-continue` the
/// client is told to go ahead here, so one that was refused never sends it.
async fn read_body<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, head: Head) -> Result<Request, ApiError> {
    let Head { method, path, token, signature, content_length, expect_continue, leftover: mut body, .. } = head;
    if expect_continue && body.len() < content_length {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await.map_err(io_error)?;
    }
    let mut chunk = [0u8; 4096];
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await.map_err(io_error)?;
        if n == 0 {
            return Err(ApiError::bad_request("the connection closed before the body was complete"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Ok(Request { method, path, token, signature, body })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_parse_to_the_start_of_the_utc_day() {
        assert_eq!(parse_day("1970-01-01"), Some(0));
        assert_eq!(parse_day("2000-02-29"), Some(951_782_400));
        assert_eq!(parse_day("2026-10-22"), Some(1_792_627_200));
        assert_eq!(parse_day("2027-01-15"), Some(1_799_971_200));
    }

    #[test]
    fn malformed_days_are_refused() {
        for bad in ["", "2026-10", "2026-10-22-1", "26-10-22", "2026-13-01", "2026-00-10", "2026-10-32", "2026/10/22", "abcd-10-22"] {
            assert_eq!(parse_day(bad), None, "{bad}");
        }
    }

    #[test]
    fn query_parameters_are_found_by_name() {
        let path = "/v1/accounts/a/usage?from=2026-10-01&to=2026-10-22";
        assert_eq!(query_param(path, "from"), Some("2026-10-01"));
        assert_eq!(query_param(path, "to"), Some("2026-10-22"));
        assert_eq!(query_param(path, "other"), None);
        assert_eq!(query_param("/v1/accounts/a/usage", "from"), None);
        assert_eq!(query_param("/x?tomorrow=1", "to"), None, "a longer name is not a match");
    }
}

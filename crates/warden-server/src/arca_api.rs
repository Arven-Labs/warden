//! The arca storage routes (`/arca/v1/`) on the hub: a device that is paired and approved keeps encrypted objects here, so
//! its other devices can pull them (arca's phase 2b). The contract itself (`PUT`/`GET`/`HEAD` of an object, the paged
//! list) is `arca_core::server`; this module adds what a host has to: who may call, whose folder it is, and how much one
//! caller can cost.
//!
//! The order of a call matters, because the caller is a stranger until it has proven itself (same reasoning as
//! `webhooks.rs`):
//! 1. the target is short, then the proof: `Authorization: Bearer <device token>`, the token the pairing handshake issued.
//!    Unknown, missing or malformed all answer the same `401`, after `WRONG_KEY_DELAY`; a device that is pending or
//!    revoked is `403`. The registry is read on every call, so a revocation holds at once;
//! 2. the room: one request of this owner and one of the hub at a time each take a permit, or the answer is `503` with
//!    `Retry-After` (no queue: a queue would hold the connections without anyone paying for them). The permit is kept
//!    until the answer is written, so a body arriving slowly counts as work in progress;
//! 3. the body of an object `PUT`, only now: `Content-Length` required, over `arca_core::server::body_limit` is `413` before a
//!    byte is read, and an object that would take the owner past `max_owner_bytes` is `507`. Everything else (a `PUT` on the
//!    list, an id that is not one, an unknown route) goes to arca with no body, so its status is arca's, not a misleading
//!    `413`; a `GET` or `HEAD` with a body is refused;
//! 4. the work: arca's `handle` hashes the whole object (up to 256 MiB, about 0.7 s of CPU), so it runs on the blocking
//!    pool, never on the threads that also serve the WebSocket and the other routes.
//!
//! One folder per owner under the arca directory: `owner` for the root and `member-<sha256 of the user name>` for a member,
//! never the name itself, so what a user can type is never part of a path.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arca_core::disk::DiskStore;
use arca_core::server::{self, Reply};
use arca_core::store::BlobStore;
use arca_core::ObjectId;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::arca_pairing::{self, Pairing, Route};
use crate::attempt_limit::Reservation;
use crate::device_registry::{PairingStatus, PairingStore};
use crate::settings::WRONG_KEY_DELAY;
use crate::web_ui::{response_head, write_response, RequestHead};

/// Every route of the arca API starts with this.
pub const ARCA_PREFIX: &str = "/arca/v1/";

/// The longest target a call may have. The longest real one (`/arca/v1/objects?kind=manifest&after=<64 hex>&limit=1000`)
/// is about 110 bytes.
const MAX_TARGET_BYTES: usize = 512;

/// The slowest an upload may go, in bytes per second, counted from the first byte of the body. Below it the hub hangs up
/// (`408`): without a floor, a byte every few seconds would hold a request (and its permits) for as long as the caller liked.
/// About 0.5 Mbit/s: a slower line than that cannot upload to this hub.
const MIN_UPLOAD_RATE: u64 = 64 * 1024;

/// The pieces a body is read in and an answer is written in. A caller is believed only as far as it has shown it: the hub
/// never sets aside memory for what has not arrived, and never waits on a whole answer at once.
const CHUNK: usize = 64 * 1024;

/// What a file really takes on disk, whatever its size: a 36-byte object costs a whole block. The quota counts blocks.
const BLOCK: u64 = 4096;

fn disk_cost(len: u64) -> u64 {
    len.div_ceil(BLOCK).max(1) * BLOCK
}

/// What one owner (and the hub as a whole) may cost the machine. An approved device is trusted with its owner's data, not
/// with the hub's disk, memory or CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArcaLimits {
    /// What one owner's objects may take on disk, each rounded up to whole 4 KiB blocks (a 36-byte object is a block).
    /// A `PUT` that would go over answers `507`. Checked against the size the caller
    /// declares before the body is read, so requests of one owner racing each other can pass it by at most
    /// `max_concurrent_per_owner` objects.
    pub max_owner_bytes: u64,
    /// Requests of one owner being served at once (the body counts from the moment it is accepted).
    pub max_concurrent_per_owner: usize,
    /// Requests being served at once on the whole hub.
    pub max_concurrent: usize,
    /// How long a caller may leave the hub waiting, for the next piece of its body or to take the next piece of the answer.
    /// An upload also has to keep above `MIN_UPLOAD_RATE`, so trickling a byte just inside this does not hold a request.
    pub io_idle_timeout: Duration,
    /// How long a pairing offer, and a grant waiting to be read, stay valid (`arca_pairing`).
    pub pairing_ttl: Duration,
    /// Pairing offers of one owner waiting at once; a new one past it answers `507`.
    pub max_pending_offers: usize,
}

impl Default for ArcaLimits {
    fn default() -> Self {
        Self {
            max_owner_bytes: 10 * 1024 * 1024 * 1024,
            max_concurrent_per_owner: 4,
            max_concurrent: 16,
            io_idle_timeout: Duration::from_secs(10),
            pairing_ttl: Duration::from_secs(600),
            max_pending_offers: 8,
        }
    }
}

/// Where the objects live and what they may cost.
#[derive(Debug, Clone)]
pub struct ArcaConfig {
    pub dir: PathBuf,
    pub limits: ArcaLimits,
}

/// What the routes need from the hub. Cheap to clone; every connection shares the same permits.
#[derive(Clone)]
pub(crate) struct ArcaContext {
    dir: Arc<PathBuf>,
    limits: ArcaLimits,
    devices_path: Arc<PathBuf>,
    global: Arc<Semaphore>,
    owners: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    /// One lock per owner for the pairing files (`arca_pairing`).
    pairing_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl ArcaContext {
    pub(crate) fn new(config: ArcaConfig, devices_path: Arc<PathBuf>) -> Self {
        Self {
            dir: Arc::new(config.dir),
            limits: config.limits,
            devices_path,
            global: Arc::new(Semaphore::new(config.limits.max_concurrent)),
            owners: Arc::default(),
            pairing_locks: Arc::default(),
        }
    }

    fn pairing_lock(&self, folder: &str) -> Arc<Mutex<()>> {
        let mut locks = self.pairing_locks.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        locks.entry(folder.to_string()).or_default().clone()
    }

    fn owner_semaphore(&self, folder: &str) -> Arc<Semaphore> {
        let mut owners = self.owners.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        owners.entry(folder.to_string()).or_insert_with(|| Arc::new(Semaphore::new(self.limits.max_concurrent_per_owner))).clone()
    }
}

/// The folder name of whoever owns the device: fixed shapes only.
fn owner_folder(user: Option<&str>) -> String {
    match user {
        None => "owner".to_string(),
        Some(user) => format!("member-{}", Sha256::digest(user.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>()),
    }
}

/// A refusal, or the answer of arca, ready to be written.
struct Answer {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    retry_after: bool,
}

impl Answer {
    fn text(status: u16, message: &'static str) -> Self {
        Self { status, content_type: "text/plain; charset=utf-8", body: message.as_bytes().to_vec(), retry_after: false }
    }
}

impl From<Reply> for Answer {
    fn from(reply: Reply) -> Self {
        Self { status: reply.status, content_type: reply.content_type, body: reply.body, retry_after: false }
    }
}

fn status_line(status: u16) -> &'static str {
    match status {
        200 => "200 OK",
        204 => "204 No Content",
        400 => "400 Bad Request",
        401 => "401 Unauthorized",
        403 => "403 Forbidden",
        404 => "404 Not Found",
        405 => "405 Method Not Allowed",
        408 => "408 Request Timeout",
        409 => "409 Conflict",
        411 => "411 Length Required",
        413 => "413 Payload Too Large",
        414 => "414 URI Too Long",
        503 => "503 Service Unavailable",
        507 => "507 Insufficient Storage",
        _ => "500 Internal Server Error",
    }
}

/// Answers one request under `ARCA_PREFIX`; the connection is done afterwards.
/// `true`: the token was wrong or missing, a guess the caller counts (P142). `attempt` is released as soon as the token is
/// accepted, not when a long upload ends.
pub(crate) async fn serve<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, head: &RequestHead, ctx: &ArcaContext, attempt: &Reservation) -> std::io::Result<bool> {
    let owner = match authenticate(head, ctx).await {
        Ok(owner) => owner,
        Err(answer) => {
            send(stream, head, &answer).await?;
            return Ok(answer.status == 401);
        }
    };
    attempt.release();
    // Kept until the answer is written: a slow body, and a big answer on its way out, are both work in progress.
    let _permits = match acquire(ctx, &owner) {
        Ok(permits) => permits,
        Err(answer) => {
            send(stream, head, &answer).await?;
            return Ok(false);
        }
    };
    let answer = call(stream, head, ctx, &owner).await;
    send_within(stream, head, &answer, ctx.limits.io_idle_timeout).await?;
    Ok(false)
}

/// `send` for an answer that may be big and go to someone who reads it slowly or not at all: the body goes out in pieces, each
/// one with `idle` to be taken, or the connection is dropped and the permits come back.
async fn send_within<S: AsyncWrite + Unpin>(stream: &mut S, head: &RequestHead, answer: &Answer, idle: Duration) -> std::io::Result<()> {
    let timed_out = || std::io::Error::new(std::io::ErrorKind::TimedOut, "the caller stopped reading the answer");
    if head.method == "HEAD" || answer.body.len() <= CHUNK {
        return tokio::time::timeout(idle, send(stream, head, answer)).await.map_err(|_| timed_out())?;
    }
    let extra: &[(&str, &str)] = if answer.retry_after { &[("Retry-After", "1")] } else { &[] };
    let preamble = response_head(status_line(answer.status), extra, answer.content_type, answer.body.len());
    tokio::time::timeout(idle, stream.write_all(preamble.as_bytes())).await.map_err(|_| timed_out())??;
    for piece in answer.body.chunks(CHUNK) {
        tokio::time::timeout(idle, stream.write_all(piece)).await.map_err(|_| timed_out())??;
    }
    tokio::time::timeout(idle, async {
        stream.flush().await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| timed_out())?
}

async fn send<S: AsyncWrite + Unpin>(stream: &mut S, head: &RequestHead, answer: &Answer) -> std::io::Result<()> {
    let extra: &[(&str, &str)] = if answer.retry_after { &[("Retry-After", "1")] } else { &[] };
    write_response(stream, status_line(answer.status), extra, answer.content_type, &answer.body, head.method == "HEAD").await
}

/// Whose folder this call works in.
struct Owner {
    folder: String,
    /// The device whose token made the call.
    device_id: String,
}

/// The target, then the proof. Anything but an approved device's token is refused here, before a body is looked at.
async fn authenticate(head: &RequestHead, ctx: &ArcaContext) -> Result<Owner, Answer> {
    if head.path.len() > MAX_TARGET_BYTES {
        return Err(Answer::text(414, "the target is too long"));
    }
    let token = head.header("authorization").and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))).map(str::trim);
    let holder = match token {
        Some(token) => PairingStore::new(ctx.devices_path.as_ref().clone()).find_by_token(token).map_err(|_| Answer::text(500, "could not read the devices"))?,
        None => None,
    };
    let Some(holder) = holder else {
        tokio::time::sleep(WRONG_KEY_DELAY).await;
        return Err(Answer::text(401, "missing or unknown device token"));
    };
    match holder.status {
        PairingStatus::Approved => Ok(Owner { folder: owner_folder(holder.user.as_deref()), device_id: holder.device_id }),
        _ => Err(Answer::text(403, "this device is not approved")),
    }
}

/// The owner's permit and the hub's. No room: `503`, at once.
fn acquire(ctx: &ArcaContext, owner: &Owner) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit), Answer> {
    let busy = || Answer { status: 503, content_type: "text/plain; charset=utf-8", body: b"too many requests at once, try again".to_vec(), retry_after: true };
    let owner_permit = ctx.owner_semaphore(&owner.folder).try_acquire_owned().map_err(|_| busy())?;
    let hub_permit = ctx.global.clone().try_acquire_owned().map_err(|_| busy())?;
    Ok((owner_permit, hub_permit))
}

async fn call<S: AsyncRead + Unpin>(stream: &mut S, head: &RequestHead, ctx: &ArcaContext, owner: &Owner) -> Answer {
    let (method, target) = (head.method.clone(), head.path.clone());
    if let Some(route) = arca_pairing::route(&method, &target) {
        return pairing_call(stream, head, ctx, owner, route).await;
    }
    let dir = ctx.dir.join(&owner.folder);
    let body = if server::body_limit(&method, &target) > 0 {
        let length = match declared_length(head) {
            Ok(length) => length,
            Err(answer) => return answer,
        };
        if length > server::body_limit(&method, &target) {
            return Answer::text(413, "the object is over the size cap");
        }
        if let Some(refusal) = over_quota(&dir, &target, length, ctx.limits.max_owner_bytes).await {
            return refusal;
        }
        match read_body(stream, head, length, ctx.limits.io_idle_timeout).await {
            Ok(body) => body,
            Err(answer) => return answer,
        }
    } else {
        if matches!(method.as_str(), "GET" | "HEAD") && has_body(head) {
            return Answer::text(400, "this call takes no body");
        }
        Vec::new()
    };
    let work = async move {
        match DiskStore::open(dir) {
            Ok(store) => Answer::from(server::handle(&store, &method, &target, &body).await),
            Err(_) => Answer::text(503, "storage unavailable"),
        }
    };
    run_blocking(work).await.unwrap_or_else(|| Answer::text(500, "the request failed"))
}

/// A pairing call (`arca_pairing`): same order as an object call, the body only after the proof and the permits, and a body
/// over `MAX_BODY_BYTES` is `413` before a byte is read. The files are touched on the blocking pool.
async fn pairing_call<S: AsyncRead + Unpin>(stream: &mut S, head: &RequestHead, ctx: &ArcaContext, owner: &Owner, route: Result<Route, u16>) -> Answer {
    let route = match route {
        Ok(route) => route,
        Err(404) => return Answer::text(404, "no such pairing route"),
        Err(_) => return Answer::text(405, "that method is not for this pairing route"),
    };
    let takes_body = matches!(route, Route::Offer | Route::Grant(..));
    let body = if takes_body {
        let length = match declared_length(head) {
            Ok(length) => length,
            Err(answer) => return answer,
        };
        if length > arca_pairing::MAX_BODY_BYTES {
            return Answer::text(413, "the body is over the size cap");
        }
        match read_body(stream, head, length, ctx.limits.io_idle_timeout).await {
            Ok(body) => body,
            Err(answer) => return answer,
        }
    } else if has_body(head) {
        return Answer::text(400, "this call takes no body");
    } else {
        Vec::new()
    };
    let devices = PairingStore::new(ctx.devices_path.as_ref().clone());
    let pairing = Pairing::new(ctx.dir.join(format!("{}.pairing", owner.folder)), ctx.limits.pairing_ttl, ctx.limits.max_pending_offers, devices, ctx.pairing_lock(&owner.folder));
    let device_id = owner.device_id.clone();
    let work = async move {
        match route {
            Route::Offer => pairing.put_offer(&device_id, &body),
            Route::Offers => pairing.list_offers(),
            Route::Grant(to, saw) => pairing.put_grant(&device_id, &to, saw.as_deref(), &body),
            Route::TakeGrant => pairing.take_grant(&device_id),
        }
    };
    match run_blocking(work).await {
        Some(done) => Answer { status: done.status, content_type: done.content_type, body: done.body, retry_after: false },
        None => Answer::text(500, "the request failed"),
    }
}

/// Runs `work` on the blocking pool (its CPU-heavy parts must not take a thread of the async executor), driving it with the
/// runtime's handle. `None`: the task panicked.
async fn run_blocking<T: Send + 'static>(work: impl Future<Output = T> + Send + 'static) -> Option<T> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || handle.block_on(work)).await.ok()
}

fn has_body(head: &RequestHead) -> bool {
    head.header("transfer-encoding").is_some() || head.header("content-length").is_some_and(|v| v.trim() != "0")
}

fn declared_length(head: &RequestHead) -> Result<u64, Answer> {
    let needs_length = || Answer::text(411, "send the body with a Content-Length");
    if head.header("transfer-encoding").is_some() {
        return Err(needs_length());
    }
    head.header("content-length").ok_or_else(needs_length)?.trim().parse().map_err(|_| Answer::text(400, "Content-Length isn't a number"))
}

/// The body, `length` bytes of it, read in pieces. Each piece has to arrive within `idle`, plus the time `MIN_UPLOAD_RATE`
/// allows for what came before it: the hub waits for a caller that is sending, not for one that is only holding the line.
async fn read_body<S: AsyncRead + Unpin>(stream: &mut S, head: &RequestHead, length: u64, idle: Duration) -> Result<Vec<u8>, Answer> {
    // `length` is at most the object cap here, so it fits.
    let length = usize::try_from(length).map_err(|_| Answer::text(413, "the object is over the size cap"))?;
    // Reserved, not filled: the pages are only touched as the bytes arrive, so a caller that declares 256 MiB and sends nothing
    // costs the hub nothing but the address space.
    let mut body = Vec::with_capacity(length);
    let arrived = &head.raw[head.body_start.min(head.raw.len())..];
    body.extend_from_slice(&arrived[..arrived.len().min(length)]);
    let (started, mut piece) = (std::time::Instant::now(), vec![0u8; CHUNK.min(length.max(1))]);
    while body.len() < length {
        let allowed = idle + Duration::from_secs_f64(body.len() as f64 / MIN_UPLOAD_RATE as f64);
        let Some(left) = allowed.checked_sub(started.elapsed()) else {
            return Err(Answer::text(408, "the body is arriving too slowly"));
        };
        let want = piece.len().min(length - body.len());
        match tokio::time::timeout(left, stream.read(&mut piece[..want])).await {
            Err(_) => return Err(Answer::text(408, "the body is arriving too slowly")),
            Ok(Err(_)) | Ok(Ok(0)) => return Err(Answer::text(400, "the connection closed before the whole body arrived")),
            Ok(Ok(n)) => body.extend_from_slice(&piece[..n]),
        }
    }
    Ok(body)
}

/// `507` when storing the object `target` names would take the owner past the quota. An object already kept costs nothing
/// (sending it again is how a `PUT` is retried).
async fn over_quota(dir: &Path, target: &str, length: u64, max_owner_bytes: u64) -> Option<Answer> {
    let id: ObjectId = target.strip_prefix("/arca/v1/objects/")?.parse().ok()?;
    let dir = dir.to_path_buf();
    let checked = run_blocking(async move {
        let store = DiskStore::open(&dir).map_err(|_| ())?;
        if store.has(&id).await.map_err(|_| ())? {
            return Ok(false);
        }
        Ok::<_, ()>(used_bytes(&dir).map_err(|_| ())?.saturating_add(disk_cost(length)) > max_owner_bytes)
    })
    .await;
    match checked {
        Some(Ok(false)) => None,
        Some(Ok(true)) => Some(Answer::text(507, "this owner has no room for more objects")),
        _ => Some(Answer::text(503, "storage unavailable")),
    }
}

/// What the owner's objects take on disk: every file in `blob/` and `manifest/`, each rounded up to whole blocks. One
/// `read_dir` of each, the same order of cost as listing them.
fn used_bytes(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    for kind in ["blob", "manifest"] {
        for entry in std::fs::read_dir(dir.join(kind))? {
            let meta = entry?.metadata()?;
            if meta.is_file() {
                total = total.saturating_add(disk_cost(meta.len()));
            }
        }
    }
    Ok(total)
}

//! The orchestrator's HTTP side of the contract with the site mãe (`HOSPEDAGEM.md` B1), signed with the shared secret.
//!
//!   PUT    /v1/instances/{customer_id}            provision (or confirm); body = `{image, memory_mb?, cpus?, config?}`
//!   GET    /v1/instances/{customer_id}            state, image, health, and the delete deadline if there is one
//!   POST   /v1/instances/{customer_id}:suspend    stop it, keep the data
//!   POST   /v1/instances/{customer_id}:resume     start it again (also takes back a scheduled delete)
//!   POST   /v1/instances/{customer_id}:export     take an encrypted backup now; answers its file name
//!   DELETE /v1/instances/{customer_id}            schedule the delete; the data stays for the retention period
//!
//! One handler takes every request, and it checks the signature before it looks at the path, so a caller without the secret
//! can't tell a route that exists from one that doesn't (it is a 401 either way). The signature covers the method, the path
//! and the body (`warden-hosted-protocol`), so a captured call is good for that call only. The route never erases data:
//! `DELETE` schedules, and only `InstanceManager::purge_expired`, run by the operator, erases it after the deadline.
//! The answers carry no pairing key and no starting config.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Request, State as AxumState};
use axum::http::{header::CONTENT_TYPE, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde::{Deserialize, Serialize};
use warden_hosted_protocol::{verify_any, ErrorBody, SIGNATURE_HEADER};
use warden_server_protocol::HubStatus;

use crate::instance::{validate_customer_id, Instance, InstanceSpec, State};
use crate::manager::InstanceManager;
use crate::runtime::ContainerRuntime;
use crate::status::StatusSource;

/// The biggest body read. The starting config is the largest thing a call carries, and a few KiB is plenty.
const MAX_BODY: usize = 64 * 1024;

pub struct ApiConfig {
    /// The secrets a call may be signed with: the current one, and the previous while it is being rotated.
    pub secrets: Vec<String>,
    /// How long the data of a deleted instance is kept before `purge_expired` may erase it.
    pub retention: Duration,
    /// How many backups of a customer stay after an export (older ones are deleted once a new one succeeds).
    pub keep_backups: usize,
}

struct Api<R: ContainerRuntime, S: StatusSource> {
    manager: Arc<InstanceManager<R>>,
    status: Arc<S>,
    config: ApiConfig,
}

pub fn router<R, S>(manager: Arc<InstanceManager<R>>, status: Arc<S>, config: ApiConfig) -> Router
where
    R: ContainerRuntime + 'static,
    S: StatusSource + 'static,
{
    Router::new().fallback(handle::<R, S>).with_state(Arc::new(Api { manager, status, config }))
}

/// The shortest secret `serve` takes: long enough that a guess is not a way in.
pub const MIN_SECRET_LEN: usize = 32;

/// The secrets a call may be signed with, current first, after checking them. A missing, blank or short secret is refused
/// (`serve` would otherwise run with next to no authentication), and so is a previous one equal to the current. The
/// error never repeats a secret.
pub fn validate_secrets(current: &str, previous: Option<&str>) -> anyhow::Result<Vec<String>> {
    let long_enough = |secret: &str| secret.trim().len() >= MIN_SECRET_LEN;
    // Refused, not trimmed: the other side signs with the string it has, so a trimmed copy here would be a different
    // secret and every call would be a 401 with no hint why. A pasted final newline is the usual cause.
    let bare = |secret: &str| secret == secret.trim();
    anyhow::ensure!(long_enough(current), "the admin secret is missing or shorter than {MIN_SECRET_LEN} characters");
    anyhow::ensure!(bare(current), "the admin secret has a space or line break at its start or end (a pasted newline, usually); remove it");
    let mut secrets = vec![current.to_string()];
    if let Some(previous) = previous {
        anyhow::ensure!(long_enough(previous), "the previous admin secret is empty or shorter than {MIN_SECRET_LEN} characters; leave it out when no rotation is going on");
        anyhow::ensure!(bare(previous), "the previous admin secret has a space or line break at its start or end (a pasted newline, usually); remove it");
        anyhow::ensure!(previous != current, "the previous admin secret is the same as the current one");
        secrets.push(previous.to_string());
    }
    Ok(secrets)
}

/// A warning for an address the API would be reachable from outside the machine on: it speaks plain HTTP, so the TLS is a
/// reverse proxy's job. `None` for loopback.
pub fn non_loopback_warning(addr: std::net::SocketAddr) -> Option<String> {
    (!addr.ip().is_loopback()).then(|| format!("{addr} is not a loopback address and the orchestrator API has no TLS of its own: put a reverse proxy with TLS in front, or listen on 127.0.0.1"))
}

/// The body of `PUT`. The fields of the contract that nothing implements yet (`plan_ref`, `region`, `domain`, `limits`,
/// `owner_identity`) are refused rather than dropped: a caller that sends them would believe they were applied.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PutBody {
    image: String,
    #[serde(default)]
    memory_mb: Option<u64>,
    #[serde(default)]
    cpus: Option<f64>,
    #[serde(default)]
    config: Option<String>,
}

/// What an instance looks like from outside. Not the `Instance` itself: that one holds the pairing key and the config.
#[derive(Serialize)]
struct View {
    customer_id: String,
    image: String,
    state: State,
    created_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    delete_after_ms: Option<u64>,
    /// What the hub says about itself; `null` when it is stopped or does not answer.
    status: Option<HubStatus>,
}

enum Route<'a> {
    Instance(&'a str),
    Action(&'a str, &'a str),
}

fn parse_route(path: &str) -> Option<Route<'_>> {
    let rest = path.strip_prefix("/v1/instances/")?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    match rest.split_once(':') {
        None => Some(Route::Instance(rest)),
        Some((id, action @ ("suspend" | "resume" | "export"))) => Some(Route::Action(id, action)),
        Some(_) => None,
    }
}

async fn handle<R, S>(AxumState(api): AxumState<Arc<Api<R, S>>>, request: Request) -> Response
where
    R: ContainerRuntime + 'static,
    S: StatusSource + 'static,
{
    let (parts, body) = request.into_parts();
    let Ok(body) = axum::body::to_bytes(Body::new(body), MAX_BODY).await else {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "the body is over 64 KiB, or could not be read");
    };

    // The signature first: nothing below runs, and no route is revealed, for a caller that can't sign.
    let signed_path = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or_else(|| parts.uri.path());
    let header = parts.headers.get(SIGNATURE_HEADER).and_then(|v| v.to_str().ok());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let secrets: Vec<&str> = api.config.secrets.iter().map(String::as_str).collect();
    if let Err(err) = verify_any(&secrets, header, parts.method.as_str(), signed_path, &body, now) {
        return error(StatusCode::UNAUTHORIZED, "invalid_signature", &format!("{err} ({SIGNATURE_HEADER})"));
    }

    let Some(route) = parse_route(parts.uri.path()) else {
        return error(StatusCode::NOT_FOUND, "not_found", "no such route");
    };
    let (customer_id, allowed) = match &route {
        Route::Instance(id) => (*id, [Method::PUT, Method::GET, Method::DELETE].as_slice()),
        Route::Action(id, _) => (*id, [Method::POST].as_slice()),
    };
    if !allowed.contains(&parts.method) {
        return error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "that method is not supported on this route");
    }
    if let Err(err) = validate_customer_id(customer_id) {
        return error(StatusCode::BAD_REQUEST, "invalid_customer_id", &err.to_string());
    }

    let api: &Api<R, S> = &api;
    match route {
        Route::Instance(id) if parts.method == Method::PUT => put(api, id, &body).await,
        Route::Instance(id) if parts.method == Method::GET => with_instance(api, id, |instance| async move { Ok((instance, StatusCode::OK)) }).await,
        Route::Instance(id) => {
            let retention = api.config.retention;
            with_instance(api, id, |_| async move { api.manager.schedule_delete(id, retention).await.map(|i| (i, StatusCode::ACCEPTED)) }).await
        }
        Route::Action(id, "suspend") => with_instance(api, id, |_| async move { api.manager.suspend(id).await.map(|i| (i, StatusCode::OK)) }).await,
        Route::Action(id, "resume") => with_instance(api, id, |_| async move { api.manager.resume(id).await.map(|i| (i, StatusCode::OK)) }).await,
        Route::Action(id, _) => export(api, id).await,
    }
}

async fn put<R: ContainerRuntime + 'static, S: StatusSource + 'static>(api: &Api<R, S>, customer_id: &str, body: &[u8]) -> Response {
    let body: PutBody = match serde_json::from_slice(body) {
        Ok(body) => body,
        Err(err) => return error(StatusCode::BAD_REQUEST, "invalid_request", &format!("the body is not a valid instance spec: {err}")),
    };
    if body.image.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "invalid_request", "the image is empty");
    }
    let spec = InstanceSpec { customer_id: customer_id.to_string(), image: body.image, memory_mb: body.memory_mb, cpus: body.cpus, config: body.config };
    let existing = match api.manager.get(customer_id) {
        Ok(existing) => existing,
        Err(err) => return internal(&err),
    };
    if let Some(existing) = &existing {
        let differs = existing.spec.differs_from(&spec);
        if !differs.is_empty() {
            return error(StatusCode::CONFLICT, "spec_conflict", &format!("'{customer_id}' already exists with a different spec (differs in: {})", differs.join(", ")));
        }
    }
    match api.manager.provision(spec).await {
        Ok(instance) => view(api, instance, if existing.is_some() { StatusCode::OK } else { StatusCode::CREATED }).await,
        Err(err) => internal(&err),
    }
}

/// Looks the instance up (a 404 if there is none), runs `act` on it, and answers with the instance it returns.
async fn with_instance<R, S, F, Fut>(api: &Api<R, S>, customer_id: &str, act: F) -> Response
where
    R: ContainerRuntime + 'static,
    S: StatusSource + 'static,
    F: FnOnce(Instance) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<(Instance, StatusCode)>>,
{
    match api.manager.get(customer_id) {
        Ok(Some(instance)) => match act(instance).await {
            Ok((instance, status)) => view(api, instance, status).await,
            Err(err) => internal(&err),
        },
        Ok(None) => not_found(customer_id),
        Err(err) => internal(&err),
    }
}

async fn export<R: ContainerRuntime + 'static, S: StatusSource + 'static>(api: &Api<R, S>, customer_id: &str) -> Response {
    match api.manager.get(customer_id) {
        Ok(Some(_)) => match api.manager.backup(customer_id, api.config.keep_backups).await {
            // The file's name only: the folder is the host's business.
            Ok(path) => json(StatusCode::OK, &serde_json::json!({ "backup": path.file_name().and_then(|n| n.to_str()).unwrap_or_default() })),
            Err(err) => internal(&err),
        },
        Ok(None) => not_found(customer_id),
        Err(err) => internal(&err),
    }
}

async fn view<R: ContainerRuntime + 'static, S: StatusSource + 'static>(api: &Api<R, S>, instance: Instance, status: StatusCode) -> Response {
    let hub = if instance.state == State::Active { api.status.fetch(&instance).await.ok() } else { None };
    json(
        status,
        &View {
            customer_id: instance.spec.customer_id,
            image: instance.spec.image,
            state: instance.state,
            created_at_ms: instance.created_at_ms,
            delete_after_ms: instance.delete_after_ms,
            status: hub,
        },
    )
}

fn json<T: Serialize>(status: StatusCode, body: &T) -> Response {
    (status, [(CONTENT_TYPE, "application/json")], serde_json::to_string(body).unwrap_or_else(|_| "{}".into())).into_response()
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    json(status, &ErrorBody::new(code, message))
}

fn not_found(customer_id: &str) -> Response {
    error(StatusCode::NOT_FOUND, "instance_not_found", &format!("there is no instance for '{customer_id}'"))
}

/// The cause goes to the operator's log; the caller gets a fixed message, because the text of a runtime failure is not
/// something to hand to the other side of the contract.
fn internal(err: &anyhow::Error) -> Response {
    tracing::error!("orchestrator API call failed: {err:#}");
    error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "the orchestrator could not do that; see its log")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_secret_must_be_given_and_long_enough_and_the_previous_one_too() {
        assert_eq!(validate_secrets(GOOD, None).unwrap(), [GOOD]);
        let previous = "fedcba9876543210fedcba9876543210";
        assert_eq!(validate_secrets(GOOD, Some(previous)).unwrap(), [GOOD, previous], "the current one first");

        for bad in ["", "   ", "hunter2", &GOOD[..31]] {
            let err = validate_secrets(bad, None).unwrap_err().to_string();
            assert!(err.contains("32"), "says how long: {err}");
            if !bad.trim().is_empty() {
                assert!(!err.contains(bad), "never echoes the secret: {err}");
            }
        }
        assert!(validate_secrets(GOOD, Some("")).is_err(), "a previous secret that is set but empty is a mistake, not 'none'");
        assert!(validate_secrets(GOOD, Some("short")).is_err());
        assert!(validate_secrets(GOOD, Some(GOOD)).is_err(), "rotating to the same secret rotates nothing");
    }

    #[test]
    fn a_secret_with_a_space_or_a_line_break_at_either_end_is_refused_not_trimmed() {
        // A secret pasted from a file or a `echo` usually carries a final newline. The HMAC would be made over the string
        // with it, and the site mãe, which most likely has it without, would be answered with 401 and no hint why.
        for padded in [format!("{GOOD}\n"), format!("{GOOD}\r\n"), format!(" {GOOD}"), format!("{GOOD} "), format!("\t{GOOD}")] {
            let err = validate_secrets(&padded, None).unwrap_err().to_string();
            assert!(err.contains("space") && err.contains("start or end"), "says what is wrong: {err}");
            assert!(!err.contains(GOOD), "never echoes the secret: {err}");

            let previous_err = validate_secrets(GOOD, Some(&padded)).unwrap_err().to_string();
            assert!(previous_err.contains("previous") && previous_err.contains("start or end"), "{previous_err}");
        }
        // Inside the secret, a space is the owner's business: only the ends are checked.
        let inner = "0123456789abcdef 0123456789abcdef";
        assert_eq!(validate_secrets(inner, None).unwrap(), [inner]);
    }

    #[test]
    fn only_an_address_off_loopback_gets_a_warning_and_it_says_why() {
        for local in ["127.0.0.1:7440", "[::1]:7440", "127.0.0.2:1"] {
            assert_eq!(non_loopback_warning(local.parse().unwrap()), None, "{local}");
        }
        for open in ["0.0.0.0:7440", "10.0.0.5:7440", "[::]:7440"] {
            let warning = non_loopback_warning(open.parse().unwrap()).unwrap_or_else(|| panic!("{open}"));
            assert!(warning.contains("TLS") && warning.contains(open), "{warning}");
        }
    }
}

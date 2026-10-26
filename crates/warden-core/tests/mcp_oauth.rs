//! Real (not mocked) end-to-end test of the OAuth-authenticated MCP HTTP transport
//! (`tool/mcp_oauth.rs`, PENDING.md P26). A single local `axum` server plays both the
//! protected-resource role (the MCP endpoint itself, mirroring `tests/mcp_http.rs`'s
//! `EchoServer`) and the authorization-server role (discovery, Dynamic Client Registration,
//! `/authorize`, `/token`) — the same self-referential shape `rmcp`'s own
//! `tests/test_client_credentials.rs` uses for the simpler client_credentials grant.
//!
//! The one necessarily-faked step is `/authorize` auto-approving instead of showing a real human
//! a consent screen — nothing in CI can click "Allow". Everything downstream of that is exercised
//! for real over real HTTP: RFC 9728/8414 discovery, Dynamic Client Registration, the PKCE code
//! exchange, token persistence to a real file on disk, and the bearer-protected MCP call itself.
//! `authorize_interactively`'s `open_browser` callback stands in for the user's click with a real
//! HTTP client, landing on the module's own local callback listener exactly as production does.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use warden_core::tool::mcp_oauth::{already_authorized, authorize_interactively, connect_http_oauth, CallbackListener, PendingAuthorization};
use warden_core::tool::ToolProvider;

const CLIENT_ID: &str = "test-oauth-client";
const AUTH_CODE: &str = "test-authorization-code";
const ACCESS_TOKEN: &str = "test-access-token";
const REFRESH_TOKEN: &str = "test-refresh-token";
const ROTATED_TOKEN: &str = "test-rotated-access-token";

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct EchoRequest {
    text: String,
}

#[derive(Clone)]
struct EchoServer {
    tool_router: ToolRouter<Self>,
}

impl EchoServer {
    fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }
}

#[tool_router]
impl EchoServer {
    #[tool(description = "Echoes the given text back")]
    fn echo(&self, Parameters(EchoRequest { text }): Parameters<EchoRequest>) -> String {
        text
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EchoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }
}

/// Stands in for a real server's auth check on the resource itself — proves the token this test
/// drives all the way through discovery/DCR/PKCE exchange is what actually unlocks the MCP call,
/// not just accepted and ignored. The token it accepts is whatever `accepted` holds right now, so a test can rotate it
/// under a client that is already connected.
async fn require_bearer_token(State(accepted): State<Arc<Mutex<String>>>, req: Request, next: Next) -> Response {
    let expected = format!("Bearer {}", accepted.lock().unwrap());
    let authorized = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) == Some(expected.as_str());
    if authorized {
        next.run(req).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// RFC 9728 protected resource metadata — `resource` is the server's own root, not the deeper
/// `/mcp` path `connect_http_oauth`/`authorize_interactively` are given, on purpose: rmcp accepts
/// any origin-matching ancestor path as the resource identifier, and serving discovery at the
/// canonical root path (rather than nested under `/mcp/.well-known/...`) is the simplest layout
/// that still exercises the exact fallback rmcp's own discovery logic documents as always tried.
async fn resource_metadata_handler(req: Request) -> impl IntoResponse {
    let base_url = base_url_of(&req);
    Json(json!({
        "resource": base_url,
        "authorization_servers": [base_url],
    }))
}

/// RFC 8414 authorization server metadata — same host plays both resource and authorization
/// server, mirroring `rmcp`'s own `test_client_credentials.rs`. Advertises exactly what
/// `AuthorizationManager::validate_server_metadata`/`register_client` require: the `"code"`
/// response type and an `S256` PKCE method.
async fn as_metadata_handler(req: Request) -> impl IntoResponse {
    let base_url = base_url_of(&req);
    Json(json!({
        "issuer": base_url,
        "authorization_endpoint": format!("{base_url}/authorize"),
        "token_endpoint": format!("{base_url}/token"),
        "registration_endpoint": format!("{base_url}/register"),
        "response_types_supported": ["code"],
        "code_challenge_methods_supported": ["S256"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "scopes_supported": ["mcp"],
    }))
}

fn base_url_of(req: &Request) -> String {
    let host = req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).expect("Host header");
    format!("http://{host}")
}

/// Dynamic Client Registration (RFC 7591) — echoes back whatever `redirect_uris` the client
/// registered with (that's the exact URI `authorize_interactively`'s local callback listener is
/// bound to) alongside a fixed client_id; no client_secret, a public client, same as any native
/// app doing PKCE.
async fn register_handler(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let redirect_uris = body.get("redirect_uris").cloned().unwrap_or_else(|| json!([]));
    Json(json!({
        "client_id": CLIENT_ID,
        "client_secret": null,
        "client_name": body.get("client_name"),
        "redirect_uris": redirect_uris,
    }))
}

#[derive(Deserialize)]
struct AuthorizeParams {
    redirect_uri: String,
    state: String,
}

/// The one necessarily-faked step: a real authorization server would show a human a consent
/// screen here. This test server auto-approves and redirects straight back with a code — nothing
/// automated can click "Allow", so this is the honest way to exercise everything around it for
/// real instead of mocking the whole OAuth exchange.
async fn authorize_handler(Query(params): Query<AuthorizeParams>) -> impl IntoResponse {
    Redirect::to(&format!("{}?code={AUTH_CODE}&state={}", params.redirect_uri, params.state))
}

/// Token endpoint (RFC 6749 §4.1.3) — doesn't re-derive the PKCE `code_verifier` against a stored
/// `code_challenge` (this test server never captured one; the client always sends it, proving the
/// real client-side PKCE machinery ran, but a from-scratch OAuth server isn't what's under test
/// here). Rejects anything that isn't the expected grant/code so the test still fails loudly if
/// the exchange sends something unexpected.
async fn token_handler(Form(params): Form<HashMap<String, String>>) -> impl IntoResponse {
    let grant_type = params.get("grant_type").map(String::as_str).unwrap_or_default();
    let code = params.get("code").map(String::as_str).unwrap_or_default();
    if grant_type != "authorization_code" || code != AUTH_CODE {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant"}))).into_response();
    }
    Json(json!({
        "access_token": ACCESS_TOKEN,
        "token_type": "Bearer",
        "expires_in": 3600,
        "refresh_token": REFRESH_TOKEN,
        "scope": "mcp",
    }))
    .into_response()
}

/// Binds a server that's simultaneously the MCP endpoint (at `/mcp`, bearer-protected) and its
/// own OAuth authorization server (discovery + DCR + `/authorize` + `/token` at the root) to a
/// real local port. Returns the MCP endpoint URL plus a handle that shuts the server down when
/// cancelled.
async fn spawn_server() -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    spawn_server_accepting(Arc::new(Mutex::new(ACCESS_TOKEN.to_string()))).await
}

/// `spawn_server`, with the bearer token the MCP endpoint accepts held in `accepted`, which the caller can change later.
async fn spawn_server_accepting(accepted: Arc<Mutex<String>>) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let ct = CancellationToken::new();

    let service: StreamableHttpService<EchoServer, LocalSessionManager> = StreamableHttpService::new(
        || Ok(EchoServer::new()),
        Default::default(),
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
    );

    let mcp_route = Router::new().nest_service("/mcp", service).layer(middleware::from_fn_with_state(accepted, require_bearer_token));

    let router = Router::new()
        .route("/.well-known/oauth-protected-resource", get(resource_metadata_handler))
        .route("/.well-known/oauth-authorization-server", get(as_metadata_handler))
        .route("/register", post(register_handler))
        .route("/authorize", get(authorize_handler))
        .route("/token", post(token_handler))
        .merge(mcp_route);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind local test port");
    let addr = listener.local_addr().expect("local addr");

    let handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(listener, router).with_graceful_shutdown(async move { ct.cancelled_owned().await }).await;
        }
    });

    (format!("http://{addr}/mcp"), ct, handle)
}

#[tokio::test]
async fn authorizes_persists_and_connects_to_a_real_oauth_mcp_server() {
    let (url, ct, handle) = spawn_server().await;
    let credential_store_path = std::env::temp_dir().join(format!(
        "warden-mcp-oauth-test-{}.json",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let _ = tokio::fs::remove_file(&credential_store_path).await;

    // Stands in for the user's browser click: a real HTTP GET against the authorization URL,
    // following the test server's auto-approval redirect straight to the module's own local
    // one-shot callback listener — proving that listener actually works, not just the OAuth
    // protocol exchange around it.
    let open_browser = {
        move |auth_url: &str| {
            let auth_url = auth_url.to_string();
            tokio::spawn(async move {
                let _ = reqwest::get(&auth_url).await;
            });
        }
    };

    authorize_interactively("oauth-echo-server", &url, &credential_store_path, open_browser)
        .await
        .expect("interactive OAuth authorization against a real (test) server");

    assert!(credential_store_path.is_file(), "authorize_interactively should have persisted a credential file");

    // Headless reconnect — the path every `bootstrap()` takes — must succeed purely from what
    // was just persisted, no browser involved.
    let provider = connect_http_oauth("oauth-echo-server", &url, &credential_store_path)
        .await
        .expect("headless reconnect using the persisted OAuth token");

    let tools = provider.tools().await.expect("list tools from the real OAuth-protected MCP server");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].spec().name, "echo");

    let result = tools[0].call(json!({ "text": "hello over oauth" })).await.expect("call the real MCP tool over OAuth");
    let result_str = result.to_string();
    assert!(result_str.contains("hello over oauth"), "unexpected result: {result_str}");

    let _ = tokio::fs::remove_file(&credential_store_path).await;
    ct.cancel();
    let _ = handle.await;
}

/// P54: a pull that brings another device's login overwrites `<server>.json` under an MCP client that is already
/// connected. `warden-bootstrap` only rebuilds the orchestrator for a server that never connected or whose client id
/// changed, so for the rest this is what makes the new token count: the client reads the file again on every request.
#[tokio::test]
async fn a_login_overwritten_on_disk_is_used_by_a_client_already_connected() {
    let accepted = Arc::new(Mutex::new(ACCESS_TOKEN.to_string()));
    let (url, ct, handle) = spawn_server_accepting(accepted.clone()).await;
    let credential_store_path = std::env::temp_dir().join(format!(
        "warden-mcp-oauth-test-overwritten-{}.json",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let _ = tokio::fs::remove_file(&credential_store_path).await;

    authorize_interactively("oauth-echo-server", &url, &credential_store_path, |auth_url: &str| {
        let auth_url = auth_url.to_string();
        tokio::spawn(async move {
            let _ = reqwest::get(&auth_url).await;
        });
    })
    .await
    .expect("interactive OAuth authorization against a real (test) server");

    let provider = connect_http_oauth("oauth-echo-server", &url, &credential_store_path).await.expect("headless connect");
    let tools = provider.tools().await.expect("list tools");
    let echo = &tools[0];
    echo.call(json!({ "text": "before" })).await.expect("the login on disk works");

    // The server moves to another token, as if the login had been renewed on the other device: the one the client holds
    // on disk is no longer good, which proves the call below can only pass by reading the new file.
    *accepted.lock().unwrap() = ROTATED_TOKEN.to_string();
    assert!(echo.call(json!({ "text": "stale" })).await.is_err(), "the old token must be refused once the server rotated it");

    // What a pull does: the same file, now with the other device's token.
    let mut login: serde_json::Value = serde_json::from_slice(&tokio::fs::read(&credential_store_path).await.unwrap()).unwrap();
    login["token_response"]["access_token"] = json!(ROTATED_TOKEN);
    tokio::fs::write(&credential_store_path, serde_json::to_vec_pretty(&login).unwrap()).await.unwrap();

    let result = echo.call(json!({ "text": "after" })).await.expect("the client already connected uses the login the pull wrote");
    assert!(result.to_string().contains("after"), "unexpected result: {result}");

    let _ = tokio::fs::remove_file(&credential_store_path).await;
    ct.cancel();
    let _ = handle.await;
}

/// Where a browser that nothing listens for would end up: the test server approves and answers with a redirect, which is read
/// instead of followed. `redirect_uri` is registered but nobody is there, as for a hub whose person is on another machine.
async fn address_the_browser_ends_at(auth_url: &str) -> String {
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let response = client.get(auth_url).send().await.expect("the authorization server answers");
    response.headers().get(header::LOCATION).expect("it redirects back").to_str().unwrap().to_string()
}

fn temp_store(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("warden-mcp-oauth-test-{label}-{}.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
}

/// P131: the two halves apart, with the person's browser on another machine. Begin gives the address to open, the browser ends at an
/// address nobody listens at, and handing that address back finishes the login, which the headless connection then uses.
#[tokio::test]
async fn a_login_begun_and_finished_apart_is_persisted_and_used() {
    let (url, ct, handle) = spawn_server().await;
    let store = temp_store("apart");
    let redirect = "http://127.0.0.1:1/callback";

    let mut pending = PendingAuthorization::begin("oauth-echo-server", &url, &store, redirect).await.expect("begin");
    assert_eq!(pending.redirect_uri(), redirect);
    assert!(!store.exists(), "nothing is saved before the browser comes back");

    let returned = address_the_browser_ends_at(pending.auth_url()).await;
    assert!(returned.starts_with(&format!("{redirect}?code=")), "{returned}");
    pending.complete(&returned).await.expect("complete");
    assert!(store.is_file());

    let provider = connect_http_oauth("oauth-echo-server", &url, &store).await.expect("headless connect with the login just made");
    assert_eq!(provider.tools().await.unwrap()[0].spec().name, "echo");
    assert!(already_authorized(&url, &store).await.unwrap());

    let _ = tokio::fs::remove_file(&store).await;
    ct.cancel();
    let _ = handle.await;
}

/// What comes back has to be this flow's own: another address, or the right one with a `state` that was not issued, saves nothing.
#[tokio::test]
async fn an_address_that_is_not_this_flows_own_return_saves_nothing() {
    let (url, ct, handle) = spawn_server().await;
    let store = temp_store("refused");
    let redirect = "http://127.0.0.1:1/callback";
    let mut pending = PendingAuthorization::begin("oauth-echo-server", &url, &store, redirect).await.expect("begin");
    let returned = address_the_browser_ends_at(pending.auth_url()).await;
    let query = returned.split_once('?').unwrap().1;

    for bad in [
        String::new(),
        "not an address".to_string(),
        format!("http://127.0.0.1:2/callback?{query}"),
        format!("https://evil.example/callback?{query}"),
        format!("{redirect}/../x?{query}"),
        redirect.to_string(),
        format!("{redirect}?code=test-authorization-code&state=somebody-elses"),
    ] {
        assert!(pending.complete(&bad).await.is_err(), "{bad:?} should be refused");
        assert!(!store.exists(), "{bad:?} saved something");
    }

    // The refused tries did not use the flow up: the real address still finishes it.
    pending.complete(&returned).await.expect("the real return still works");
    assert!(store.is_file());

    let _ = tokio::fs::remove_file(&store).await;
    ct.cancel();
    let _ = handle.await;
}

/// P131: the desktop opens this listener for the hub's sign-in, tells the hub its port, and hands what arrives back to the hub.
#[tokio::test]
async fn the_callback_listener_hands_back_the_query_the_browser_arrives_with() {
    let listener = CallbackListener::bind().await.expect("bind");
    assert_eq!(listener.redirect_uri(), format!("http://127.0.0.1:{}/callback", listener.port()));
    assert!(listener.port() >= 1024, "a free port, not a privileged one");
    let address = format!("{}?code=abc&state=xyz", listener.redirect_uri());
    let browser = tokio::spawn(async move { reqwest::get(&address).await.expect("the browser reaches the listener").text().await.unwrap() });
    let query = listener.wait(std::time::Duration::from_secs(5)).await.expect("the query arrives");
    assert_eq!(query, "code=abc&state=xyz");
    assert!(browser.await.unwrap().contains("authorization complete"), "the person is told they can close the tab");
}

#[tokio::test]
async fn the_callback_listener_gives_up_on_time_and_on_a_request_with_nothing_to_hand_back() {
    let quiet = CallbackListener::bind().await.unwrap();
    assert!(quiet.wait(std::time::Duration::from_millis(50)).await.unwrap_err().to_string().contains("timed out"));

    let bare = CallbackListener::bind().await.unwrap();
    let address = bare.redirect_uri();
    tokio::spawn(async move {
        let _ = reqwest::get(&address).await;
    });
    assert!(bare.wait(std::time::Duration::from_secs(5)).await.unwrap_err().to_string().contains("no query string"));
}

#[tokio::test]
async fn connect_http_oauth_fails_clearly_without_prior_authorization() {
    let (url, ct, handle) = spawn_server().await;
    let credential_store_path = std::env::temp_dir().join(format!(
        "warden-mcp-oauth-test-unauthorized-{}.json",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let _ = tokio::fs::remove_file(&credential_store_path).await;

    let err = match connect_http_oauth("oauth-echo-server", &url, &credential_store_path).await {
        Ok(_) => panic!("connecting without prior authorization must fail"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("oauth-echo-server"), "error should name the server: {err}");

    ct.cancel();
    let _ = handle.await;
}

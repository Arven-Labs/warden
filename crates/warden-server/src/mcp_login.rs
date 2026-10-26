//! Signing in an OAuth MCP server from the web (P131). The desktop signs in with a browser and a listener on its own machine; a hub has
//! neither, and the person's browser is on another machine than the hub. So the sign-in is two calls with the hub holding the middle:
//! `BeginMcpOauth` does discovery and registration and answers with the address to open; the browser ends at `redirect_prefix?code=…`,
//! which nothing listens at, and the person copies that address back with `CompleteMcpOauth`. The PKCE verifier and the registered
//! client never leave this process, so a copied code is worth nothing to anyone else.
//!
//! The login is the hub's, one per server, and it changes what reaches the machine, so it is held to the same conditions as the machine's
//! settings: the owner only, `--allow-machine-settings`, an encrypted or local connection and the pairing key. The server's address comes
//! from the hub's own config, never from the message.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use warden_bootstrap::{load_config_from_path, oauth_credential_store_path_in, McpServerConfig};
use warden_core::tool::mcp_oauth::{forget_credentials, PendingAuthorization};
use warden_server_protocol::ServerMessage;

use crate::settings::{keys_match, machine_gate, org_refusal, settings_error, SettingsAccess, WRONG_KEY_DELAY};

/// Where the browser is sent back to. Nothing listens here, on purpose: the person copies the address the browser ends at. It is a
/// loopback address because that is what an authorization server accepts for an app that cannot keep a secret.
pub const REDIRECT_URI: &str = "http://127.0.0.1:45871/callback";

/// How many sign-ins can be open at once, and for how long.
const MAX_FLOWS: usize = 4;
const FLOW_TTL: Duration = Duration::from_secs(5 * 60);

struct Flow {
    server: String,
    pending: PendingAuthorization,
    started: Instant,
}

/// The sign-ins in progress and the folder the finished ones are saved in.
pub struct McpLogins {
    dir: PathBuf,
    ttl: Duration,
    flows: Mutex<HashMap<String, Flow>>,
}

impl McpLogins {
    /// Logins saved in the folder every other part of Warden reads them from.
    pub fn new() -> Self {
        Self::in_dir(warden_bootstrap::oauth_credentials_dir())
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self { dir, ttl: FLOW_TTL, flows: Mutex::new(HashMap::new()) }
    }

    #[cfg(test)]
    fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    fn store_of(&self, name: &str) -> PathBuf {
        oauth_credential_store_path_in(&self.dir, name)
    }

    /// Drops the flows that ran out of time.
    fn sweep(&self, flows: &mut HashMap<String, Flow>) {
        flows.retain(|_, flow| flow.started.elapsed() < self.ttl);
    }

    /// Opens a flow for `server`, replacing the one it had, and returns its id. `None` when too many other servers are signing in.
    fn open(&self, server: &str, pending: PendingAuthorization) -> Option<String> {
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        self.sweep(&mut flows);
        flows.retain(|_, flow| flow.server != server);
        if flows.len() >= MAX_FLOWS {
            return None;
        }
        let id = warden_bootstrap::generate_auth_key();
        flows.insert(id.clone(), Flow { server: server.to_string(), pending, started: Instant::now() });
        Some(id)
    }

    /// Takes the flow out while it is being finished, so it cannot be finished twice at once.
    fn take(&self, id: &str) -> Option<Flow> {
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        self.sweep(&mut flows);
        flows.remove(id)
    }

    /// Puts a flow back after a try that did not finish it (a wrong address pasted), so the person can paste the right one.
    fn give_back(&self, id: String, flow: Flow) {
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        flows.insert(id, flow);
    }

    #[cfg(test)]
    fn open_count(&self) -> usize {
        self.flows.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl Default for McpLogins {
    fn default() -> Self {
        Self::new()
    }
}

/// The URL of the saved OAuth MCP server `name`, or why there is none.
fn oauth_server_url(config: &warden_bootstrap::FileConfig, name: &str) -> Result<String, String> {
    let saved = config.mcp_servers.iter().find(|s| match s {
        McpServerConfig::Http { name: n, .. } | McpServerConfig::Stdio { name: n, .. } => n == name,
    });
    match saved {
        Some(McpServerConfig::Http { url, oauth: true, .. }) => Ok(url.clone()),
        Some(_) => Err(format!("MCP server '{name}' does not sign in with OAuth")),
        None => Err(format!("there is no saved MCP server called '{name}' (save it first)")),
    }
}

/// What every handler checks before it touches a login: the owner, the settings, the pairing key (when the call changes something) and the
/// machine gate. Holds the settings lock only while it looks, never over the network.
async fn authorize(access: &SettingsAccess<'_>, request_id: u64, member: Option<&str>, pairing_key: Option<&str>) -> Result<warden_bootstrap::FileConfig, ServerMessage> {
    if member.is_some() {
        return Err(*org_refusal(request_id, "only the workspace's owner signs the hub in to a service"));
    }
    let Some(host) = access.host else {
        return Err(settings_error(request_id, "this hub doesn't offer its settings over the network"));
    };
    let _held = access.lock.lock().await;
    if let Some(key) = pairing_key {
        if !keys_match(key, access.auth_key) {
            tokio::time::sleep(WRONG_KEY_DELAY).await;
            return Err(ServerMessage::SettingsError { request_id, message: "wrong pairing key".to_string(), conflict: false, auth_rejected: true });
        }
        if let Err(reason) = machine_gate(access.allow_machine, access.secure) {
            return Err(settings_error(request_id, reason));
        }
    }
    load_config_from_path(&host.config_path(), false).map_err(|err| settings_error(request_id, format!("{err:#}")))
}

fn names_with_login(logins: &McpLogins, config: &warden_bootstrap::FileConfig) -> Vec<String> {
    let mut names: Vec<String> = config
        .mcp_servers
        .iter()
        .filter_map(|s| match s {
            McpServerConfig::Http { name, oauth: true, .. } if logins.store_of(name).is_file() => Some(name.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    names
}

/// Answers `ListMcpLogins`: which of the hub's OAuth servers have a login saved.
pub async fn handle_list(access: &SettingsAccess<'_>, logins: &McpLogins, request_id: u64, member: Option<&str>) -> ServerMessage {
    match authorize(access, request_id, member, None).await {
        Ok(config) => ServerMessage::McpLogins { request_id, names: names_with_login(logins, &config) },
        Err(reply) => reply,
    }
}

/// Where the browser is sent back to: the hub's fixed address, or, for a client that listens for the browser itself, the port it named on
/// `127.0.0.1`. Only loopback and only an unprivileged port, so a client cannot aim the return at anything else.
fn redirect_for(port: Option<u16>) -> Result<String, String> {
    match port {
        None => Ok(REDIRECT_URI.to_string()),
        Some(port) if port >= 1024 => Ok(format!("http://127.0.0.1:{port}/callback")),
        Some(port) => Err(format!("the return port has to be 1024 or more (got {port})")),
    }
}

/// Answers `BeginMcpOauth`: discovery and registration, and the address the person opens. `redirect_port` is the port a client that listens
/// for the browser itself (the desktop) asks the return to come to; without it the person copies the address back.
pub async fn handle_begin(access: &SettingsAccess<'_>, logins: &McpLogins, request_id: u64, member: Option<&str>, pairing_key: &str, name: &str, redirect_port: Option<u16>) -> ServerMessage {
    let config = match authorize(access, request_id, member, Some(pairing_key)).await {
        Ok(config) => config,
        Err(reply) => return reply,
    };
    let redirect_uri = match redirect_for(redirect_port) {
        Ok(uri) => uri,
        Err(message) => return settings_error(request_id, message),
    };
    let url = match oauth_server_url(&config, name) {
        Ok(url) => url,
        Err(message) => return settings_error(request_id, message),
    };
    let pending = match PendingAuthorization::begin(name, &url, &logins.store_of(name), &redirect_uri).await {
        Ok(pending) => pending,
        Err(err) => return settings_error(request_id, format!("{err:#}")),
    };
    let auth_url = pending.auth_url().to_string();
    match logins.open(name, pending) {
        Some(flow_id) => {
            tracing::info!(server = name, "MCP sign-in started");
            ServerMessage::McpOauthStarted { request_id, flow_id, auth_url, redirect_prefix: redirect_uri }
        }
        None => settings_error(request_id, "too many MCP sign-ins are open at once; finish or wait for one to run out"),
    }
}

/// Answers `CompleteMcpOauth`: the address the browser ended at finishes the flow, the login is saved, and the hub starts its
/// orchestrator again so the server connects with it.
pub async fn handle_complete(access: &SettingsAccess<'_>, logins: &McpLogins, request_id: u64, member: Option<&str>, flow_id: &str, callback_url: &str) -> ServerMessage {
    if let Err(reply) = authorize(access, request_id, member, None).await {
        return reply;
    }
    let Some(mut flow) = logins.take(flow_id) else {
        return settings_error(request_id, "that sign-in ran out of time or does not exist; start it again");
    };
    if let Err(err) = flow.pending.complete(callback_url).await {
        let message = format!("{err:#}");
        logins.give_back(flow_id.to_string(), flow);
        return settings_error(request_id, message);
    }
    let name = flow.server.clone();
    tracing::warn!(server = %name, from = ?access.peer, "MCP login saved");
    if let Some(reply) = reload(access, request_id).await {
        return reply;
    }
    ServerMessage::McpOauthDone { request_id, name }
}

/// Answers `ForgetMcpOauth`: deletes the saved login, so the server's next sign-in starts over.
pub async fn handle_forget(access: &SettingsAccess<'_>, logins: &McpLogins, request_id: u64, member: Option<&str>, pairing_key: &str, name: &str) -> ServerMessage {
    let config = match authorize(access, request_id, member, Some(pairing_key)).await {
        Ok(config) => config,
        Err(reply) => return reply,
    };
    if let Err(message) = oauth_server_url(&config, name) {
        return settings_error(request_id, message);
    }
    if let Err(err) = forget_credentials(&logins.store_of(name)).await {
        return settings_error(request_id, format!("{err:#}"));
    }
    tracing::warn!(server = name, from = ?access.peer, "MCP login forgotten");
    if let Some(reply) = reload(access, request_id).await {
        return reply;
    }
    ServerMessage::McpLogins { request_id, names: names_with_login(logins, &config) }
}

/// Starts the hub's orchestrator again on the file as it is, so a server picks up (or drops) its login. `Some` is the error to answer
/// with: the login itself is already saved or forgotten by then, and the message says so.
async fn reload(access: &SettingsAccess<'_>, request_id: u64) -> Option<ServerMessage> {
    let host = access.host?;
    let _saving = access.lock.lock().await;
    match host.build().await {
        Ok(orchestrator) => {
            host.installed(&orchestrator);
            access.shared.replace(orchestrator);
            None
        }
        Err(err) => Some(settings_error(request_id, format!("the login was saved, but the hub couldn't start with it: {err:#}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as Map;

    use async_trait::async_trait;
    use axum::extract::{Query, Request};
    use axum::http::header;
    use axum::response::{IntoResponse, Redirect};
    use axum::routing::{get, post};
    use axum::{Form, Json, Router};
    use serde_json::json;
    use warden_bootstrap::{bootstrap, Overrides};
    use warden_core::orchestrator::Orchestrator;

    use crate::settings::{SettingsHost, SharedOrchestrator};

    const KEY: &str = "pairing-key-0123456789";
    const CODE: &str = "test-authorization-code";

    // A stand-in authorization server: discovery, registration, an `/authorize` that approves at once and a `/token` that wants `CODE`.
    // Only the sign-in is exercised, so there is no MCP endpoint behind it.
    fn base_of(req: &Request) -> String {
        format!("http://{}", req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).expect("Host header"))
    }

    async fn spawn_authorization_server() -> String {
        let router = Router::new()
            .route("/.well-known/oauth-protected-resource", get(|req: Request| async move { let b = base_of(&req); Json(json!({ "resource": b, "authorization_servers": [b] })) }))
            .route(
                "/.well-known/oauth-authorization-server",
                get(|req: Request| async move {
                    let b = base_of(&req);
                    Json(json!({
                        "issuer": b, "authorization_endpoint": format!("{b}/authorize"), "token_endpoint": format!("{b}/token"),
                        "registration_endpoint": format!("{b}/register"), "response_types_supported": ["code"],
                        "code_challenge_methods_supported": ["S256"], "grant_types_supported": ["authorization_code", "refresh_token"], "scopes_supported": ["mcp"],
                    }))
                }),
            )
            .route("/register", post(|Json(body): Json<serde_json::Value>| async move { Json(json!({ "client_id": "test-client", "client_secret": null, "redirect_uris": body.get("redirect_uris") })) }))
            .route(
                "/authorize",
                get(|Query(q): Query<Map<String, String>>| async move { Redirect::to(&format!("{}?code={CODE}&state={}", q["redirect_uri"], q["state"])) }),
            )
            .route(
                "/token",
                post(|Form(p): Form<Map<String, String>>| async move {
                    if p.get("code").map(String::as_str) != Some(CODE) {
                        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant" }))).into_response();
                    }
                    Json(json!({ "access_token": "tok", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "ref", "scope": "mcp" })).into_response()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        format!("http://{addr}")
    }

    /// Where a browser that nothing listens for would end up after approving: the redirect is read, not followed.
    async fn browser_ends_at(auth_url: &str) -> String {
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let response = client.get(auth_url).send().await.unwrap();
        response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string()
    }

    struct TestHost {
        dir: PathBuf,
    }

    #[async_trait]
    impl SettingsHost for TestHost {
        fn config_path(&self) -> PathBuf {
            self.dir.join("config.toml")
        }
        async fn build(&self) -> anyhow::Result<Orchestrator> {
            bootstrap(Some(self.config_path().to_str().unwrap()), Overrides::default(), self.dir.join("vault")).await
        }
    }

    /// A hub whose config has the OAuth server `slack`, the plain `keyed` and the local `local`, all pointed at one stand-in server.
    struct Fixture {
        host: TestHost,
        shared: SharedOrchestrator,
        lock: tokio::sync::Mutex<()>,
        logins: McpLogins,
        server: String,
    }

    impl Fixture {
        async fn new(name: &str, extra_oauth: usize) -> Self {
            let server = spawn_authorization_server().await;
            let dir = std::env::temp_dir().join(format!("warden-mcp-login-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let mut config = format!(
                "active_provider = \"main\"\n\n[[providers]]\nid = \"main\"\nkind = \"anthropic\"\napi_key = \"sk-ant-x\"\n\n\
                 [[mcp_servers]]\nname = \"slack\"\nurl = \"{server}/mcp\"\noauth = true\n\n\
                 [[mcp_servers]]\nname = \"keyed\"\nurl = \"{server}/mcp\"\n\n[mcp_servers.headers]\nAuthorization = \"Bearer x\"\n\n\
                 [[mcp_servers]]\nname = \"local\"\ncommand = \"true\"\n"
            );
            for n in 0..extra_oauth {
                config.push_str(&format!("\n[[mcp_servers]]\nname = \"extra{n}\"\nurl = \"{server}/mcp\"\noauth = true\n"));
            }
            std::fs::write(dir.join("config.toml"), config).unwrap();
            let host = TestHost { dir: dir.clone() };
            let shared = SharedOrchestrator::new(host.build().await.unwrap());
            Self { host, shared, lock: tokio::sync::Mutex::new(()), logins: McpLogins::in_dir(dir.join("mcp_oauth")), server }
        }

        fn access(&self, allow_machine: bool, secure: bool) -> SettingsAccess<'_> {
            SettingsAccess { host: Some(&self.host), shared: &self.shared, lock: &self.lock, auth_key: KEY, secure, allow_machine, peer: None }
        }

        async fn begin(&self, name: &str) -> ServerMessage {
            handle_begin(&self.access(true, true), &self.logins, 1, None, KEY, name, None).await
        }
    }

    fn started(reply: ServerMessage) -> (String, String) {
        match reply {
            ServerMessage::McpOauthStarted { flow_id, auth_url, redirect_prefix, .. } => {
                assert_eq!(redirect_prefix, REDIRECT_URI);
                (flow_id, auth_url)
            }
            other => panic!("expected a started sign-in, got {other:?}"),
        }
    }

    fn error_of(reply: ServerMessage) -> (String, bool) {
        match reply {
            ServerMessage::SettingsError { message, auth_rejected, .. } => (message, auth_rejected),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_sign_in_is_begun_finished_listed_and_forgotten() {
        let fx = Fixture::new("flow", 0).await;
        let access = fx.access(true, true);
        let (flow_id, auth_url) = started(fx.begin("slack").await);
        assert!(auth_url.starts_with(&fx.server), "{auth_url}");
        assert!(matches!(handle_list(&access, &fx.logins, 2, None).await, ServerMessage::McpLogins { names, .. } if names.is_empty()));

        let returned = browser_ends_at(&auth_url).await;
        assert!(returned.starts_with(&format!("{REDIRECT_URI}?code=")), "{returned}");
        match handle_complete(&access, &fx.logins, 3, None, &flow_id, &returned).await {
            ServerMessage::McpOauthDone { name, .. } => assert_eq!(name, "slack"),
            other => panic!("{other:?}"),
        }
        assert!(fx.logins.store_of("slack").is_file(), "the login is on the hub's disk");
        assert!(matches!(handle_list(&access, &fx.logins, 4, None).await, ServerMessage::McpLogins { names, .. } if names == ["slack"]));
        // The flow is spent.
        assert!(error_of(handle_complete(&access, &fx.logins, 5, None, &flow_id, &returned).await).0.contains("ran out of time"));

        match handle_forget(&access, &fx.logins, 6, None, KEY, "slack").await {
            ServerMessage::McpLogins { names, .. } => assert!(names.is_empty()),
            other => panic!("{other:?}"),
        }
        assert!(!fx.logins.store_of("slack").exists());
        let _ = std::fs::remove_dir_all(&fx.host.dir);
    }

    #[tokio::test]
    async fn only_the_owner_with_the_key_on_a_hub_that_allows_it_signs_in_a_saved_oauth_server() {
        let fx = Fixture::new("refused", 0).await;
        let begin = |allow, secure, key: &'static str, member: Option<&'static str>, name: &'static str| {
            let (fx, access) = (&fx, fx.access(allow, secure));
            async move { handle_begin(&access, &fx.logins, 1, member, key, name, None).await }
        };
        let (message, rejected) = error_of(begin(true, true, "wrong-key", None, "slack").await);
        assert!(message.contains("wrong pairing key") && rejected);
        assert!(error_of(begin(false, true, KEY, None, "slack").await).0.contains("--allow-machine-settings"));
        assert!(error_of(begin(true, false, KEY, None, "slack").await).0.contains("encrypted"));
        assert!(error_of(begin(true, true, KEY, Some("ana"), "slack").await).0.contains("owner"));
        assert!(error_of(begin(true, true, KEY, None, "keyed").await).0.contains("does not sign in with OAuth"));
        assert!(error_of(begin(true, true, KEY, None, "local").await).0.contains("does not sign in with OAuth"));
        assert!(error_of(begin(true, true, KEY, None, "nothing").await).0.contains("save it first"));
        assert_eq!(fx.logins.open_count(), 0, "none of them opened a flow");
        // Nor do a member or a wrong key list or forget.
        let access = fx.access(true, true);
        assert!(error_of(handle_list(&access, &fx.logins, 1, Some("ana")).await).0.contains("owner"));
        assert!(error_of(handle_forget(&access, &fx.logins, 1, None, "wrong-key", "slack").await).1);
        let _ = std::fs::remove_dir_all(&fx.host.dir);
    }

    /// A client that listens for the browser itself (the desktop) names the port, and the return comes to it: no copying.
    #[tokio::test]
    async fn a_client_that_listens_for_the_browser_names_the_port_and_gets_that_return() {
        let fx = Fixture::new("port", 0).await;
        let access = fx.access(true, true);
        let listener = warden_core::tool::mcp_oauth::CallbackListener::bind().await.unwrap();
        let (port, wanted) = (listener.port(), listener.redirect_uri());

        let (flow_id, auth_url) = match handle_begin(&access, &fx.logins, 1, None, KEY, "slack", Some(port)).await {
            ServerMessage::McpOauthStarted { flow_id, auth_url, redirect_prefix, .. } => {
                assert_eq!(redirect_prefix, wanted, "the hub registered the return at the port it was given");
                (flow_id, auth_url)
            }
            other => panic!("{other:?}"),
        };
        assert!(auth_url.contains(&format!("127.0.0.1%3A{port}")) || auth_url.contains(&format!("127.0.0.1:{port}")), "{auth_url}");

        // The browser approves and is sent to the desktop's own listener, which hands the query to the hub.
        let browser = tokio::spawn(async move { reqwest::get(&auth_url).await.unwrap().text().await.unwrap() });
        let query = listener.wait(Duration::from_secs(5)).await.expect("the browser came back to the listener");
        browser.await.unwrap();
        assert!(matches!(handle_complete(&access, &fx.logins, 2, None, &flow_id, &format!("{wanted}?{query}")).await, ServerMessage::McpOauthDone { .. }));
        assert!(fx.logins.store_of("slack").is_file());
        let _ = std::fs::remove_dir_all(&fx.host.dir);
    }

    #[tokio::test]
    async fn a_return_port_that_is_privileged_is_refused_and_none_keeps_the_fixed_address() {
        let fx = Fixture::new("badport", 0).await;
        let access = fx.access(true, true);
        for port in [0u16, 1, 80, 1023] {
            let reply = handle_begin(&access, &fx.logins, 1, None, KEY, "slack", Some(port)).await;
            assert!(error_of(reply).0.contains("1024"), "port {port}");
        }
        assert_eq!(fx.logins.open_count(), 0);
        assert_eq!(redirect_for(None).unwrap(), REDIRECT_URI);
        assert_eq!(redirect_for(Some(1024)).unwrap(), "http://127.0.0.1:1024/callback");
        let _ = std::fs::remove_dir_all(&fx.host.dir);
    }

    #[tokio::test]
    async fn a_wrong_address_leaves_the_sign_in_open_for_the_right_one() {
        let fx = Fixture::new("wrong", 0).await;
        let access = fx.access(true, true);
        let (flow_id, auth_url) = started(fx.begin("slack").await);
        let returned = browser_ends_at(&auth_url).await;
        let query = returned.split_once('?').unwrap().1;

        for bad in ["".to_string(), "nonsense".to_string(), format!("https://evil.example/callback?{query}"), format!("{REDIRECT_URI}?code={CODE}&state=somebody-elses")] {
            let (message, _) = error_of(handle_complete(&access, &fx.logins, 1, None, &flow_id, &bad).await);
            assert!(!message.is_empty(), "{bad:?}");
            assert!(!fx.logins.store_of("slack").exists(), "{bad:?} saved something");
            assert_eq!(fx.logins.open_count(), 1, "{bad:?} used the flow up");
        }
        assert!(matches!(handle_complete(&access, &fx.logins, 2, None, &flow_id, &returned).await, ServerMessage::McpOauthDone { .. }));
        // An id nobody issued.
        assert!(error_of(handle_complete(&access, &fx.logins, 3, None, "0000", &returned).await).0.contains("ran out of time"));
        let _ = std::fs::remove_dir_all(&fx.host.dir);
    }

    #[tokio::test]
    async fn sign_ins_run_out_of_time_are_replaced_per_server_and_are_few() {
        let mut fx = Fixture::new("limits", 5).await;
        // The same server again replaces its flow instead of adding one.
        let (first, _) = started(fx.begin("slack").await);
        let (second, _) = started(fx.begin("slack").await);
        assert_ne!(first, second);
        assert_eq!(fx.logins.open_count(), 1);
        let access = fx.access(true, true);
        assert!(error_of(handle_complete(&access, &fx.logins, 1, None, &first, &format!("{REDIRECT_URI}?code=x&state=y")).await).0.contains("ran out of time"));

        for n in 0..3 {
            started(fx.begin(&format!("extra{n}")).await);
        }
        assert_eq!(fx.logins.open_count(), 4);
        assert!(error_of(fx.begin("extra3").await).0.contains("too many"), "a fifth server is turned away");

        // Past its time a flow is gone.
        fx.logins = McpLogins::in_dir(fx.host.dir.join("mcp_oauth")).with_ttl(Duration::ZERO);
        let (id, auth_url) = started(fx.begin("slack").await);
        let returned = browser_ends_at(&auth_url).await;
        let access = fx.access(true, true);
        assert!(error_of(handle_complete(&access, &fx.logins, 2, None, &id, &returned).await).0.contains("ran out of time"));
        let _ = std::fs::remove_dir_all(&fx.host.dir);
    }
}

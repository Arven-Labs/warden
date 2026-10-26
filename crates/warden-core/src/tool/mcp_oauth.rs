//! OAuth 2.0 client for the MCP streamable-HTTP transport (PENDING.md P26) — the piece
//! `tool/mcp.rs`'s `connect_http` deliberately left out (static headers only). Almost all of the
//! actual OAuth mechanics (RFC 9728/8414 discovery, Dynamic Client Registration, PKCE, token
//! refresh) come from `rmcp`'s own `auth` feature; this module is the glue between that and
//! Warden's shape: a JSON-file-backed `CredentialStore` so tokens survive app restarts, and a
//! one-shot local HTTP listener to catch the browser redirect during the interactive flow.
//!
//! Two entry points, matching the two places OAuth-authenticated MCP servers get used:
//! `connect_http_oauth` (headless — every `bootstrap()`, using whatever's already on disk) and
//! `authorize_interactively` (run once from the desktop "Connect" button, opens a browser).

use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::service::ServiceExt;
use rmcp::transport::auth::{AuthClient, AuthorizationManager, AuthorizationRequest, CredentialStore, OAuthState, StoredCredentials};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::auth::AuthError;
use rmcp::transport::StreamableHttpClientTransport;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::tool::mcp::McpToolProvider;

/// How long the interactive flow waits for the user to finish the browser consent step before
/// giving up. Generous on purpose — this is a one-time setup action, not a hot path.
const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(300);

/// Connects to an OAuth-protected MCP server using whatever credentials are already on disk —
/// the path every `bootstrap()` takes. Never opens a browser: if there's no usable stored token
/// (never authorized, or the server's issuer changed), it fails with a message pointing at the
/// interactive flow instead, and the caller (`warden-bootstrap`) treats that like any other
/// failed MCP connection — this one server's tools are unavailable, nothing else breaks.
pub async fn connect_http_oauth(server_name: impl Into<String>, url: &str, credential_store_path: &Path) -> anyhow::Result<McpToolProvider> {
    let server_name = server_name.into();

    let mut manager = AuthorizationManager::new(url)
        .await
        .map_err(|err| anyhow::anyhow!("failed to start OAuth discovery for MCP server '{server_name}' at {url}: {err}"))?;
    manager.set_credential_store(FileCredentialStore::new(credential_store_path.to_path_buf()));

    let authorized = manager
        .initialize_from_store()
        .await
        .map_err(|err| anyhow::anyhow!("failed to load stored OAuth credentials for MCP server '{server_name}': {err}"))?;
    if !authorized {
        anyhow::bail!("MCP server '{server_name}' needs OAuth authorization — connect it from Settings first");
    }

    let http_client = reqwest_oauth::Client::new();
    let auth_client = AuthClient::new(http_client, manager);
    let config = StreamableHttpClientTransportConfig::with_uri(url);
    let transport = StreamableHttpClientTransport::with_client(auth_client, config);

    let session = ()
        .serve(transport)
        .await
        .map_err(|err| anyhow::anyhow!("failed to initialize OAuth MCP server '{server_name}' at {url}: {err}"))?;

    Ok(McpToolProvider::from_session(server_name, session))
}

/// Runs the interactive OAuth authorization dance for one MCP server: discovery, Dynamic Client
/// Registration (no pre-registered client support yet — see PENDING.md/ARCHITECTURE.md), opening
/// the authorization URL in the user's browser via `open_browser`, catching the redirect on a
/// local one-shot listener, and exchanging the code for a token. On success the token (and
/// refresh token, if any) is already persisted to `credential_store_path` — `connect_http_oauth`
/// can use it on every subsequent run without repeating this. Safe to call again on an
/// already-authorized server: it short-circuits without touching the browser.
pub async fn authorize_interactively<F>(server_name: &str, url: &str, credential_store_path: &Path, open_browser: F) -> anyhow::Result<()>
where
    F: FnOnce(&str) + Send + 'static,
{
    if already_authorized(url, credential_store_path).await? {
        return Ok(());
    }

    let listener = CallbackListener::bind().await.map_err(|err| anyhow::anyhow!("MCP server '{server_name}': {err}"))?;
    let redirect_uri = listener.redirect_uri();

    let mut pending = PendingAuthorization::begin(server_name, url, credential_store_path, &redirect_uri).await?;

    open_browser(pending.auth_url());

    let callback_query = listener
        .wait(AUTHORIZATION_TIMEOUT)
        .await
        .map_err(|err| anyhow::anyhow!("didn't receive an OAuth callback for MCP server '{server_name}': {err}"))?;
    pending.complete(&format!("{redirect_uri}?{callback_query}")).await
}

/// The one-shot listener on this machine that a browser is sent back to after the person approves (`127.0.0.1`, a free port). The desktop
/// uses it for the hub's sign-in too (P131): it opens the listener, tells the hub its port, and hands the address that arrives back to the hub.
pub struct CallbackListener {
    listener: TcpListener,
    port: u16,
}

impl CallbackListener {
    /// Listens on a free port of `127.0.0.1`.
    pub async fn bind() -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|err| anyhow::anyhow!("failed to bind a local OAuth callback listener: {err}"))?;
        let port = listener
            .local_addr()
            .map_err(|err| anyhow::anyhow!("failed to read the local OAuth callback listener's port: {err}"))?
            .port();
        Ok(Self { listener, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The address the browser is sent back to, which this listener answers.
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// Waits for one request and returns its query string (`code=…&state=…`), answering the browser with a page that says it can close.
    pub async fn wait(self, timeout: Duration) -> anyhow::Result<String> {
        accept_oauth_callback(self.listener, timeout).await
    }
}

/// An authorization that has begun and waits for the browser to come back, for a caller whose browser is not on this machine (the hub,
/// P131). It holds what must never leave this process: the PKCE verifier and the client that Dynamic Client Registration made.
pub struct PendingAuthorization {
    server_name: String,
    state: OAuthState,
    redirect_uri: String,
    auth_url: String,
}

impl PendingAuthorization {
    /// Discovery and Dynamic Client Registration against `url`, with `redirect_uri` as the place the browser will be sent back to.
    /// Nothing needs to listen there: the person copies the address the browser ends up at. The login goes to `credential_store_path`.
    pub async fn begin(server_name: &str, url: &str, credential_store_path: &Path, redirect_uri: &str) -> anyhow::Result<Self> {
        let mut manager = AuthorizationManager::new(url)
            .await
            .map_err(|err| anyhow::anyhow!("failed to start OAuth discovery for MCP server '{server_name}' at {url}: {err}"))?;
        manager.set_credential_store(FileCredentialStore::new(credential_store_path.to_path_buf()));

        let mut state = OAuthState::Unauthorized(manager);
        let request = AuthorizationRequest::new(redirect_uri.to_string()).with_client_name("Warden");
        state
            .start_authorization(request)
            .await
            .map_err(|err| anyhow::anyhow!("failed to start OAuth authorization for MCP server '{server_name}': {err}"))?;

        let auth_url = state
            .get_authorization_url()
            .await
            .map_err(|err| anyhow::anyhow!("failed to build the OAuth authorization URL for MCP server '{server_name}': {err}"))?;
        Ok(Self { server_name: server_name.to_string(), state, redirect_uri: redirect_uri.to_string(), auth_url })
    }

    /// The address the person opens to sign in.
    pub fn auth_url(&self) -> &str {
        &self.auth_url
    }

    /// The address the browser is sent back to, which `complete` requires the callback to start with.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Finishes with `callback_url`, the address the browser ended at. It has to be at this flow's `redirect_uri` (anything else is
    /// refused before it reaches the token exchange) and the `state` in it is checked against the flow's own. On success the token is
    /// already persisted to the credential store the flow began with.
    pub async fn complete(&mut self, callback_url: &str) -> anyhow::Result<()> {
        let callback_url = callback_url.trim();
        let at_redirect = callback_url.strip_prefix(self.redirect_uri.as_str()).is_some_and(|rest| rest.starts_with('?'));
        if !at_redirect {
            anyhow::bail!("that is not the address the browser was sent back to (it starts with {}?)", self.redirect_uri);
        }
        self.state
            .handle_callback_url(callback_url)
            .await
            .map_err(|err| anyhow::anyhow!("OAuth callback for MCP server '{}' failed: {err}", self.server_name))?;
        Ok(())
    }
}

/// Deletes any stored credentials for a server, so its next connection attempt starts a fresh
/// authorization instead of reusing (or trying to refresh) an old token — the "Disconnect"
/// affordance next to "Connect" in Settings.
pub async fn forget_credentials(credential_store_path: &Path) -> anyhow::Result<()> {
    FileCredentialStore::new(credential_store_path.to_path_buf())
        .clear()
        .await
        .map_err(|err| anyhow::anyhow!("failed to clear stored OAuth credentials at {}: {err}", credential_store_path.display()))
}

/// Whether `credential_store_path` already holds a usable login for the server at `url`.
pub async fn already_authorized(url: &str, credential_store_path: &Path) -> anyhow::Result<bool> {
    let mut manager = AuthorizationManager::new(url)
        .await
        .map_err(|err| anyhow::anyhow!("failed to start OAuth discovery at {url}: {err}"))?;
    manager.set_credential_store(FileCredentialStore::new(credential_store_path.to_path_buf()));
    manager
        .initialize_from_store()
        .await
        .map_err(|err| anyhow::anyhow!("failed to load stored OAuth credentials: {err}"))
}

/// Accepts exactly one HTTP connection, reads only enough to pull the request's query string
/// (the redirect carries `code`/`state` there — everything else about the request is ignored),
/// replies with a minimal page telling the user they can close the tab, and returns. No HTTP
/// library needed for a single fire-and-forget request/response.
async fn accept_oauth_callback(listener: TcpListener, timeout: Duration) -> anyhow::Result<String> {
    let (stream, _) = tokio::time::timeout(timeout, listener.accept())
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for the browser to complete authorization"))?
        .map_err(|err| anyhow::anyhow!("failed to accept the OAuth callback connection: {err}"))?;

    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .await
        .map_err(|err| anyhow::anyhow!("failed to read the OAuth callback request: {err}"))?;
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("malformed OAuth callback request: {request_line:?}"))?;
    let query = path.split_once('?').map(|(_, query)| query).unwrap_or("").to_string();

    // Drain the rest of the request headers so the browser doesn't see a broken connection.
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await.unwrap_or(0);
        if bytes_read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }

    let body = "<html><body>Warden: authorization complete. You can close this window.</body></html>";
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
    let mut stream = reader.into_inner();
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;

    if query.is_empty() {
        anyhow::bail!("OAuth callback request had no query string (path was {path:?})");
    }
    Ok(query)
}

/// `CredentialStore` backed by one JSON file per server. Plaintext, same as every other secret
/// Warden stores today (API keys in `config.toml` — see ARCHITECTURE.md); not a new security
/// posture, just this one applied to a new kind of secret.
struct FileCredentialStore {
    path: PathBuf,
}

impl FileCredentialStore {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait::async_trait]
impl CredentialStore for FileCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        match tokio::fs::read(&self.path).await {
            Ok(bytes) => {
                let credentials = serde_json::from_slice(&bytes)
                    .map_err(|err| AuthError::InternalError(format!("failed to parse {}: {err}", self.path.display())))?;
                Ok(Some(credentials))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(AuthError::InternalError(format!("failed to read {}: {err}", self.path.display()))),
        }
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| AuthError::InternalError(format!("failed to create {}: {err}", parent.display())))?;
        }
        let bytes = serde_json::to_vec_pretty(&credentials)
            .map_err(|err| AuthError::InternalError(format!("failed to serialize OAuth credentials: {err}")))?;
        tokio::fs::write(&self.path, bytes)
            .await
            .map_err(|err| AuthError::InternalError(format!("failed to write {}: {err}", self.path.display())))
    }

    async fn clear(&self) -> Result<(), AuthError> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(AuthError::InternalError(format!("failed to remove {}: {err}", self.path.display()))),
        }
    }
}

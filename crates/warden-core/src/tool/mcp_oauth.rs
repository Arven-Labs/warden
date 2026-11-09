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
        let bytes = serde_json::to_vec_pretty(&credentials)
            .map_err(|err| AuthError::InternalError(format!("failed to serialize OAuth credentials: {err}")))?;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || write_login_atomically(&path, &bytes))
            .await
            .map_err(|err| AuthError::InternalError(format!("the write of {} did not finish: {err}", self.path.display())))?
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

/// Writes a login so that nobody sees it half written and nobody but its owner can read it (P-36, 9a and 9b): the bytes go to a
/// temporary file in the same folder (the same filesystem, so the `rename` is atomic) that is born `0600`, and the `rename` swaps it
/// in. A reader that already had the old file open keeps reading the old one whole; a failure leaves the old login as it was. A
/// folder that is missing is created `0700`; one that exists is left as it is (it may be the operator's).
fn write_login_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    create_private_dir(parent)?;
    let tmp = parent.join(temp_login_name());
    let result = write_then_swap(&tmp, path, bytes);
    if result.is_err() {
        // Best effort: if the write never created it this fails too, and the original error is the one that matters.
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn write_then_swap(tmp: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    // `create_new`: never open a file that is already there, so a planted link at the temporary name is not followed.
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    // Data on disk before the name changes: otherwise a power cut can persist the rename and leave an empty login under it.
    file.sync_all()?;
    drop(file);
    std::fs::rename(tmp, path)
}

/// Like `create_dir_all`, but every folder it has to create gets `0o700` on unix; a folder that exists is not touched. The same
/// rule as `warden-sync`'s `create_private_dir`, which this crate cannot use (`warden-sync` depends on `warden-core`).
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// `.tmp-` and 16 lowercase hex: the name the arca takes for its own work file (`arca_core::is_reserved_name`). The arca never
/// snapshots such a file and the Warden's guard of the logins folder lets it stay, so a snapshot that lands between the write and the
/// `rename` does not stop the sync cycle. A name that looked like a login (`github.json.tmp`) would be refused there.
fn temp_login_name() -> String {
    format!(".tmp-{:016x}", rand::random::<u64>())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path no other test shares (pid, a process-wide counter and the clock). It is not created.
    fn unique_dir(name: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("warden-login-write-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)))
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_new_login_is_complete_private_and_leaves_nothing_behind() {
        let dir = unique_dir("new");
        let path = dir.join("github.json");
        write_login_atomically(&path, br#"{"token":"t1"}"#).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"token":"t1"}"#);
        #[cfg(unix)]
        assert_eq!(mode_of(&path), 0o600, "a token is for its owner only, from the moment the file exists");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["github.json"], "no temporary file is left in the logins folder");
    }

    #[cfg(unix)]
    #[test]
    fn saving_over_an_old_open_login_gives_it_the_new_content_and_closes_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = unique_dir("old-mode");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("github.json");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_login_atomically(&path, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(mode_of(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_reader_holding_the_old_login_still_reads_it_whole() {
        use std::io::Read;
        let dir = unique_dir("reader");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("github.json");
        std::fs::write(&path, "old and complete").unwrap();
        let mut reader = std::fs::File::open(&path).unwrap();
        write_login_atomically(&path, b"new").unwrap();
        let mut seen = String::new();
        reader.read_to_string(&mut seen).unwrap();
        assert_eq!(seen, "old and complete", "the file was swapped, not truncated and rewritten in place");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn a_destination_that_cannot_be_replaced_fails_and_leaves_no_temporary_file() {
        let dir = unique_dir("blocked");
        let path = dir.join("github.json");
        std::fs::create_dir_all(&path).unwrap();
        assert!(write_login_atomically(&path, b"new").is_err());
        assert!(path.is_dir(), "the destination is untouched");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["github.json"], "no temporary file is left behind");
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_folder_is_created_private() {
        let dir = unique_dir("missing");
        let path = dir.join("deep").join("mcp_oauth").join("github.json");
        write_login_atomically(&path, b"t").unwrap();
        assert_eq!(mode_of(path.parent().unwrap()), 0o700);
        assert_eq!(mode_of(&dir.join("deep")), 0o700);
    }

    #[test]
    fn the_temporary_name_is_one_the_arca_takes_for_its_own_work_file() {
        // `arca_core::is_reserved_name`: `.tmp-` and 16 lowercase hex. warden-core cannot depend on the arca, so the rule is
        // pinned here by text; `warden-server/tests/arca_mode.rs` shows the arca tolerates such a name in the logins folder.
        let name = temp_login_name();
        let hex = name.strip_prefix(".tmp-").unwrap_or_else(|| panic!("{name:?} does not start with .tmp-"));
        assert_eq!(hex.len(), 16, "{name:?}");
        assert!(hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')), "{name:?}");
        assert_ne!(name, temp_login_name(), "two writes at once must not share a temporary file");
    }
}

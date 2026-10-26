//! The listening half of signing a hub in to an OAuth MCP server from the desktop (P131). The hub does discovery and registration and
//! finishes the exchange; this computer's job is to be where the browser is sent back to. The screen opens a listener (`mcp_login_listen`),
//! gives the hub its port, opens the page the hub answers with, then waits (`mcp_login_wait`) for the query that arrives and hands it to the
//! hub. One sign-in at a time: a new listener replaces the old one, and `mcp_login_cancel` ends a wait that is under way.

use std::sync::Mutex;
use std::time::Duration;

use tauri::State;
use tokio_util::sync::CancellationToken;
use warden_core::tool::mcp_oauth::CallbackListener;

/// How long the person has to approve in the browser before the wait gives up.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// The listener waiting for the browser, between `mcp_login_listen` and `mcp_login_wait`, and what ends the wait that follows.
#[derive(Default)]
pub struct McpLoginListener {
    listener: Mutex<Option<CallbackListener>>,
    cancel: Mutex<CancellationToken>,
}

/// Opens a listener on a free port of `127.0.0.1` and returns the port, for `BeginMcpOauth`'s `redirectPort`.
#[tauri::command]
pub async fn mcp_login_listen(state: State<'_, McpLoginListener>) -> Result<u16, String> {
    let listener = CallbackListener::bind().await.map_err(|e| format!("{e:#}"))?;
    let port = listener.port();
    // A sign-in that was still waiting is over: the new one replaces it.
    let mut cancel = state.cancel.lock().map_err(|_| UNAVAILABLE.to_string())?;
    cancel.cancel();
    *cancel = CancellationToken::new();
    *state.listener.lock().map_err(|_| UNAVAILABLE.to_string())? = Some(listener);
    Ok(port)
}

/// Waits for the browser to come back to the listener and returns the query (`code=…&state=…`). Gives up after five minutes, or when
/// `mcp_login_cancel` is called.
#[tauri::command]
pub async fn mcp_login_wait(state: State<'_, McpLoginListener>) -> Result<String, String> {
    let token = state.cancel.lock().map_err(|_| UNAVAILABLE.to_string())?.clone();
    let listener = state.listener.lock().map_err(|_| UNAVAILABLE.to_string())?.take();
    let Some(listener) = listener else {
        return Err("no sign-in is waiting; start it again".to_string());
    };
    tokio::select! {
        query = listener.wait(LOGIN_TIMEOUT) => query.map_err(|e| format!("{e:#}")),
        _ = token.cancelled() => Err("the sign-in was cancelled".to_string()),
    }
}

/// Ends the sign-in: drops the listener and stops a wait under way (the hub refused, or the person gave up), so the port is free again.
#[tauri::command]
pub fn mcp_login_cancel(state: State<'_, McpLoginListener>) {
    if let Ok(mut held) = state.listener.lock() {
        *held = None;
    }
    if let Ok(cancel) = state.cancel.lock() {
        cancel.cancel();
    }
}

const UNAVAILABLE: &str = "the sign-in listener is unavailable";

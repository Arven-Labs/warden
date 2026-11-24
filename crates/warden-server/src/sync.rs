//! Vault sync from a client (P61): the hub's `SyncRunner` state for the web's Sync screen, and the
//! same "sync now", init and pairing as `warden-server sync`. Reading the state is open to any
//! paired device; the actions ask for the pairing key again, with the same 1 s wait and the same
//! per-hub lock as a settings save, so all of them share one guessing rate. The lock is only held
//! for the key check: a sync round or a pairing can take a while and must not hold up a save.
//!
//! The hub can also show a pairing code (P88) for another device to join through. That code is
//! only ever in the reply to the `PairHost` that asked for it, never in the status any paired
//! device can read: whoever has it gets the vault key.

use std::net::Ipv4Addr;

use warden_bootstrap::auto_sync::{PairingResult, SyncBackend, SyncReport, SyncRunner, SyncState};
use warden_server_protocol::protocol::{SyncActionDto, SyncBackendDto, SyncPairingDto, SyncPulledDto, SyncPushedDto, SyncRoundDto, SyncStatusDto};
use warden_server_protocol::ServerMessage;

use crate::settings::{keys_match, SettingsHost, SharedOrchestrator, WRONG_KEY_DELAY};

const NO_SYNC: &str = "this hub doesn't offer vault sync over the network";

/// What the automatic loop did not do because a key rotation is waiting for confirmation. The DTO has no field for "paused", and a
/// quiet round (no pull, no push, no error) reads as "nothing changed", so the web shows this in its error banner.
const PAUSED_FOR_ROTATION: &str = "auto-sync paused: a key rotation is waiting for confirmation";

pub fn round_dto(report: &SyncReport) -> SyncRoundDto {
    SyncRoundDto {
        at_ms: report.at_ms,
        pulled: report.pulled.as_ref().map(|p| SyncPulledDto { files_written: p.files_written, files_deleted: p.files_deleted, config_updated: p.config_updated }),
        pushed: report.pushed.as_ref().map(|p| SyncPushedDto { commit_sha: p.commit_sha.clone(), files_changed: p.files_changed }),
        error: report.error.clone().or_else(|| report.paused_for_rotation.then(|| PAUSED_FOR_ROTATION.to_string())),
    }
}

fn status_dto(state: SyncState) -> SyncStatusDto {
    SyncStatusDto {
        backend: match state.backend {
            SyncBackend::NotSetUp => SyncBackendDto::NotSetUp,
            SyncBackend::Git => SyncBackendDto::Git,
            SyncBackend::Arweave => SyncBackendDto::Arweave,
            SyncBackend::Arca => SyncBackendDto::Arca,
        },
        git_remote: state.git_remote,
        last_synced_at_ms: state.last_synced_at_ms,
        pending_vault_changes: state.pending_vault_changes,
        pending_config_changed: state.pending_config_changed,
        pending_oauth_changes: state.pending_oauth_changes,
        last_round: state.last_round.as_ref().map(round_dto),
        hosting_until_ms: state.hosting_until_ms,
        last_pairing: state.last_pairing.map(|PairingResult { at_ms, error }| SyncPairingDto { at_ms, error }),
    }
}

fn sync_error(request_id: u64, message: String, auth_rejected: bool) -> ServerMessage {
    ServerMessage::SyncError { request_id, message, auth_rejected }
}

/// Answers `RequestSyncStatus`.
pub fn handle_sync_status(runner: Option<&SyncRunner>, request_id: u64) -> ServerMessage {
    let Some(runner) = runner else {
        return sync_error(request_id, NO_SYNC.to_string(), false);
    };
    status_reply(runner, request_id, None)
}

fn status_reply(runner: &SyncRunner, request_id: u64, pairing_code: Option<String>) -> ServerMessage {
    match runner.state() {
        Ok(state) => ServerMessage::SyncStatus { request_id, status: status_dto(state), pairing_code },
        Err(err) => sync_error(request_id, format!("{err:#}"), false),
    }
}

/// What a sync action needs from the hub besides the runner: the pairing key it checks and, for a
/// round that brings a new `config.toml`, what to rebuild the orchestrator with.
pub struct SyncAccess<'a> {
    pub runner: Option<&'a SyncRunner>,
    pub lock: &'a tokio::sync::Mutex<()>,
    pub auth_key: &'a str,
    pub settings: Option<&'a dyn SettingsHost>,
    pub shared: &'a SharedOrchestrator,
}

/// Answers `SyncAction` with the state after it.
pub async fn handle_sync_action(access: &SyncAccess<'_>, request_id: u64, pairing_key: &str, action: SyncActionDto) -> ServerMessage {
    let Some(runner) = access.runner else {
        return sync_error(request_id, NO_SYNC.to_string(), false);
    };
    {
        let _serialized = access.lock.lock().await;
        if !keys_match(pairing_key, access.auth_key) {
            tokio::time::sleep(WRONG_KEY_DELAY).await;
            return sync_error(request_id, "wrong pairing key".to_string(), true);
        }
    }
    let result = match action {
        SyncActionDto::SyncNow => {
            let report = runner.run_once().await;
            if report.needs_reload() {
                reload_orchestrator(access.settings, access.shared).await;
            }
            Ok(())
        }
        SyncActionDto::Init => runner.init_fresh().await,
        SyncActionDto::PairHost => {
            return match runner.start_hosting().await {
                Ok(shown) => status_reply(runner, request_id, Some(shown.code)),
                Err(err) => sync_error(request_id, format!("{err:#}"), false),
            };
        }
        SyncActionDto::CancelPairHost => {
            runner.cancel_hosting();
            Ok(())
        }
        SyncActionDto::PairJoin { code, host } => match host.as_deref().map(str::trim).filter(|h| !h.is_empty()).map(str::parse::<Ipv4Addr>) {
            Some(Err(_)) => Err(anyhow::anyhow!("'{}' is not an IPv4 address", host.unwrap_or_default().trim())),
            Some(Ok(host)) => runner.pair_join(code.trim(), Some(host)).await,
            None => runner.pair_join(code.trim(), None).await,
        },
    };
    match result {
        Ok(()) => status_reply(runner, request_id, None),
        Err(err) => sync_error(request_id, format!("{err:#}"), false),
    }
}

/// A sync round brought another device's `config.toml`: rebuild the orchestrator from it, the
/// same way a settings save does. Without a settings host there is nothing to rebuild with, and a
/// file the hub can't start with leaves the running orchestrator in place.
pub async fn reload_orchestrator(settings: Option<&dyn SettingsHost>, shared: &SharedOrchestrator) {
    let Some(host) = settings else {
        tracing::warn!("sync brought a new config.toml; restart the hub to use it");
        return;
    };
    match host.build().await {
        Ok(orchestrator) => {
            host.installed(&orchestrator);
            shared.replace(orchestrator);
            tracing::info!("sync brought a new config.toml; reloaded");
        }
        Err(err) => tracing::warn!("sync brought a config.toml this hub can't start with, kept the old one: {err:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use warden_core::model::{ChatStream, Message, ModelProvider};
    use warden_core::orchestrator::Orchestrator;
    use warden_core::tool::ToolSpec;

    const KEY: &str = "pairing-key-0123456789-0123456789";

    fn round(error: Option<&str>, paused_for_rotation: bool) -> SyncReport {
        SyncReport { at_ms: 42, backend: SyncBackend::Arca, pulled: None, pushed: None, error: error.map(String::from), paused_for_rotation }
    }

    /// The web has no field for "paused", and a round with nothing in it reads as "nothing changed": a round the loop skipped for a
    /// rotation must show up in the error banner.
    #[test]
    fn a_round_paused_for_a_rotation_reaches_the_web_as_an_error_that_says_so() {
        let dto = round_dto(&round(None, true));
        assert_eq!(dto.at_ms, 42);
        assert_eq!(dto.error.as_deref(), Some("auto-sync paused: a key rotation is waiting for confirmation"));
        assert!(dto.pulled.is_none() && dto.pushed.is_none());
    }

    #[test]
    fn a_round_that_was_not_paused_reaches_the_web_as_before() {
        assert_eq!(round_dto(&round(None, false)).error, None, "a quiet round stays quiet");
        assert_eq!(round_dto(&round(Some("pull: connection refused"), false)).error.as_deref(), Some("pull: connection refused"));
        // An error the round did have is not hidden behind the pause text.
        assert_eq!(round_dto(&round(Some("pull: connection refused"), true)).error.as_deref(), Some("pull: connection refused"));
    }

    fn runner(name: &str) -> (SyncRunner, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "warden-hub-sync-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(dir.join("vault")).unwrap();
        let runner = SyncRunner::new(dir.join("vault"), dir.join("config.toml"), dir.join("secrets.json"), dir.join("manifest.json"), dir.join("git"));
        (runner, dir)
    }

    /// These tests never run a chat turn.
    struct NoModel;

    #[async_trait::async_trait]
    impl ModelProvider for NoModel {
        async fn chat_stream(&self, _messages: Vec<Message>, _tools: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
            anyhow::bail!("no model in these tests")
        }
    }

    fn shared() -> SharedOrchestrator {
        let vault = std::sync::Arc::new(warden_core::memory::Vault::new(std::env::temp_dir().join("warden-hub-sync-unused")));
        SharedOrchestrator::new(Orchestrator::new(std::sync::Arc::new(NoModel), vault))
    }

    async fn act(runner: &SyncRunner, key: &str, action: SyncActionDto) -> ServerMessage {
        let lock = tokio::sync::Mutex::new(());
        let shared = shared();
        let access = SyncAccess { runner: Some(runner), lock: &lock, auth_key: KEY, settings: None, shared: &shared };
        handle_sync_action(&access, 7, key, action).await
    }

    #[tokio::test]
    async fn init_needs_the_pairing_key_then_sets_up_arweave() {
        let (runner, _dir) = runner("init");
        let ServerMessage::SyncStatus { status, .. } = handle_sync_status(Some(&runner), 1) else { panic!() };
        assert_eq!(status.backend, SyncBackendDto::NotSetUp);

        let started = std::time::Instant::now();
        let reply = act(&runner, "wrong", SyncActionDto::Init).await;
        assert!(matches!(reply, ServerMessage::SyncError { auth_rejected: true, .. }), "{reply:?}");
        assert!(started.elapsed() >= WRONG_KEY_DELAY);
        assert_eq!(runner.state().unwrap().backend, SyncBackend::NotSetUp, "nothing ran");

        let reply = act(&runner, KEY, SyncActionDto::Init).await;
        let ServerMessage::SyncStatus { request_id: 7, status, .. } = reply else { panic!("{reply:?}") };
        assert_eq!(status.backend, SyncBackendDto::Arweave);

        let reply = act(&runner, KEY, SyncActionDto::Init).await;
        assert!(matches!(reply, ServerMessage::SyncError { auth_rejected: false, .. }), "a second init would replace the key: {reply:?}");
    }

    #[tokio::test]
    async fn sync_now_reports_the_round_and_a_bad_host_is_refused() {
        let (runner, _dir) = runner("now");
        let reply = act(&runner, KEY, SyncActionDto::SyncNow).await;
        let ServerMessage::SyncStatus { status, .. } = reply else { panic!("{reply:?}") };
        let round = status.last_round.expect("the round is reported");
        assert_eq!((round.pulled, round.pushed, round.error), (None, None, None));

        let reply = act(&runner, KEY, SyncActionDto::PairJoin { code: "AB12".into(), host: Some("my-laptop".into()) }).await;
        let ServerMessage::SyncError { message, auth_rejected: false, .. } = reply else { panic!("{reply:?}") };
        assert!(message.contains("IPv4"), "{message}");
    }

    #[tokio::test]
    async fn the_pairing_code_only_goes_to_whoever_asked_with_the_key() {
        let (runner, _dir) = runner("host");
        let reply = act(&runner, KEY, SyncActionDto::PairHost).await;
        assert!(matches!(reply, ServerMessage::SyncError { auth_rejected: false, .. }), "no vault key yet: {reply:?}");
        act(&runner, KEY, SyncActionDto::Init).await;

        let reply = act(&runner, "wrong", SyncActionDto::PairHost).await;
        assert!(matches!(reply, ServerMessage::SyncError { auth_rejected: true, .. }), "{reply:?}");
        assert_eq!(runner.state().unwrap().hosting_until_ms, None, "nothing was opened");

        let reply = act(&runner, KEY, SyncActionDto::PairHost).await;
        let ServerMessage::SyncStatus { status, pairing_code: Some(code), .. } = reply else { panic!("{reply:?}") };
        assert!(status.hosting_until_ms.is_some());
        assert!(!code.is_empty());

        let ServerMessage::SyncStatus { status, pairing_code, .. } = handle_sync_status(Some(&runner), 2) else { panic!() };
        assert_eq!((status.hosting_until_ms.is_some(), pairing_code), (true, None), "reading the status never shows the code");

        let reply = act(&runner, KEY, SyncActionDto::CancelPairHost).await;
        let ServerMessage::SyncStatus { status, pairing_code: None, .. } = reply else { panic!("{reply:?}") };
        assert_eq!(status.hosting_until_ms, None);
        assert_eq!(status.last_pairing.and_then(|p| p.error).as_deref(), Some("cancelled"));
    }

    #[test]
    fn a_hub_without_a_runner_says_so() {
        assert!(matches!(handle_sync_status(None, 1), ServerMessage::SyncError { auth_rejected: false, .. }));
    }
}

import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { intlTag, t } from "../i18n";
import type { GitPullResult, GitPushResult, SyncPullResult, SyncPushBegin, SyncPushResult, SyncStatus } from "../types";

function formatTimestamp(ms: number | null): string {
  if (ms === null) return t("sync.never");
  return new Date(ms).toLocaleString(intlTag());
}

function StatusCard({ status }: { status: SyncStatus }) {
  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("sync.status")}</h3>
      </div>
      <div className="sync-status-grid">
        <div className="sync-status-item">
          <span className="sync-status-label">{t("sync.paired")}</span>
          <span className="sync-status-value">{status.paired ? t("sync.yes") : t("sync.no")}</span>
        </div>
        <div className="sync-status-item">
          <span className="sync-status-label">{t("sync.ownerAddress")}</span>
          <span className="sync-status-value sync-status-value--mono">{status.ownerAddress ?? "—"}</span>
        </div>
        <div className="sync-status-item">
          <span className="sync-status-label">{t("sync.lastSync")}</span>
          <span className="sync-status-value">{formatTimestamp(status.lastSyncedAtMs)}</span>
        </div>
        <div className="sync-status-item">
          <span className="sync-status-label">{t("sync.pending")}</span>
          <span className="sync-status-value">
            {t("sync.pendingFiles", { count: status.pendingVaultChanges })}
            {status.pendingConfigChanged ? t("sync.pendingConfig") : ""}
            {status.pendingOauthChanges > 0 ? t("sync.pendingLogins", { count: status.pendingOauthChanges }) : ""}
          </span>
        </div>
        <div className="sync-status-item">
          <span className="sync-status-label">{t("sync.ignoreRules")}</span>
          <span className="sync-status-value">{status.syncignorePatternCount}</span>
        </div>
      </div>
    </section>
  );
}

function SyncView() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [pushBegin, setPushBegin] = useState<SyncPushBegin | null>(null);
  const [pushResult, setPushResult] = useState<SyncPushResult | null>(null);
  const [pullResult, setPullResult] = useState<SyncPullResult | null>(null);

  const [pairingCode, setPairingCode] = useState<string | null>(null);
  const [pairingStatus, setPairingStatus] = useState<"idle" | "hosting" | "completed" | "failed">("idle");
  const [joinCode, setJoinCode] = useState("");
  const [joinHost, setJoinHost] = useState("");
  const [joining, setJoining] = useState(false);

  const [autoSyncMessage, setAutoSyncMessage] = useState<string | null>(null);

  const [gitSyncConfigured, setGitSyncConfigured] = useState<boolean | null>(null);
  const [gitBusy, setGitBusy] = useState(false);
  const [gitError, setGitError] = useState<string | null>(null);
  const [gitPushResult, setGitPushResult] = useState<GitPushResult | null>(null);
  const [gitPullResult, setGitPullResult] = useState<GitPullResult | null>(null);

  const unlistenRef = useRef<(() => void) | null>(null);

  function refreshStatus() {
    invoke<SyncStatus>("sync_status")
      .then(setStatus)
      .catch((err) => setError(String(err)));
  }

  // Same "refetch on mount" posture as `UsageView`/`SettingsView` — the sidebar remounts this
  // view on each visit, so a stale status right after a change made elsewhere never lingers.
  useEffect(() => {
    refreshStatus();
    invoke<boolean>("git_sync_configured")
      .then(setGitSyncConfigured)
      .catch((err) => setGitError(String(err)));
    return () => {
      unlistenRef.current?.();
    };
  }, []);

  // P71 — the background auto-pull (`sync_cmds::spawn_auto_sync`) emits this whenever it applies
  // a real change on its own, every few minutes, for as long as this view has ever been mounted
  // (the listener itself is cheap to keep registered — it's the pull that's throttled, not this).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ txId: string | null; filesWritten: number; filesDeleted: number; configUpdated: boolean }>("auto-sync-pulled", (event) => {
      refreshStatus();
      const { filesWritten, filesDeleted, configUpdated } = event.payload;
      const parts: string[] = [];
      if (filesWritten > 0) parts.push(t("sync.filesUpdated", { count: filesWritten }));
      if (filesDeleted > 0) parts.push(t("sync.filesRemoved", { count: filesDeleted }));
      if (configUpdated) parts.push(t("sync.configUpdated"));
      setAutoSyncMessage(t("sync.autoPulled", { parts: parts.join(", ") }));
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  // P71 — same as the `auto-sync-pulled` listener above, but for the git backend's automatic push
  // (only ever emitted when git sync is the active backend — Arweave push always stays manual).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ commitSha: string; filesChanged: number; configChanged: boolean }>("auto-sync-pushed", (event) => {
      const { filesChanged, configChanged } = event.payload;
      const parts: string[] = [t("sync.filesSent", { count: filesChanged })];
      if (configChanged) parts.push(t("sync.configUpdated"));
      setAutoSyncMessage(t("sync.autoPushed", { parts: parts.join(", ") }));
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  async function handleInit() {
    setBusy(true);
    setError(null);
    try {
      await invoke("sync_init");
      refreshStatus();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleSend() {
    setBusy(true);
    setError(null);
    setPushResult(null);
    try {
      const begin = await invoke<SyncPushBegin>("sync_push_begin");
      setPushBegin(begin);
      const result = await invoke<SyncPushResult>("sync_push_await");
      setPushResult(result);
      setPushBegin(null);
      refreshStatus();
    } catch (err) {
      setError(String(err));
      setPushBegin(null);
    } finally {
      setBusy(false);
    }
  }

  async function handlePull() {
    setBusy(true);
    setError(null);
    setPullResult(null);
    try {
      const result = await invoke<SyncPullResult>("sync_pull");
      setPullResult(result);
      refreshStatus();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleGitPush() {
    setGitBusy(true);
    setGitError(null);
    setGitPushResult(null);
    try {
      const result = await invoke<GitPushResult | null>("git_sync_push");
      if (result === null) {
        setGitError(t("sync.gitNothing"));
      } else {
        setGitPushResult(result);
      }
    } catch (err) {
      setGitError(String(err));
    } finally {
      setGitBusy(false);
    }
  }

  async function handleGitPull() {
    setGitBusy(true);
    setGitError(null);
    setGitPullResult(null);
    try {
      const result = await invoke<GitPullResult>("git_sync_pull");
      setGitPullResult(result);
    } catch (err) {
      setGitError(String(err));
    } finally {
      setGitBusy(false);
    }
  }

  async function handleStartPairing() {
    setError(null);
    setPairingStatus("hosting");
    try {
      const { code } = await invoke<{ code: string }>("pairing_start");
      setPairingCode(code);
      unlistenRef.current?.();
      const unlistenCompleted = await listen("pairing-completed", () => {
        setPairingStatus("completed");
        refreshStatus();
      });
      const unlistenFailed = await listen<string>("pairing-failed", (event) => {
        setPairingStatus("failed");
        setError(event.payload);
      });
      unlistenRef.current = () => {
        unlistenCompleted();
        unlistenFailed();
      };
    } catch (err) {
      setError(String(err));
      setPairingStatus("idle");
    }
  }

  async function handleJoin(e: React.FormEvent) {
    e.preventDefault();
    setJoining(true);
    setError(null);
    try {
      const host = joinHost.trim();
      await invoke("pairing_join", { code: joinCode.trim(), host: host || null });
      setJoinCode("");
      setJoinHost("");
      refreshStatus();
    } catch (err) {
      setError(String(err));
    } finally {
      setJoining(false);
    }
  }

  if (error && !status) {
    return (
      <div className="settings-view">
        <h2 className="settings-title">{t("sync.title")}</h2>
        <p className="settings-error-banner">{error}</p>
      </div>
    );
  }

  if (!status) {
    return (
      <div className="settings-view">
        <p>{t("sync.loading")}</p>
      </div>
    );
  }

  return (
    <div className="settings-view">
      <h2 className="settings-title">{t("sync.title")}</h2>
      <p className="settings-hint">{t("sync.hint")}</p>

      {error && <p className="settings-error-banner">{error}</p>}
      {autoSyncMessage && <p className="settings-success-banner">{autoSyncMessage}</p>}

      <StatusCard status={status} />

      {!status.paired ? (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("sync.setUp")}</h3>
          </div>
          <p className="settings-hint">{t("sync.setUpHint")}</p>
          <button type="button" className="settings-save-btn" onClick={handleInit} disabled={busy}>
            {t("sync.start")}
          </button>
        </section>
      ) : (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("sync.sendPull")}</h3>
          </div>
          <div className="sync-actions-row">
            <button type="button" className="settings-save-btn" onClick={handleSend} disabled={busy}>
              {t("sync.send")}
            </button>
            <button type="button" className="settings-save-btn" onClick={handlePull} disabled={busy}>
              {t("sync.pull")}
            </button>
          </div>

          {pushBegin && (
            <div className="sync-qr-card">
              <p className="settings-hint">{t("sync.scan", { count: pushBegin.filesChanged, config: pushBegin.configChanged ? t("sync.scanConfig") : "" })}</p>
              <div className="sync-qr-image" dangerouslySetInnerHTML={{ __html: pushBegin.qrSvg }} />
              <p className="settings-hint">{t("sync.waiting")}</p>
            </div>
          )}

          {pushResult && <p className="settings-success-banner">{t("sync.sent", { tx: pushResult.txId, count: pushResult.filesChanged })}</p>}

          {pullResult && (
            <div className="settings-success-banner">
              <p>
                {t("sync.pullDone", {
                  written: pullResult.filesWritten,
                  removed: pullResult.filesDeleted,
                  ignored: pullResult.filesIgnored > 0 ? t("sync.ignored", { count: pullResult.filesIgnored }) : "",
                  config: pullResult.configUpdated ? t("sync.configUpdatedComma") : "",
                })}
              </p>
              {pullResult.warnings.length > 0 && (
                <ul className="sync-warning-list">
                  {pullResult.warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </section>
      )}

      <section className="settings-section">
        <div className="settings-section-header">
          <h3 className="settings-section-title">{t("sync.git")}</h3>
        </div>
        <p className="settings-hint">{t("sync.gitHint")}</p>

        {gitSyncConfigured === false && <p className="settings-hint">{t("sync.gitNotConfigured")}</p>}

        {gitSyncConfigured && (
          <>
            <div className="sync-actions-row">
              <button type="button" className="settings-save-btn" onClick={handleGitPush} disabled={gitBusy}>
                {t("sync.push")}
              </button>
              <button type="button" className="settings-save-btn" onClick={handleGitPull} disabled={gitBusy}>
                {t("sync.pull")}
              </button>
            </div>

            {gitError && <p className="settings-error-banner">{gitError}</p>}

            {gitPushResult && (
              <p className="settings-success-banner">{t("sync.gitSent", { sha: gitPushResult.commitSha.slice(0, 8), count: gitPushResult.filesChanged })}</p>
            )}

            {gitPullResult && (
              <div className="settings-success-banner">
                <p>
                  {t("sync.gitPullDone", {
                    commits: gitPullResult.commitsApplied,
                    written: gitPullResult.filesWritten,
                    removed: gitPullResult.filesDeleted,
                    ignored: gitPullResult.filesIgnored > 0 ? t("sync.ignored", { count: gitPullResult.filesIgnored }) : "",
                    config: gitPullResult.configUpdated ? t("sync.configUpdatedComma") : "",
                  })}
                </p>
                {gitPullResult.warnings.length > 0 && (
                  <ul className="sync-warning-list">
                    {gitPullResult.warnings.map((w) => (
                      <li key={w}>{w}</li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </>
        )}
      </section>

      <section className="settings-section">
        <div className="settings-section-header">
          <h3 className="settings-section-title">{t("sync.pairNew")}</h3>
        </div>

        {status.paired && (
          <div className="sync-pairing-block">
            <p className="settings-hint">{t("sync.showCodeHint")}</p>
            {pairingStatus === "idle" ? (
              <button type="button" className="settings-browse-btn" onClick={handleStartPairing}>
                {t("sync.showCode")}
              </button>
            ) : (
              <>
                {pairingCode && <p className="sync-pairing-code">{pairingCode}</p>}
                {pairingStatus === "hosting" && <p className="settings-hint">{t("sync.waitingDevice")}</p>}
                {pairingStatus === "completed" && <p className="settings-success-banner">{t("sync.pairedDone")}</p>}
              </>
            )}
          </div>
        )}

        <div className="sync-pairing-block">
          <p className="settings-hint">{t("sync.typeCodeHint")}</p>
          <form className="sync-join-form" onSubmit={handleJoin}>
            <input
              type="text"
              className="settings-input"
              placeholder="XXXXXXXX"
              value={joinCode}
              onChange={(e) => setJoinCode(e.target.value)}
              disabled={joining}
            />
            <input
              type="text"
              className="settings-input"
              placeholder={t("sync.ipPlaceholder")}
              value={joinHost}
              onChange={(e) => setJoinHost(e.target.value)}
              disabled={joining}
            />
            <button type="submit" className="settings-save-btn" disabled={joining || joinCode.trim().length === 0}>
              {t("sync.pair")}
            </button>
          </form>
          <p className="settings-hint">{t("sync.ipHint")}</p>
        </div>
      </section>
    </div>
  );
}

export default SyncView;

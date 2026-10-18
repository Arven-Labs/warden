import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { GitPullResult, GitPushResult, SyncPullResult, SyncPushBegin, SyncPushResult, SyncStatus } from "../types";

function formatTimestamp(ms: number | null): string {
  if (ms === null) return "never";
  return new Date(ms).toLocaleString();
}

function StatusCard({ status }: { status: SyncStatus }) {
  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">Status</h3>
      </div>
      <div className="sync-status-grid">
        <div className="sync-status-item">
          <span className="sync-status-label">Paired</span>
          <span className="sync-status-value">{status.paired ? "Yes" : "No"}</span>
        </div>
        <div className="sync-status-item">
          <span className="sync-status-label">Owner address (Arweave)</span>
          <span className="sync-status-value sync-status-value--mono">{status.ownerAddress ?? "—"}</span>
        </div>
        <div className="sync-status-item">
          <span className="sync-status-label">Last sync</span>
          <span className="sync-status-value">{formatTimestamp(status.lastSyncedAtMs)}</span>
        </div>
        <div className="sync-status-item">
          <span className="sync-status-label">Pending</span>
          <span className="sync-status-value">
            {status.pendingVaultChanges} file(s){status.pendingConfigChanged ? " + config.toml" : ""}
            {status.pendingOauthChanges > 0 ? ` + ${status.pendingOauthChanges} MCP login(s)` : ""}
          </span>
        </div>
        <div className="sync-status-item">
          <span className="sync-status-label">.syncignore rules</span>
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
      if (filesWritten > 0) parts.push(`${filesWritten} file(s) updated`);
      if (filesDeleted > 0) parts.push(`${filesDeleted} file(s) removed`);
      if (configUpdated) parts.push("config.toml updated");
      setAutoSyncMessage(`Automatic sync — ${parts.join(", ")}.`);
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
      const parts: string[] = [`${filesChanged} file(s) sent`];
      if (configChanged) parts.push("config.toml updated");
      setAutoSyncMessage(`Automatic sync (git) — ${parts.join(", ")}.`);
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
        setGitError("Nothing to send — the vault and the config already match the last push.");
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
        <h2 className="settings-title">Sync</h2>
        <p className="settings-error-banner">{error}</p>
      </div>
    );
  }

  if (!status) {
    return (
      <div className="settings-view">
        <p>Loading sync…</p>
      </div>
    );
  }

  return (
    <div className="settings-view">
      <h2 className="settings-title">Sync</h2>
      <p className="settings-hint">
        Syncs the vault and config.toml between your devices through Arweave, paid by the TruthID app. Everything is
        encrypted on this device before it leaves — neither TruthID nor Arweave can see the content in plain text.
      </p>

      {error && <p className="settings-error-banner">{error}</p>}
      {autoSyncMessage && <p className="settings-success-banner">{autoSyncMessage}</p>}

      <StatusCard status={status} />

      {!status.paired ? (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">Set up</h3>
          </div>
          <p className="settings-hint">
            Is this the first device? Start a new key. Already have another device with sync set up? Ask it to show a
            pairing code (below).
          </p>
          <button type="button" className="settings-save-btn" onClick={handleInit} disabled={busy}>
            Start sync on this device
          </button>
        </section>
      ) : (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">Send / Pull</h3>
          </div>
          <div className="sync-actions-row">
            <button type="button" className="settings-save-btn" onClick={handleSend} disabled={busy}>
              Send
            </button>
            <button type="button" className="settings-save-btn" onClick={handlePull} disabled={busy}>
              Pull
            </button>
          </div>

          {pushBegin && (
            <div className="sync-qr-card">
              <p className="settings-hint">
                Scan with the TruthID app to approve and publish ({pushBegin.filesChanged} file(s)
                {pushBegin.configChanged ? " + config.toml" : ""}).
              </p>
              <div className="sync-qr-image" dangerouslySetInnerHTML={{ __html: pushBegin.qrSvg }} />
              <p className="settings-hint">Waiting for approval…</p>
            </div>
          )}

          {pushResult && (
            <p className="settings-success-banner">
              Sent — tx {pushResult.txId} ({pushResult.filesChanged} file(s))
            </p>
          )}

          {pullResult && (
            <div className="settings-success-banner">
              <p>
                Pull done — {pullResult.filesWritten} written, {pullResult.filesDeleted} removed
                {pullResult.filesIgnored > 0 ? `, ${pullResult.filesIgnored} ignored` : ""}
                {pullResult.configUpdated ? ", config.toml updated" : ""}.
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
          <h3 className="settings-section-title">Git</h3>
        </div>
        <p className="settings-hint">
          An alternative to Arweave — a git repository of your own. It needs no approval from the phone, so it is the only one
          the app can push by itself every few minutes, besides pulling (see "Automatic sync" above).
        </p>

        {gitSyncConfigured === false && (
          <p className="settings-hint">Set the URL and the token in Settings, under "Sync via Git", to use it.</p>
        )}

        {gitSyncConfigured && (
          <>
            <div className="sync-actions-row">
              <button type="button" className="settings-save-btn" onClick={handleGitPush} disabled={gitBusy}>
                Push
              </button>
              <button type="button" className="settings-save-btn" onClick={handleGitPull} disabled={gitBusy}>
                Pull
              </button>
            </div>

            {gitError && <p className="settings-error-banner">{gitError}</p>}

            {gitPushResult && (
              <p className="settings-success-banner">
                Sent — commit {gitPushResult.commitSha.slice(0, 8)} ({gitPushResult.filesChanged} file(s))
              </p>
            )}

            {gitPullResult && (
              <div className="settings-success-banner">
                <p>
                  Pull done — {gitPullResult.commitsApplied} commit(s), {gitPullResult.filesWritten} written,{" "}
                  {gitPullResult.filesDeleted} removed
                  {gitPullResult.filesIgnored > 0 ? `, ${gitPullResult.filesIgnored} ignored` : ""}
                  {gitPullResult.configUpdated ? ", config.toml updated" : ""}.
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
          <h3 className="settings-section-title">Pair a new device</h3>
        </div>

        {status.paired && (
          <div className="sync-pairing-block">
            <p className="settings-hint">Show this code on the device you want to pair.</p>
            {pairingStatus === "idle" ? (
              <button type="button" className="settings-browse-btn" onClick={handleStartPairing}>
                Show pairing code
              </button>
            ) : (
              <>
                {pairingCode && <p className="sync-pairing-code">{pairingCode}</p>}
                {pairingStatus === "hosting" && <p className="settings-hint">Waiting for the other device…</p>}
                {pairingStatus === "completed" && <p className="settings-success-banner">Paired!</p>}
              </>
            )}
          </div>
        )}

        <div className="sync-pairing-block">
          <p className="settings-hint">Have a code shown on another device? Type it here.</p>
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
              placeholder="IP (optional)"
              value={joinHost}
              onChange={(e) => setJoinHost(e.target.value)}
              disabled={joining}
            />
            <button type="submit" className="settings-save-btn" disabled={joining || joinCode.trim().length === 0}>
              Pair
            </button>
          </form>
          <p className="settings-hint">
            The IP is the one of the device showing the code (a Tailscale one works, like a hub on a VPS). Without it, the search is
            only on the local network.
          </p>
        </div>
      </section>
    </div>
  );
}

export default SyncView;

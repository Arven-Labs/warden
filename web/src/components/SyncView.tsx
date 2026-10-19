import { useCallback, useEffect, useState } from "react";
import { SyncError, type ServerConnection } from "../hub/connection";
import type { SyncAction, SyncRound, SyncStatus } from "../hub/messages";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";

// The hub's vault sync (P61). The agent always works on the hub's own disk; this keeps that disk
// in step with the other devices, every 5 minutes on its own. Here: how it's going, "sync now",
// and — on a hub with no vault key yet — becoming the first device or pairing with one that has
// the key. The git remote itself is set in Settings. Every action asks for the pairing key.
// Once the hub is in a group it can also show a pairing code for another device to join through
// (P88); the code only comes back to the request that asked for it, so it lives in this screen's
// state and nowhere else.

const backendLabel = (backend: SyncStatus["backend"]) => (backend === "notSetUp" ? t("sync.notSetUp") : backend === "git" ? "Git" : t("sync.arweave"));

const dateFormatter = { format: (date: Date) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(date) };

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function roundSummary(round: SyncRound): string {
  const parts: string[] = [];
  if (round.pulled) {
    parts.push(`${t("sync.received", { count: round.pulled.filesWritten })}${round.pulled.filesDeleted ? t("sync.removed", { count: round.pulled.filesDeleted }) : ""}`);
    if (round.pulled.configUpdated) parts.push(t("sync.configUpdated"));
  }
  if (round.pushed) parts.push(t("sync.sent", { count: round.pushed.filesChanged }));
  if (parts.length === 0 && !round.error) parts.push(t("sync.nothingChanged"));
  return parts.join(", ");
}

/** What the pairing-key prompt is about to run. */
type Pending = { action: SyncAction; label: string };

/** The code this screen asked the hub to show, and the key it was asked with (to stop it without typing it again). */
type Hosting = { code: string; pairingKey: string };

const HOSTING_POLL_MS = 3000;

export default function SyncView({ conn }: { conn: ServerConnection | null }) {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [code, setCode] = useState("");
  const [host, setHost] = useState("");
  const [hosting, setHosting] = useState<Hosting | null>(null);

  const load = useCallback(async () => {
    if (!conn) return;
    setError(null);
    try {
      setStatus(await conn.requestSyncStatus());
    } catch (err) {
      setError(message(err));
    }
  }, [conn]);

  useEffect(() => {
    void load();
  }, [load]);

  // While a code is up, watch for the other device joining (or the code running out).
  const hostingUntil = status?.hostingUntilMs;
  useEffect(() => {
    if (!hostingUntil) return;
    const timer = window.setInterval(() => void load(), HOSTING_POLL_MS);
    return () => window.clearInterval(timer);
  }, [hostingUntil, load]);

  useEffect(() => {
    if (status && !status.hostingUntilMs) setHosting(null);
  }, [status]);

  function cancel() {
    setPending(null);
    setPairingKey("");
    setKeyError(null);
  }

  async function confirm() {
    if (!conn || !pending) return;
    setBusy(true);
    setKeyError(null);
    try {
      if (pending.action.kind === "pairHost") {
        const shown = await conn.startPairHost(pairingKey);
        setHosting({ code: shown.code, pairingKey });
        setStatus(shown.status);
        cancel();
        return;
      }
      setStatus(await conn.syncAction(pairingKey, pending.action));
      if (pending.action.kind === "pairJoin") {
        setCode("");
        setHost("");
      }
      cancel();
    } catch (err) {
      if (err instanceof SyncError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        cancel();
        setError(message(err));
      }
    } finally {
      setBusy(false);
    }
  }

  async function stopHosting() {
    if (!conn || !hosting) return;
    setBusy(true);
    try {
      setStatus(await conn.syncAction(hosting.pairingKey, { kind: "cancelPairHost" }));
    } catch (err) {
      setError(message(err));
    } finally {
      setBusy(false);
    }
  }

  const keyPrompt = pending && (
    <form
      className="settings-confirm devices-confirm"
      onSubmit={(e) => {
        e.preventDefault();
        void confirm();
      }}
    >
      <label className="settings-field">
        {t("pairingKey.label")}
        <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
        <span className="field-hint">{t("sync.pairKeyHint")}</span>
      </label>
      {keyError && <p className="error-banner">{keyError}</p>}
      {busy && pending.action.kind === "pairJoin" && <p className="skills-hint">{t("sync.looking")}</p>}
      <div className="skills-actions">
        <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
          {busy ? t("common.wait") : pending.label}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={cancel}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );

  return (
    <div className="usage-view sync-view">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("sync.intro")}</span>
        <button type="button" className="link-button" onClick={() => void load()} disabled={!conn || busy}>
          {t("devices.reload")}
        </button>
      </div>

      {error && <p className="error-banner">{error}</p>}
      {!status && !error && <p className="skills-hint">{t("common.loading")}</p>}

      {status && (
        <section className="usage-section settings-card">
          <h2 className="usage-heading">{t("sync.target", { backend: backendLabel(status.backend) })}</h2>
          {status.gitRemote && <p className="skills-item-description">{rich("sync.repository", { remote: <code>{status.gitRemote}</code> })}</p>}
          {status.backend !== "notSetUp" && (
            <p className="skills-item-description">
              {status.pendingVaultChanges === 0 && !status.pendingConfigChanged && !status.pendingOauthChanges
                ? t("sync.nothingWaiting")
                : t("sync.waiting", {
                    files: status.pendingVaultChanges,
                    config: status.pendingConfigChanged ? t("sync.andSettings") : "",
                    oauth: status.pendingOauthChanges ? t("sync.andLogins", { count: status.pendingOauthChanges }) : "",
                  })}{" "}
              {status.lastSyncedAtMs ? t("sync.lastSync", { when: dateFormatter.format(new Date(status.lastSyncedAtMs)) }) : t("sync.neverSynced")}
            </p>
          )}
          {status.lastRound && (
            <p className="skills-item-description">
              {t("sync.lastRound", { when: dateFormatter.format(new Date(status.lastRound.atMs)), summary: roundSummary(status.lastRound) })}
            </p>
          )}
          {status.lastRound?.error && <p className="error-banner">{status.lastRound.error}</p>}
          {status.backend === "arweave" && <p className="skills-hint">{t("sync.arweaveNote")}</p>}
          {status.backend !== "notSetUp" &&
            (pending?.action.kind === "syncNow" ? (
              keyPrompt
            ) : (
              <div className="skills-actions">
                <button
                  type="button"
                  className="primary-button"
                  disabled={!conn || pending !== null}
                  onClick={() => setPending({ action: { kind: "syncNow" }, label: t("sync.syncNow") })}
                >
                  {t("sync.syncNow")}
                </button>
              </div>
            ))}
        </section>
      )}

      {status && status.backend !== "notSetUp" && (
        <section className="usage-section settings-card">
          <h2 className="usage-heading">{t("sync.pairTitle")}</h2>
          {hosting && status.hostingUntilMs ? (
            <>
              <p className="skills-hint">{rich("sync.pairOther", { cmd: <code>/sync pair &lt;code&gt; &lt;IP&gt;</code> })}</p>
              <p className="sync-pairing-code">{hosting.code}</p>
              <p className="skills-item-description">
                {rich("sync.validFor", {
                  minutes: Math.max(1, Math.ceil((status.hostingUntilMs - Date.now()) / 60_000)),
                  host: <code>{window.location.hostname}</code>,
                })}
              </p>
              <div className="skills-actions">
                <button type="button" className="link-button" disabled={busy || !conn} onClick={() => void stopHosting()}>
                  {t("sync.stopShowing")}
                </button>
              </div>
            </>
          ) : pending?.action.kind === "pairHost" ? (
            keyPrompt
          ) : (
            <>
              <p className="skills-hint">
                {t("sync.pairIntro")}
                {status.hostingUntilMs && t("sync.codeOpen")}
              </p>
              {status.lastPairing && (
                <p className={status.lastPairing.error ? "error-banner" : "skills-item-description"}>
                  {t("sync.lastPairing", { when: dateFormatter.format(new Date(status.lastPairing.atMs)) })}
                  {status.lastPairing.error ? status.lastPairing.error : t("sync.pairedOk")}
                </p>
              )}
              <div className="skills-actions">
                <button
                  type="button"
                  className="primary-button"
                  disabled={!conn || pending !== null}
                  onClick={() => setPending({ action: { kind: "pairHost" }, label: t("sync.showCode") })}
                >
                  {t("sync.showACode")}
                </button>
              </div>
            </>
          )}
        </section>
      )}

      {status?.backend === "notSetUp" && (
        <>
          <section className="usage-section settings-card">
            <h2 className="usage-heading">{t("sync.joinTitle")}</h2>
            <p className="skills-hint">{rich("sync.joinIntro", { cmd: <code>/sync pair</code> })}</p>
            {pending?.action.kind === "pairJoin" ? (
              keyPrompt
            ) : (
              <form
                className="settings-card"
                onSubmit={(e) => {
                  e.preventDefault();
                  const trimmedHost = host.trim();
                  setPending({ action: { kind: "pairJoin", code: code.trim(), ...(trimmedHost && { host: trimmedHost }) }, label: t("sync.pair") });
                }}
              >
                <label className="settings-field">
                  {t("sync.pairingCode")}
                  <input value={code} onChange={(e) => setCode(e.target.value)} autoComplete="off" />
                </label>
                <label className="settings-field">
                  {t("sync.otherAddress")}
                  <input value={host} onChange={(e) => setHost(e.target.value)} placeholder="100.64.0.2" autoComplete="off" />
                  <span className="field-hint">{t("sync.otherAddressHint")}</span>
                </label>
                <div className="skills-actions">
                  <button type="submit" className="primary-button" disabled={!conn || pending !== null || code.trim() === ""}>
                    {t("sync.continue")}
                  </button>
                </div>
              </form>
            )}
          </section>

          <section className="usage-section settings-card">
            <h2 className="usage-heading">{t("sync.firstTitle")}</h2>
            <p className="skills-hint">{t("sync.firstIntro")}</p>
            {pending?.action.kind === "init" ? (
              keyPrompt
            ) : (
              <div className="skills-actions">
                <button type="button" className="link-button" disabled={!conn || pending !== null} onClick={() => setPending({ action: { kind: "init" }, label: t("sync.createKey") })}>
                  {t("sync.createKeyHere")}
                </button>
              </div>
            )}
          </section>
        </>
      )}
    </div>
  );
}

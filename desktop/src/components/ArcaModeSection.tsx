import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t, type Key } from "../i18n";
import { hubChoice, loginsView, loopView, noticesOf, summaryOf, viewOf, type EmptyableClass, type LoopStatus, type ModeStatus, type SyncReport } from "../lib/arcaMode";
import type { SavedHub } from "../types";

/** Warden P183, desktop: turns the arca mode of the vault on (one way, so it takes a second click), chooses the saved hub the automatic
 * sync uses (recorded as soon as it is picked) and syncs with it by hand. The work is `warden_sync::arca_sync`, run by the
 * `arca_mode_enable`, `arca_mode_set_hub` and `arca_sync_run` commands. The hub shown is always the one the backend recorded. */
function ArcaModeSection() {
  const [status, setStatus] = useState<ModeStatus | null>(null);
  const [hubs, setHubs] = useState<SavedHub[] | null>(null);
  const [confirming, setConfirming] = useState(false);
  // The box of the confirmation: the MCP logins travel too. Off every time the confirmation opens.
  const [syncLogins, setSyncLogins] = useState(false);
  const [confirmingLogins, setConfirmingLogins] = useState(false);
  // The class whose removal on purpose is waiting for the second click. Only one at a time.
  const [confirmingEmpty, setConfirmingEmpty] = useState<EmptyableClass | null>(null);
  // The last automatic cycle, and when the last manual sync ended (ms; 0 until one did): the newer of the two is what the screen says.
  const [loop, setLoop] = useState<LoopStatus | null>(null);
  const [manualAt, setManualAt] = useState(0);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string; notices?: ReturnType<typeof noticesOf> } | null>(null);
  // A second click must not start a second cycle: the state below is only read after a render, this one at once.
  const busyRef = useRef(false);

  function refreshStatus() {
    invoke<ModeStatus>("arca_mode_status")
      .then(setStatus)
      .catch((e) => setResult({ ok: false, text: String(e) }));
  }

  // What the last automatic cycle did. Best effort: if the question fails the screen keeps what it had (it is a warning, not a result).
  function refreshLoop() {
    invoke<LoopStatus | null>("arca_loop_last")
      .then(setLoop)
      .catch(() => {});
  }

  useEffect(() => {
    refreshStatus();
    refreshLoop();
    invoke<SavedHub[]>("list_hubs")
      .then(setHubs)
      .catch(() => setHubs([]));
  }, []);

  // The loop emits this after every round, with no data: the answer is always asked of `arca_loop_last`.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    listen("arca-loop-report", () => refreshLoop()).then((fn) => {
      if (gone) fn();
      else unlisten = fn;
    });
    return () => {
      gone = true;
      unlisten?.();
    };
  }, []);

  async function chooseHub(id: string) {
    if (busyRef.current || id === "") return;
    busyRef.current = true;
    setBusy(true);
    setResult(null);
    try {
      await invoke("arca_mode_set_hub", { hubId: id });
    } catch (e) {
      setResult({ ok: false, text: t("arca.chooseFailed", { message: String(e) }) });
    } finally {
      // Whether it worked or not, what the screen shows is what the backend has recorded now (a new hub also lifts a stop of the loop).
      refreshStatus();
      refreshLoop();
      busyRef.current = false;
      setBusy(false);
    }
  }

  async function turnOn() {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setResult(null);
    try {
      await invoke("arca_mode_enable", { syncLogins });
      setResult({ ok: true, text: t("arca.enabled") });
      setConfirming(false);
      refreshStatus();
    } catch (e) {
      setResult({ ok: false, text: t("arca.enableFailed", { message: String(e) }) });
      setConfirming(false);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  async function acceptLogins() {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setResult(null);
    try {
      await invoke("arca_mode_accept_logins");
    } catch (e) {
      setResult({ ok: false, text: t("arca.loginsFailed", { message: String(e) }) });
    } finally {
      // Whether it worked or not, what the screen shows is what the backend has recorded now.
      setConfirmingLogins(false);
      refreshStatus();
      busyRef.current = false;
      setBusy(false);
    }
  }

  /** One sync cycle. With `emptyClass` the person confirmed that this class may end up with no file, and the cycle sends that removal. */
  async function syncNow(emptyClass?: EmptyableClass) {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setResult(null);
    setConfirmingEmpty(null);
    try {
      const report = emptyClass
        ? await invoke<SyncReport>("arca_sync_allow_empty", { hubId: selected, class: emptyClass })
        : await invoke<SyncReport>("arca_sync_run", { hubId: selected });
      const summary = summaryOf(report);
      const sentence = t(summary.key, summary.values);
      setResult({ ok: true, text: summary.conflicts > 0 ? `${sentence} ${t("arca.syncConflicts", { count: summary.conflicts })}` : sentence, notices: noticesOf(report) });
    } catch (e) {
      setResult({ ok: false, text: t("arca.syncFailed", { message: String(e) }) });
    } finally {
      // From here on this result is the newest word; a sync that worked also lifts a stop of the loop, so ask again what it says.
      setManualAt(Date.now());
      refreshLoop();
      busyRef.current = false;
      setBusy(false);
    }
  }

  /** The warning of an emptied class and its button: the first click only asks (and says it reaches the other devices). */
  function emptyOnPurpose(which: EmptyableClass, warningKey: Key, buttonKey: Key, confirmKey: Key) {
    return (
      <>
        <p className="settings-error-banner">{t(warningKey)}</p>
        {view === "on" && confirmingEmpty === which ? (
          <>
            <p className="settings-hint">{t(confirmKey)}</p>
            <div className="sync-join-form">
              <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void syncNow(which)}>
                {t("arca.emptyYes")}
              </button>
              <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingEmpty(null)}>
                {t("arca.cancel")}
              </button>
            </div>
          </>
        ) : (
          view === "on" && (
            <button type="button" className="settings-browse-btn" disabled={busy || selected === ""} onClick={() => setConfirmingEmpty(which)}>
              {t(buttonKey)}
            </button>
          )
        )}
      </>
    );
  }

  /** The warnings of a cycle, from the manual sync or from the automatic one: the same lines and the same buttons. */
  function noticesBlock(notices: ReturnType<typeof noticesOf>) {
    return (
      <>
        {notices.folderMissing && <p className="settings-error-banner">{t("arca.noticeFolderMissing")}</p>}
        {notices.vaultEmptied && emptyOnPurpose("vault", "arca.noticeVaultEmptied", "arca.emptyVault", "arca.emptyVaultConfirm")}
        {notices.loginsEmptied && emptyOnPurpose("secret", "arca.noticeLoginsEmptied", "arca.emptyLogins", "arca.emptyLoginsConfirm")}
        {notices.counts.map((notice) => (
          <p key={notice.key} className="settings-hint">
            {t(notice.key, { count: notice.count })}
          </p>
        ))}
        {notices.skipped.length > 0 && (
          <>
            <p className="settings-hint">{t("arca.skippedTitle")}</p>
            <ul>
              {notices.skipped.map((file) => (
                <li key={file.path} className="settings-hint">
                  {t("arca.skipItem", { path: file.path, reason: file.reasonKey ? t(file.reasonKey) : file.reason })}
                </li>
              ))}
            </ul>
          </>
        )}
      </>
    );
  }

  const view = viewOf(status);
  // The automatic cycle only speaks while the mode is on and when it is newer than the last manual sync: the newer of the two wins,
  // so an old automatic warning never comes back over a manual sync made after it, and a newer one replaces the manual warnings.
  const autoNewer = view === "on" && loop !== null && loop.atMs > manualAt;
  const auto = autoNewer ? loopView(loop) : null;
  const autoHidesManual = autoNewer && loop?.report != null;
  const noHubs = hubs !== null && hubs.length === 0;
  const { selected, removed } = hubChoice(status?.hubId ?? null, hubs);
  const chosenName = hubs?.find((hub) => hub.id === selected)?.name;

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("arca.title")}</h3>
      </div>
      <div className="sync-pairing-block">
        {view === "needsKey" && <p className="settings-hint">{t("arca.hintNeedsKey")}</p>}
        {view === "pendingSend" && <p className="settings-hint">{t("arca.hintPendingSend")}</p>}

        {view === "ready" && (
          <>
            <p className="settings-hint">{t("arca.hintReady")}</p>
            {confirming ? (
              <>
                <p className="settings-hint">{t("arca.confirmWarning")}</p>
                <label className="settings-hint">
                  <input type="checkbox" checked={syncLogins} disabled={busy} onChange={(e) => setSyncLogins(e.target.checked)} /> {t("arca.loginsOption")}
                </label>
                <div className="sync-join-form">
                  <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void turnOn()}>
                    {t("arca.confirmYes")}
                  </button>
                  <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirming(false)}>
                    {t("arca.cancel")}
                  </button>
                </div>
              </>
            ) : (
              <button
                type="button"
                className="settings-save-btn"
                onClick={() => {
                  setSyncLogins(false);
                  setConfirming(true);
                }}
              >
                {t("arca.turnOn")}
              </button>
            )}
          </>
        )}

        {view === "on" && (
          <>
            <p className="settings-hint">{t("arca.onHint")}</p>
            {noHubs ? (
              <p className="settings-hint">{t("arca.noHubs")}</p>
            ) : (
              <div className="sync-join-form">
                <label className="settings-hint" htmlFor="arca-sync-hub">
                  {t("arca.hub")}
                </label>
                <select id="arca-sync-hub" className="settings-input" value={selected} disabled={busy} onChange={(e) => void chooseHub(e.target.value)}>
                  <option value="" disabled>
                    {t("arca.chooseHub")}
                  </option>
                  {(hubs ?? []).map((hub) => (
                    <option key={hub.id} value={hub.id}>
                      {hub.name}
                    </option>
                  ))}
                </select>
              </div>
            )}
            {loginsView(status) === "syncing" && <p className="settings-hint">{t("arca.loginsOnHint")}</p>}
            {loginsView(status) === "offer" && (
              <>
                <p className="settings-hint">{t("arca.loginsOffHint")}</p>
                {confirmingLogins ? (
                  <>
                    <p className="settings-hint">{t("arca.loginsConfirm")}</p>
                    <div className="sync-join-form">
                      <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void acceptLogins()}>
                        {t("arca.loginsConfirmYes")}
                      </button>
                      <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingLogins(false)}>
                        {t("arca.cancel")}
                      </button>
                    </div>
                  </>
                ) : (
                  <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingLogins(true)}>
                    {t("arca.loginsAccept")}
                  </button>
                )}
              </>
            )}
            {removed && <p className="settings-error-banner">{t("arca.hubRemoved")}</p>}
            {chosenName && <p className="settings-hint">{t("arca.autoHub", { name: chosenName })}</p>}
            <button type="button" className="settings-save-btn" disabled={busy || selected === "" || noHubs} onClick={() => void syncNow()}>
              {busy ? t("arca.syncing") : t("arca.syncNow")}
            </button>
          </>
        )}

        {result && <p className={result.ok ? "settings-success-banner" : "settings-error-banner"}>{result.text}</p>}
        {!autoHidesManual && result?.notices && noticesBlock(result.notices)}
        {auto?.error && <p className="settings-error-banner">{t(auto.error.key, { message: auto.error.message })}</p>}
        {auto?.notices && noticesBlock(auto.notices)}
      </div>
    </section>
  );
}

export default ArcaModeSection;

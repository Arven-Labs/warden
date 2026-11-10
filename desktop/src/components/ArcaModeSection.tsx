import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { hubChoice, noticesOf, summaryOf, viewOf, type ModeStatus, type SyncReport } from "../lib/arcaMode";
import type { SavedHub } from "../types";

/** Warden P183, desktop: turns the arca mode of the vault on (one way, so it takes a second click), chooses the saved hub the automatic
 * sync uses (recorded as soon as it is picked) and syncs with it by hand. The work is `warden_sync::arca_sync`, run by the
 * `arca_mode_enable`, `arca_mode_set_hub` and `arca_sync_run` commands. The hub shown is always the one the backend recorded. */
function ArcaModeSection() {
  const [status, setStatus] = useState<ModeStatus | null>(null);
  const [hubs, setHubs] = useState<SavedHub[] | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string; notices?: ReturnType<typeof noticesOf> } | null>(null);
  // A second click must not start a second cycle: the state below is only read after a render, this one at once.
  const busyRef = useRef(false);

  function refreshStatus() {
    invoke<ModeStatus>("arca_mode_status")
      .then(setStatus)
      .catch((e) => setResult({ ok: false, text: String(e) }));
  }

  useEffect(() => {
    refreshStatus();
    invoke<SavedHub[]>("list_hubs")
      .then(setHubs)
      .catch(() => setHubs([]));
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
      // Whether it worked or not, what the screen shows is what the backend has recorded now.
      refreshStatus();
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
      await invoke("arca_mode_enable");
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

  async function syncNow() {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setResult(null);
    try {
      const report = await invoke<SyncReport>("arca_sync_run", { hubId: selected });
      const summary = summaryOf(report);
      const sentence = t(summary.key, summary.values);
      setResult({ ok: true, text: summary.conflicts > 0 ? `${sentence} ${t("arca.syncConflicts", { count: summary.conflicts })}` : sentence, notices: noticesOf(report) });
    } catch (e) {
      setResult({ ok: false, text: t("arca.syncFailed", { message: String(e) }) });
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  const view = viewOf(status);
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
              <button type="button" className="settings-save-btn" onClick={() => setConfirming(true)}>
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
            {removed && <p className="settings-error-banner">{t("arca.hubRemoved")}</p>}
            {chosenName && <p className="settings-hint">{t("arca.autoHub", { name: chosenName })}</p>}
            <button type="button" className="settings-save-btn" disabled={busy || selected === "" || noHubs} onClick={() => void syncNow()}>
              {busy ? t("arca.syncing") : t("arca.syncNow")}
            </button>
          </>
        )}

        {result && <p className={result.ok ? "settings-success-banner" : "settings-error-banner"}>{result.text}</p>}
        {result?.notices?.counts.map((notice) => (
          <p key={notice.key} className="settings-hint">
            {t(notice.key, { count: notice.count })}
          </p>
        ))}
        {result?.notices && result.notices.skipped.length > 0 && (
          <>
            <p className="settings-hint">{t("arca.skippedTitle")}</p>
            <ul>
              {result.notices.skipped.map((file) => (
                <li key={file.path} className="settings-hint">
                  {t("arca.skipItem", { path: file.path, reason: file.reasonKey ? t(file.reasonKey) : file.reason })}
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
    </section>
  );
}

export default ArcaModeSection;

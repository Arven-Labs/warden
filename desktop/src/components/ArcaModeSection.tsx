import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { intlTag, t, type Key } from "../i18n";
import {
  clearSummary,
  deletableIds,
  epochShortId,
  hubChoice,
  keptClassKey,
  keptClearSummary,
  loginsView,
  loopView,
  noticesOf,
  sizeLabel,
  summaryOf,
  verifyErrorMessage,
  verifyView,
  viewOf,
  type ClearReport,
  type EmptyableClass,
  type KeptItem,
  type KeptSkipKey,
  type LoserItem,
  type LoserSkipKey,
  type LoopStatus,
  type ModeStatus,
  type PendingEpoch,
  type SyncReport,
  type VerifyStatus,
} from "../lib/arcaMode";
import type { SavedHub } from "../types";
import ArcaRotationBlock from "./ArcaRotationBlock";

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
  // The second click of "stop syncing the MCP logins".
  const [confirmingStop, setConfirmingStop] = useState(false);
  // The class whose removal on purpose is waiting for the second click. Only one at a time.
  const [confirmingEmpty, setConfirmingEmpty] = useState<EmptyableClass | null>(null);
  // The last automatic cycle, and when the last manual sync ended (ms; 0 until one did): the newer of the two is what the screen says.
  const [loop, setLoop] = useState<LoopStatus | null>(null);
  const [manualAt, setManualAt] = useState(0);
  // The last integrity round of the copy the hub holds (its own loop, about every 6 hours).
  const [verify, setVerify] = useState<VerifyStatus | null>(null);
  // The copies of the losers of conflicts (logins in clear text; the list never carries the text), the one waiting for its second
  // click to be deleted (or all of them), and what the last deletion said.
  const [losers, setLosers] = useState<LoserItem[]>([]);
  const [confirmingLoser, setConfirmingLoser] = useState<{ id: string } | "all" | null>(null);
  const [losersResult, setLosersResult] = useState<
    { kind: "done"; removed: number; notRemoved: { label: string; reasonKey: LoserSkipKey }[]; keptTmp: number } | { kind: "failed"; message: string } | null
  >(null);
  // The files an interrupted update could not put back (`.arca-kept-*`, files of the person), the one waiting for its second click, and
  // what the last deletion said. One at a time, by decision: there is no "delete all" here.
  const [kept, setKept] = useState<KeptItem[]>([]);
  const [confirmingKept, setConfirmingKept] = useState<string | null>(null);
  const [keptResult, setKeptResult] = useState<
    { kind: "done"; removed: number; notRemoved: { label: string; reasonKey: KeptSkipKey }[]; keptTmp: number } | { kind: "failed"; message: string } | null
  >(null);
  // The newer key epochs that stop the vault (arca ADR 0018), the second click of "acknowledge", and what the last acknowledgement said.
  // Acknowledging sends back exactly the pairs shown here; the backend looks them up in a fresh list.
  const [epochs, setEpochs] = useState<PendingEpoch[]>([]);
  const [confirmingEpochs, setConfirmingEpochs] = useState(false);
  const [epochsResult, setEpochsResult] = useState<{ kind: "done" } | { kind: "failed"; message: string } | null>(null);
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
    // A cycle (the loop's or a manual one) is what saves a copy, so the list is asked again at the same moments. The same goes for the
    // files set aside: they come from the recovery a cycle starts with.
    refreshLosers();
    refreshKept();
  }

  function refreshKept() {
    invoke<KeptItem[] | null>("arca_kept_list")
      .then((items) => setKept(items ?? []))
      .catch(() => {});
  }

  /** The folder a kept file is in, as the person reads it (the class's folder itself has no name of its own). */
  function keptFolder(item: KeptItem): string {
    return item.folder === "" ? t("arca.keptRoot") : item.folder;
  }

  /** Deletes one kept file, after the second click. The backend looks the id up in a fresh list: no path goes with it. */
  async function clearKept(id: string) {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setConfirmingKept(null);
    setKeptResult(null);
    const labels = new Map(kept.map((item) => [item.id, keptFolder(item)]));
    try {
      const summary = keptClearSummary(await invoke<ClearReport>("arca_kept_clear", { ids: [id] }));
      setKeptResult({ kind: "done", removed: summary.removed, notRemoved: summary.notRemoved.map((n) => ({ label: labels.get(n.id) ?? n.id, reasonKey: n.reasonKey })), keptTmp: summary.keptTmp });
    } catch (e) {
      setKeptResult({ kind: "failed", message: String(e) });
    } finally {
      refreshKept();
      busyRef.current = false;
      setBusy(false);
    }
  }

  // Best effort: if the question fails the screen keeps what it had. Only asked with the mode on (see the effect below).
  function refreshEpochs() {
    invoke<PendingEpoch[] | null>("arca_epochs_pending")
      .then((items) => setEpochs(items ?? []))
      .catch(() => {});
  }

  /** Acknowledges the pairs shown, after the second click. Nothing is deleted and the newer key is not adopted: the cycle just goes on. */
  async function acknowledgeEpochs() {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setConfirmingEpochs(false);
    setEpochsResult(null);
    try {
      await invoke("arca_epochs_acknowledge", { seen: epochs });
      setEpochsResult({ kind: "done" });
    } catch (e) {
      setEpochsResult({ kind: "failed", message: String(e) });
    } finally {
      refreshEpochs();
      busyRef.current = false;
      setBusy(false);
    }
  }

  function refreshLosers() {
    invoke<LoserItem[] | null>("arca_losers_list")
      .then((items) => setLosers(items ?? []))
      .catch(() => {});
  }

  /** Deletes the copies with these ids, after the second click. The backend looks each id up in a fresh list: no path goes with it. */
  async function clearLosers(ids: string[]) {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setConfirmingLoser(null);
    setLosersResult(null);
    const names = new Map(losers.map((copy) => [copy.id, copy.path]));
    try {
      const summary = clearSummary(await invoke<ClearReport>("arca_losers_clear", { ids }));
      setLosersResult({ kind: "done", removed: summary.removed, notRemoved: summary.notRemoved.map((n) => ({ label: names.get(n.id) ?? n.id, reasonKey: n.reasonKey })), keptTmp: summary.keptTmp });
    } catch (e) {
      setLosersResult({ kind: "failed", message: String(e) });
    } finally {
      refreshLosers();
      busyRef.current = false;
      setBusy(false);
    }
  }

  // Best effort, like `refreshLoop`: if the question fails the screen keeps what it had.
  function refreshVerify() {
    invoke<VerifyStatus | null>("arca_verify_last")
      .then(setVerify)
      .catch(() => {});
  }

  useEffect(() => {
    refreshStatus();
    refreshLoop();
    refreshVerify();
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

  // After every integrity round that ran, with no data: the answer is asked of `arca_verify_last`.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    listen("arca-verify-report", () => refreshVerify()).then((fn) => {
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

  /** Stops syncing the MCP logins, after the second click. Nothing is deleted anywhere: the next cycle just leaves the folder alone. */
  async function revokeLogins() {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setResult(null);
    try {
      await invoke("arca_mode_revoke_logins");
    } catch (e) {
      setResult({ ok: false, text: t("arca.loginsStopFailed", { message: String(e) }) });
    } finally {
      // Whether it worked or not, what the screen shows is what the backend has recorded now.
      setConfirmingStop(false);
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

  /** The list of copies, each with its own second click, and "delete all". An edited copy is marked and has no button: it is kept. */
  /** The newer key epochs that stop syncing. It says what they are (an unauthenticated notice), that acknowledging adopts nothing and
   * deletes nothing, and puts the second click between the person and the acknowledgement. */
  function epochsBlock() {
    return (
      <>
        {epochs.length > 0 && (
          <>
            <h4 className="settings-hint">{t("arca.epochsTitle")}</h4>
            <p className="settings-hint">{t("arca.epochsHint")}</p>
            <ul>
              {epochs.map((item) => (
                <li key={`${item.epoch}-${item.id}`} className="settings-hint">
                  {t("arca.epochItem", { epoch: item.epoch, id: epochShortId(item.id) })}
                </li>
              ))}
            </ul>
            {confirmingEpochs ? (
              <>
                <p className="settings-hint">{t("arca.epochsAckConfirm")}</p>
                <div className="sync-join-form">
                  <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void acknowledgeEpochs()}>
                    {t("arca.epochsAckYes")}
                  </button>
                  <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingEpochs(false)}>
                    {t("arca.cancel")}
                  </button>
                </div>
              </>
            ) : (
              <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingEpochs(true)}>
                {t("arca.epochsAck")}
              </button>
            )}
          </>
        )}
        {epochsResult?.kind === "failed" && <p className="settings-error-banner">{t("arca.epochsFailed", { message: epochsResult.message })}</p>}
        {epochsResult?.kind === "done" && <p className="settings-success-banner">{t("arca.epochsDone")}</p>}
      </>
    );
  }

  function losersBlock() {
    const deletable = deletableIds(losers);
    return (
      <>
        <h4 className="settings-hint">{t("arca.losersTitle")}</h4>
        <p className="settings-hint">{t("arca.losersHint")}</p>
        <ul>
          {losers.map((copy) => (
            <li key={copy.id} className="settings-hint">
              {t("arca.loserItem", {
                path: copy.path,
                device: copy.device,
                size: sizeLabel(copy.sizeBytes),
                date: copy.modifiedAtMs === null ? "—" : new Date(copy.modifiedAtMs).toLocaleString(intlTag()),
              })}{" "}
              {copy.edited ? (
                <em>{t("arca.loserEdited")}</em>
              ) : confirmingLoser !== null && confirmingLoser !== "all" && confirmingLoser.id === copy.id ? (
                <>
                  <p className="settings-hint">{t("arca.loserDeleteConfirm", { path: copy.path })}</p>
                  <div className="sync-join-form">
                    <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void clearLosers([copy.id])}>
                      {t("arca.loserDeleteYes")}
                    </button>
                    <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingLoser(null)}>
                      {t("arca.cancel")}
                    </button>
                  </div>
                </>
              ) : (
                <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingLoser({ id: copy.id })}>
                  {t("arca.loserDelete")}
                </button>
              )}
            </li>
          ))}
        </ul>
        {deletable.length > 0 &&
          (confirmingLoser === "all" ? (
            <>
              <p className="settings-hint">{t("arca.losersDeleteAllConfirm")}</p>
              <div className="sync-join-form">
                <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void clearLosers(deletable)}>
                  {t("arca.losersDeleteAllYes")}
                </button>
                <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingLoser(null)}>
                  {t("arca.cancel")}
                </button>
              </div>
            </>
          ) : (
            <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingLoser("all")}>
              {t("arca.losersDeleteAll")}
            </button>
          ))}
        {losersResult?.kind === "failed" && <p className="settings-error-banner">{t("arca.losersFailed", { message: losersResult.message })}</p>}
        {losersResult?.kind === "done" && (
          <>
            <p className="settings-success-banner">{t("arca.losersRemoved", { count: losersResult.removed })}</p>
            {losersResult.notRemoved.map((kept) => (
              <p key={`${kept.label}-${kept.reasonKey}`} className="settings-hint">
                {t("arca.loserNotDeleted", { path: kept.label, reason: t(kept.reasonKey) })}
              </p>
            ))}
            {losersResult.keptTmp > 0 && <p className="settings-error-banner">{t("arca.losersKept")}</p>}
          </>
        )}
      </>
    );
  }

  /** The files set aside, each with its own second click. A file the person changed is marked and has no button: it is kept. */
  function keptBlock() {
    return (
      <>
        <h4 className="settings-hint">{t("arca.keptTitle")}</h4>
        <p className="settings-hint">{t("arca.keptHint")}</p>
        <ul>
          {kept.map((item) => (
            <li key={item.id} className="settings-hint">
              {t("arca.keptItem", {
                folder: keptFolder(item),
                class: t(keptClassKey(item)),
                size: sizeLabel(item.sizeBytes),
                date: item.modifiedAtMs === null ? "—" : new Date(item.modifiedAtMs).toLocaleString(intlTag()),
              })}{" "}
              {item.edited ? (
                <em>{t("arca.keptEdited")}</em>
              ) : confirmingKept === item.id ? (
                <>
                  <p className="settings-hint">{t("arca.keptDeleteConfirm", { folder: keptFolder(item) })}</p>
                  <div className="sync-join-form">
                    <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void clearKept(item.id)}>
                      {t("arca.keptDeleteYes")}
                    </button>
                    <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingKept(null)}>
                      {t("arca.cancel")}
                    </button>
                  </div>
                </>
              ) : (
                <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingKept(item.id)}>
                  {t("arca.keptDelete")}
                </button>
              )}
            </li>
          ))}
        </ul>
        {keptResult?.kind === "failed" && <p className="settings-error-banner">{t("arca.keptFailed", { message: keptResult.message })}</p>}
        {keptResult?.kind === "done" && (
          <>
            <p className="settings-success-banner">{t("arca.keptRemoved", { count: keptResult.removed })}</p>
            {keptResult.notRemoved.map((stayed) => (
              <p key={`${stayed.label}-${stayed.reasonKey}`} className="settings-hint">
                {t("arca.keptNotDeleted", { folder: stayed.label, reason: t(stayed.reasonKey) })}
              </p>
            ))}
            {keptResult.keptTmp > 0 && <p className="settings-error-banner">{t("arca.keptTmp")}</p>}
          </>
        )}
      </>
    );
  }

  const view = viewOf(status);
  // A cycle (the loop's or a manual one) is what notes a newer epoch, so the list is asked when the mode shows as on and after each cycle.
  useEffect(() => {
    if (view === "on") refreshEpochs();
  }, [view, loop?.atMs, manualAt]);
  // The pairing section (a key swap) can clear what is pending without any cycle here, and it has its own state: it says so with an event.
  useEffect(() => {
    const onChanged = () => {
      if (view === "on") refreshEpochs();
    };
    window.addEventListener("arca-epochs-changed", onChanged);
    return () => window.removeEventListener("arca-epochs-changed", onChanged);
  }, [view]);
  // The automatic cycle only speaks while the mode is on and when it is newer than the last manual sync: the newer of the two wins,
  // so an old automatic warning never comes back over a manual sync made after it, and a newer one replaces the manual warnings.
  const autoNewer = view === "on" && loop !== null && loop.atMs > manualAt;
  const auto = autoNewer ? loopView(loop) : null;
  const autoHidesManual = autoNewer && loop?.report != null;
  const integrity = view === "on" ? verifyView(verify, (ms) => new Date(ms).toLocaleString(intlTag())) : null;
  const integrityError = view === "on" ? verifyErrorMessage(verify) : null;
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
            {loginsView(status) === "syncing" && (
              <>
                <p className="settings-hint">{t("arca.loginsOnHint")}</p>
                {confirmingStop ? (
                  <>
                    <p className="settings-hint">{t("arca.loginsStopConfirm")}</p>
                    <div className="sync-join-form">
                      <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void revokeLogins()}>
                        {t("arca.loginsStopYes")}
                      </button>
                      <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingStop(false)}>
                        {t("arca.cancel")}
                      </button>
                    </div>
                  </>
                ) : (
                  <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirmingStop(true)}>
                    {t("arca.loginsStop")}
                  </button>
                )}
              </>
            )}
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
        {integrity && (
          <p className={integrity.key === "arca.verifyProblem" || integrity.key === "arca.verifyProblemNoCounts" ? "settings-error-banner" : "settings-hint"}>
            {t(integrity.key, integrity.params)}
          </p>
        )}
        {integrityError && <p className="settings-error-banner">{t("arca.verifyFailed", { message: integrityError })}</p>}
        {view === "on" && <ArcaRotationBlock hubId={selected} />}
        {view === "on" && (epochs.length > 0 || epochsResult !== null) && epochsBlock()}
        {view === "on" && (losers.length > 0 || losersResult !== null) && losersBlock()}
        {view === "on" && (kept.length > 0 || keptResult !== null) && keptBlock()}
      </div>
    </section>
  );
}

export default ArcaModeSection;

import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import type { RotationStart } from "../lib/arcaMode";
import { failureOf, failureView, type RotationPhase } from "../lib/arcaRotation";

type Step = { kind: "idle" } | { kind: "shown"; start: RotationStart } | { kind: "resume" };
type Outcome = { ok: true; text: string } | { ok: false; text: string } | null;

/** P-49, desktop: the key rotation for "I lost a device" (arca ADR 0018, Warden ADR 0002). The work is `PendingRotation` in
 * `warden_sync::arca_rotation`; the key waits in the backend, and only its phrase comes here, once. The phrase lives in this component's
 * state and nowhere else (no storage, no log): it is dropped when the person confirms, cancels, or leaves the screen. A rotation sends
 * nothing by itself, so after it worked this runs the normal sync with the chosen hub. `onChanged` is called when a confirmation ends
 * (with success or not) and after a cancel: those are the moments the backend's "left half done" state can change, and the section
 * that owns the status asks for it again (see `rotationHalfDoneShown`). */
function ArcaRotationBlock({ hubId, onChanged }: { hubId: string; onChanged: () => void }) {
  const [step, setStep] = useState<Step>({ kind: "idle" });
  const [typed, setTyped] = useState<string[]>([]);
  const [phrase, setPhrase] = useState("");
  const [revoked, setRevoked] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome>(null);
  // A second click must not start a second call: the state is only read after a render, this one at once.
  const busyRef = useRef(false);

  // Leaving the screen drops the key that was waiting in the backend (best effort: nothing was written, so a failure loses nothing).
  // It is `leave`, not `cancel`: leaving is not giving up a rotation that stopped half way (only the Cancel button does that), so the
  // backend keeps the automatic sync stopped for it.
  useEffect(() => {
    return () => {
      void invoke("arca_rotation_leave").catch(() => {});
    };
  }, []);

  function reset() {
    setStep({ kind: "idle" });
    setTyped([]);
    setPhrase("");
    setRevoked(false);
    setConfirming(false);
  }

  async function cancel() {
    reset();
    try {
      await invoke("arca_rotation_cancel");
    } catch {
      // Nothing was written; the key is dropped when the app closes.
    }
    onChanged();
  }

  async function begin() {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setOutcome(null);
    try {
      const start = await invoke<RotationStart>("arca_rotation_begin");
      setTyped(start.positions.map(() => ""));
      setRevoked(false);
      setConfirming(false);
      setStep({ kind: "shown", start });
    } catch (e) {
      setOutcome({ ok: false, text: t("arca.rotationFailed", { message: String(e) }) });
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  /** Confirms (after the second click) and, when it worked, sends the new epoch with the hub the screen has chosen. */
  async function confirm() {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setOutcome(null);
    setConfirming(false);
    // Where the 24 words are when something fails: the ones shown, or the ones typed to resume.
    const words = step.kind === "shown" ? step.start.phrase.join(" ") : phrase.trim();
    // Which call is in flight: the resume call only reads the phrase, and its errors are about the phrase (see `failureView`).
    let phase: RotationPhase = "confirm";
    try {
      if (step.kind === "resume") {
        phase = "resume";
        await invoke("arca_rotation_resume", { phrase: phrase.trim() });
        phase = "confirm";
      }
      await invoke("arca_rotation_confirm", { typedWords: step.kind === "shown" ? typed : [], revokedOnHub: revoked });
    } catch (e) {
      const failure = failureOf(e);
      const view = failureView(step.kind === "shown" ? "shown" : "resume", phase, failure);
      const text = t(view.key, { message: failure.message });
      if (view.next === "resume") {
        // It may have written something before it stopped: only the SAME key finishes it, so the person resumes with the same 24 words
        // (a new key would be a different rotation). They are put in the box, and the second click is asked again.
        setPhrase(words);
        setTyped([]);
        setConfirming(false);
        setStep({ kind: "resume" });
      } else if (view.next === "keep") {
        // The 24 words typed are the person's own work: a typo is fixed in place, not typed again from nothing.
        setConfirming(false);
      } else {
        // Nothing was written and the backend dropped the key with the try: the person starts again.
        reset();
      }
      setOutcome({ ok: false, text });
      busyRef.current = false;
      setBusy(false);
      // A stop half way raised the backend's state (or a refusal left it as it was): the section asks for it again.
      onChanged();
      return;
    }
    reset();
    // A success ended the half-done state; this is before the sync below, which can take a while.
    if (hubId === "") {
      setOutcome({ ok: true, text: t("arca.rotationDoneNoHub") });
    } else {
      try {
        await invoke("arca_sync_run", { hubId });
        setOutcome({ ok: true, text: t("arca.rotationDone") });
      } catch (e) {
        setOutcome({ ok: false, text: t("arca.rotationDoneNotSent", { message: String(e) }) });
      }
    }
    busyRef.current = false;
    setBusy(false);
  }

  const canConfirm = revoked && (step.kind === "shown" ? typed.every((word) => word.trim() !== "") : phrase.trim().split(/\s+/).length === 24);

  function confirmArea() {
    return confirming ? (
      <>
        <p className="settings-error-banner">{t("arca.rotationConfirmWarning")}</p>
        <div className="sync-join-form">
          <button type="button" className="settings-save-btn" disabled={busy} onClick={() => void confirm()}>
            {t("arca.rotationConfirmYes")}
          </button>
          <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => setConfirming(false)}>
            {t("arca.cancel")}
          </button>
        </div>
      </>
    ) : (
      <div className="sync-join-form">
        <button type="button" className="settings-save-btn" disabled={busy || !canConfirm} onClick={() => setConfirming(true)}>
          {step.kind === "resume" ? t("arca.rotationResumeGo") : t("arca.rotationConfirm")}
        </button>
        <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => void cancel()}>
          {t("arca.cancel")}
        </button>
      </div>
    );
  }

  return (
    <>
      <h4 className="settings-hint">{t("arca.rotationTitle")}</h4>
      {step.kind === "idle" && (
        <>
          <p className="settings-hint">{t("arca.rotationHint")}</p>
          <div className="sync-join-form">
            <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => void begin()}>
              {t("arca.rotationStart")}
            </button>
            <button
              type="button"
              className="settings-browse-btn"
              disabled={busy}
              onClick={() => {
                setOutcome(null);
                setRevoked(false);
                setStep({ kind: "resume" });
              }}
            >
              {t("arca.rotationResume")}
            </button>
          </div>
        </>
      )}
      {step.kind === "shown" && (
        <>
          <h5 className="settings-hint">{t("arca.rotationPhraseTitle")}</h5>
          <p className="settings-hint">{t("arca.rotationPhraseHint")}</p>
          <ol>
            {step.start.phrase.map((word, index) => (
              <li key={index} className="settings-hint">
                {word}
              </li>
            ))}
          </ol>
          <p className="settings-hint">{t("arca.rotationAsk")}</p>
          {step.start.positions.map((position, index) => (
            <div key={position} className="sync-join-form">
              <label className="settings-hint" htmlFor={`arca-rotation-word-${position}`}>
                {t("arca.rotationWord", { n: position })}
              </label>
              <input
                id={`arca-rotation-word-${position}`}
                className="settings-input"
                type="text"
                autoComplete="off"
                autoCapitalize="off"
                spellCheck={false}
                value={typed[index] ?? ""}
                disabled={busy}
                onChange={(e) => setTyped(typed.map((word, i) => (i === index ? e.target.value : word)))}
              />
            </div>
          ))}
          <label className="settings-hint">
            <input type="checkbox" checked={revoked} disabled={busy} onChange={(e) => setRevoked(e.target.checked)} /> {t("arca.rotationRevoked")}
          </label>
          {confirmArea()}
        </>
      )}
      {step.kind === "resume" && (
        <>
          <p className="settings-hint">{t("arca.rotationResumeHint")}</p>
          <label className="settings-hint" htmlFor="arca-rotation-phrase">
            {t("arca.rotationResumePhrase")}
          </label>
          <textarea
            id="arca-rotation-phrase"
            className="settings-input"
            rows={3}
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            value={phrase}
            disabled={busy}
            onChange={(e) => setPhrase(e.target.value)}
          />
          <label className="settings-hint">
            <input type="checkbox" checked={revoked} disabled={busy} onChange={(e) => setRevoked(e.target.checked)} /> {t("arca.rotationRevoked")}
          </label>
          {confirmArea()}
        </>
      )}
      {outcome && <p className={outcome.ok ? "settings-success-banner" : "settings-error-banner"}>{outcome.text}</p>}
    </>
  );
}

export default ArcaRotationBlock;

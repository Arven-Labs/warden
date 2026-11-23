import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import type { RotationStart } from "../lib/arcaMode";

type Step = { kind: "idle" } | { kind: "shown"; start: RotationStart } | { kind: "resume" };
type Outcome = { ok: true; text: string } | { ok: false; text: string } | null;

/** What `arca_rotation_confirm` rejects with: `partial` says the rotation may have written something before it stopped. Any other
 * rejection (a plain string from the other commands) is nothing-written. */
function failureOf(e: unknown): { partial: boolean; message: string } {
  if (typeof e === "object" && e !== null && "message" in e) {
    const { partial, message } = e as { partial?: unknown; message: unknown };
    return { partial: partial === true, message: String(message) };
  }
  return { partial: false, message: String(e) };
}

/** P-49, desktop: the key rotation for "I lost a device" (arca ADR 0018, Warden ADR 0002). The work is `PendingRotation` in
 * `warden_sync::arca_rotation`; the key waits in the backend, and only its phrase comes here, once. The phrase lives in this component's
 * state and nowhere else (no storage, no log): it is dropped when the person confirms, cancels, or leaves the screen. A rotation sends
 * nothing by itself, so after it worked this runs the normal sync with the chosen hub. */
function ArcaRotationBlock({ hubId }: { hubId: string }) {
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
  useEffect(() => {
    return () => {
      void invoke("arca_rotation_cancel").catch(() => {});
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
    try {
      if (step.kind === "resume") {
        await invoke("arca_rotation_resume", { phrase: phrase.trim() });
      }
      await invoke("arca_rotation_confirm", { typedWords: step.kind === "shown" ? typed : [], revokedOnHub: revoked });
    } catch (e) {
      const failure = failureOf(e);
      if (failure.partial) {
        // It may have written something before it stopped: only the SAME key finishes it, so the person resumes with the same 24 words
        // (a new key would be a different rotation). They are put in the box, and the second click is asked again.
        setPhrase(words);
        setTyped([]);
        setConfirming(false);
        setStep({ kind: "resume" });
        setOutcome({ ok: false, text: t("arca.rotationPartial", { message: failure.message }) });
      } else if (step.kind === "resume") {
        // The 24 words typed are the person's own work: a typo is fixed in place, not typed again from nothing. And nothing written by
        // THIS attempt says nothing about the store: an earlier attempt may have published this very key, so it is not "not changed".
        setConfirming(false);
        setOutcome({ ok: false, text: t("arca.rotationResumeNotWritten", { message: failure.message }) });
      } else {
        // Nothing was written and the backend dropped the key with the try: the person starts again.
        reset();
        setOutcome({ ok: false, text: t("arca.rotationFailed", { message: failure.message }) });
      }
      busyRef.current = false;
      setBusy(false);
      return;
    }
    reset();
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

import { useState } from "react";
import { SettingsError, type ServerConnection } from "../hub/connection";
import { trustKey } from "../hub/trustedPublishers";
import { t, type Key } from "../i18n";

/** The buttons under "signed by a key you have not listed" in the import preview (P160): copy the key, or trust it right there.
 * Trusting reads the hub's list as it is now, adds the key and saves the whole list with the hub's pairing key (as any change of the
 * owner's). `onTrusted` runs once the key is in the list, or was already, so the preview can be read again. The owner's only: the
 * caller shows just the copy button to a member. */
export default function TrustThisKey({ conn, fullKey, onTrusted }: { conn: ServerConnection; fullKey: string; onTrusted: () => void }) {
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [asking, setAsking] = useState(false);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  function reset() {
    setNaming(false);
    setAsking(false);
    setName("");
    setPairingKey("");
    setKeyError(null);
  }

  async function handleTrust() {
    setBusy(true);
    setKeyError(null);
    setProblem(null);
    try {
      const result = trustKey(await conn.listTrustedPublishers(), fullKey, name);
      if ("problem" in result) {
        // Listed since the preview was read (another screen): nothing to save, the preview only needs to be read again.
        if (result.problem === "duplicate") {
          reset();
          onTrusted();
          return;
        }
        setAsking(false);
        setProblem(t(`skills.trustProblem.${result.problem}` as Key));
        return;
      }
      await conn.saveTrustedPublishers(pairingKey, result.list);
      reset();
      onTrusted();
    } catch (err) {
      if (err instanceof SettingsError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        reset();
        setProblem(t("skills.trustSaveFailed", { error: err instanceof Error ? err.message : String(err) }));
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      {!naming && (
        <div className="skills-actions">
          <button type="button" className="link-button" onClick={() => void navigator.clipboard?.writeText(fullKey)}>
            {t("skills.copyKey")}
          </button>
          <button type="button" className="link-button" onClick={() => setNaming(true)}>
            {t("skills.trustThisKey")}
          </button>
        </div>
      )}
      {naming && !asking && (
        <>
          <label className="settings-field">
            {t("skills.trustThisKeyName")}
            <input value={name} autoFocus onChange={(e) => setName(e.target.value)} />
          </label>
          <div className="skills-actions">
            <button type="button" className="primary-button" onClick={() => setAsking(true)}>
              {t("skills.trustThisKeyConfirm")}
            </button>
            <button type="button" className="link-button" onClick={reset}>
              {t("common.cancel")}
            </button>
          </div>
        </>
      )}
      {asking && (
        <form
          className="skills-editor"
          onSubmit={(e) => {
            e.preventDefault();
            void handleTrust();
          }}
        >
          <span className="skills-hint">{t("skills.trustKeyPrompt")}</span>
          <label className="settings-field">
            {t("pairingKey.label")}
            <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
          </label>
          {keyError && <p className="error-banner">{keyError}</p>}
          <div className="skills-actions">
            <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === ""}>
              {busy ? t("skills.trustSaving") : t("common.confirm")}
            </button>
            <button type="button" className="link-button" disabled={busy} onClick={reset}>
              {t("common.cancel")}
            </button>
          </div>
        </form>
      )}
      {problem && <p className="error-banner">{problem}</p>}
    </div>
  );
}

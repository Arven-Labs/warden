import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { t, type Key } from "../i18n";
import { hubListTrustedPublishers, hubSaveTrustedPublishers } from "../lib/hub";
import { trustKey, type Publisher } from "../lib/trustedPublishers";
import CopyButton from "./CopyButton";
import { KeyCancelled, usePairingKey } from "./PairingKeyDialog";

/** The buttons under "signed by a key you have not listed" in the import preview (P160): copy the key, or trust it right there.
 * Trusting reads the list as it is now, adds the key and saves the whole list, on this computer's config or on the hub in use (which
 * asks for its pairing key). `onTrusted` runs once the key is in the list, or was already, so the preview can be read again. */
function TrustThisKey({ fullKey, remote, onTrusted }: { fullKey: string; remote: boolean; onTrusted: () => void }) {
  const { askKey, dialog } = usePairingKey();
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  async function handleTrust() {
    setBusy(true);
    setProblem(null);
    try {
      const current = remote ? await hubListTrustedPublishers() : await invoke<Publisher[]>("list_trusted_publishers");
      const result = trustKey(current, fullKey, name);
      if ("problem" in result) {
        // Listed since the preview was read (another screen): nothing to save, the preview only needs to be read again.
        if (result.problem === "duplicate") {
          onTrusted();
          return;
        }
        setProblem(t(`skills.trustProblem.${result.problem}` as Key));
        return;
      }
      if (remote) await hubSaveTrustedPublishers(askKey, result.list);
      else await invoke<Publisher[]>("save_trusted_publishers", { publishers: result.list });
      setNaming(false);
      setName("");
      onTrusted();
    } catch (err) {
      if (!(err instanceof KeyCancelled)) setProblem(t("skills.trustSaveFailed", { error: err instanceof Error ? err.message : String(err) }));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      {dialog}
      {!naming && (
        <>
          <CopyButton text={fullKey} label={t("skills.copyKey")} />{" "}
          <button type="button" className="settings-browse-btn" onClick={() => setNaming(true)}>
            {t("skills.trustThisKey")}
          </button>
        </>
      )}
      {naming && (
        <>
          <label className="settings-label">
            {t("skills.trustThisKeyName")}
            <input className="settings-input" value={name} autoFocus onChange={(e) => setName(e.currentTarget.value)} />
          </label>
          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={() => void handleTrust()} disabled={busy}>
              {busy ? t("skills.trustSaving") : t("skills.trustThisKeyConfirm")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setNaming(false)} disabled={busy}>
              {t("common.cancel")}
            </button>
          </div>
        </>
      )}
      {problem && <span className="settings-error-banner">{problem}</span>}
    </>
  );
}

export default TrustThisKey;

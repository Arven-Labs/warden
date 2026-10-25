import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { t, type Key } from "../i18n";
import { hubListTrustedPublishers, hubSaveTrustedPublishers } from "../lib/hub";
import { addPublisher, checkDraft, removePublisher, samePublishers, shortKey, type Publisher } from "../lib/trustedPublishers";
import { KeyCancelled, usePairingKey } from "./PairingKeyDialog";

/** "Who signs" (P160): the keys whose skill packages the person trusts, so the import preview can say who signed one. Trusting a key
 * only changes that label, so the list is edited here and saved in one go: on this computer's own config, or on the hub in use
 * (which asks for its pairing key, as for any change). The server checks the list again; `checkDraft` only spares the round trip. */
function TrustedPublishers({ remote }: { remote: boolean }) {
  const { askKey, dialog } = usePairingKey();
  const [saved, setSaved] = useState<Publisher[] | null>(null);
  const [draft, setDraft] = useState<Publisher[]>([]);
  const [key, setKey] = useState("");
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const list = remote ? await hubListTrustedPublishers() : await invoke<Publisher[]>("list_trusted_publishers");
      setSaved(list);
      setDraft(list);
      setMessage(null);
    } catch (err) {
      setMessage({ text: t("skills.trustLoadFailed", { error: String(err) }), error: true });
    }
  }, [remote]);

  useEffect(() => {
    void load();
  }, [load]);

  function handleAdd() {
    const found = checkDraft(draft, key, name);
    if (found !== null) {
      setProblem(t(`skills.trustProblem.${found}` as Key));
      return;
    }
    setProblem(null);
    setDraft(addPublisher(draft, key, name));
    setKey("");
    setName("");
    setMessage(null);
  }

  async function handleSave() {
    setBusy(true);
    setMessage(null);
    try {
      const list = remote ? await hubSaveTrustedPublishers(askKey, draft) : await invoke<Publisher[]>("save_trusted_publishers", { publishers: draft });
      setSaved(list);
      setDraft(list);
      setMessage({ text: t("skills.trustSaved"), error: false });
    } catch (err) {
      if (!(err instanceof KeyCancelled)) setMessage({ text: t("skills.trustSaveFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
    } finally {
      setBusy(false);
    }
  }

  if (saved === null && message === null) return null;
  const changed = saved !== null && !samePublishers(saved, draft);

  return (
    <section className="settings-section">
      {dialog}
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("skills.trustTitle")}</h3>
      </div>
      <p className="settings-hint">{t("skills.trustHint")}</p>
      {saved === null ? (
        <p className="settings-error-banner">{message?.text}</p>
      ) : (
        <>
          {draft.length === 0 ? (
            <p className="settings-hint">{t("skills.trustNone")}</p>
          ) : (
            <div className="provider-list">
              {draft.map((p) => (
                <div className="provider-card skill-card" key={p.key}>
                  <div className="skill-card-header">
                    <span className="skill-card-name">{p.name || t("skills.trustNoName")}</span>
                    <div className="skill-card-actions">
                      <code title={p.key}>{shortKey(p.key)}…</code>
                      <button type="button" className="settings-browse-btn" onClick={() => setDraft(removePublisher(draft, p.key))} disabled={busy}>
                        {t("skills.trustRemove")}
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
          <label className="settings-label">
            {t("skills.trustKeyLabel")}
            <input className="settings-input" value={key} spellCheck={false} autoComplete="off" onChange={(e) => setKey(e.currentTarget.value)} />
          </label>
          <label className="settings-label">
            {t("skills.trustNameLabel")}
            <input className="settings-input" value={name} onChange={(e) => setName(e.currentTarget.value)} />
          </label>
          {problem && <p className="settings-error-banner">{problem}</p>}
          <div className="skill-editor-actions">
            <button type="button" className="settings-browse-btn" onClick={handleAdd} disabled={key.trim() === "" || busy}>
              {t("skills.trustAdd")}
            </button>
            <button type="button" className="settings-save-btn" onClick={() => void handleSave()} disabled={!changed || busy}>
              {busy ? t("skills.trustSaving") : t("skills.trustSave")}
            </button>
          </div>
          {changed && !busy && <p className="settings-hint">{t("skills.trustUnsaved")}</p>}
          {message && <p className={message.error ? "settings-error-banner" : "settings-hint"}>{message.text}</p>}
        </>
      )}
    </section>
  );
}

export default TrustedPublishers;

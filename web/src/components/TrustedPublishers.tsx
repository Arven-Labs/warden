import { useCallback, useEffect, useState } from "react";
import { SettingsError, type ServerConnection } from "../hub/connection";
import { addPublisher, checkDraft, removePublisher, samePublishers, shortKey, type Publisher } from "../hub/trustedPublishers";
import { t, type Key } from "../i18n";

/** "Who signs" (P160): the keys whose skill packages the hub's owner trusts, so the import preview can say who signed one. Trusting a
 * key only changes that label, so the list is edited here and saved in one go, with the hub's pairing key (as any change of the owner's).
 * The hub checks the list again; `checkDraft` only spares the round trip. The owner's only: the caller leaves it out for a member. */
export default function TrustedPublishers({ conn }: { conn: ServerConnection | null }) {
  const [saved, setSaved] = useState<Publisher[] | null>(null);
  const [draft, setDraft] = useState<Publisher[]>([]);
  const [key, setKey] = useState("");
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const [asking, setAsking] = useState(false);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      const list = await conn.listTrustedPublishers();
      setSaved(list);
      setDraft(list);
      setMessage(null);
    } catch (err) {
      setMessage({ text: t("skills.trustLoadFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
    }
  }, [conn]);

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

  function cancelKey() {
    setAsking(false);
    setPairingKey("");
    setKeyError(null);
  }

  async function handleSave() {
    if (!conn) return;
    setBusy(true);
    setKeyError(null);
    try {
      const list = await conn.saveTrustedPublishers(pairingKey, draft);
      setSaved(list);
      setDraft(list);
      setMessage({ text: t("skills.trustSaved"), error: false });
      cancelKey();
    } catch (err) {
      if (err instanceof SettingsError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        cancelKey();
        setMessage({ text: t("skills.trustSaveFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
      }
    } finally {
      setBusy(false);
    }
  }

  if (saved === null && message === null) return null;
  const changed = saved !== null && !samePublishers(saved, draft);

  return (
    <section>
      <h3>{t("skills.trustTitle")}</h3>
      <p className="skills-hint">{t("skills.trustHint")}</p>
      {saved === null ? (
        <p className="error-banner">{message?.text}</p>
      ) : (
        <>
          {draft.length === 0 ? (
            <p className="skills-hint">{t("skills.trustNone")}</p>
          ) : (
            <ul className="skills-list">
              {draft.map((p) => (
                <li key={p.key} className="skills-item">
                  <div className="skills-item-header">
                    <span className="skills-item-name">{p.name || t("skills.trustNoName")}</span>
                    <span className="skills-actions">
                      <code title={p.key}>{shortKey(p.key)}…</code>
                      <button type="button" className="link-button" disabled={busy || asking} onClick={() => setDraft(removePublisher(draft, p.key))}>
                        {t("skills.trustRemove")}
                      </button>
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          )}
          <label className="settings-field">
            {t("skills.trustKeyLabel")}
            <input value={key} spellCheck={false} autoComplete="off" onChange={(e) => setKey(e.target.value)} />
          </label>
          <label className="settings-field">
            {t("skills.trustNameLabel")}
            <input value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          {problem && <p className="error-banner">{problem}</p>}
          <div className="skills-actions">
            <button type="button" className="link-button" disabled={key.trim() === "" || busy || asking} onClick={handleAdd}>
              {t("skills.trustAdd")}
            </button>
            <button type="button" className="primary-button" disabled={!changed || busy || asking || !conn} onClick={() => setAsking(true)}>
              {t("skills.trustSave")}
            </button>
          </div>
          {changed && !asking && <p className="skills-hint">{t("skills.trustUnsaved")}</p>}
          {asking && (
            <form
              className="skills-editor"
              onSubmit={(e) => {
                e.preventDefault();
                void handleSave();
              }}
            >
              <span className="skills-hint">{t("skills.trustKeyPrompt")}</span>
              <label className="settings-field">
                {t("pairingKey.label")}
                <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
              </label>
              {keyError && <p className="error-banner">{keyError}</p>}
              <div className="skills-actions">
                <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
                  {busy ? t("skills.trustSaving") : t("common.confirm")}
                </button>
                <button type="button" className="link-button" disabled={busy} onClick={cancelKey}>
                  {t("common.cancel")}
                </button>
              </div>
            </form>
          )}
          {message && <p className={message.error ? "error-banner" : "skills-hint"}>{message.text}</p>}
        </>
      )}
    </section>
  );
}

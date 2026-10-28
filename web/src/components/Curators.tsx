import { useCallback, useEffect, useState } from "react";
import { SettingsError, type ServerConnection } from "../hub/connection";
import type { CuratorEntryDto } from "../hub/messages";
import { FABIO_SOURCE, addCurator, checkCurator, hasFabioSource, removeCurator, sameCurators, type Curator } from "../hub/curators";
import { shortKey } from "../hub/trustedPublishers";
import { t, type Key } from "../i18n";

interface OpenIndex {
  curator: string;
  /** Null while the hub is reading it. */
  index: { name: string; sequence: number; entries: CuratorEntryDto[] } | null;
}

/** "Sources" (P160): the curators whose signed lists of skills the hub takes from. Warden ships none; the owner adds a name, the key that
 * signs the list and where it is, and saves the whole list with the hub's pairing key (as any change of the owner's). Anyone signed in
 * can open a source and read what it lists; "Read and install" has the hub download the package, checked against the list, and hands the
 * text to `onInstall`, which previews it like a file. The list is edited by the owner only (`owner`), and it must not sit inside a form:
 * the pairing-key prompt is one of its own. */
export default function Curators({ conn, owner, onInstall }: { conn: ServerConnection | null; owner: boolean; onInstall: (content: string) => void }) {
  const [saved, setSaved] = useState<Curator[] | null>(null);
  const [draft, setDraft] = useState<Curator[]>([]);
  const [name, setName] = useState("");
  const [key, setKey] = useState("");
  const [url, setUrl] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const [asking, setAsking] = useState(false);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<OpenIndex | null>(null);
  const [getting, setGetting] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      const list = await conn.listCurators();
      setSaved(list);
      setDraft(list);
      setMessage(null);
    } catch (err) {
      setMessage({ text: t("skills.curatorLoadFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
    }
  }, [conn]);

  useEffect(() => {
    void load();
  }, [load]);

  function handleAdd() {
    const found = checkCurator(draft, { name, key, url });
    if (found !== null) {
      setProblem(t(`skills.curatorProblem.${found}` as Key));
      return;
    }
    setProblem(null);
    setDraft(addCurator(draft, { name, key, url }));
    setName("");
    setKey("");
    setUrl("");
    setMessage(null);
  }

  /** Puts the first source in the draft, to be saved like any other change; a name or key that clashes says so as for any entry. */
  function handleAddFabio() {
    const found = checkCurator(draft, FABIO_SOURCE);
    if (found !== null) {
      setProblem(t(`skills.curatorProblem.${found}` as Key));
      return;
    }
    setProblem(null);
    setDraft(addCurator(draft, FABIO_SOURCE));
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
      const list = await conn.saveCurators(pairingKey, draft);
      setSaved(list);
      setDraft(list);
      setMessage({ text: t("skills.curatorSaved"), error: false });
      cancelKey();
    } catch (err) {
      if (err instanceof SettingsError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        cancelKey();
        setMessage({ text: t("skills.curatorSaveFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
      }
    } finally {
      setBusy(false);
    }
  }

  function handleOpen(curator: string) {
    if (!conn) return;
    setMessage(null);
    setOpen({ curator, index: null });
    conn.fetchCuratorIndex(curator).then(
      (index) => setOpen((current) => (current?.curator === curator ? { curator, index } : current)),
      (err) => {
        setOpen(null);
        setMessage({ text: t("skills.curatorIndexFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
      },
    );
  }

  function handleInstall(curator: string, entry: CuratorEntryDto) {
    if (!conn) return;
    setMessage(null);
    setGetting(entry.hash);
    conn.fetchCuratorPackage(curator, entry.hash).then(
      (content) => {
        setGetting(null);
        onInstall(content);
      },
      (err) => {
        setGetting(null);
        setMessage({ text: t("skills.curatorGetFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
      },
    );
  }

  if (saved === null && message === null) return null;
  const changed = saved !== null && !sameCurators(saved, draft);

  return (
    <section>
      <h3>{t("skills.curatorsTitle")}</h3>
      <p className="skills-hint">{t("skills.curatorsHint")}</p>
      {saved === null ? (
        <p className="error-banner">{message?.text}</p>
      ) : (
        <>
          {draft.length === 0 ? (
            <p className="skills-hint">{t("skills.curatorsNone")}</p>
          ) : (
            <ul className="skills-list">
              {draft.map((c) => (
                <li key={c.name} className="skills-item">
                  <div className="skills-item-header">
                    <span className="skills-item-name">{c.name}</span>
                    <span className="skills-actions">
                      <code title={c.key}>{shortKey(c.key)}…</code>
                      {!changed && (
                        <button type="button" className="link-button" disabled={!conn || open?.curator === c.name} onClick={() => handleOpen(c.name)}>
                          {t("skills.curatorOpen")}
                        </button>
                      )}
                      {owner && (
                        <button type="button" className="link-button" disabled={busy || asking} onClick={() => setDraft(removeCurator(draft, c.name))}>
                          {t("skills.curatorRemove")}
                        </button>
                      )}
                    </span>
                  </div>
                  <p className="skills-hint">{c.url}</p>
                  {open?.curator === c.name && (
                    <div className="skills-editor">
                      {open.index === null ? (
                        <span className="skills-hint">{t("skills.curatorLoading")}</span>
                      ) : (
                        <>
                          <strong>{t("skills.curatorEntries", { name: open.index.name, sequence: String(open.index.sequence) })}</strong>
                          {open.index.entries.length === 0 && <span className="skills-hint">{t("skills.curatorEmpty")}</span>}
                          <ul className="skills-list">
                            {open.index.entries.map((entry) => (
                              <li key={entry.hash} className="skills-item">
                                <div className="skills-item-header">
                                  <span className="skills-item-name">{entry.id}</span>
                                  <span className="skills-actions">
                                    {entry.revoked && <span className="error-banner">{t("skills.curatorRevoked")}</span>}
                                    <button type="button" className="link-button" disabled={!conn || !!entry.revoked || getting !== null} onClick={() => handleInstall(c.name, entry)}>
                                      {t("skills.curatorInstall")}
                                    </button>
                                  </span>
                                </div>
                                <p className="skills-item-description">{entry.description}</p>
                                <p className="skills-hint">
                                  {t("skills.curatorEntryMeta", { version: entry.version, license: entry.license })}
                                  {entry.author ? t("skills.fromAuthor", { author: entry.author }) : ""}
                                </p>
                                {entry.revoked && entry.revokedReason && <p className="skills-hint">{t("skills.curatorRevokedWhy", { reason: entry.revokedReason })}</p>}
                              </li>
                            ))}
                          </ul>
                          <div className="skills-actions">
                            <button type="button" className="link-button" onClick={() => setOpen(null)}>
                              {t("skills.curatorClose")}
                            </button>
                          </div>
                        </>
                      )}
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
          {owner && (
            <>
              <label className="settings-field">
                {t("skills.curatorName")}
                <input value={name} onChange={(e) => setName(e.target.value)} />
              </label>
              <label className="settings-field">
                {t("skills.curatorKey")}
                <input value={key} spellCheck={false} autoComplete="off" onChange={(e) => setKey(e.target.value)} />
              </label>
              <label className="settings-field">
                {t("skills.curatorUrl")}
                <input value={url} spellCheck={false} autoComplete="off" onChange={(e) => setUrl(e.target.value)} />
              </label>
              {problem && <p className="error-banner">{problem}</p>}
              {!hasFabioSource(draft) && (
                <>
                  <p className="skills-hint">{t("skills.curatorAddFabioHint")}</p>
                  <div className="skills-actions">
                    <button type="button" className="link-button" disabled={busy || asking} onClick={handleAddFabio}>
                      {t("skills.curatorAddFabio")}
                    </button>
                  </div>
                </>
              )}
              <div className="skills-actions">
                <button type="button" className="link-button" disabled={key.trim() === "" || busy || asking} onClick={handleAdd}>
                  {t("skills.curatorAdd")}
                </button>
                <button type="button" className="primary-button" disabled={!changed || busy || asking || !conn} onClick={() => setAsking(true)}>
                  {t("skills.curatorSave")}
                </button>
              </div>
              {changed && !asking && <p className="skills-hint">{t("skills.curatorUnsaved")}</p>}
              {asking && (
                <form
                  className="skills-editor"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void handleSave();
                  }}
                >
                  <span className="skills-hint">{t("skills.curatorKeyPrompt")}</span>
                  <label className="settings-field">
                    {t("pairingKey.label")}
                    <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
                  </label>
                  {keyError && <p className="error-banner">{keyError}</p>}
                  <div className="skills-actions">
                    <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
                      {busy ? t("skills.curatorSaving") : t("common.confirm")}
                    </button>
                    <button type="button" className="link-button" disabled={busy} onClick={cancelKey}>
                      {t("common.cancel")}
                    </button>
                  </div>
                </form>
              )}
            </>
          )}
          {message && <p className={message.error ? "error-banner" : "skills-hint"}>{message.text}</p>}
        </>
      )}
    </section>
  );
}

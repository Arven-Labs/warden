import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { t, type Key } from "../i18n";
import { hubFetchCuratorIndex, hubFetchCuratorPackage, hubListCurators, hubSaveCurators } from "../lib/hub";
import { FABIO_SOURCE, addCurator, checkCurator, hasFabioSource, removeCurator, sameCurators, type Curator } from "../lib/curators";
import { shortKey } from "../lib/trustedPublishers";
import type { CuratorEntry, CuratorIndex } from "../types";
import { KeyCancelled, usePairingKey } from "./PairingKeyDialog";

/** "Sources" (P160): the curators whose signed lists of skills this computer, or the hub in use, takes from. Warden ships none: the person
 * adds a name, the key that signs the list and where it is, and saves the whole list (on this computer's own config, or on the hub, which
 * asks for its pairing key). Opening a source reads its list and shows what it holds; "Read and install" downloads the one package, checked
 * against the list, and hands its text to `onInstall`, which previews it like a file: nothing is installed without reading it. The server
 * checks every entry again; `checkCurator` only spares the round trip. */
function Curators({
  remote,
  have,
  onInstall,
}: {
  remote: boolean;
  /** The names of the skills the person already has: an entry with one of them says so, and its preview shows what would change. */
  have: readonly string[];
  onInstall: (content: string) => void;
}) {
  const { askKey, dialog } = usePairingKey();
  const [saved, setSaved] = useState<Curator[] | null>(null);
  const [draft, setDraft] = useState<Curator[]>([]);
  const [name, setName] = useState("");
  const [key, setKey] = useState("");
  const [url, setUrl] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  // The source being looked at; `index` is null while it is being read.
  const [open, setOpen] = useState<{ curator: string; index: CuratorIndex | null } | null>(null);
  const [getting, setGetting] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const list = remote ? await hubListCurators() : await invoke<Curator[]>("list_curators");
      setSaved(list);
      setDraft(list);
      setMessage(null);
    } catch (err) {
      setMessage({ text: t("skills.curatorLoadFailed", { error: String(err) }), error: true });
    }
  }, [remote]);

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

  async function handleSave() {
    setBusy(true);
    setMessage(null);
    try {
      const list = remote ? await hubSaveCurators(askKey, draft) : await invoke<Curator[]>("save_curators", { curators: draft });
      setSaved(list);
      setDraft(list);
      setMessage({ text: t("skills.curatorSaved"), error: false });
    } catch (err) {
      if (!(err instanceof KeyCancelled)) setMessage({ text: t("skills.curatorSaveFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
    } finally {
      setBusy(false);
    }
  }

  async function handleOpen(curator: string) {
    setMessage(null);
    setOpen({ curator, index: null });
    try {
      const index = remote ? await hubFetchCuratorIndex(curator) : await invoke<CuratorIndex>("fetch_curator_index", { curator });
      setOpen((current) => (current?.curator === curator ? { curator, index } : current));
    } catch (err) {
      setOpen(null);
      setMessage({ text: t("skills.curatorIndexFailed", { error: String(err) }), error: true });
    }
  }

  async function handleInstall(curator: string, entry: CuratorEntry) {
    setMessage(null);
    setGetting(entry.hash);
    try {
      const content = remote ? await hubFetchCuratorPackage(curator, entry.hash) : await invoke<string>("fetch_curator_package", { curator, hash: entry.hash });
      onInstall(content);
    } catch (err) {
      setMessage({ text: t("skills.curatorGetFailed", { error: String(err) }), error: true });
    } finally {
      setGetting(null);
    }
  }

  if (saved === null && message === null) return null;
  const changed = saved !== null && !sameCurators(saved, draft);

  return (
    <section className="settings-section">
      {dialog}
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("skills.curatorsTitle")}</h3>
      </div>
      <p className="settings-hint">{t("skills.curatorsHint")}</p>
      {saved === null ? (
        <p className="settings-error-banner">{message?.text}</p>
      ) : (
        <>
          {draft.length === 0 ? (
            <p className="settings-hint">{t("skills.curatorsNone")}</p>
          ) : (
            <div className="provider-list">
              {draft.map((c) => (
                <div className="provider-card skill-card" key={c.name}>
                  <div className="skill-card-header">
                    <span className="skill-card-name">{c.name}</span>
                    <div className="skill-card-actions">
                      <code title={c.key}>{shortKey(c.key)}…</code>
                      {!changed && (
                        <button type="button" className="settings-browse-btn" onClick={() => void handleOpen(c.name)} disabled={open?.curator === c.name}>
                          {t("skills.curatorOpen")}
                        </button>
                      )}
                      <button type="button" className="settings-browse-btn" onClick={() => setDraft(removeCurator(draft, c.name))} disabled={busy}>
                        {t("skills.curatorRemove")}
                      </button>
                    </div>
                  </div>
                  <p className="settings-hint">{c.url}</p>
                  {open?.curator === c.name && (
                    <div className="skill-editor">
                      {open.index === null ? (
                        <span className="settings-hint">{t("skills.curatorLoading")}</span>
                      ) : (
                        <>
                          <strong>{t("skills.curatorEntries", { name: open.index.name, sequence: String(open.index.sequence) })}</strong>
                          {open.index.entries.length === 0 && <span className="settings-hint">{t("skills.curatorEmpty")}</span>}
                          {open.index.entries.map((entry) => (
                            <div className="provider-card skill-card" key={entry.hash}>
                              <div className="skill-card-header">
                                <span className="skill-card-name">{entry.id}</span>
                                <div className="skill-card-actions">
                                  {entry.revoked && <span className="settings-error-banner">{t("skills.curatorRevoked")}</span>}
                                  <button type="button" className="settings-browse-btn" onClick={() => void handleInstall(c.name, entry)} disabled={entry.revoked || getting !== null}>
                                    {entry.update ? t("skills.curatorSeeUpdate") : t("skills.curatorInstall")}
                                  </button>
                                </div>
                              </div>
                              <p className="skill-card-description">{entry.description}</p>
                              <p className="settings-hint">
                                {t("skills.curatorEntryMeta", { version: entry.version, license: entry.license })}
                                {entry.author ? t("skills.by", { author: entry.author }) : ""}
                              </p>
                              {entry.update ? (
                                <p className="settings-hint">{t("skills.curatorUpdate")}</p>
                              ) : (
                                have.includes(entry.id) && <p className="settings-hint">{t("skills.curatorHaveName")}</p>
                              )}
                              {entry.revoked && entry.revokedReason && <p className="settings-hint">{t("skills.curatorRevokedWhy", { reason: entry.revokedReason })}</p>}
                            </div>
                          ))}
                          <div className="skill-editor-actions">
                            <button type="button" className="settings-browse-btn" onClick={() => setOpen(null)}>
                              {t("skills.curatorClose")}
                            </button>
                          </div>
                        </>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
          <label className="settings-label">
            {t("skills.curatorName")}
            <input className="settings-input" value={name} onChange={(e) => setName(e.currentTarget.value)} />
          </label>
          <label className="settings-label">
            {t("skills.curatorKey")}
            <input className="settings-input" value={key} spellCheck={false} autoComplete="off" onChange={(e) => setKey(e.currentTarget.value)} />
          </label>
          <label className="settings-label">
            {t("skills.curatorUrl")}
            <input className="settings-input" value={url} spellCheck={false} autoComplete="off" onChange={(e) => setUrl(e.currentTarget.value)} />
          </label>
          {problem && <p className="settings-error-banner">{problem}</p>}
          {!hasFabioSource(draft) && (
            <>
              <p className="settings-hint">{t("skills.curatorAddFabioHint")}</p>
              <div className="skill-editor-actions">
                <button type="button" className="settings-browse-btn" onClick={handleAddFabio} disabled={busy}>
                  {t("skills.curatorAddFabio")}
                </button>
              </div>
            </>
          )}
          <div className="skill-editor-actions">
            <button type="button" className="settings-browse-btn" onClick={handleAdd} disabled={key.trim() === "" || busy}>
              {t("skills.curatorAdd")}
            </button>
            <button type="button" className="settings-save-btn" onClick={() => void handleSave()} disabled={!changed || busy}>
              {busy ? t("skills.curatorSaving") : t("skills.curatorSave")}
            </button>
          </div>
          {changed && !busy && <p className="settings-hint">{t("skills.curatorUnsaved")}</p>}
          {message && <p className={message.error ? "settings-error-banner" : "settings-hint"}>{message.text}</p>}
        </>
      )}
    </section>
  );
}

export default Curators;

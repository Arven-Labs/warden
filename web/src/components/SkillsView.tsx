import { useCallback, useEffect, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { CatalogSkillDto, PackagePreviewDto, SkillDto, UserInfo } from "../hub/messages";
import { intlTag, t, type Key } from "../i18n";
import { catalogDescription } from "../i18n/catalog";
import { rich } from "../i18n/rich";
import { packageChange } from "../hub/curators";
import { signatureView } from "../hub/skillSignature";
import { SigningError, createKeyFile, publicKeyOf, signPackageText } from "../hub/signing";
import CopyButton from "./CopyButton";
import Curators from "./Curators";
import TrustedPublishers from "./TrustedPublishers";
import TrustThisKey from "./TrustThisKey";
import { diffLines, hasChanges, withContext } from "./skillDiff";

// Adapted from the extension's `sidepanel/SkillsView.tsx` (P78), talking to the hub directly
// instead of through a background service worker.

/** What the editor form is doing: `new` (name editable, refuses a taken name) or `edit` (name
 * locked, overwrites) — same split as the desktop's SkillsView. */
interface EditorState {
  mode: "new" | "edit";
  skill: SkillDto;
  /** A suggestion being edited: whether saving accepts it (P104). */
  accept?: boolean;
}

const emptySkill: SkillDto = { name: "", description: "", body: "", agents: [] };

/** The licenses offered when sharing a skill: what its author lets others do with it. */
const LICENSES: Array<{ id: string; label: Key }> = [
  { id: "CC0-1.0", label: "skills.license.cc0" },
  { id: "MIT", label: "skills.license.mit" },
  { id: "CC-BY-4.0", label: "skills.license.ccby" },
];

/** The skill being shared: the file's data, asked for before generating it. */
interface ExportState {
  name: string;
  version: string;
  author: string;
  license: string;
  /** Sign the file with the person's key (P160). `keyText` is the key file's text, held in memory only until the export ends, and
   * `signer` its public key, shown so the person sees whose signature it will be. */
  sign: boolean;
  keyText: string | null;
  signer: string | null;
  /** The key was made on this screen (`new`) and, for the hub's owner, then listed in "Who signs" (`trusted`). */
  created: "new" | "trusted" | null;
}

/** A package being imported: the text and, once read and checked, what it says. */
interface ImportState {
  content: string;
  preview: PackagePreviewDto | null;
  /** The person read a text with a warning and wants to install it anyway. */
  allowFlagged: boolean;
  /** The person agrees to replace one of their skills of the same name that is different. */
  replace: boolean;
  /** An Arweave address typed in to fetch a published package, instead of choosing a file. */
  address: string;
}

/** Downloads `content` as a file, through the browser. */
function downloadText(fileName: string, content: string, type = "application/json") {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  link.click();
  URL.revokeObjectURL(url);
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export default function SkillsView({
  conn,
  user,
  onLearningChange,
}: {
  conn: ServerConnection | null;
  /** Set for a member: their own learning switch shows when the workspace has learning on. */
  user?: UserInfo;
  onLearningChange?: (optOut: boolean) => void;
}) {
  const [skills, setSkills] = useState<SkillDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  // The gallery of ready-made skills (P140): `null` while loading or when the hub does not have it (a hub older than the gallery).
  const [catalog, setCatalog] = useState<CatalogSkillDto[] | null>(null);
  const [confirmReplace, setConfirmReplace] = useState<string | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  // Compartilhar uma skill como arquivo e importar uma (P160).
  const [exporting, setExporting] = useState<ExportState | null>(null);
  const [importing, setImporting] = useState<ImportState | null>(null);
  // Goes up when a key is trusted from the preview, so the "Who signs" list below reads itself again.
  const [trustedVersion, setTrustedVersion] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!conn) return;
    conn.listSkills().then(
      (list) => {
        setSkills(list);
        setError(null);
      },
      (err) => {
        setSkills((current) => current ?? []);
        setError(t("skills.listFailed", { error: message(err) }));
      },
    );
    conn.listSkillCatalog().then(setCatalog, () => setCatalog(null));
  }, [conn]);

  /** Reads the key file picked for signing into memory (it is dropped when the export ends) and shows whose key it is. */
  function handleChooseKeyFile(file: File | undefined) {
    if (!file) return;
    file.text().then(
      (text) => {
        try {
          const signer = publicKeyOf(text);
          setError(null);
          setExporting((current) => (current ? { ...current, keyText: text, signer, created: null } : current));
        } catch (err) {
          setExporting((current) => (current ? { ...current, keyText: null, signer: null, created: null } : current));
          setError(t("skills.signFailed", { error: message(err) }));
        }
      },
      (err) => setError(t("skills.readFailed", { error: message(err) })),
    );
  }

  /** Makes a new signing key here, gives its file to the person to keep, and uses it for this export. The private key is made in the
   * browser and never goes to the hub. */
  async function handleCreateKey() {
    setError(null);
    try {
      const key = await createKeyFile();
      downloadText("warden-signing-key.txt", key.text, "text/plain");
      setExporting((current) => (current ? { ...current, keyText: key.text, signer: key.publicKey, created: "new" } : current));
    } catch (err) {
      setError(t("skills.signFailed", { error: message(err) }));
    }
  }

  function handleExport() {
    if (!conn || !exporting) return;
    setError(null);
    setNotice(null);
    const author = exporting.author.trim();
    const { name, sign, keyText } = exporting;
    if (sign && keyText === null) {
      setError(t("skills.signNeedsKey"));
      return;
    }
    conn.exportSkill(name, exporting.version.trim(), author === "" ? null : author, exporting.license).then(
      async (packed) => {
        let content = packed.content;
        let signedBy: string | null = null;
        if (sign && keyText !== null) {
          // Signed here, so the key never reaches the hub; the hub then reads the signed file back, which is the proof that it accepts it.
          try {
            const signed = await signPackageText(content, keyText);
            const back = await conn.previewSkillPackage(signed.content);
            if (back.signer !== signed.signer) throw new Error("it read the signature as another key's");
            content = signed.content;
            signedBy = signed.signer;
          } catch (err) {
            setError(err instanceof SigningError ? t("skills.signFailed", { error: err.message }) : t("skills.signNotAccepted", { error: message(err) }));
            return;
          }
        }
        downloadText(`${name}.warden-skill.json`, content);
        const signedNote = signedBy ? ` ${t("skills.signedDone", { key: signedBy.slice(0, 16) })}` : "";
        setNotice(`${t("skills.fileDone")}${packed.attachmentsSkipped ? t("skills.attachmentsSkipped") : ""}${signedNote}`);
        setExporting(null);
      },
      (err) => setError(t("skills.exportFailed", { error: message(err) })),
    );
  }

  /** Reads and checks the text; nothing is installed yet. */
  function handleCheckImport(content: string) {
    if (!conn) return;
    setError(null);
    conn.previewSkillPackage(content).then(
      (preview) => setImporting({ content, preview, allowFlagged: false, replace: false, address: "" }),
      (err) => {
        // Stays on the paste step, with the text, so it can be fixed or swapped.
        setImporting({ content, preview: null, allowFlagged: false, replace: false, address: "" });
        setError(message(err));
      },
    );
  }

  /** Downloads the package published at the typed address and reads it as if it were a file. */
  function handleFetchImport() {
    if (!conn || !importing) return;
    setError(null);
    conn.fetchSkillPackage(importing.address.trim()).then(handleCheckImport, (err) => setError(message(err)));
  }

  function handleChooseImportFile(file: File | undefined) {
    if (!file) return;
    file.text().then(handleCheckImport, (err) => setError(t("skills.readFailed", { error: message(err) })));
  }

  /** The signing key was just added to the trusted list: reads the same text again so the line under the preview says so, keeping the
   * boxes the person already ticked, and has the "Who signs" list read itself again. */
  function handleTrusted() {
    if (!conn || !importing) return;
    setTrustedVersion((n) => n + 1);
    conn.previewSkillPackage(importing.content).then(
      (preview) => setImporting((current) => (current ? { ...current, preview } : current)),
      (err) => setError(message(err)),
    );
  }

  function handleInstallImport() {
    if (!conn || !importing?.preview) return;
    setError(null);
    const name = importing.preview.name;
    conn.installSkillPackage(importing.content, importing.replace, importing.allowFlagged).then(
      () => {
        setNotice(t("skills.installed", { name }));
        setImporting(null);
        refresh();
      },
      (err) => setError(message(err)),
    );
  }

  function handleInstall(name: string, overwrite: boolean) {
    if (!conn) return;
    setError(null);
    setInstalling(name);
    conn.installCatalogSkill(name, overwrite).then(
      () => {
        setInstalling(null);
        setConfirmReplace(null);
        refresh();
      },
      (err) => {
        setInstalling(null);
        setError(t("skills.installFailed", { error: message(err) }));
      },
    );
  }

  // Also re-runs after a reconnect, when `conn` is a new connection.
  useEffect(refresh, [refresh]);

  function handleSave() {
    if (!editor || !conn) return;
    setSaving(true);
    setError(null);
    // Accepting a suggestion is saving it without the mark; editing it without accepting keeps it a suggestion.
    const skill: SkillDto = editor.skill.proposed && editor.accept ? { ...editor.skill, proposed: false } : editor.skill;
    if (!skill.proposed) {
      delete skill.source;
      delete skill.proposedAt;
    }
    conn.saveSkill(skill, editor.mode === "edit").then(
      () => {
        setSaving(false);
        setEditor(null);
        refresh();
      },
      (err) => {
        setSaving(false);
        setError(t("skills.saveFailed", { error: message(err) }));
      },
    );
  }

  /** Accepts a suggestion as it is. */
  function handleAccept(skill: SkillDto) {
    if (!conn) return;
    setError(null);
    const accepted: SkillDto = { name: skill.name, description: skill.description, body: skill.body, agents: skill.agents };
    conn.saveSkill(accepted, true).then(refresh, (err) => setError(t("skills.acceptFailed", { error: message(err) })));
  }

  function handleDelete(name: string) {
    if (!conn) return;
    setError(null);
    conn.deleteSkill(name).then(
      () => {
        setConfirmDelete(null);
        refresh();
      },
      (err) => {
        setConfirmDelete(null);
        setError(t("skills.deleteFailed", { error: message(err) }));
      },
    );
  }

  function updateEditor(patch: Partial<SkillDto>) {
    setEditor((current) => (current ? { ...current, skill: { ...current.skill, ...patch } } : current));
  }

  const suggested = (skills ?? []).filter((s) => s.proposed);
  const active = (skills ?? []).filter((s) => !s.proposed);

  if (editor) {
    return (
      <form
        className="skills-editor"
        onSubmit={(e) => {
          e.preventDefault();
          handleSave();
        }}
      >
        <strong>{editor.mode === "new" ? t("skills.new") : t("skills.editing", { name: editor.skill.name })}</strong>
        <label>
          {t("common.name")}
          <input
            value={editor.skill.name}
            onChange={(e) => updateEditor({ name: e.target.value })}
            placeholder={t("skills.namePlaceholder")}
            disabled={editor.mode === "edit"}
            required
          />
          <span className="skills-hint">{t("skills.nameHint")}</span>
        </label>
        <label>
          {t("skills.description")}
          <input
            value={editor.skill.description}
            onChange={(e) => updateEditor({ description: e.target.value })}
            placeholder={t("skills.descriptionPlaceholder")}
            required
          />
        </label>
        <label>
          {t("skills.instructions")}
          <textarea
            value={editor.skill.body}
            onChange={(e) => updateEditor({ body: e.target.value })}
            rows={10}
            placeholder={t("skills.instructionsPlaceholder")}
            required
          />
        </label>
        {editor.skill.agents.length > 0 && <span className="skills-hint">{t("skills.restrictedTo", { agents: editor.skill.agents.join(", ") })}</span>}
        {editor.skill.proposed && (
          <label className="checkbox-row">
            <input type="checkbox" checked={editor.accept ?? false} onChange={(e) => setEditor({ ...editor, accept: e.target.checked })} />
            {t("skills.acceptThis")}
          </label>
        )}
        {error && <p className="error-banner">{error}</p>}
        <div className="skills-actions">
          <button type="submit" className="primary-button" disabled={saving || !conn}>
            {saving ? t("common.saving") : t("common.save")}
          </button>
          <button
            type="button"
            className="link-button"
            disabled={saving}
            onClick={() => {
              setEditor(null);
              setError(null);
            }}
          >
            {t("common.cancel")}
          </button>
        </div>
      </form>
    );
  }

  return (
    <div className="skills-view">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("skills.intro")}</span>
        <span className="skills-actions">
          <button
            type="button"
            className="link-button"
            disabled={!conn}
            onClick={() => {
              setError(null);
              setNotice(null);
              setImporting({ content: "", preview: null, allowFlagged: false, replace: false, address: "" });
            }}
          >
            {t("skills.import")}
          </button>
          <button
            type="button"
            className="primary-button"
            onClick={() => {
              setError(null);
              setEditor({ mode: "new", skill: emptySkill });
            }}
          >
            {t("skills.newButton")}
          </button>
        </span>
      </div>
      {error && <p className="error-banner">{error}</p>}
      {notice && <p className="skills-hint">{notice}</p>}
      {exporting && (
        <form
          className="skills-editor"
          onSubmit={(e) => {
            e.preventDefault();
            handleExport();
          }}
        >
          <strong>{t("skills.share", { name: exporting.name })}</strong>
          <span className="skills-hint">{t("skills.shareIntro")}</span>
          <label>
            {t("skills.version")}
            <input value={exporting.version} onChange={(e) => setExporting({ ...exporting, version: e.target.value })} required />
            <span className="skills-hint">{t("skills.versionHint")}</span>
          </label>
          <label>
            {t("skills.author")}
            <input value={exporting.author} placeholder={t("skills.authorPlaceholder")} onChange={(e) => setExporting({ ...exporting, author: e.target.value })} />
          </label>
          <label>
            {t("skills.license")}
            <select value={exporting.license} onChange={(e) => setExporting({ ...exporting, license: e.target.value })}>
              {LICENSES.map((l) => (
                <option key={l.id} value={l.id}>
                  {t(l.label)}
                </option>
              ))}
            </select>
          </label>
          <label className="checkbox-row">
            <input type="checkbox" checked={exporting.sign} onChange={(e) => setExporting({ ...exporting, sign: e.target.checked, keyText: null, signer: null, created: null })} />
            {t("skills.signWith")}
          </label>
          {exporting.sign && (
            <>
              <span className="skills-hint">{t("skills.signHint")}</span>
              <label>
                {t("skills.signKeyFile")}
                <input type="file" onChange={(e) => handleChooseKeyFile(e.target.files?.[0])} />
              </label>
              <button type="button" className="link-button" onClick={() => void handleCreateKey()}>
                {t("skills.signCreate")}
              </button>
              {exporting.signer && <span className="skills-hint">{t("skills.signAs", { key: exporting.signer.slice(0, 16) })}</span>}
              {exporting.created !== null && (
                <>
                  <span className="skills-hint">{t("skills.signCreated")}</span>
                  {/* The owner's "trust this key" already has its own copy button. */}
                  {!(exporting.created === "new" && conn && !user) && <CopyButton text={exporting.signer ?? ""} label={t("skills.copyKey")} />}
                  {exporting.created === "trusted" && <span className="skills-hint">{t("skills.signCreatedTrusted")}</span>}
                </>
              )}
            </>
          )}
          <div className="skills-actions">
            <button type="submit" className="primary-button" disabled={!conn}>
              {t("skills.download")}
            </button>
            <button type="button" className="link-button" onClick={() => setExporting(null)}>
              {t("common.cancel")}
            </button>
          </div>
        </form>
      )}
      {/* Outside the export form: "trust this key" has a form of its own, and a form inside a form is not valid HTML (its submit reloaded the page). */}
      {exporting?.created === "new" && conn && !user && exporting.signer && (
        <TrustThisKey
          conn={conn}
          fullKey={exporting.signer}
          onTrusted={() => {
            setTrustedVersion((n) => n + 1);
            setExporting((current) => (current ? { ...current, created: "trusted" } : current));
          }}
        />
      )}
      {importing && (
        <div className="skills-editor">
          <strong>{t("skills.importTitle")}</strong>
          {importing.preview === null ? (
            <>
              <span className="skills-hint">{t("skills.importIntro")}</span>
              <label>
                {t("skills.arweave")}
                <input value={importing.address} placeholder="ar://…" onChange={(e) => setImporting({ ...importing, address: e.target.value })} />
              </label>
              <button type="button" className="link-button" disabled={!conn || importing.address.trim() === ""} onClick={handleFetchImport}>
                {t("skills.fetch")}
              </button>
              <textarea
                rows={6}
                placeholder={t("skills.pastePlaceholder")}
                value={importing.content}
                onChange={(e) => setImporting({ ...importing, content: e.target.value })}
              />
              <input type="file" accept=".json,application/json" onChange={(e) => handleChooseImportFile(e.target.files?.[0])} />
              <div className="skills-actions">
                <button type="button" className="primary-button" disabled={!conn || importing.content.trim() === ""} onClick={() => handleCheckImport(importing.content)}>
                  {t("skills.read")}
                </button>
                <button type="button" className="link-button" onClick={() => setImporting(null)}>
                  {t("common.cancel")}
                </button>
              </div>
            </>
          ) : (
            <>
              <span className="skills-item-name">{importing.preview.name}</span>
              <p className="skills-item-description">{importing.preview.description}</p>
              <p className="skills-hint">
                {t("skills.previewMeta", { version: importing.preview.version, license: importing.preview.license })}
                {importing.preview.author ? t("skills.fromAuthor", { author: importing.preview.author }) : t("skills.noAuthor")} ·{" "}
                <code>{importing.preview.hash.slice(0, 19)}…</code>
              </p>
              <p className="skills-hint">{t("skills.hashNote")}</p>
              {(() => {
                const sig = signatureView(importing.preview);
                if (sig.kind === "trusted") {
                  return <p className="skills-hint">{sig.name ? t("skills.signedTrusted", { name: sig.name }) : t("skills.signedTrustedNoName", { key: sig.key ?? "" })}</p>;
                }
                if (sig.kind === "unknown") {
                  return (
                    <>
                      <p className="error-banner">{t("skills.signedUnknown", { key: sig.key ?? "" })}</p>
                      {sig.fullKey && conn && !user && <TrustThisKey conn={conn} fullKey={sig.fullKey} onTrusted={handleTrusted} />}
                      {sig.fullKey && (user || !conn) && (
                        <button type="button" className="link-button" onClick={() => void navigator.clipboard?.writeText(sig.fullKey ?? "")}>
                          {t("skills.copyKey")}
                        </button>
                      )}
                    </>
                  );
                }
                return <p className="skills-hint">{t("skills.signedNone")}</p>;
              })()}
              {importing.preview.finding && <p className="error-banner">{t("skills.finding", { finding: importing.preview.finding })}</p>}
              {(() => {
                const change = importing.preview.status === "modified" ? packageChange(active.find((s) => s.name === importing.preview?.name), importing.preview) : null;
                if (!change) return null;
                return (
                  <details open>
                    <summary>{t("skills.previewChange")}</summary>
                    {change.description && <p className="skills-hint">{t("skills.previewDescription", change.description)}</p>}
                    <pre className="skills-body skills-diff">
                      {change.body.map((line, i) => (
                        <span key={i} className={`skills-diff-line skills-diff-line--${line.op}`}>
                          {line.op === "add" ? "+ " : line.op === "del" ? "- " : "  "}
                          {line.text}
                          {"\n"}
                        </span>
                      ))}
                    </pre>
                  </details>
                );
              })()}
              <pre className="skills-body">{importing.preview.body}</pre>
              {importing.preview.status === "installed" && <p className="skills-hint">{t("skills.alreadyHave")}</p>}
              {importing.preview.status === "modified" && (
                <label className="checkbox-row">
                  <input type="checkbox" checked={importing.replace} onChange={(e) => setImporting({ ...importing, replace: e.target.checked })} />
                  {t("skills.replaceMine")}
                </label>
              )}
              {importing.preview.finding && (
                <label className="checkbox-row">
                  <input type="checkbox" checked={importing.allowFlagged} onChange={(e) => setImporting({ ...importing, allowFlagged: e.target.checked })} />
                  {t("skills.installAnyway")}
                </label>
              )}
              <div className="skills-actions">
                <button
                  type="button"
                  className="primary-button"
                  disabled={
                    !conn ||
                    importing.preview.status === "installed" ||
                    (importing.preview.status === "modified" && !importing.replace) ||
                    (!!importing.preview.finding && !importing.allowFlagged)
                  }
                  onClick={handleInstallImport}
                >
                  {t("skills.installButton")}
                </button>
                <button type="button" className="link-button" onClick={() => setImporting({ ...importing, preview: null })}>
                  {t("skills.back")}
                </button>
                <button type="button" className="link-button" onClick={() => setImporting(null)}>
                  {t("common.cancel")}
                </button>
              </div>
            </>
          )}
        </div>
      )}
      {user && user.learningEnabled && (
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={!user.learningOptOut}
            disabled={!conn}
            onChange={(e) => {
              const optOut = !e.target.checked;
              setError(null);
              conn?.setLearning(!optOut).then(
                () => onLearningChange?.(optOut),
                (err) => setError(t("skills.learningFailed", { error: message(err) })),
              );
            }}
          />
          {t("skills.learning")}
        </label>
      )}
      {suggested.length > 0 && (
        <section>
          <h3>{t("skills.suggestedTitle")}</h3>
          <p className="skills-hint">{rich("skills.suggestedIntro", { strong: <strong>{t("skills.nothingValid")}</strong> })}</p>
          <ul className="skills-list">
            {suggested.map((skill) => (
              <li key={skill.name} className="skills-item skills-item--suggested">
                <div className="skills-item-header">
                  <span className="skills-item-name">{skill.name}</span>
                  <span className="devices-status devices-status--pending">{skill.revises ? t("skills.changeTo", { name: skill.revises }) : t("skills.suggested")}</span>
                </div>
                <p className="skills-item-description">{skill.description || t("skills.noDescription")}</p>
                <p className="skills-hint">
                  {skill.source ? rich("skills.fromConversation", { id: <code>{skill.source}</code> }) : t("skills.fromSome")}
                  {skill.proposedAt ? ` · ${new Date(skill.proposedAt).toLocaleString(intlTag())}` : ""}
                  {skill.agents.length > 0 ? t("skills.onlyFor", { agents: skill.agents.join(", ") }) : ""}
                </p>
                {(() => {
                  const target = skill.revises ? active.find((s) => s.name === skill.revises) : undefined;
                  if (!target) {
                    return (
                      <details>
                        <summary>{t("skills.viewInstructions")}</summary>
                        <pre className="skills-body">{skill.body}</pre>
                      </details>
                    );
                  }
                  const diff = diffLines(target.body, skill.body);
                  return (
                    <details>
                      <summary>{t("skills.viewChange", { name: target.name })}</summary>
                      {hasChanges(diff) ? (
                        <pre className="skills-body skills-diff">
                          {withContext(diff).map((line, i) => (
                            <span key={i} className={`skills-diff-line skills-diff-line--${line.op}`}>
                              {line.op === "add" ? "+ " : line.op === "del" ? "- " : "  "}
                              {line.text}
                              {"\n"}
                            </span>
                          ))}
                        </pre>
                      ) : (
                        <p className="skills-hint">{t("skills.noTextChange")}</p>
                      )}
                    </details>
                  );
                })()}
                {confirmDelete === skill.name ? (
                  <span className="skills-actions">
                    <button type="button" className="link-button skills-danger" onClick={() => handleDelete(skill.name)}>
                      {t("skills.rejectForSure")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(null)}>
                      {t("common.keep")}
                    </button>
                  </span>
                ) : (
                  <span className="skills-actions">
                    <button type="button" className="primary-button" disabled={!conn} onClick={() => handleAccept(skill)}>
                      {t("skills.accept")}
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setEditor({ mode: "edit", skill, accept: true });
                      }}
                    >
                      {t("common.edit")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(skill.name)}>
                      {t("skills.reject")}
                    </button>
                  </span>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
      {skills === null ? (
        <p className="skills-hint">{t("common.loading")}</p>
      ) : active.length === 0 ? (
        <p className="skills-hint">{t("skills.none")}</p>
      ) : (
        <ul className="skills-list">
          {active.map((skill) => (
            <li key={skill.name} className="skills-item">
              <div className="skills-item-header">
                <span className="skills-item-name">{skill.name}</span>
                {confirmDelete === skill.name ? (
                  <span className="skills-actions">
                    <button type="button" className="link-button skills-danger" onClick={() => handleDelete(skill.name)}>
                      {t("common.delete")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(null)}>
                      {t("common.keep")}
                    </button>
                  </span>
                ) : (
                  <span className="skills-actions">
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setEditor({ mode: "edit", skill });
                      }}
                    >
                      {t("common.edit")}
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setNotice(null);
                        setExporting({ name: skill.name, version: "1.0.0", author: "", license: LICENSES[0].id, sign: false, keyText: null, signer: null, created: null });
                      }}
                    >
                      {t("spaces.share")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(skill.name)}>
                      {t("common.delete")}
                    </button>
                  </span>
                )}
              </div>
              <p className="skills-item-description">{skill.description || t("skills.noDescription")}</p>
              {skill.agents.length > 0 && <p className="skills-hint">{t("skills.onlyForPlain", { agents: skill.agents.join(", ") })}</p>}
            </li>
          ))}
        </ul>
      )}
      {catalog !== null && catalog.length > 0 && (
        <section>
          <h3>{t("skills.gallery")}</h3>
          <p className="skills-hint">{rich("skills.galleryIntro", { path: <code>skills/</code> })}</p>
          <ul className="skills-list">
            {catalog.map((entry) => (
              <li key={entry.name} className="skills-item">
                <div className="skills-item-header">
                  <span className="skills-item-name">{entry.name}</span>
                  {entry.status === "installed" ? (
                    <span className="skills-hint">{t("skills.galleryInstalled")}</span>
                  ) : entry.status === "modified" && confirmReplace === entry.name ? (
                    <span className="skills-actions">
                      <button
                        type="button"
                        className="link-button skills-danger"
                        disabled={installing === entry.name}
                        onClick={() => handleInstall(entry.name, true)}
                      >
                        {t("skills.replaceMineButton")}
                      </button>
                      <button type="button" className="link-button" onClick={() => setConfirmReplace(null)}>
                        {t("skills.keepMine")}
                      </button>
                    </span>
                  ) : entry.status === "modified" ? (
                    <span className="skills-actions">
                      <span className="skills-hint">{t("skills.otherSameName")}</span>
                      <button type="button" className="link-button" onClick={() => setConfirmReplace(entry.name)}>
                        {t("skills.useGallery")}
                      </button>
                    </span>
                  ) : entry.status === "outdated" ? (
                    <span className="skills-actions">
                      <span className="skills-hint">{t("skills.updateAvailable")}</span>
                      <button
                        type="button"
                        className="primary-button"
                        disabled={!conn || installing === entry.name}
                        onClick={() => handleInstall(entry.name, false)}
                      >
                        {installing === entry.name ? t("skills.installing") : t("skills.updateButton")}
                      </button>
                    </span>
                  ) : (
                    <span className="skills-actions">
                      <button
                        type="button"
                        className="primary-button"
                        disabled={!conn || installing === entry.name}
                        onClick={() => handleInstall(entry.name, false)}
                      >
                        {installing === entry.name ? t("skills.installing") : t("skills.installButton")}
                      </button>
                    </span>
                  )}
                </div>
                <p className="skills-item-description">{catalogDescription(entry.name, entry.description)}</p>
                <details>
                  <summary>{t("skills.viewInstructions")}</summary>
                  <pre className="skills-body">{entry.body}</pre>
                </details>
              </li>
            ))}
          </ul>
        </section>
      )}
      {/* The sources are the owner's to change; a member reads what they list. The package they pick opens in the import preview above. */}
      <Curators
        conn={conn}
        owner={!user}
        have={active.map((s) => s.name)}
        onInstall={(content) => {
          window.scrollTo?.({ top: 0 });
          handleCheckImport(content);
        }}
      />
      {/* The keys the hub trusts are the owner's to change: a member does not see them. */}
      {!user && <TrustedPublishers conn={conn} key={trustedVersion} />}
    </div>
  );
}

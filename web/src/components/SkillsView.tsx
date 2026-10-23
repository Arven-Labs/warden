import { useCallback, useEffect, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { CatalogSkillDto, PackagePreviewDto, SkillDto, UserInfo } from "../hub/messages";
import { intlTag, t, type Key } from "../i18n";
import { catalogDescription } from "../i18n/catalog";
import { rich } from "../i18n/rich";
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
function downloadText(fileName: string, content: string) {
  const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
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

  function handleExport() {
    if (!conn || !exporting) return;
    setError(null);
    setNotice(null);
    const author = exporting.author.trim();
    conn.exportSkill(exporting.name, exporting.version.trim(), author === "" ? null : author, exporting.license).then(
      (packed) => {
        downloadText(`${exporting.name}.warden-skill.json`, packed.content);
        setNotice(`${t("skills.fileDone")}${packed.attachmentsSkipped ? t("skills.attachmentsSkipped") : ""}`);
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
              {importing.preview.finding && <p className="error-banner">{t("skills.finding", { finding: importing.preview.finding })}</p>}
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
                        setExporting({ name: skill.name, version: "1.0.0", author: "", license: LICENSES[0].id });
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
    </div>
  );
}

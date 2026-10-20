import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { rich } from "../i18n/rich";
import type { ProjectEntry } from "../types";

const emptyProject: ProjectEntry = { id: "", name: "", description: "", instructions: "" };

interface EditorState {
  mode: "new" | "edit";
  project: ProjectEntry;
}

/** `Tax return 2026` → `tax-return-2026`: a first guess at the folder name, which the person can change until saving. */
function slugFrom(name: string): string {
  return name
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

/** The files of a project: the notes of the vault under `projects/<id>/`, listed here and added to from here
 * (anything else is done in the Vault screen, which sees the same files). */
function ProjectFiles({ projectId }: { projectId: string }) {
  const prefix = `projects/${projectId}/`;
  const [files, setFiles] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<{ name: string; content: string } | null>(null);
  const [busy, setBusy] = useState(false);

  function refresh() {
    invoke<string[]>("list_vault_files")
      .then((all) => {
        setFiles(all.filter((path) => path.startsWith(prefix) && path !== `${prefix}PROJECT.md`).map((path) => path.slice(prefix.length)));
        setError(null);
      })
      .catch((err) => setError(String(err)));
  }

  useEffect(refresh, [projectId]);

  async function saveDraft() {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const name = draft.name.trim().replace(/^\/+/, "");
      // No `expectedVersion`: creating, so an existing note of that name is refused instead of overwritten.
      await invoke("save_vault_note", { path: `${prefix}${name}`, content: draft.content, expectedVersion: null });
      setDraft(null);
      refresh();
    } catch (err) {
      setError(typeof err === "object" && err !== null && "message" in err ? String((err as { message: unknown }).message) : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="settings-field">
      <span className="settings-label">{t("projects.files")}</span>
      <span className="settings-hint">{rich("projects.filesHint", { prefix: <code>{prefix}</code> })}</span>
      {error && <p className="settings-error-banner">{error}</p>}
      {files === null ? (
        <span className="settings-hint">{t("common.loading")}</span>
      ) : files.length === 0 ? (
        <span className="settings-hint">{t("projects.noFiles")}</span>
      ) : (
        <ul className="project-file-list">
          {files.map((file) => (
            <li key={file}>
              <code>{file}</code>
            </li>
          ))}
        </ul>
      )}
      {draft ? (
        <div className="skill-file-draft">
          <input
            className="settings-input"
            type="text"
            placeholder={t("projects.notePlaceholder")}
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.currentTarget.value })}
          />
          <textarea
            className="settings-input settings-textarea"
            rows={8}
            placeholder={t("projects.noteText")}
            value={draft.content}
            onChange={(e) => setDraft({ ...draft, content: e.currentTarget.value })}
          />
          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={saveDraft} disabled={busy || draft.name.trim() === ""}>
              {busy ? t("common.saving") : t("projects.saveNote")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setDraft(null)} disabled={busy}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      ) : (
        <div>
          <button type="button" className="settings-browse-btn" onClick={() => setDraft({ name: "", content: "" })}>
            {t("projects.addNote")}
          </button>
        </div>
      )}
    </div>
  );
}

interface ShareAllResult {
  moved: string[];
  skipped: { id: string; reason: string }[];
}

/** `onConversationsMoved` is given on this computer only: the conversations of code projects it can put on the hub (P124). */
function ProjectsView({ onChanged, onConversationsMoved }: { onChanged: () => void; onConversationsMoved?: () => void }) {
  const [confirmShareAll, setConfirmShareAll] = useState(false);
  const [confirmTakeBack, setConfirmTakeBack] = useState(false);
  const [sharing, setSharing] = useState(false);
  const [shareResult, setShareResult] = useState<ShareAllResult | null>(null);
  const [shareError, setShareError] = useState<string | null>(null);

  /** Moves the code conversations to the hub (`toHub`) or back to this computer, all at once. */
  async function handleMoveAll(toHub: boolean) {
    setSharing(true);
    setShareError(null);
    setShareResult(null);
    try {
      setShareResult(await invoke<ShareAllResult>(toHub ? "share_all_conversations" : "unshare_all_conversations"));
      onConversationsMoved?.();
    } catch (err) {
      setShareError(String(err));
    } finally {
      setSharing(false);
      setConfirmShareAll(false);
      setConfirmTakeBack(false);
    }
  }

  const [projects, setProjects] = useState<ProjectEntry[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  // Whether the id was typed by hand: until then it follows the name.
  const [idTouched, setIdTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  function refresh() {
    invoke<ProjectEntry[]>("list_projects")
      .then((list) => {
        setProjects(list);
        setLoadError(null);
      })
      .catch((err) => setLoadError(String(err)));
    onChanged();
  }

  useEffect(refresh, []);

  function openNew() {
    setError(null);
    setIdTouched(false);
    setEditor({ mode: "new", project: emptyProject });
  }

  function openEdit(project: ProjectEntry) {
    setError(null);
    setEditor({ mode: "edit", project });
  }

  function updateEditor(patch: Partial<ProjectEntry>) {
    setEditor((current) => (current ? { ...current, project: { ...current.project, ...patch } } : current));
  }

  async function handleSave() {
    if (!editor) return;
    setError(null);
    setSaving(true);
    try {
      await invoke("save_project", { project: editor.project, overwrite: editor.mode === "edit" });
      setEditor(null);
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(id: string) {
    setError(null);
    try {
      await invoke("delete_project", { id });
      setConfirmDelete(null);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div className="settings-view">
      <h2 className="settings-title">{t("projects.title")}</h2>
      <p className="settings-hint">{rich("projects.hint", { folder: <code>projects/</code> })}</p>

      {loadError && <p className="settings-error-banner">{loadError}</p>}
      {error && <p className="settings-error-banner">{error}</p>}

      {editor && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">{editor.mode === "new" ? t("projects.newProject") : t("projects.editProject", { name: editor.project.name })}</span>

          <label className="settings-field">
            <span className="settings-label">{t("projects.name")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder={t("projects.namePlaceholder")}
              value={editor.project.name}
              onChange={(e) => {
                const name = e.currentTarget.value;
                updateEditor(editor.mode === "new" && !idTouched ? { name, id: slugFrom(name) } : { name });
              }}
            />
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("projects.folderName")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder={t("projects.folderPlaceholder")}
              value={editor.project.id}
              disabled={editor.mode === "edit"}
              onChange={(e) => {
                setIdTouched(true);
                updateEditor({ id: e.currentTarget.value });
              }}
            />
            <span className="settings-hint">{t("projects.folderHint")}</span>
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("projects.description")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder={t("projects.descriptionPlaceholder")}
              value={editor.project.description}
              onChange={(e) => updateEditor({ description: e.currentTarget.value })}
            />
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("projects.workdir")}</span>
            <input
              className="settings-input"
              placeholder={t("projects.workdirPlaceholder")}
              value={editor.project.workdir ?? ""}
              onChange={(e) => updateEditor({ workdir: e.currentTarget.value })}
            />
            <span className="settings-hint">{t("projects.workdirHint")}</span>
          </label>

          <label className="settings-checkbox-row">
            <input
              type="checkbox"
              checked={editor.project.code ?? false}
              disabled={!(editor.project.workdir ?? "").trim()}
              onChange={(e) => updateEditor({ code: e.currentTarget.checked })}
            />
            <span>{t("projects.codeMode")}</span>
          </label>
          <span className="settings-hint">{t("projects.codeHint")}</span>

          <label className="settings-field">
            <span className="settings-label">{t("projects.instructions")}</span>
            <textarea
              className="settings-input settings-textarea"
              rows={10}
              placeholder={t("projects.instructionsPlaceholder")}
              value={editor.project.instructions}
              onChange={(e) => updateEditor({ instructions: e.currentTarget.value })}
            />
            <span className="settings-hint">{t("projects.instructionsHint")}</span>
          </label>

          {editor.mode === "edit" ? (
            <ProjectFiles projectId={editor.project.id} />
          ) : (
            <span className="settings-hint">{t("projects.saveFirst")}</span>
          )}

          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={handleSave} disabled={saving}>
              {saving ? t("common.saving") : t("projects.save")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setEditor(null)} disabled={saving}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}

      <section className="settings-section">
        <div className="settings-section-header">
          <h3 className="settings-section-title">{t("projects.yours")}</h3>
          <button type="button" className="settings-browse-btn" onClick={openNew}>
            {t("projects.add")}
          </button>
        </div>

        {projects === null ? (
          <p className="settings-hint">{t("projects.loading")}</p>
        ) : projects.length === 0 ? (
          <p className="settings-hint">{t("projects.none")}</p>
        ) : (
          <div className="provider-list">
            {projects.map((project) => (
              <div className="provider-card skill-card" key={project.id}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{project.name}</span>
                  <span className="settings-hint">
                    <code>{project.id}</code>
                  </span>
                  <div className="skill-card-actions">
                    {confirmDelete === project.id ? (
                      <>
                        <span className="settings-hint">{t("projects.confirmRemove")}</span>
                        <button type="button" className="provider-delete-btn" onClick={() => handleDelete(project.id)}>
                          {t("common.remove")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                          {t("common.keep")}
                        </button>
                      </>
                    ) : (
                      <>
                        <button type="button" className="settings-browse-btn" onClick={() => openEdit(project)}>
                          {t("common.edit")}
                        </button>
                        <button
                          type="button"
                          className="provider-delete-btn"
                          onClick={() => setConfirmDelete(project.id)}
                          aria-label={t("projects.removeLabel", { name: project.name })}
                          title={t("projects.removeTitle")}
                        >
                          🗑
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">{project.description || t("projects.noDescription")}</p>
              </div>
            ))}
          </div>
        )}
      </section>

      {onConversationsMoved && projects?.some((p) => p.code) && (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("projects.codeOnHub")}</h3>
            {confirmShareAll || confirmTakeBack ? (
              <div className="skill-card-actions">
                <span className="settings-hint">
                  {confirmShareAll ? t("projects.confirmShare") : t("projects.confirmTakeBack")}
                </span>
                <button type="button" className="settings-save-btn" onClick={() => void handleMoveAll(confirmShareAll)} disabled={sharing}>
                  {sharing ? t("projects.moving") : confirmShareAll ? t("projects.shareAllNow") : t("projects.takeAllBackNow")}
                </button>
                <button
                  type="button"
                  className="settings-browse-btn"
                  onClick={() => {
                    setConfirmShareAll(false);
                    setConfirmTakeBack(false);
                  }}
                  disabled={sharing}
                >
                  {t("common.cancel")}
                </button>
              </div>
            ) : (
              <div className="skill-card-actions">
                <button type="button" className="settings-browse-btn" onClick={() => setConfirmShareAll(true)}>
                  {t("projects.shareAll")}
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setConfirmTakeBack(true)}>
                  {t("projects.takeAllBack")}
                </button>
              </div>
            )}
          </div>
          <p className="settings-hint">{t("projects.shareHint")}</p>
          {shareError && <p className="settings-error-banner">{shareError}</p>}
          {shareResult && (
            <>
              <p className="settings-hint">
                {shareResult.skipped.length > 0
                  ? t("projects.movedSkipped", { count: shareResult.moved.length, skipped: shareResult.skipped.length })
                  : t("projects.movedDone", { count: shareResult.moved.length })}
              </p>
              {shareResult.skipped.length > 0 && (
                <ul className="settings-hint">
                  {shareResult.skipped.map((s) => (
                    <li key={s.id}>
                      <code>{s.id}</code>: {s.reason}
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}
        </section>
      )}
    </div>
  );
}

export default ProjectsView;

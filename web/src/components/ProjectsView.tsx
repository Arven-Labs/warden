import { useCallback, useEffect, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { ProjectDto } from "../hub/messages";
import { t } from "../i18n";
import { rich } from "../i18n/rich";

/** What the editor form is doing: `new` (id editable, refuses a taken one) or `edit` (id locked, overwrites) — the same
 * split as the Skills screen. */
interface EditorState {
  mode: "new" | "edit";
  project: ProjectDto;
}

const emptyProject: ProjectDto = { id: "", name: "", description: "", instructions: "" };

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** `Declaração 2026` → `declaracao-2026`: a first guess at the folder name, which can be changed until saving. */
function slugFrom(name: string): string {
  return name
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

/** The files of a project: the vault's notes under `projects/<id>/`, listed here and added to from here (the Vault tab
 * sees the same files). */
function ProjectFiles({ conn, projectId }: { conn: ServerConnection | null; projectId: string }) {
  const prefix = `projects/${projectId}/`;
  const [files, setFiles] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<{ name: string; content: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    if (!conn) return;
    conn.listVaultFiles().then(
      (all) => {
        setFiles(all.filter((path) => path.startsWith(prefix) && path !== `${prefix}PROJECT.md`).map((path) => path.slice(prefix.length)));
        setError(null);
      },
      (err) => setError(t("projects.listFilesFailed", { error: message(err) })),
    );
  }, [conn, prefix]);

  useEffect(refresh, [refresh]);

  function saveDraft() {
    if (!draft || !conn) return;
    setBusy(true);
    setError(null);
    // No expected version: this creates the note, so one that exists is refused instead of overwritten.
    conn.saveVaultNote(`${prefix}${draft.name.trim().replace(/^\/+/, "")}`, draft.content).then(
      () => {
        setBusy(false);
        setDraft(null);
        refresh();
      },
      (err) => {
        setBusy(false);
        setError(t("projects.saveNoteFailed", { error: message(err) }));
      },
    );
  }

  return (
    <div className="project-files">
      <strong>{t("projects.files")}</strong>
      <span className="skills-hint">{rich("projects.filesHint", { prefix: <code>{prefix}</code> })}</span>
      {error && <p className="error-banner">{error}</p>}
      {files === null ? (
        <span className="skills-hint">{t("common.loading")}</span>
      ) : files.length === 0 ? (
        <span className="skills-hint">{t("projects.noFiles")}</span>
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
        <div className="project-file-draft">
          <input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="notes.md" aria-label={t("projects.fileName")} />
          <textarea value={draft.content} onChange={(e) => setDraft({ ...draft, content: e.target.value })} rows={8} placeholder={t("projects.noteTextPlaceholder")} aria-label={t("projects.noteText")} />
          <div className="skills-actions">
            <button type="button" className="primary-button" onClick={saveDraft} disabled={busy || !conn || draft.name.trim() === ""}>
              {busy ? t("common.saving") : t("projects.saveNote")}
            </button>
            <button type="button" className="link-button" onClick={() => setDraft(null)} disabled={busy}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      ) : (
        <div>
          <button type="button" className="link-button" onClick={() => setDraft({ name: "", content: "" })}>
            {t("projects.addNote")}
          </button>
        </div>
      )}
    </div>
  );
}

/** `onChanged` gets the list just read, so the chat's picker and list follow what was made, edited or removed here. It
 * must be a stable function (a state setter): `refresh` depends on it. */
export default function ProjectsView({ conn, onChanged }: { conn: ServerConnection | null; onChanged: (projects: ProjectDto[]) => void }) {
  const [projects, setProjects] = useState<ProjectDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  // Whether the id was typed by hand: until then it follows the name.
  const [idTouched, setIdTouched] = useState(false);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!conn) return;
    conn.listProjects().then(
      (list) => {
        setProjects(list);
        setError(null);
        onChanged(list);
      },
      (err) => {
        setProjects((current) => current ?? []);
        setError(t("projects.listFailed", { error: message(err) }));
      },
    );
  }, [conn, onChanged]);

  // Also re-runs after a reconnect, when `conn` is a new connection.
  useEffect(refresh, [refresh]);

  function updateEditor(patch: Partial<ProjectDto>) {
    setEditor((current) => (current ? { ...current, project: { ...current.project, ...patch } } : current));
  }

  function handleSave() {
    if (!editor || !conn) return;
    setSaving(true);
    setError(null);
    conn.saveProject(editor.project, editor.mode === "edit").then(
      () => {
        setSaving(false);
        setEditor(null);
        refresh();
      },
      (err) => {
        setSaving(false);
        setError(t("projects.saveFailed", { error: message(err) }));
      },
    );
  }

  function handleDelete(id: string) {
    if (!conn) return;
    setError(null);
    conn.deleteProject(id).then(
      () => {
        setConfirmDelete(null);
        refresh();
      },
      (err) => {
        setConfirmDelete(null);
        setError(t("projects.removeFailed", { error: message(err) }));
      },
    );
  }

  if (editor) {
    return (
      <form
        className="skills-editor"
        onSubmit={(e) => {
          e.preventDefault();
          handleSave();
        }}
      >
        <strong>{editor.mode === "new" ? t("projects.new") : t("projects.editing", { name: editor.project.name })}</strong>
        <label>
          {t("common.name")}
          <input
            value={editor.project.name}
            onChange={(e) => {
              const name = e.target.value;
              updateEditor(editor.mode === "new" && !idTouched ? { name, id: slugFrom(name) } : { name });
            }}
            placeholder={t("projects.namePlaceholder")}
            required
          />
        </label>
        <label>
          {t("projects.folderName")}
          <input
            value={editor.project.id}
            onChange={(e) => {
              setIdTouched(true);
              updateEditor({ id: e.target.value });
            }}
            placeholder={t("projects.folderNamePlaceholder")}
            disabled={editor.mode === "edit"}
            required
          />
          <span className="skills-hint">{t("projects.folderNameHint")}</span>
        </label>
        <label>
          {t("projects.description")}
          <input value={editor.project.description} onChange={(e) => updateEditor({ description: e.target.value })} placeholder={t("projects.descriptionPlaceholder")} />
        </label>
        <label>
          {t("projects.workdir")}
          <input
            value={editor.project.workdir ?? ""}
            onChange={(e) => updateEditor({ workdir: e.target.value })}
            placeholder={t("projects.workdirPlaceholder")}
          />
          <span className="skills-hint">{t("projects.workdirHint")}</span>
        </label>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={editor.project.code ?? false}
            disabled={!(editor.project.workdir ?? "").trim()}
            onChange={(e) => updateEditor({ code: e.target.checked })}
          />
          {t("projects.codeMode")}
        </label>
        <span className="skills-hint">{t("projects.codeModeHint")}</span>
        <label>
          {t("projects.instructions")}
          <textarea
            value={editor.project.instructions}
            onChange={(e) => updateEditor({ instructions: e.target.value })}
            rows={10}
            placeholder={t("projects.instructionsPlaceholder")}
          />
          <span className="skills-hint">{t("projects.instructionsHint")}</span>
        </label>
        {editor.mode === "edit" ? (
          <ProjectFiles conn={conn} projectId={editor.project.id} />
        ) : (
          <span className="skills-hint">{t("projects.saveFirst")}</span>
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
        <span className="skills-hint">{t("projects.intro")}</span>
        <button
          type="button"
          className="primary-button"
          onClick={() => {
            setError(null);
            setIdTouched(false);
            setEditor({ mode: "new", project: emptyProject });
          }}
        >
          {t("projects.newButton")}
        </button>
      </div>
      {error && <p className="error-banner">{error}</p>}
      {projects === null ? (
        <p className="skills-hint">{t("common.loading")}</p>
      ) : projects.length === 0 ? (
        <p className="skills-hint">{t("projects.none")}</p>
      ) : (
        <ul className="skills-list">
          {projects.map((project) => (
            <li key={project.id} className="skills-item">
              <div className="skills-item-header">
                <span className="skills-item-name">{project.name}</span>
                <code className="skills-hint">{project.id}</code>
                {confirmDelete === project.id ? (
                  <span className="skills-actions">
                    <span className="skills-hint">{t("projects.filesStay")}</span>
                    <button type="button" className="link-button skills-danger" onClick={() => handleDelete(project.id)}>
                      {t("projects.removeForSure")}
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
                        setEditor({ mode: "edit", project });
                      }}
                    >
                      {t("common.edit")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(project.id)}>
                      {t("common.remove")}
                    </button>
                  </span>
                )}
              </div>
              <p className="skills-item-description">{project.description || t("projects.noDescription")}</p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

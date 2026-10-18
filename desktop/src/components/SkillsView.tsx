import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AgentEntry, CatalogSkill, ExportedPackage, PackagePreview, ProviderEntry, SkillEntry } from "../types";
import { diffLines, hasChanges, withContext } from "./skillDiff";
import CopyButton from "./CopyButton";
import { hubSkills } from "../lib/hub";

/** What the editor form is doing: `new` (name editable, refuses a taken name) or `edit` (name
 * locked, overwrites). `fromAi` only drives the "review before saving" hint. */
interface EditorState {
  mode: "new" | "edit";
  skill: SkillEntry;
  fromAi: boolean;
  /** Editing a suggestion: saving accepts it only when this is on (P104). */
  accept?: boolean;
}

const emptySkill: SkillEntry = { name: "", description: "", body: "", agents: [] };

/** An attachment being written: `existing` locks the name (it's an edit of a file already there). */
interface FileDraft {
  name: string;
  content: string;
  existing: boolean;
}

/** Text files attached to a saved skill (P72 d) — scripts, templates, notes the AI can read through
 * `read_skill_file`. Changes hit the vault straight away, independent of the form's "Save skill". */
function SkillFiles({ skillName }: { skillName: string }) {
  const [files, setFiles] = useState<string[] | null>(null);
  const [draft, setDraft] = useState<FileDraft | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function refresh() {
    invoke<string[]>("list_skill_files", { name: skillName })
      .then(setFiles)
      .catch((err) => setError(String(err)));
  }

  useEffect(refresh, [skillName]);

  async function openFile(file: string) {
    setError(null);
    try {
      const content = await invoke<string>("read_skill_attachment", { name: skillName, file });
      setDraft({ name: file, content, existing: true });
    } catch (err) {
      setError(String(err));
    }
  }

  async function saveDraft() {
    if (!draft) return;
    setError(null);
    setBusy(true);
    try {
      await invoke("save_skill_attachment", { name: skillName, file: draft.name, content: draft.content });
      setDraft(null);
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function removeFile(file: string) {
    setError(null);
    try {
      await invoke("delete_skill_attachment", { name: skillName, file });
      setConfirmDelete(null);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div className="settings-field">
      <span className="settings-label">Attached files</span>
      <span className="settings-hint">
        Text files the AI can read when it uses this skill, such as a script or a template (up to 20 files, 64 KB
        each). A script only runs if the shell tool is enabled in Settings.
      </span>
      {error && <p className="settings-error-banner">{error}</p>}
      {files === null ? null : files.length === 0 ? (
        <span className="settings-hint">No files attached.</span>
      ) : (
        <div className="skill-file-list">
          {files.map((file) => (
            <div className="skill-file-row" key={file}>
              <span className="skill-card-name">{file}</span>
              <div className="skill-card-actions">
                {confirmDelete === file ? (
                  <>
                    <span className="settings-hint">Remove?</span>
                    <button type="button" className="provider-delete-btn" onClick={() => removeFile(file)}>
                      Remove
                    </button>
                    <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                      Keep
                    </button>
                  </>
                ) : (
                  <>
                    <button type="button" className="settings-browse-btn" onClick={() => openFile(file)}>
                      Edit
                    </button>
                    <button
                      type="button"
                      className="provider-delete-btn"
                      onClick={() => setConfirmDelete(file)}
                      aria-label={`Remove ${file}`}
                      title="Remove this file"
                    >
                      🗑
                    </button>
                  </>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      {draft ? (
        <div className="skill-file-draft">
          <input
            className="settings-input"
            type="text"
            placeholder="run.sh"
            value={draft.name}
            disabled={draft.existing}
            onChange={(e) => setDraft({ ...draft, name: e.currentTarget.value })}
          />
          <textarea
            className="settings-input settings-textarea"
            rows={8}
            placeholder="File content (text)."
            value={draft.content}
            onChange={(e) => setDraft({ ...draft, content: e.currentTarget.value })}
          />
          <div className="skill-editor-actions">
            <button
              type="button"
              className="settings-save-btn"
              onClick={saveDraft}
              disabled={busy || draft.name.trim() === ""}
            >
              {busy ? "Saving…" : "Save file"}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setDraft(null)} disabled={busy}>
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div>
          <button
            type="button"
            className="settings-browse-btn"
            onClick={() => setDraft({ name: "", content: "", existing: false })}
          >
            + Attach a file
          </button>
        </div>
      )}
    </div>
  );
}

/** Where the skills live: this computer's vault, or the hub in use. */
const localSkills = {
  list: () => invoke<SkillEntry[]>("list_skills"),
  save: (skill: SkillEntry, overwrite: boolean) => invoke<void>("save_skill", { skill, overwrite }),
  remove: (name: string) => invoke<void>("delete_skill", { name }),
  catalog: () => invoke<CatalogSkill[]>("list_skill_catalog"),
  install: (name: string, overwrite: boolean) => invoke<void>("install_catalog_skill", { name, overwrite }),
  exportPackage: (name: string, version: string, author: string | null, license: string) =>
    invoke<ExportedPackage>("export_skill_package", { name, version, author, license }),
  previewPackage: (content: string) => invoke<PackagePreview>("preview_skill_package", { content }),
  installPackage: (content: string, overwrite: boolean, allowFlagged: boolean) =>
    invoke<void>("install_skill_package", { content, overwrite, allowFlagged }),
};

/** The licenses offered when sharing a skill: what the person who made it lets others do with it. */
const LICENSES = [
  { id: "CC0-1.0", label: "CC0 (public domain, no conditions)" },
  { id: "MIT", label: "MIT (keep the copyright notice)" },
  { id: "CC-BY-4.0", label: "CC BY (credit the author)" },
];

/** The skill being shared: the file's details, asked for before it is written. */
interface ExportState {
  name: string;
  version: string;
  author: string;
  license: string;
}

/** A skill being published to Arweave through the TruthID app (P160): what the person is asked, and how far it got. */
interface PublishState extends ExportState {
  /** Ticked: the person knows it becomes public and permanent. */
  understood: boolean;
  /** The text looks unsafe (the scan's reason), once checked; installing readers will be warned too. */
  finding: string | null;
  /** Ticked: the person read the flagged text and still wants to publish it. */
  flaggedOk: boolean;
  phase: "form" | "waiting" | "done";
  qrSvg: string | null;
  txId: string | null;
}

/** A package being imported: its text, and what it says once it has been read and checked. */
interface ImportState {
  content: string;
  preview: PackagePreview | null;
  /** The person read a flagged text and wants it anyway. */
  allowFlagged: boolean;
  /** The person agrees to replace a skill of theirs that differs. */
  replace: boolean;
}

/** `remote`: the skills are the hub's (P102), without attached files or drafting, which the hub has no messages for.
 * The caller remounts the screen (`key`) when the machine changes. */
function SkillsView({
  agents,
  providers,
  activeProvider,
  remote = false,
}: {
  agents: AgentEntry[];
  providers: ProviderEntry[];
  activeProvider: string;
  remote?: boolean;
}) {
  const api = remote ? hubSkills : localSkills;
  const [skills, setSkills] = useState<SkillEntry[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [generating, setGenerating] = useState(false);
  // Which provider drafts the skill; "" = the active one (same convention as the chat's model picker).
  const [draftProvider, setDraftProvider] = useState("");
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  // The built-in gallery (P140): `null` while loading or when it could not be read (a hub older than the gallery).
  const [catalog, setCatalog] = useState<CatalogSkill[] | null>(null);
  const [confirmReplace, setConfirmReplace] = useState<string | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  // Sharing a skill as a file and importing one (P160).
  const [exporting, setExporting] = useState<ExportState | null>(null);
  const [importing, setImporting] = useState<ImportState | null>(null);
  const [publishing, setPublishing] = useState<PublishState | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  function refresh() {
    api
      .list()
      .then((list) => {
        setSkills(list);
        setLoadError(null);
      })
      .catch((err) => setLoadError(String(err)));
    api
      .catalog()
      .then(setCatalog)
      .catch(() => setCatalog(null));
  }

  async function handleExport() {
    if (!exporting) return;
    setError(null);
    setNotice(null);
    try {
      const author = exporting.author.trim();
      const packed = await api.exportPackage(exporting.name, exporting.version.trim(), author === "" ? null : author, exporting.license);
      const path = await save({
        defaultPath: `${exporting.name}.warden-skill.json`,
        filters: [{ name: "Warden skill", extensions: ["json"] }],
      });
      if (!path) return;
      await invoke("write_skill_package_file", { path, content: packed.content });
      setNotice(
        `Saved ${path}.` + (packed.attachmentsSkipped ? " The files attached to this skill are not in it: only the text is shared." : ""),
      );
      setExporting(null);
    } catch (err) {
      setError(String(err));
    }
  }

  /** Packs the skill, checks it for anything unsafe, shows the QR for the TruthID app and waits for the phone to pin it. */
  async function handlePublish() {
    if (!publishing) return;
    setError(null);
    try {
      const author = publishing.author.trim();
      const packed = await api.exportPackage(publishing.name, publishing.version.trim(), author === "" ? null : author, publishing.license);
      // The same scan a reader runs before installing: better to hear it here than for them to be warned about the skill.
      const preview = await api.previewPackage(packed.content);
      const finding = preview.finding ?? null;
      if (finding && !publishing.flaggedOk) {
        setPublishing({ ...publishing, finding });
        return;
      }
      const begin = await invoke<{ qrSvg: string; hash: string }>("skill_publish_begin", { content: packed.content });
      setPublishing({ ...publishing, finding, phase: "waiting", qrSvg: begin.qrSvg });
      const result = await invoke<{ txId: string }>("skill_publish_await");
      setPublishing((current) => (current ? { ...current, phase: "done", qrSvg: null, txId: result.txId } : current));
    } catch (err) {
      setError(String(err));
      setPublishing((current) => (current ? { ...current, phase: "form", qrSvg: null } : current));
    }
  }

  async function handleChooseImportFile() {
    setError(null);
    try {
      const path = await open({ multiple: false, filters: [{ name: "Warden skill", extensions: ["json"] }] });
      if (typeof path !== "string") return;
      const content = await invoke<string>("read_skill_package_file", { path });
      await handleCheckImport(content);
    } catch (err) {
      setError(String(err));
    }
  }

  /** Reads and checks the text; nothing is installed yet. */
  async function handleCheckImport(content: string) {
    setError(null);
    try {
      const preview = await api.previewPackage(content);
      setImporting({ content, preview, allowFlagged: false, replace: false });
    } catch (err) {
      // Stay on the paste step with the text, so it can be fixed or replaced.
      setImporting({ content, preview: null, allowFlagged: false, replace: false });
      setError(String(err));
    }
  }

  async function handleInstallImport() {
    if (!importing?.preview) return;
    setError(null);
    try {
      await api.installPackage(importing.content, importing.replace, importing.allowFlagged);
      setNotice(`Installed ${importing.preview.name}.`);
      setImporting(null);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleInstall(name: string, overwrite: boolean) {
    setError(null);
    setInstalling(name);
    try {
      await api.install(name, overwrite);
      setConfirmReplace(null);
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setInstalling(null);
    }
  }

  useEffect(refresh, []);

  function openNew() {
    setError(null);
    setEditor({ mode: "new", skill: emptySkill, fromAi: false });
  }

  function openEdit(skill: SkillEntry) {
    setError(null);
    setEditor({ mode: "edit", skill, fromAi: false, accept: skill.proposed ? true : undefined });
  }

  /** Accepts a suggestion as it is: a save without the `proposed` mark. */
  async function handleAccept(skill: SkillEntry) {
    setError(null);
    try {
      await api.save({ name: skill.name, description: skill.description, body: skill.body, agents: skill.agents }, true);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleGenerate() {
    setError(null);
    setGenerating(true);
    try {
      const providerId = draftProvider === "" || draftProvider === activeProvider ? null : draftProvider;
      const draft = await invoke<SkillEntry>("generate_skill_draft", { prompt, providerId });
      setEditor({ mode: "new", skill: draft, fromAi: true });
    } catch (err) {
      setError(String(err));
    } finally {
      setGenerating(false);
    }
  }

  async function handleSave() {
    if (!editor) return;
    setError(null);
    setSaving(true);
    try {
      // A suggestion (P104) stays one unless "Accept" is ticked; accepted, the marks that made it one go.
      const accepting = editor.skill.proposed && editor.accept;
      const { proposed, source, proposedAt, ...rest } = editor.skill;
      const skill = proposed && !accepting ? editor.skill : rest;
      await api.save(skill, editor.mode === "edit");
      setEditor(null);
      setPrompt("");
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(name: string) {
    setError(null);
    try {
      await api.remove(name);
      setConfirmDelete(null);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  /** An id already on the skill but no longer in the registry (agent deleted/renamed) stays
   * listed, so saving doesn't silently drop it — the user can untick it deliberately. */
  function toggleAgent(id: string, checked: boolean) {
    if (!editor) return;
    const current = editor.skill.agents;
    updateEditor({ agents: checked ? [...current.filter((a) => a !== id), id] : current.filter((a) => a !== id) });
  }

  function updateEditor(patch: Partial<SkillEntry>) {
    setEditor((current) => (current ? { ...current, skill: { ...current.skill, ...patch } } : current));
  }

  const suggested = (skills ?? []).filter((s) => s.proposed);
  const active = (skills ?? []).filter((s) => !s.proposed);

  return (
    <div className="settings-view">
      <h2 className="settings-title">Skills</h2>
      <p className="settings-hint">
        Skills are reusable instructions the AI loads on its own when a request matches one — they cost nothing in a
        conversation until used. Create one here, describe one and let the AI draft it, or just ask for one in a chat.
        They're stored as plain markdown in <code>skills/</code> inside your vault, so they sync with it.
      </p>

      {loadError && <p className="settings-error-banner">{loadError}</p>}
      {error && <p className="settings-error-banner">{error}</p>}

      {!remote && (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">Describe a skill</h3>
          </div>
          <label className="settings-field">
            <span className="settings-hint">
              Say roughly what it should do — the AI writes a draft you can review and edit before saving.
            </span>
            <textarea
              className="settings-input settings-textarea"
              rows={3}
              placeholder="e.g. 'How to review a pull request: check tests first, then naming, then security, and answer with a short verdict plus a bullet list of issues.'"
              value={prompt}
              onChange={(e) => setPrompt(e.currentTarget.value)}
            />
          </label>
          {providers.length > 1 && (
            <label className="settings-field">
              <span className="settings-label">Model</span>
              <select
                className="settings-select"
                value={draftProvider}
                onChange={(e) => setDraftProvider(e.currentTarget.value)}
              >
                <option value="">Active provider{activeProvider ? ` (${activeProvider})` : ""}</option>
                {providers
                  .filter((p) => p.id !== activeProvider)
                  .map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.id}
                    </option>
                  ))}
              </select>
            </label>
          )}
          <button
            type="button"
            className="settings-save-btn"
            onClick={handleGenerate}
            disabled={generating || prompt.trim() === ""}
          >
            {generating ? "Drafting…" : "Generate draft"}
          </button>
        </section>
      )}

      {editor && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">{editor.mode === "new" ? "New skill" : `Edit ${editor.skill.name}`}</span>
          {editor.fromAi && (
            <p className="settings-hint">AI draft — read it through and adjust anything before saving.</p>
          )}

          <label className="settings-field">
            <span className="settings-label">Name</span>
            <input
              className="settings-input"
              type="text"
              placeholder="review-pr"
              value={editor.skill.name}
              disabled={editor.mode === "edit"}
              onChange={(e) => updateEditor({ name: e.currentTarget.value })}
            />
            <span className="settings-hint">Lowercase letters, digits and hyphens. It can't be changed after saving.</span>
          </label>

          <label className="settings-field">
            <span className="settings-label">Description</span>
            <input
              className="settings-input"
              type="text"
              placeholder="One sentence: what it does and when to use it"
              value={editor.skill.description}
              onChange={(e) => updateEditor({ description: e.currentTarget.value })}
            />
          </label>

          <label className="settings-field">
            <span className="settings-label">Instructions</span>
            <textarea
              className="settings-input settings-textarea"
              rows={10}
              placeholder="The full instructions the AI should follow, in markdown."
              value={editor.skill.body}
              onChange={(e) => updateEditor({ body: e.currentTarget.value })}
            />
          </label>

          <div className="settings-field">
            <span className="settings-label">Available to</span>
            {(() => {
              const known = new Set(agents.map((a) => a.id));
              const ids = [...agents.map((a) => a.id), ...editor.skill.agents.filter((id) => !known.has(id))];
              return ids.length === 0 ? (
                <span className="settings-hint">No agents configured — every skill is visible to every conversation.</span>
              ) : (
                <div className="skill-agent-list">
                  {ids.map((id) => (
                    <label className="skill-agent-option" key={id}>
                      <input
                        type="checkbox"
                        checked={editor.skill.agents.includes(id)}
                        onChange={(e) => toggleAgent(id, e.currentTarget.checked)}
                      />
                      {id}
                      {!known.has(id) && <span className="settings-hint"> (agent no longer exists)</span>}
                    </label>
                  ))}
                </div>
              );
            })()}
            <span className="settings-hint">
              None ticked means every agent sees it. Ticked means only those agents — chats without an agent won't
              see it.
            </span>
          </div>

          {editor.skill.proposed && (
            <label className="skill-agent-option">
              <input
                type="checkbox"
                checked={editor.accept ?? false}
                onChange={(e) => setEditor({ ...editor, accept: e.currentTarget.checked })}
              />
              Accept this skill — it starts applying in conversations
            </label>
          )}

          {remote ? null : editor.mode === "edit" ? (
            <SkillFiles skillName={editor.skill.name} />
          ) : (
            <span className="settings-hint">Save the skill first, then you can attach files to it.</span>
          )}

          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={handleSave} disabled={saving}>
              {saving ? "Saving…" : "Save skill"}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setEditor(null)} disabled={saving}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {notice && <p className="settings-hint">{notice}</p>}

      {exporting && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">Share {exporting.name}</span>
          <p className="settings-hint">
            This saves the skill's text as a file anyone can import. Files attached to the skill are not included. Once you give
            the file to someone, you can't take it back.
          </p>
          <label className="settings-field">
            <span className="settings-label">Version</span>
            <input
              className="settings-input"
              type="text"
              value={exporting.version}
              onChange={(e) => setExporting({ ...exporting, version: e.currentTarget.value })}
            />
            <span className="settings-hint">Numbers separated by dots, like 1.0.0. Raise it when you change the skill.</span>
          </label>
          <label className="settings-field">
            <span className="settings-label">Author (optional)</span>
            <input
              className="settings-input"
              type="text"
              placeholder="Your name or handle"
              value={exporting.author}
              onChange={(e) => setExporting({ ...exporting, author: e.currentTarget.value })}
            />
          </label>
          <label className="settings-field">
            <span className="settings-label">License</span>
            <select className="settings-select" value={exporting.license} onChange={(e) => setExporting({ ...exporting, license: e.currentTarget.value })}>
              {LICENSES.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.label}
                </option>
              ))}
            </select>
          </label>
          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={handleExport}>
              Save as file…
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setExporting(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {publishing && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">Publish {publishing.name}</span>
          {publishing.phase === "form" && (
            <>
              <p className="settings-hint">
                This puts the skill's text on Arweave through your TruthID app, so anyone can find and read it.{" "}
                <strong>It is public and permanent: it can't be taken back or edited</strong> (a new version is a new publication), and the
                fee is paid from your TruthID wallet. Keep your phone on the same network as this computer.
              </p>
              <label className="settings-field">
                <span className="settings-label">Version</span>
                <input
                  className="settings-input"
                  type="text"
                  value={publishing.version}
                  onChange={(e) => setPublishing({ ...publishing, version: e.currentTarget.value })}
                />
                <span className="settings-hint">Numbers separated by dots, like 1.0.0.</span>
              </label>
              <label className="settings-field">
                <span className="settings-label">Author (optional)</span>
                <input
                  className="settings-input"
                  type="text"
                  placeholder="Your name or handle"
                  value={publishing.author}
                  onChange={(e) => setPublishing({ ...publishing, author: e.currentTarget.value })}
                />
              </label>
              <label className="settings-field">
                <span className="settings-label">License</span>
                <select className="settings-select" value={publishing.license} onChange={(e) => setPublishing({ ...publishing, license: e.currentTarget.value })}>
                  {LICENSES.map((l) => (
                    <option key={l.id} value={l.id}>
                      {l.label}
                    </option>
                  ))}
                </select>
              </label>
              {publishing.finding && (
                <p className="settings-error-banner">
                  This text contains {publishing.finding}. People who import it are warned about that. Check that it is what you mean to publish.
                </p>
              )}
              {publishing.finding && (
                <label className="skill-agent-option">
                  <input type="checkbox" checked={publishing.flaggedOk} onChange={(e) => setPublishing({ ...publishing, flaggedOk: e.currentTarget.checked })} />
                  I checked it and want to publish it anyway
                </label>
              )}
              <label className="skill-agent-option">
                <input type="checkbox" checked={publishing.understood} onChange={(e) => setPublishing({ ...publishing, understood: e.currentTarget.checked })} />
                I understand it will be public and permanent
              </label>
              <div className="skill-editor-actions">
                <button
                  type="button"
                  className="settings-save-btn"
                  onClick={handlePublish}
                  disabled={!publishing.understood || (publishing.finding !== null && !publishing.flaggedOk)}
                >
                  Show the QR code
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setPublishing(null)}>
                  Cancel
                </button>
              </div>
            </>
          )}
          {publishing.phase === "waiting" && (
            <>
              <p className="settings-hint">Scan this code with the TruthID app and approve the publication. This waits up to 3 minutes.</p>
              {publishing.qrSvg && <div className="sync-qr-image" dangerouslySetInnerHTML={{ __html: publishing.qrSvg }} />}
              <p className="settings-hint">Stay on this screen: the address of the publication appears here when the phone is done.</p>
            </>
          )}
          {publishing.phase === "done" && publishing.txId && (
            <>
              <p className="settings-hint">
                Published. Its address is <code>ar://{publishing.txId}</code>. It can take a few minutes to show up on the gateways. Keep
                this address: nothing in Warden lists what you published.
              </p>
              <div className="skill-editor-actions">
                <CopyButton text={`ar://${publishing.txId}`} className="settings-browse-btn" label="Copy the address" />
                <button type="button" className="settings-browse-btn" onClick={() => setPublishing(null)}>
                  Close
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {importing && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">Import a skill</span>
          {importing.preview === null ? (
            <>
              <p className="settings-hint">Choose a skill file, or paste its text. Nothing is installed until you have read it.</p>
              <textarea
                className="settings-input settings-textarea"
                rows={6}
                placeholder="Paste the contents of a .warden-skill.json file"
                value={importing.content}
                onChange={(e) => setImporting({ ...importing, content: e.currentTarget.value })}
              />
              <div className="skill-editor-actions">
                <button type="button" className="settings-browse-btn" onClick={handleChooseImportFile}>
                  Choose a file…
                </button>
                <button type="button" className="settings-save-btn" onClick={() => handleCheckImport(importing.content)} disabled={importing.content.trim() === ""}>
                  Read it
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setImporting(null)}>
                  Cancel
                </button>
              </div>
            </>
          ) : (
            <>
              <p className="skill-card-name">{importing.preview.name}</p>
              <p className="skill-card-description">{importing.preview.description}</p>
              <p className="settings-hint">
                Version {importing.preview.version} · License {importing.preview.license}
                {importing.preview.author ? ` · By ${importing.preview.author}` : " · No author given"} ·{" "}
                <code>{importing.preview.hash.slice(0, 19)}…</code>
              </p>
              <p className="settings-hint">
                The code above only shows the file was not changed on the way. It does not prove who wrote it: read the text before
                installing.
              </p>
              {importing.preview.finding && (
                <p className="settings-error-banner">
                  This text contains {importing.preview.finding}. A skill is instructions the AI follows, so check that it is what you
                  expect.
                </p>
              )}
              <pre className="skill-suggestion-body">{importing.preview.body}</pre>
              {importing.preview.status === "installed" && <p className="settings-hint">You already have this exact skill.</p>}
              {importing.preview.status === "modified" && (
                <label className="skill-agent-option">
                  <input type="checkbox" checked={importing.replace} onChange={(e) => setImporting({ ...importing, replace: e.currentTarget.checked })} />
                  Replace the skill I already have with this name
                </label>
              )}
              {importing.preview.finding && (
                <label className="skill-agent-option">
                  <input
                    type="checkbox"
                    checked={importing.allowFlagged}
                    onChange={(e) => setImporting({ ...importing, allowFlagged: e.currentTarget.checked })}
                  />
                  I read it and want to install it anyway
                </label>
              )}
              <div className="skill-editor-actions">
                <button
                  type="button"
                  className="settings-save-btn"
                  onClick={handleInstallImport}
                  disabled={
                    importing.preview.status === "installed" ||
                    (importing.preview.status === "modified" && !importing.replace) ||
                    (!!importing.preview.finding && !importing.allowFlagged)
                  }
                >
                  Install
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setImporting({ ...importing, preview: null })}>
                  Back
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setImporting(null)}>
                  Cancel
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {suggested.length > 0 && (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">Suggested by the AI</h3>
          </div>
          <p className="settings-hint">
            After some conversations the assistant suggested these. <strong>None of them applies until you accept it</strong>:
            read it, edit it if needed, then accept or reject.
          </p>
          <div className="provider-list">
            {suggested.map((skill) => (
              <div className="provider-card skill-card" key={skill.name}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{skill.name}</span>
                  <span className="settings-hint">{skill.revises ? `suggested change to ${skill.revises} — not applied` : "suggested — not active"}</span>
                  <div className="skill-card-actions">
                    {confirmDelete === skill.name ? (
                      <>
                        <span className="settings-hint">Reject this suggestion?</span>
                        <button type="button" className="provider-delete-btn" onClick={() => handleDelete(skill.name)}>
                          Reject
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                          Keep
                        </button>
                      </>
                    ) : (
                      <>
                        <button type="button" className="settings-save-btn" onClick={() => handleAccept(skill)}>
                          Accept
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => openEdit(skill)}>
                          Edit
                        </button>
                        <button type="button" className="provider-delete-btn" onClick={() => setConfirmDelete(skill.name)}>
                          Reject
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">{skill.description || "(no description)"}</p>
                <p className="settings-hint">
                  {skill.source ? `From conversation ${skill.source}` : "From a conversation"}
                  {skill.proposedAt ? ` · ${new Date(skill.proposedAt).toLocaleString()}` : ""}
                  {skill.agents.length > 0 ? ` · only for: ${skill.agents.join(", ")}` : ""}
                </p>
                {(() => {
                  const target = skill.revises ? active.find((s) => s.name === skill.revises) : undefined;
                  if (!target) {
                    return (
                      <details>
                        <summary className="settings-hint">View the instructions</summary>
                        <pre className="skill-suggestion-body">{skill.body}</pre>
                      </details>
                    );
                  }
                  const diff = diffLines(target.body, skill.body);
                  return (
                    <details>
                      <summary className="settings-hint">View what changes in {target.name}</summary>
                      {hasChanges(diff) ? (
                        <pre className="skill-suggestion-body skill-diff">
                          {withContext(diff).map((line, i) => (
                            <span key={i} className={`skill-diff-line skill-diff-line--${line.op}`}>
                              {line.op === "add" ? "+ " : line.op === "del" ? "- " : "  "}
                              {line.text}
                              {"\n"}
                            </span>
                          ))}
                        </pre>
                      ) : (
                        <p className="settings-hint">No change in the instruction text.</p>
                      )}
                    </details>
                  );
                })()}
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="settings-section">
        <div className="settings-section-header">
          <h3 className="settings-section-title">Your skills</h3>
          <div className="skill-card-actions">
            <button
              type="button"
              className="settings-browse-btn"
              onClick={() => {
                setError(null);
                setNotice(null);
                setImporting({ content: "", preview: null, allowFlagged: false, replace: false });
              }}
            >
              Import
            </button>
            <button type="button" className="settings-browse-btn" onClick={openNew}>
              + New skill
            </button>
          </div>
        </div>

        {skills === null ? (
          <p className="settings-hint">Loading skills…</p>
        ) : active.length === 0 ? (
          <p className="settings-hint">{suggested.length > 0 ? "No active skills yet." : "No skills yet."}</p>
        ) : (
          <div className="provider-list">
            {active.map((skill) => (
              <div className="provider-card skill-card" key={skill.name}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{skill.name}</span>
                  <div className="skill-card-actions">
                    {confirmDelete === skill.name ? (
                      <>
                        <span className="settings-hint">Delete this skill?</span>
                        <button type="button" className="provider-delete-btn" onClick={() => handleDelete(skill.name)}>
                          Delete
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                          Keep
                        </button>
                      </>
                    ) : (
                      <>
                        <button type="button" className="settings-browse-btn" onClick={() => openEdit(skill)}>
                          Edit
                        </button>
                        <button
                          type="button"
                          className="settings-browse-btn"
                          onClick={() => {
                            setError(null);
                            setNotice(null);
                            setExporting({ name: skill.name, version: "1.0.0", author: "", license: LICENSES[0].id });
                          }}
                        >
                          Share
                        </button>
                        <button
                          type="button"
                          className="settings-browse-btn"
                          onClick={() => {
                            setError(null);
                            setNotice(null);
                            setPublishing({
                              name: skill.name,
                              version: "1.0.0",
                              author: "",
                              license: LICENSES[0].id,
                              understood: false,
                              finding: null,
                              flaggedOk: false,
                              phase: "form",
                              qrSvg: null,
                              txId: null,
                            });
                          }}
                        >
                          Publish
                        </button>
                        <button
                          type="button"
                          className="provider-delete-btn"
                          onClick={() => setConfirmDelete(skill.name)}
                          aria-label={`Delete ${skill.name}`}
                          title="Delete this skill"
                        >
                          🗑
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">{skill.description || "(no description)"}</p>
                {skill.agents.length > 0 && <p className="settings-hint">Only for: {skill.agents.join(", ")}</p>}
              </div>
            ))}
          </div>
        )}
      </section>

      {catalog !== null && catalog.length > 0 && (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">Gallery</h3>
          </div>
          <p className="settings-hint">
            Ready-made skills that come with Warden. Installing one copies it into <code>skills/</code> in your vault, where
            you can edit it like any other skill. Nothing is downloaded.
          </p>
          <div className="provider-list">
            {catalog.map((entry) => (
              <div className="provider-card skill-card" key={entry.name}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{entry.name}</span>
                  <div className="skill-card-actions">
                    {entry.status === "installed" ? (
                      <span className="settings-hint">Installed</span>
                    ) : entry.status === "modified" && confirmReplace === entry.name ? (
                      <>
                        <span className="settings-hint">Replace your version of this skill?</span>
                        <button
                          type="button"
                          className="provider-delete-btn"
                          onClick={() => handleInstall(entry.name, true)}
                          disabled={installing === entry.name}
                        >
                          Replace
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmReplace(null)}>
                          Keep mine
                        </button>
                      </>
                    ) : entry.status === "modified" ? (
                      <>
                        <span className="settings-hint">You have a different skill with this name</span>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmReplace(entry.name)}>
                          Use the gallery version
                        </button>
                      </>
                    ) : (
                      <button
                        type="button"
                        className="settings-save-btn"
                        onClick={() => handleInstall(entry.name, false)}
                        disabled={installing === entry.name}
                      >
                        {installing === entry.name ? "Installing…" : "Install"}
                      </button>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">{entry.description}</p>
                <details>
                  <summary className="settings-hint">View the instructions</summary>
                  <pre className="skill-suggestion-body">{entry.body}</pre>
                </details>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

export default SkillsView;

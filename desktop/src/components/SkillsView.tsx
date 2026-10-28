import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { intlTag, t } from "../i18n";
import { catalogDescription } from "../i18n/catalog";
import { rich } from "../i18n/rich";
import type { AgentEntry, CatalogSkill, ExportedPackage, PackagePreview, ProviderEntry, SkillEntry } from "../types";
import { diffLines, hasChanges, withContext } from "./skillDiff";
import CopyButton from "./CopyButton";
import Curators from "./Curators";
import TrustedPublishers from "./TrustedPublishers";
import TrustThisKey from "./TrustThisKey";
import SigningChoice, { type SigningState } from "./SigningChoice";
import { packageChange } from "../lib/curators";
import { rememberedKeyPath } from "../lib/signingKey";
import { hubSkills } from "../lib/hub";
import { signatureView } from "../lib/skillSignature";

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
      <span className="settings-label">{t("skillFiles.title")}</span>
      <span className="settings-hint">{t("skillFiles.hint")}</span>
      {error && <p className="settings-error-banner">{error}</p>}
      {files === null ? null : files.length === 0 ? (
        <span className="settings-hint">{t("skillFiles.none")}</span>
      ) : (
        <div className="skill-file-list">
          {files.map((file) => (
            <div className="skill-file-row" key={file}>
              <span className="skill-card-name">{file}</span>
              <div className="skill-card-actions">
                {confirmDelete === file ? (
                  <>
                    <span className="settings-hint">{t("skillFiles.confirm")}</span>
                    <button type="button" className="provider-delete-btn" onClick={() => removeFile(file)}>
                      {t("common.remove")}
                    </button>
                    <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                      {t("common.keep")}
                    </button>
                  </>
                ) : (
                  <>
                    <button type="button" className="settings-browse-btn" onClick={() => openFile(file)}>
                      {t("common.edit")}
                    </button>
                    <button
                      type="button"
                      className="provider-delete-btn"
                      onClick={() => setConfirmDelete(file)}
                      aria-label={t("skillFiles.removeLabel", { file })}
                      title={t("skillFiles.removeTitle")}
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
            placeholder={t("skillFiles.contentPlaceholder")}
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
              {busy ? t("common.saving") : t("skillFiles.save")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setDraft(null)} disabled={busy}>
              {t("common.cancel")}
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
            {t("skillFiles.attach")}
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
  fetchPackage: (address: string) => invoke<string>("fetch_skill_package", { address }),
  previewPackage: (content: string) => invoke<PackagePreview>("preview_skill_package", { content }),
  installPackage: (content: string, overwrite: boolean, allowFlagged: boolean) =>
    invoke<void>("install_skill_package", { content, overwrite, allowFlagged }),
};

/** The licenses offered when sharing a skill: what the person who made it lets others do with it. */
const licenses = () => [
  { id: "CC0-1.0", label: t("skills.licenseCc0") },
  { id: "MIT", label: t("skills.licenseMit") },
  { id: "CC-BY-4.0", label: t("skills.licenseCcBy") },
];

/** The skill being shared: the file's details, asked for before it is written. */
interface ExportState extends SigningState {
  name: string;
  version: string;
  author: string;
  license: string;
}

/** Signing starts on when a key file was chosen on an earlier share, so the same key is used again without asking. */
function startSigning(): SigningState {
  const keyPath = rememberedKeyPath();
  return { sign: keyPath !== null, keyPath };
}

/** `content` signed with the key file at `keyPath` when the person asked for it, else as it is. The key is read on this computer only. */
async function signedIfAsked(content: string, { sign, keyPath }: SigningState): Promise<string> {
  if (!sign) return content;
  if (!keyPath) throw t("skills.signNeedsKey");
  try {
    return (await invoke<{ content: string }>("sign_skill_package", { content, keyPath })).content;
  } catch (err) {
    throw t("skills.signFailed", { error: String(err) });
  }
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
  /** An Arweave address typed to fetch a published package instead of choosing a file. */
  address: string;
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
  // Goes up when a key is trusted from the preview, so the "Who signs" list below reads itself again.
  const [trustedVersion, setTrustedVersion] = useState(0);
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
      const content = await signedIfAsked(packed.content, exporting);
      const path = await save({
        defaultPath: `${exporting.name}.warden-skill.json`,
        filters: [{ name: t("skills.fileFilter"), extensions: ["json"] }],
      });
      if (!path) return;
      await invoke("write_skill_package_file", { path, content });
      setNotice(t("skills.saved", { path }) + (packed.attachmentsSkipped ? t("skills.attachmentsSkipped") : ""));
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
      const content = await signedIfAsked(packed.content, publishing);
      // The same scan a reader runs before installing: better to hear it here than for them to be warned about the skill.
      const preview = await api.previewPackage(content);
      const finding = preview.finding ?? null;
      if (finding && !publishing.flaggedOk) {
        setPublishing({ ...publishing, finding });
        return;
      }
      const begin = await invoke<{ qrSvg: string; hash: string }>("skill_publish_begin", { content });
      setPublishing({ ...publishing, finding, phase: "waiting", qrSvg: begin.qrSvg });
      const result = await invoke<{ txId: string }>("skill_publish_await");
      setPublishing((current) => (current ? { ...current, phase: "done", qrSvg: null, txId: result.txId } : current));
    } catch (err) {
      setError(String(err));
      setPublishing((current) => (current ? { ...current, phase: "form", qrSvg: null } : current));
    }
  }

  /** Downloads the package published at the address in the box, then reads it like a file. */
  async function handleFetchImport() {
    if (!importing) return;
    setError(null);
    try {
      const content = await api.fetchPackage(importing.address.trim());
      await handleCheckImport(content);
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleChooseImportFile() {
    setError(null);
    try {
      const path = await open({ multiple: false, filters: [{ name: t("skills.fileFilter"), extensions: ["json"] }] });
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
      setImporting({ content, preview, allowFlagged: false, replace: false, address: "" });
    } catch (err) {
      // Stay on the paste step with the text, so it can be fixed or replaced.
      setImporting({ content, preview: null, allowFlagged: false, replace: false, address: "" });
      setError(String(err));
    }
  }

  /** The signing key was just added to the trusted list: reads the same text again so the line under the preview says so, keeping the
   * boxes the person already ticked, and has the "Who signs" list read itself again. */
  async function handleTrusted() {
    if (!importing) return;
    setTrustedVersion((n) => n + 1);
    try {
      const preview = await api.previewPackage(importing.content);
      setImporting((current) => (current ? { ...current, preview } : current));
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleInstallImport() {
    if (!importing?.preview) return;
    setError(null);
    try {
      await api.installPackage(importing.content, importing.replace, importing.allowFlagged);
      setNotice(t("skills.installed", { name: importing.preview.name }));
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
      <h2 className="settings-title">{t("skills.title")}</h2>
      <p className="settings-hint">{rich("skills.hint", { folder: <code>skills/</code> })}</p>

      {loadError && <p className="settings-error-banner">{loadError}</p>}
      {error && <p className="settings-error-banner">{error}</p>}

      {!remote && (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("skills.describe")}</h3>
          </div>
          <label className="settings-field">
            <span className="settings-hint">{t("skills.describeHint")}</span>
            <textarea
              className="settings-input settings-textarea"
              rows={3}
              placeholder={t("skills.describePlaceholder")}
              value={prompt}
              onChange={(e) => setPrompt(e.currentTarget.value)}
            />
          </label>
          {providers.length > 1 && (
            <label className="settings-field">
              <span className="settings-label">{t("skills.model")}</span>
              <select
                className="settings-select"
                value={draftProvider}
                onChange={(e) => setDraftProvider(e.currentTarget.value)}
              >
                <option value="">{activeProvider ? t("skills.activeProviderNamed", { name: activeProvider }) : t("skills.activeProvider")}</option>
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
            {generating ? t("skills.drafting") : t("skills.generate")}
          </button>
        </section>
      )}

      {editor && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">{editor.mode === "new" ? t("skills.newSkill") : t("skills.editSkill", { name: editor.skill.name })}</span>
          {editor.fromAi && <p className="settings-hint">{t("skills.aiDraft")}</p>}

          <label className="settings-field">
            <span className="settings-label">{t("skills.name")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder="review-pr"
              value={editor.skill.name}
              disabled={editor.mode === "edit"}
              onChange={(e) => updateEditor({ name: e.currentTarget.value })}
            />
            <span className="settings-hint">{t("skills.nameHint")}</span>
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("skills.description")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder={t("skills.descriptionPlaceholder")}
              value={editor.skill.description}
              onChange={(e) => updateEditor({ description: e.currentTarget.value })}
            />
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("skills.instructions")}</span>
            <textarea
              className="settings-input settings-textarea"
              rows={10}
              placeholder={t("skills.instructionsPlaceholder")}
              value={editor.skill.body}
              onChange={(e) => updateEditor({ body: e.currentTarget.value })}
            />
          </label>

          <div className="settings-field">
            <span className="settings-label">{t("skills.availableTo")}</span>
            {(() => {
              const known = new Set(agents.map((a) => a.id));
              const ids = [...agents.map((a) => a.id), ...editor.skill.agents.filter((id) => !known.has(id))];
              return ids.length === 0 ? (
                <span className="settings-hint">{t("skills.noAgents")}</span>
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
                      {!known.has(id) && <span className="settings-hint">{t("skills.agentGone")}</span>}
                    </label>
                  ))}
                </div>
              );
            })()}
            <span className="settings-hint">{t("skills.availableHint")}</span>
          </div>

          {editor.skill.proposed && (
            <label className="skill-agent-option">
              <input
                type="checkbox"
                checked={editor.accept ?? false}
                onChange={(e) => setEditor({ ...editor, accept: e.currentTarget.checked })}
              />
              {t("skills.accept")}
            </label>
          )}

          {remote ? null : editor.mode === "edit" ? (
            <SkillFiles skillName={editor.skill.name} />
          ) : (
            <span className="settings-hint">{t("skills.saveFirst")}</span>
          )}

          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={handleSave} disabled={saving}>
              {saving ? t("common.saving") : t("skills.save")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setEditor(null)} disabled={saving}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}

      {notice && <p className="settings-hint">{notice}</p>}

      {exporting && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">{t("skills.share", { name: exporting.name })}</span>
          <p className="settings-hint">{t("skills.shareHint")}</p>
          <label className="settings-field">
            <span className="settings-label">{t("skills.version")}</span>
            <input
              className="settings-input"
              type="text"
              value={exporting.version}
              onChange={(e) => setExporting({ ...exporting, version: e.currentTarget.value })}
            />
            <span className="settings-hint">{t("skills.versionHint")}</span>
          </label>
          <label className="settings-field">
            <span className="settings-label">{t("skills.author")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder={t("skills.authorPlaceholder")}
              value={exporting.author}
              onChange={(e) => setExporting({ ...exporting, author: e.currentTarget.value })}
            />
          </label>
          <label className="settings-field">
            <span className="settings-label">{t("skills.license")}</span>
            <select className="settings-select" value={exporting.license} onChange={(e) => setExporting({ ...exporting, license: e.currentTarget.value })}>
              {licenses().map((l) => (
                <option key={l.id} value={l.id}>
                  {l.label}
                </option>
              ))}
            </select>
          </label>
          <SigningChoice value={exporting} onChange={(next) => setExporting({ ...exporting, ...next })} />
          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={handleExport}>
              {t("skills.saveAsFile")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setExporting(null)}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}

      {publishing && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">{t("skills.publish", { name: publishing.name })}</span>
          {publishing.phase === "form" && (
            <>
              <p className="settings-hint">{rich("skills.publishHint", { permanent: <strong>{t("skills.publishPermanent")}</strong> })}</p>
              <label className="settings-field">
                <span className="settings-label">{t("skills.version")}</span>
                <input
                  className="settings-input"
                  type="text"
                  value={publishing.version}
                  onChange={(e) => setPublishing({ ...publishing, version: e.currentTarget.value })}
                />
                <span className="settings-hint">{t("skills.versionHintShort")}</span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t("skills.author")}</span>
                <input
                  className="settings-input"
                  type="text"
                  placeholder={t("skills.authorPlaceholder")}
                  value={publishing.author}
                  onChange={(e) => setPublishing({ ...publishing, author: e.currentTarget.value })}
                />
              </label>
              <label className="settings-field">
                <span className="settings-label">{t("skills.license")}</span>
                <select className="settings-select" value={publishing.license} onChange={(e) => setPublishing({ ...publishing, license: e.currentTarget.value })}>
                  {licenses().map((l) => (
                    <option key={l.id} value={l.id}>
                      {l.label}
                    </option>
                  ))}
                </select>
              </label>
              <SigningChoice value={publishing} onChange={(next) => setPublishing({ ...publishing, ...next })} />
              {publishing.finding && <p className="settings-error-banner">{t("skills.finding", { finding: publishing.finding })}</p>}
              {publishing.finding && (
                <label className="skill-agent-option">
                  <input type="checkbox" checked={publishing.flaggedOk} onChange={(e) => setPublishing({ ...publishing, flaggedOk: e.currentTarget.checked })} />
                  {t("skills.flaggedOk")}
                </label>
              )}
              <label className="skill-agent-option">
                <input type="checkbox" checked={publishing.understood} onChange={(e) => setPublishing({ ...publishing, understood: e.currentTarget.checked })} />
                {t("skills.understood")}
              </label>
              <div className="skill-editor-actions">
                <button
                  type="button"
                  className="settings-save-btn"
                  onClick={handlePublish}
                  disabled={!publishing.understood || (publishing.finding !== null && !publishing.flaggedOk)}
                >
                  {t("skills.showQr")}
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setPublishing(null)}>
                  {t("common.cancel")}
                </button>
              </div>
            </>
          )}
          {publishing.phase === "waiting" && (
            <>
              <p className="settings-hint">{t("skills.scanPublish")}</p>
              {publishing.qrSvg && <div className="sync-qr-image" dangerouslySetInnerHTML={{ __html: publishing.qrSvg }} />}
              <p className="settings-hint">{t("skills.stayHere")}</p>
            </>
          )}
          {publishing.phase === "done" && publishing.txId && (
            <>
              <p className="settings-hint">{rich("skills.published", { address: <code>ar://{publishing.txId}</code> })}</p>
              <div className="skill-editor-actions">
                <CopyButton text={`ar://${publishing.txId}`} className="settings-browse-btn" label={t("skills.copyAddress")} />
                <button type="button" className="settings-browse-btn" onClick={() => setPublishing(null)}>
                  {t("common.close")}
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {importing && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">{t("skills.import")}</span>
          {importing.preview === null ? (
            <>
              <p className="settings-hint">{t("skills.importHint")}</p>
              <label className="settings-field">
                <span className="settings-label">{t("skills.orAddress")}</span>
                <input
                  className="settings-input"
                  type="text"
                  placeholder="ar://…"
                  value={importing.address}
                  onChange={(e) => setImporting({ ...importing, address: e.currentTarget.value })}
                />
              </label>
              <button type="button" className="settings-browse-btn" onClick={handleFetchImport} disabled={importing.address.trim() === ""}>
                {t("skills.fetch")}
              </button>
              <textarea
                className="settings-input settings-textarea"
                rows={6}
                placeholder={t("skills.pastePlaceholder")}
                value={importing.content}
                onChange={(e) => setImporting({ ...importing, content: e.currentTarget.value })}
              />
              <div className="skill-editor-actions">
                <button type="button" className="settings-browse-btn" onClick={handleChooseImportFile}>
                  {t("skills.chooseFile")}
                </button>
                <button type="button" className="settings-save-btn" onClick={() => handleCheckImport(importing.content)} disabled={importing.content.trim() === ""}>
                  {t("skills.readIt")}
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setImporting(null)}>
                  {t("common.cancel")}
                </button>
              </div>
            </>
          ) : (
            <>
              <p className="skill-card-name">{importing.preview.name}</p>
              <p className="skill-card-description">{importing.preview.description}</p>
              <p className="settings-hint">
                {rich("skills.meta", {
                  version: importing.preview.version,
                  license: importing.preview.license,
                  author: importing.preview.author ? t("skills.by", { author: importing.preview.author }) : t("skills.noAuthor"),
                  hash: <code>{importing.preview.hash.slice(0, 19)}…</code>,
                })}
              </p>
              <p className="settings-hint">{t("skills.hashNote")}</p>
              {(() => {
                const sig = signatureView(importing.preview);
                if (sig.kind === "trusted") {
                  return <p className="settings-hint">{sig.name ? t("skills.signedTrusted", { name: sig.name }) : t("skills.signedTrustedNoName", { key: sig.key ?? "" })}</p>;
                }
                if (sig.kind === "unknown") {
                  return (
                    <>
                      <p className="settings-error-banner">{t("skills.signedUnknown", { key: sig.key ?? "" })}</p>
                      {sig.fullKey && <TrustThisKey fullKey={sig.fullKey} remote={remote} onTrusted={() => void handleTrusted()} />}
                    </>
                  );
                }
                return <p className="settings-hint">{t("skills.signedNone")}</p>;
              })()}
              {importing.preview.finding && <p className="settings-error-banner">{t("skills.importFinding", { finding: importing.preview.finding })}</p>}
              {(() => {
                const change = importing.preview.status === "modified" ? packageChange(active.find((s) => s.name === importing.preview?.name), importing.preview) : null;
                if (!change) return null;
                return (
                  <details open>
                    <summary className="settings-hint">{t("skills.previewChange")}</summary>
                    {change.description && <p className="settings-hint">{t("skills.previewDescription", change.description)}</p>}
                    <pre className="skill-suggestion-body skill-diff">
                      {change.body.map((line, i) => (
                        <span key={i} className={`skill-diff-line skill-diff-line--${line.op}`}>
                          {line.op === "add" ? "+ " : line.op === "del" ? "- " : "  "}
                          {line.text}
                          {"\n"}
                        </span>
                      ))}
                    </pre>
                  </details>
                );
              })()}
              <pre className="skill-suggestion-body">{importing.preview.body}</pre>
              {importing.preview.status === "installed" && <p className="settings-hint">{t("skills.alreadyHave")}</p>}
              {importing.preview.status === "modified" && (
                <label className="skill-agent-option">
                  <input type="checkbox" checked={importing.replace} onChange={(e) => setImporting({ ...importing, replace: e.currentTarget.checked })} />
                  {t("skills.replace")}
                </label>
              )}
              {importing.preview.finding && (
                <label className="skill-agent-option">
                  <input
                    type="checkbox"
                    checked={importing.allowFlagged}
                    onChange={(e) => setImporting({ ...importing, allowFlagged: e.currentTarget.checked })}
                  />
                  {t("skills.installAnyway")}
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
                  {t("skills.installButton")}
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setImporting({ ...importing, preview: null })}>
                  {t("skills.back")}
                </button>
                <button type="button" className="settings-browse-btn" onClick={() => setImporting(null)}>
                  {t("common.cancel")}
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {suggested.length > 0 && (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("skills.suggested")}</h3>
          </div>
          <p className="settings-hint">{rich("skills.suggestedHint", { none: <strong>{t("skills.suggestedNone")}</strong> })}</p>
          <div className="provider-list">
            {suggested.map((skill) => (
              <div className="provider-card skill-card" key={skill.name}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{skill.name}</span>
                  <span className="settings-hint">{skill.revises ? t("skills.suggestedChange", { name: skill.revises }) : t("skills.suggestedInactive")}</span>
                  <div className="skill-card-actions">
                    {confirmDelete === skill.name ? (
                      <>
                        <span className="settings-hint">{t("skills.rejectAsk")}</span>
                        <button type="button" className="provider-delete-btn" onClick={() => handleDelete(skill.name)}>
                          {t("skills.reject")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                          {t("common.keep")}
                        </button>
                      </>
                    ) : (
                      <>
                        <button type="button" className="settings-save-btn" onClick={() => handleAccept(skill)}>
                          {t("skills.acceptButton")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => openEdit(skill)}>
                          {t("common.edit")}
                        </button>
                        <button type="button" className="provider-delete-btn" onClick={() => setConfirmDelete(skill.name)}>
                          {t("skills.reject")}
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">{skill.description || t("skills.noDescription")}</p>
                <p className="settings-hint">
                  {skill.source ? t("skills.fromConversation", { id: skill.source }) : t("skills.fromAConversation")}
                  {skill.proposedAt ? ` · ${new Date(skill.proposedAt).toLocaleString(intlTag())}` : ""}
                  {skill.agents.length > 0 ? t("skills.onlyFor", { agents: skill.agents.join(", ") }) : ""}
                </p>
                {(() => {
                  const target = skill.revises ? active.find((s) => s.name === skill.revises) : undefined;
                  if (!target) {
                    return (
                      <details>
                        <summary className="settings-hint">{t("skills.viewInstructions")}</summary>
                        <pre className="skill-suggestion-body">{skill.body}</pre>
                      </details>
                    );
                  }
                  const diff = diffLines(target.body, skill.body);
                  return (
                    <details>
                      <summary className="settings-hint">{t("skills.viewChanges", { name: target.name })}</summary>
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
                        <p className="settings-hint">{t("skills.noChange")}</p>
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
          <h3 className="settings-section-title">{t("skills.yours")}</h3>
          <div className="skill-card-actions">
            <button
              type="button"
              className="settings-browse-btn"
              onClick={() => {
                setError(null);
                setNotice(null);
                setImporting({ content: "", preview: null, allowFlagged: false, replace: false, address: "" });
              }}
            >
              {t("skills.importButton")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={openNew}>
              {t("skills.add")}
            </button>
          </div>
        </div>

        {skills === null ? (
          <p className="settings-hint">{t("skills.loading")}</p>
        ) : active.length === 0 ? (
          <p className="settings-hint">{suggested.length > 0 ? t("skills.noActive") : t("skills.none")}</p>
        ) : (
          <div className="provider-list">
            {active.map((skill) => (
              <div className="provider-card skill-card" key={skill.name}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{skill.name}</span>
                  <div className="skill-card-actions">
                    {confirmDelete === skill.name ? (
                      <>
                        <span className="settings-hint">{t("skills.deleteAsk")}</span>
                        <button type="button" className="provider-delete-btn" onClick={() => handleDelete(skill.name)}>
                          {t("common.delete")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                          {t("common.keep")}
                        </button>
                      </>
                    ) : (
                      <>
                        <button type="button" className="settings-browse-btn" onClick={() => openEdit(skill)}>
                          {t("common.edit")}
                        </button>
                        <button
                          type="button"
                          className="settings-browse-btn"
                          onClick={() => {
                            setError(null);
                            setNotice(null);
                            setExporting({ name: skill.name, version: "1.0.0", author: "", license: licenses()[0].id, ...startSigning() });
                          }}
                        >
                          {t("skills.shareButton")}
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
                              license: licenses()[0].id,
                              ...startSigning(),
                              understood: false,
                              finding: null,
                              flaggedOk: false,
                              phase: "form",
                              qrSvg: null,
                              txId: null,
                            });
                          }}
                        >
                          {t("skills.publishButton")}
                        </button>
                        <button
                          type="button"
                          className="provider-delete-btn"
                          onClick={() => setConfirmDelete(skill.name)}
                          aria-label={t("skills.deleteLabel", { name: skill.name })}
                          title={t("skills.deleteTitle")}
                        >
                          🗑
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">{skill.description || t("skills.noDescription")}</p>
                {skill.agents.length > 0 && <p className="settings-hint">{t("skills.onlyForLine", { agents: skill.agents.join(", ") })}</p>}
              </div>
            ))}
          </div>
        )}
      </section>

      {catalog !== null && catalog.length > 0 && (
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("skills.gallery")}</h3>
          </div>
          <p className="settings-hint">{rich("skills.galleryHint", { folder: <code>skills/</code> })}</p>
          <div className="provider-list">
            {catalog.map((entry) => (
              <div className="provider-card skill-card" key={entry.name}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{entry.name}</span>
                  <div className="skill-card-actions">
                    {entry.status === "installed" ? (
                      <span className="settings-hint">{t("skills.installedBadge")}</span>
                    ) : entry.status === "modified" && confirmReplace === entry.name ? (
                      <>
                        <span className="settings-hint">{t("skills.replaceAsk")}</span>
                        <button
                          type="button"
                          className="provider-delete-btn"
                          onClick={() => handleInstall(entry.name, true)}
                          disabled={installing === entry.name}
                        >
                          {t("skills.replaceButton")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmReplace(null)}>
                          {t("skills.keepMine")}
                        </button>
                      </>
                    ) : entry.status === "modified" ? (
                      <>
                        <span className="settings-hint">{t("skills.different")}</span>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmReplace(entry.name)}>
                          {t("skills.useGallery")}
                        </button>
                      </>
                    ) : entry.status === "outdated" ? (
                      <>
                        <span className="settings-hint">{t("skills.updateAvailable")}</span>
                        <button
                          type="button"
                          className="settings-save-btn"
                          onClick={() => handleInstall(entry.name, false)}
                          disabled={installing === entry.name}
                        >
                          {installing === entry.name ? t("skills.installing") : t("skills.updateButton")}
                        </button>
                      </>
                    ) : (
                      <button
                        type="button"
                        className="settings-save-btn"
                        onClick={() => handleInstall(entry.name, false)}
                        disabled={installing === entry.name}
                      >
                        {installing === entry.name ? t("skills.installing") : t("skills.installButton")}
                      </button>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">{catalogDescription(entry.name, entry.description)}</p>
                <details>
                  <summary className="settings-hint">{t("skills.viewInstructions")}</summary>
                  <pre className="skill-suggestion-body">{entry.body}</pre>
                </details>
              </div>
            ))}
          </div>
        </section>
      )}
      <Curators
        remote={remote}
        have={active.map((s) => s.name)}
        onInstall={(content) => {
          // The preview opens at the top of the screen, above this section.
          void handleCheckImport(content).then(() => requestAnimationFrame(() => document.querySelector(".skill-editor")?.scrollIntoView?.({ block: "start" })));
        }}
      />
      <TrustedPublishers remote={remote} key={trustedVersion} />
    </div>
  );
}

export default SkillsView;

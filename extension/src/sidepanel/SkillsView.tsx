import { useEffect, useState } from "react";
import type { CatalogSkillDto, SkillDto } from "../protocol/messages";
import type { DraftSkillResponse, ListSkillCatalogResponse, ListSkillsResponse, OkResponse, ReadSkillFileResponse } from "../background/popup_protocol";
import { t } from "../i18n/index.ts";
import { useLocale } from "../i18n/react.tsx";
import { catalogAction, catalogDescription } from "./lib/skillCatalog";
import { MAX_FILES_PER_SKILL, canAttach, decodeAttachment, validateFileName } from "./lib/skillFiles";
import { diffLines, hasChanges, withContext } from "./skillDiff";

/** What the editor form is doing: `new` (name editable, refuses a taken name) or `edit` (name
 * locked, overwrites) — same split as the desktop's SkillsView. */
interface EditorState {
  mode: "new" | "edit";
  skill: SkillDto;
  /** Editing a suggestion: saving accepts it only when this is on. */
  accept?: boolean;
}

const emptySkill: SkillDto = { name: "", description: "", body: "", agents: [] };

export default function SkillsView() {
  const [skills, setSkills] = useState<SkillDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [saving, setSaving] = useState(false);
  // P72 — "describe it and the AI drafts it": the text, and whether the hub is still writing.
  const [draftText, setDraftText] = useState("");
  const [drafting, setDrafting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  // P140 — the gallery; stays null when the hub has none (a hub from before it answers with an error), and the section is left out.
  const [catalog, setCatalog] = useState<CatalogSkillDto[] | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [confirmReplace, setConfirmReplace] = useState<string | null>(null);
  // P72 — the attachments of the skill being edited: whether one is being sent, the one shown (read on demand), the one asked to be removed.
  const [attaching, setAttaching] = useState(false);
  const [viewing, setViewing] = useState<{ file: string; content: string } | null>(null);
  const [confirmFileRemove, setConfirmFileRemove] = useState<string | null>(null);
  const locale = useLocale();

  function refresh() {
    chrome.runtime.sendMessage({ type: "listSkills" }).then((res: ListSkillsResponse) => {
      if (res.ok) {
        setSkills(res.skills);
        setError(null);
      } else {
        setSkills((current) => current ?? []);
        setError(res.error ?? t("skills.listFailed"));
      }
    });
    chrome.runtime.sendMessage({ type: "listSkillCatalog" }).then((res: ListSkillCatalogResponse) => {
      setCatalog(res.ok ? res.entries : null);
    });
  }

  useEffect(refresh, []);

  function handleInstall(name: string, overwrite: boolean) {
    setInstalling(name);
    setError(null);
    chrome.runtime.sendMessage({ type: "installCatalogSkill", name, overwrite }).then((res: OkResponse) => {
      setInstalling(null);
      if (res.ok) {
        setConfirmReplace(null);
        refresh();
      } else {
        setError(res.error ?? t("skills.installFailed"));
      }
    });
  }

  function handleSave() {
    if (!editor) return;
    setSaving(true);
    setError(null);
    // Editing a suggestion without accepting keeps it pending; the hub reads a missing `proposed` as accepted.
    const { proposed, source, proposedAt, ...accepted } = editor.skill;
    const skill = proposed && !editor.accept ? editor.skill : accepted;
    void source;
    void proposedAt;
    chrome.runtime
      .sendMessage({ type: "saveSkill", skill, overwrite: editor.mode === "edit" })
      .then((res: OkResponse) => {
        setSaving(false);
        if (res.ok) {
          setEditor(null);
          refresh();
        } else {
          setError(res.error ?? t("skills.saveFailed"));
        }
      });
  }

  /** Asks the hub to write a draft from `draftText` and loads it into the form. It is never saved here: the person reviews it
   * and saves with the form's own button. */
  function handleDraft() {
    setDrafting(true);
    setError(null);
    chrome.runtime.sendMessage({ type: "draftSkill", description: draftText }).then((res: DraftSkillResponse) => {
      setDrafting(false);
      if (res.ok && res.skill) {
        setEditor((current) => (current ? { ...current, skill: res.skill! } : current));
        setDraftText("");
      } else {
        setError(res.error ?? t("skills.draftFailed"));
      }
    });
  }

  /** Accepts a suggestion as it is: a save without the `proposed` mark. */
  function handleAccept(skill: SkillDto) {
    setError(null);
    const accepted: SkillDto = { name: skill.name, description: skill.description, body: skill.body, agents: skill.agents };
    chrome.runtime.sendMessage({ type: "saveSkill", skill: accepted, overwrite: true }).then((res: OkResponse) => {
      if (res.ok) refresh();
      else setError(res.error ?? t("skills.acceptFailed"));
    });
  }

  function handleDelete(name: string) {
    setError(null);
    chrome.runtime.sendMessage({ type: "deleteSkill", name }).then((res: OkResponse) => {
      setConfirmDelete(null);
      if (res.ok) refresh();
      else setError(res.error ?? t("skills.deleteFailed"));
    });
  }

  function updateEditor(patch: Partial<SkillDto>) {
    setEditor((current) => (current ? { ...current, skill: { ...current.skill, ...patch } } : current));
  }

  /** Reloads the list and takes only `name`'s attachments from it, so what the person typed in the form is not lost
   * (`refresh` would rebuild the whole editor). */
  function reloadFiles(name: string) {
    chrome.runtime.sendMessage({ type: "listSkills" }).then((res: ListSkillsResponse) => {
      if (!res.ok) return;
      setSkills(res.skills);
      const files = res.skills.find((s) => s.name === name)?.files;
      setEditor((current) => (current && current.skill.name === name ? { ...current, skill: { ...current.skill, files } } : current));
    });
  }

  const fileNameMessage = { length: "skills.fileNameLength", chars: "skills.fileNameChars", dot: "skills.fileNameDot" } as const;

  /** Checks the chosen file the way the hub will (name, 64 KB in bytes, UTF-8, 20 per skill) and sends it; the hub still decides. */
  async function handleAttach(chosen: File | undefined, skill: SkillDto) {
    if (!chosen) return;
    setError(null);
    const problem = validateFileName(chosen.name);
    if (problem) return setError(t(fileNameMessage[problem]));
    if (!canAttach(skill.files, chosen.name)) return setError(t("skills.fileLimit", { max: MAX_FILES_PER_SKILL }));
    const decoded = decodeAttachment(new Uint8Array(await chosen.arrayBuffer()));
    if (!decoded.ok) return setError(t(decoded.reason === "tooBig" ? "skills.fileTooBig" : "skills.fileNotText"));
    setAttaching(true);
    chrome.runtime.sendMessage({ type: "saveSkillFile", name: skill.name, file: chosen.name, content: decoded.text }).then((res: OkResponse) => {
      setAttaching(false);
      if (res.ok) {
        // A file that was on screen is now out of date.
        setViewing((current) => (current?.file === chosen.name ? null : current));
        reloadFiles(skill.name);
      } else {
        setError(res.error ?? t("skills.fileSaveFailed"));
      }
    });
  }

  function handleViewFile(name: string, file: string) {
    if (viewing?.file === file) return setViewing(null);
    setError(null);
    chrome.runtime.sendMessage({ type: "readSkillFile", name, file }).then((res: ReadSkillFileResponse) => {
      if (res.ok && res.content !== undefined) setViewing({ file, content: res.content });
      else setError(res.error ?? t("skills.fileReadFailed"));
    });
  }

  function handleRemoveFile(name: string, file: string) {
    setError(null);
    chrome.runtime.sendMessage({ type: "deleteSkillFile", name, file }).then((res: OkResponse) => {
      setConfirmFileRemove(null);
      if (res.ok) {
        setViewing((current) => (current?.file === file ? null : current));
        reloadFiles(name);
      } else {
        setError(res.error ?? t("skills.fileDeleteFailed"));
      }
    });
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
        <strong>{editor.mode === "new" ? t("skills.new") : t("skills.editing", { name: editor.skill.name })}</strong>
        {editor.mode === "new" && (
          <div className="skills-draft">
            <label>
              {t("skills.draftLabel")}
              <textarea value={draftText} onChange={(e) => setDraftText(e.target.value)} rows={3} maxLength={2000} placeholder={t("skills.draftPlaceholder")} disabled={drafting} />
            </label>
            <span className="skills-hint">{t("skills.draftHint")}</span>
            <button type="button" disabled={drafting || saving || draftText.trim() === ""} onClick={handleDraft}>
              {drafting ? t("skills.drafting") : t("skills.draftButton")}
            </button>
          </div>
        )}
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
          <label>
            <input type="checkbox" checked={editor.accept ?? false} onChange={(e) => setEditor({ ...editor, accept: e.target.checked })} />
            {t("skills.acceptThis")}
          </label>
        )}
        <fieldset className="skills-files">
          <legend>{t("skills.files")}</legend>
          {editor.mode === "new" ? (
            <span className="skills-hint">{t("skills.filesSaveFirst")}</span>
          ) : (
            <>
              <span className="skills-hint">{t("skills.filesHint", { max: MAX_FILES_PER_SKILL })}</span>
              {(editor.skill.files ?? []).length === 0 && <span className="skills-hint">{t("skills.filesNone")}</span>}
              {(editor.skill.files ?? []).map((f) => (
                <div key={f.name} className="skills-file">
                  <div className="skills-file-row">
                    <span className="skills-file-name">{f.name}</span>
                    <span className="skills-hint">{t("skills.fileSize", { size: f.size })}</span>
                    <button type="button" className="link-button" onClick={() => handleViewFile(editor.skill.name, f.name)}>
                      {viewing?.file === f.name ? t("skills.fileHide") : t("skills.fileView")}
                    </button>
                    {confirmFileRemove === f.name ? (
                      <>
                        <span className="skills-hint">{t("skills.fileRemoveConfirm", { name: f.name })}</span>
                        <button type="button" className="link-button skills-danger" onClick={() => handleRemoveFile(editor.skill.name, f.name)}>
                          {t("skills.fileRemove")}
                        </button>
                        <button type="button" className="link-button" onClick={() => setConfirmFileRemove(null)}>
                          {t("common.cancel")}
                        </button>
                      </>
                    ) : (
                      <button type="button" className="link-button skills-danger" onClick={() => setConfirmFileRemove(f.name)}>
                        {t("skills.fileRemove")}
                      </button>
                    )}
                  </div>
                  {viewing?.file === f.name && <pre className="skills-diff">{viewing.content}</pre>}
                </div>
              ))}
              <label className="skills-file-add">
                {attaching ? t("skills.fileAdding") : t("skills.fileAdd")}
                <input
                  type="file"
                  disabled={attaching}
                  onChange={(e) => {
                    const input = e.target;
                    void handleAttach(input.files?.[0], editor.skill).finally(() => {
                      input.value = "";
                    });
                  }}
                />
              </label>
            </>
          )}
        </fieldset>
        {error && <p className="error-banner">{error}</p>}
        <div className="skills-actions">
          <button type="submit" disabled={saving}>
            {saving ? t("skills.saving") : t("common.save")}
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
        <button
          type="button"
          onClick={() => {
            setError(null);
            setEditor({ mode: "new", skill: emptySkill });
          }}
        >
          {t("skills.newButton")}
        </button>
      </div>
      {error && <p className="error-banner">{error}</p>}
      {skills === null ? (
        <p className="skills-hint">{t("common.loading")}</p>
      ) : skills.length === 0 ? (
        <p className="skills-hint">{t("skills.none")}</p>
      ) : (
        <ul className="skills-list">
          {skills.map((skill) => (
            <li key={skill.name} className="skills-item">
              <div className="skills-item-header">
                <span className="skills-item-name">{skill.name}</span>
                {skill.proposed && <span className="skills-hint">{skill.revises ? t("skills.suggestedChange", { name: skill.revises }) : t("skills.suggested")}</span>}
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
                    {skill.proposed && (
                      <button type="button" className="link-button" onClick={() => handleAccept(skill)}>
                        {t("skills.accept")}
                      </button>
                    )}
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setEditor({ mode: "edit", skill, accept: skill.proposed ? true : undefined });
                      }}
                    >
                      {t("common.edit")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(skill.name)}>
                      {skill.proposed ? t("skills.reject") : t("common.delete")}
                    </button>
                  </span>
                )}
              </div>
              <p className="skills-item-description">{skill.description || t("skills.noDescription")}</p>
              {skill.agents.length > 0 && <p className="skills-hint">{t("skills.onlyFor", { agents: skill.agents.join(", ") })}</p>}
              {skill.proposed && skill.revises && (() => {
                const target = skills.find((s) => !s.proposed && s.name === skill.revises);
                if (!target) return null;
                const diff = diffLines(target.body, skill.body);
                return (
                  <details>
                    <summary className="skills-hint">{t("skills.viewChange", { name: target.name })}</summary>
                    {hasChanges(diff) ? (
                      <pre className="skills-diff">
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
            </li>
          ))}
        </ul>
      )}
      {catalog !== null && catalog.length > 0 && (
        <section className="skills-gallery">
          <strong>{t("skills.gallery")}</strong>
          <span className="skills-hint">{t("skills.galleryIntro")}</span>
          <ul className="skills-list">
            {catalog.map((entry) => {
              const action = catalogAction(entry.status, confirmReplace === entry.name);
              return (
                <li key={entry.name} className="skills-item">
                  <div className="skills-item-header">
                    <span className="skills-item-name">{entry.name}</span>
                    {action === "installed" && <span className="skills-hint">{t("skills.galleryInstalled")}</span>}
                    {action === "install" && (
                      <button type="button" disabled={installing === entry.name} onClick={() => handleInstall(entry.name, false)}>
                        {installing === entry.name ? t("skills.installing") : t("skills.installButton")}
                      </button>
                    )}
                    {action === "update" && (
                      <span className="skills-actions">
                        <span className="skills-hint">{t("skills.updateAvailable")}</span>
                        <button type="button" disabled={installing === entry.name} onClick={() => handleInstall(entry.name, false)}>
                          {installing === entry.name ? t("skills.installing") : t("skills.updateButton")}
                        </button>
                      </span>
                    )}
                    {action === "conflict" && (
                      <span className="skills-actions">
                        <span className="skills-hint">{t("skills.otherSameName")}</span>
                        <button type="button" className="link-button" onClick={() => setConfirmReplace(entry.name)}>
                          {t("skills.useGallery")}
                        </button>
                      </span>
                    )}
                    {action === "confirm" && (
                      <span className="skills-actions">
                        <button type="button" className="link-button skills-danger" disabled={installing === entry.name} onClick={() => handleInstall(entry.name, true)}>
                          {t("skills.replaceMineButton")}
                        </button>
                        <button type="button" className="link-button" onClick={() => setConfirmReplace(null)}>
                          {t("skills.keepMine")}
                        </button>
                      </span>
                    )}
                  </div>
                  <p className="skills-item-description">{catalogDescription(locale, entry.name, entry.description)}</p>
                  <details>
                    <summary className="skills-hint">{t("skills.viewInstructions")}</summary>
                    <pre className="skills-diff">{entry.body}</pre>
                  </details>
                </li>
              );
            })}
          </ul>
        </section>
      )}
    </div>
  );
}

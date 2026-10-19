import { useEffect, useState } from "react";
import type { SkillDto } from "../protocol/messages";
import type { ListSkillsResponse, OkResponse } from "../background/popup_protocol";
import { t } from "../i18n/index.ts";
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
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

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
  }

  useEffect(refresh, []);

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
          <label>
            <input type="checkbox" checked={editor.accept ?? false} onChange={(e) => setEditor({ ...editor, accept: e.target.checked })} />
            {t("skills.acceptThis")}
          </label>
        )}
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
    </div>
  );
}

import { useCallback, useEffect, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { AgentSettings } from "../hub/messages";
import { t } from "../i18n";

// A member's agents (P84 fatia 2): their own — which only they see, with the tools they have — and
// the ones the owner shared with them, which they can use but not change. The owner edits theirs in
// Settings; a member never gets there.

interface Draft {
  /** The name it had when opened; absent for a new one. */
  originalId?: string;
  id: string;
  persona: string;
  /** `null`: every tool they have. */
  allowedTools: string[] | null;
  /** A model the owner released to them (P100); empty is the hub's default. */
  providerId: string;
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function toAgent(draft: Draft): AgentSettings {
  return {
    id: draft.id.trim(),
    persona: draft.persona,
    providerId: draft.providerId,
    canDelegateToAgents: false,
    canManageAgents: false,
    canMessageAgents: false,
    canManageTasks: false,
    // O agente de um membro faz o que sempre fez: os workers e o segundo plano fazem parte das ferramentas dele.
    canStartTasks: true,
    canCreateWorkers: true,
    canMessageUser: true,
    canChooseModels: true,
    allowedTools: draft.allowedTools,
    autonomy: 4,
    approvalRequired: [],
  };
}

export default function MyAgentsView({ conn, onChanged }: { conn: ServerConnection | null; onChanged: () => void }) {
  const [agents, setAgents] = useState<AgentSettings[] | null>(null);
  const [tools, setTools] = useState<string[]>([]);
  /** The models the owner released to this person for their agents (P100). */
  const [models, setModels] = useState<string[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const apply = useCallback((settings: { agents: AgentSettings[]; toolNames: string[]; memberModels?: string[] }) => {
    setAgents(settings.agents);
    setTools(settings.toolNames);
    setModels(settings.memberModels ?? []);
  }, []);

  useEffect(() => {
    conn
      ?.requestSettings()
      .then(({ settings }) => apply(settings))
      .catch((err) => setError(message(err)));
  }, [conn, apply]);

  async function save() {
    if (!conn || !draft) return;
    setBusy(true);
    setError(null);
    try {
      apply((await conn.saveOwnAgent(toAgent(draft), draft.originalId)).settings);
      setDraft(null);
      onChanged();
    } catch (err) {
      setError(message(err));
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    if (!conn) return;
    setBusy(true);
    setError(null);
    try {
      apply((await conn.deleteOwnAgent(id)).settings);
      setConfirmDelete(null);
      onChanged();
    } catch (err) {
      setError(message(err));
    } finally {
      setBusy(false);
    }
  }

  const mine = (agents ?? []).filter((a) => a.owner);
  const shared = (agents ?? []).filter((a) => !a.owner);

  return (
    <div className="skills-view">
      <div className="skills-header">
        <h2>{t("myAgents.title")}</h2>
        <button type="button" className="primary-button" disabled={!conn || draft !== null} onClick={() => setDraft({ id: "", persona: "", allowedTools: null, providerId: "" })}>
          {t("myAgents.create")}
        </button>
      </div>
      <p className="skills-hint">{t("myAgents.intro")}</p>
      {error && <p className="error-banner">{error}</p>}

      {draft && (
        <form
          className="settings-confirm skills-editor"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <label>
            {t("common.name")}
            <input value={draft.id} onChange={(e) => setDraft({ ...draft, id: e.target.value })} placeholder={t("myAgents.namePlaceholder")} autoFocus />
          </label>
          <label>
            {t("myAgents.persona")}
            <textarea rows={6} value={draft.persona} onChange={(e) => setDraft({ ...draft, persona: e.target.value })} placeholder={t("myAgents.personaPlaceholder")} />
          </label>
          {/* Only when the owner released models to them (P100); one the owner took back stays listed so saving doesn't swap it silently. */}
          {(models.length > 0 || draft.providerId !== "") && (
            <label>
              {t("myAgents.model")}
              <select value={draft.providerId} onChange={(e) => setDraft({ ...draft, providerId: e.target.value })}>
                <option value="">{t("myAgents.hubDefault")}</option>
                {draft.providerId !== "" && !models.includes(draft.providerId) && <option value={draft.providerId}>{t("myAgents.notReleased", { id: draft.providerId })}</option>}
                {models.map((id) => (
                  <option key={id} value={id}>
                    {id}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label className="settings-check">
            <input type="checkbox" checked={draft.allowedTools !== null} onChange={(e) => setDraft({ ...draft, allowedTools: e.target.checked ? [] : null })} />
            {t("myAgents.limitTools")}
          </label>
          {draft.allowedTools !== null && (
            <fieldset className="settings-tools">
              {tools.map((tool) => (
                <label key={tool} className="settings-check">
                  <input
                    type="checkbox"
                    checked={draft.allowedTools!.includes(tool)}
                    onChange={(e) => setDraft({ ...draft, allowedTools: e.target.checked ? [...draft.allowedTools!, tool] : draft.allowedTools!.filter((x) => x !== tool) })}
                  />
                  <code>{tool}</code>
                </label>
              ))}
            </fieldset>
          )}
          <div className="skills-actions">
            <button type="submit" className="primary-button" disabled={busy || !conn || draft.id.trim() === "" || draft.persona.trim() === ""}>
              {busy ? t("common.wait") : t("common.save")}
            </button>
            <button type="button" className="link-button" disabled={busy} onClick={() => setDraft(null)}>
              {t("common.cancel")}
            </button>
          </div>
        </form>
      )}

      {agents === null ? (
        <p className="skills-hint">{t("common.loading")}</p>
      ) : (
        <>
          <h3 className="settings-section-title">{t("myAgents.yours")}</h3>
          {mine.length === 0 ? (
            <p className="skills-hint">{t("myAgents.noneYet")}</p>
          ) : (
            <ul className="skills-list">
              {mine.map((agent) => (
                <li key={agent.id} className="skills-item">
                  <div className="skills-item-header">
                    <span className="skills-item-name">{agent.id}</span>
                  </div>
                  <p className="skills-item-description">{agent.persona}</p>
                  <p className="skills-hint">
                    {agent.allowedTools === null ? t("myAgents.allTools") : t("myAgents.onlyTools", { tools: agent.allowedTools.join(", ") || t("myAgents.noTools") })}
                    {agent.providerId !== "" && t("myAgents.model.inline", { id: agent.providerId })}
                  </p>
                  {confirmDelete === agent.id ? (
                    <div className="skills-actions">
                      <button type="button" className="primary-button skills-danger" disabled={busy} onClick={() => void remove(agent.id)}>
                        {t("myAgents.deleteForGood")}
                      </button>
                      <button type="button" className="link-button" onClick={() => setConfirmDelete(null)}>
                        {t("common.cancel")}
                      </button>
                    </div>
                  ) : (
                    <div className="skills-actions">
                      <button
                        type="button"
                        className="link-button"
                        disabled={draft !== null}
                        onClick={() => setDraft({ originalId: agent.id, id: agent.id, persona: agent.persona, allowedTools: agent.allowedTools, providerId: agent.providerId })}
                      >
                        {t("common.edit")}
                      </button>
                      <button type="button" className="link-button skills-danger" onClick={() => setConfirmDelete(agent.id)}>
                        {t("common.delete")}
                      </button>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
          <h3 className="settings-section-title">{t("myAgents.shared")}</h3>
          {shared.length === 0 ? (
            <p className="skills-hint">{t("myAgents.sharedNone")}</p>
          ) : (
            <ul className="skills-list">
              {shared.map((agent) => (
                <li key={agent.id} className="skills-item">
                  <span className="skills-item-name">{agent.id}</span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}

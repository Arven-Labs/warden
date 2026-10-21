import { useEffect, useState } from "react";
import type { AgentInfo, AgentTask, HubAgents, ModelPolicy, OrgEdit } from "../protocol/messages";
import type { AgentActivityExtrasResponse, AgentTasksResponse, HubAgentsResponse } from "../background/popup_protocol";
import { activityLine, activityOf, lastConversationOf, spendOf } from "./lib/agentTasks";
import { addReportEdit, APPROVAL_CATEGORIES, AUTONOMY_LEVELS, buildOrg, moveEdit, positionEdit, powersEdit, superiorChoices, type OrgNode } from "./lib/org";
import { delegationCandidates, delegationSummary, limitEdit, limitModels, nextPolicyId, policiesEdit, policiesWith, policiesWithout } from "./lib/modelPolicies";
import { t, type Key } from "../i18n/index.ts";
import { rich } from "../i18n/react.tsx";
import PairingKeyForm from "./PairingKeyForm";

const AUTONOMY_LABELS: Record<number, Key> = { 1: "org.autonomy.1", 2: "org.autonomy.2", 3: "org.autonomy.3", 4: "org.autonomy.4", 5: "org.autonomy.5" };

/** What the agent can do, as the Settings screen defines it. */
export function badges(agent: AgentInfo): string[] {
  const list: string[] = [];
  if (agent.canDelegateToAgents) list.push(t("org.badge.delegates"));
  if (agent.canManageAgents) list.push(t("org.badge.manages"));
  if (agent.canMessageAgents) list.push(t("org.badge.notes"));
  if (agent.canManageTasks) list.push(t("org.badge.schedules"));
  // On by default: only what was taken away shows.
  if (!agent.canStartTasks) list.push(t("org.badge.noBackground"));
  if (!agent.canCreateWorkers) list.push(t("org.badge.noWorkers"));
  if (!agent.canMessageUser) list.push(t("org.badge.noMessageUser"));
  if (!agent.canChooseModels) list.push(t("org.badge.noChooseModel"));
  if (agent.autonomy !== 4) list.push(t("org.badge.autonomy", { level: agent.autonomy, what: AUTONOMY_LABELS[agent.autonomy] ? t(AUTONOMY_LABELS[agent.autonomy]) : "" }).trim());
  if (agent.approvalRequired.length > 0) list.push(t(agent.approvalRequired.length > 1 ? "org.badge.asksFirstMany" : "org.badge.asksFirstOne", { count: agent.approvalRequired.length }));
  return list;
}

/** What is open in the tree: an agent's role, a new subordinate under an agent (`null`: at the top) or the removal of one. */
type Panel =
  | { kind: "edit"; id: string }
  | { kind: "add"; under: string | null }
  | { kind: "remove"; id: string }
  /** The models an agent may choose when delegating. */
  | { kind: "limit"; id: string }
  /** What an agent can do alone: autonomy, permissions and the kinds of action that need the person's yes (P122). */
  | { kind: "powers"; id: string }
  /** A model policy: `id` is the one being edited, or `null` for a new one. */
  | { kind: "policy"; id: string | null };

interface OrgUi {
  onOpenChat: (id: string) => void;
  onOpenTasks: (id: string) => void;
  agents: AgentInfo[];
  /** Providers, combos and policies: what an agent can be limited to. */
  candidates: string[];
  /** Providers and combos: what a policy can answer. */
  modelIds: string[];
  policies: ModelPolicy[];
  panel: Panel | null;
  busy: boolean;
  open: (panel: Panel | null) => void;
  /** Asks for the pairing key for the change. */
  apply: (edit: OrgEdit) => void;
  /** The agent whose card is being dragged (P120): another card, or the top, accepts the drop if the change is valid. */
  dragging: string | null;
  drag: (id: string | null) => void;
  /** What the agent has been doing, in one line, taken from the hub's tasks; `null` with no task of theirs. */
  activity: (id: string) => string | null;
}

/** Drops the dragged card onto `target` (`null`: the top): the change, if any. A card that becomes the child of a descendant would close a cycle. */
function onDrop(ui: OrgUi, target: string | null): void {
  const edit = ui.dragging === null ? null : moveEdit(ui.agents, ui.dragging, target);
  ui.drag(null);
  if (edit) ui.apply(edit);
}

function RoleForm({ agent, ui }: { agent: AgentInfo; ui: OrgUi }) {
  const [role, setRole] = useState(agent.role ?? "");
  const [superior, setSuperior] = useState(agent.reportsTo ?? "");
  const options = superiorChoices(ui.agents, agent.id);
  return (
    <div className="connection-form org-form">
      <label>
        {t("org.role")}
        <input type="text" value={role} maxLength={80} placeholder={t("org.rolePlaceholder")} onChange={(e) => setRole(e.target.value)} />
      </label>
      <label>
        {t("org.reportsTo")}
        <select value={superior} onChange={(e) => setSuperior(e.target.value)}>
          <option value="">{t("org.nobodyTop")}</option>
          {options.map((a) => (
            <option key={a.id} value={a.id}>
              {a.id}
            </option>
          ))}
        </select>
        <span className="skills-hint">{t("org.reportsHint", { id: agent.id })}</span>
      </label>
      <div className="skills-actions">
        <button type="button" disabled={ui.busy} onClick={() => ui.apply(positionEdit(agent.id, role, superior))}>
          {t("common.save")}
        </button>
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

/** What the agent can do alone (P122): the autonomy level, the four permissions and the kinds of action that always need the person's yes. All
 * in one change, with one Save, because each change rebuilds the hub and asks for the pairing key. Starts with what the agent has today. */
function PowersForm({ agent, ui }: { agent: AgentInfo; ui: OrgUi }) {
  const [autonomy, setAutonomy] = useState(agent.autonomy);
  const [startTasks, setStartTasks] = useState(agent.canStartTasks);
  const [createWorkers, setCreateWorkers] = useState(agent.canCreateWorkers);
  const [messageUser, setMessageUser] = useState(agent.canMessageUser);
  const [chooseModels, setChooseModels] = useState(agent.canChooseModels);
  const [categories, setCategories] = useState(() => new Set(agent.approvalRequired));
  // An id that only a newer hub knows shows too, by its own id, so it does not vanish on save.
  const unknown = [...categories].filter((c) => !APPROVAL_CATEGORIES.some((k) => k.id === c));
  function toggle(id: string) {
    const next = new Set(categories);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setCategories(next);
  }
  return (
    <div className="connection-form org-form">
      <label>
        {t("org.autonomyOf", { id: agent.id })}
        <select value={autonomy} onChange={(e) => setAutonomy(Number(e.target.value))}>
          {AUTONOMY_LEVELS.map((n) => (
            <option key={n.level} value={n.level}>
              {n.label}
            </option>
          ))}
        </select>
        {autonomy === 5 && !agent.canManageAgents && <span className="skills-hint">{t("org.level5Needs", { id: agent.id })}</span>}
      </label>
      <label className="checkbox-label">
        <input type="checkbox" checked={startTasks} onChange={(e) => setStartTasks(e.target.checked)} />
        {t("org.canStart")}
      </label>
      <label className="checkbox-label">
        <input type="checkbox" checked={createWorkers} onChange={(e) => setCreateWorkers(e.target.checked)} />
        {t("org.canWorkers")}
      </label>
      <label className="checkbox-label">
        <input type="checkbox" checked={messageUser} onChange={(e) => setMessageUser(e.target.checked)} />
        {t("org.canMessageUser")}
      </label>
      <label className="checkbox-label">
        <input type="checkbox" checked={chooseModels} onChange={(e) => setChooseModels(e.target.checked)} />
        {t("org.canChooseModels")}
      </label>
      <p className="skills-hint">{t("org.asksBefore")}</p>
      {APPROVAL_CATEGORIES.map((c) => (
        <label key={c.id} className="checkbox-label">
          <input type="checkbox" checked={categories.has(c.id)} onChange={() => toggle(c.id)} />
          {c.label}
        </label>
      ))}
      {unknown.map((id) => (
        <label key={id} className="checkbox-label">
          <input type="checkbox" checked onChange={() => toggle(id)} />
          {id}
        </label>
      ))}
      <div className="skills-actions">
        <button
          type="button"
          disabled={ui.busy}
          onClick={() =>
            ui.apply(
              powersEdit(agent.id, {
                autonomy,
                canStartTasks: startTasks,
                canCreateWorkers: createWorkers,
                canMessageUser: messageUser,
                canChooseModels: chooseModels,
                approvalRequired: categories,
              }),
            )
          }
        >
          {t("org.savePowers")}
        </button>
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

/** The models the agent may choose when delegating (P123): checked, with a default (the first). Nothing checked leaves the choice open. */
function LimitForm({ agent, ui }: { agent: AgentInfo; ui: OrgUi }) {
  const [checked, setChecked] = useState(() => new Set(agent.delegationModels));
  const [defaultModel, setDefaultModel] = useState(agent.delegationModels[0] ?? "");
  const chosen = ui.candidates.filter((id) => checked.has(id));
  const validDefault = chosen.includes(defaultModel) ? defaultModel : (chosen[0] ?? "");
  function toggle(id: string) {
    const next = new Set(checked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setChecked(next);
  }
  return (
    <div className="connection-form org-form">
      <p className="skills-hint">{t("org.modelsHint", { id: agent.id })}</p>
      {ui.candidates.length === 0 && <p className="skills-hint">{t("org.noModelsToPick")}</p>}
      {ui.candidates.map((id) => (
        <label key={id} className="checkbox-label">
          <input type="checkbox" checked={checked.has(id)} onChange={() => toggle(id)} />
          {id}
          {ui.policies.some((p) => p.id === id) && <span className="skills-hint">{t("org.policyMark")}</span>}
        </label>
      ))}
      {chosen.length > 1 && (
        <label>
          {t("org.defaultModel", { id: agent.id })}
          <select value={validDefault} onChange={(e) => setDefaultModel(e.target.value)}>
            {chosen.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        </label>
      )}
      <div className="skills-actions">
        <button type="button" disabled={ui.busy} onClick={() => ui.apply(limitEdit(agent.id, limitModels(ui.candidates, checked, validDefault)))}>
          {t("common.save")}
        </button>
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

/** A model policy (P123): a name the delegating agent can say, the provider or combo that answers for it, and when to choose it. */
function PolicyForm({ original, ui }: { original: ModelPolicy | null; ui: OrgUi }) {
  const [id, setId] = useState(original?.id ?? nextPolicyId(ui.candidates));
  const [model, setModel] = useState(original?.model ?? ui.modelIds[0] ?? "");
  const [description, setDescription] = useState(original?.description ?? "");
  const ready = id.trim() !== "" && model !== "";
  return (
    <div className="connection-form org-form">
      <label>
        {t("org.name")}
        <input type="text" value={id} maxLength={64} onChange={(e) => setId(e.target.value)} />
      </label>
      <label>
        {t("org.answers")}
        <select value={model} onChange={(e) => setModel(e.target.value)}>
          {ui.modelIds.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </label>
      <label>
        {t("org.when")}
        <input type="text" value={description} maxLength={200} placeholder={t("org.whenPlaceholder")} onChange={(e) => setDescription(e.target.value)} />
      </label>
      <div className="skills-actions">
        <button
          type="button"
          disabled={ui.busy || !ready}
          onClick={() => ui.apply(policiesEdit(policiesWith(ui.policies, { id, model, description }, original?.id ?? null)))}
        >
          {t("org.savePolicy")}
        </button>
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

function NewAgentForm({ under, ui }: { under: string | null; ui: OrgUi }) {
  const [id, setId] = useState("");
  const [role, setRole] = useState("");
  const [persona, setPersona] = useState("");
  const ready = id.trim() !== "" && persona.trim() !== "";
  return (
    <div className="connection-form org-form">
      <label>
        {t("org.name")}
        <input type="text" value={id} maxLength={64} placeholder={t("org.namePlaceholder")} onChange={(e) => setId(e.target.value)} />
      </label>
      <label>
        {t("org.role")}
        <input type="text" value={role} maxLength={80} placeholder={t("org.optional")} onChange={(e) => setRole(e.target.value)} />
      </label>
      <label>
        {t("org.whatItDoes")}
        <textarea value={persona} rows={3} placeholder={t("org.personaPlaceholder")} onChange={(e) => setPersona(e.target.value)} />
        <span className="skills-hint">
          {under ? t("org.reportsToWho", { who: under }) : t("org.startsAtTop")}
          {t("org.startsCareful")}
        </span>
      </label>
      <div className="skills-actions">
        <button type="button" disabled={ui.busy || !ready} onClick={() => ui.apply(addReportEdit(id, persona, role, under))}>
          {t("org.addAgent")}
        </button>
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

function ConfirmRemoval({ node, ui }: { node: OrgNode<AgentInfo>; ui: OrgUi }) {
  const { agent, children } = node;
  return (
    <div className="connection-form org-form">
      <p className="skills-hint">
        {rich("org.removeQuestion", { id: <strong>{agent.id}</strong> })}
        {children.length > 0 &&
          ` ${t(children.length === 1 ? "org.reportOne" : "org.reportMany", { count: children.length })}${t("org.reportTo", { who: agent.reportsTo ?? t("org.nobodyTopParen") })}`}{" "}
        {t("org.conversationsStay")}
      </p>
      <div className="skills-actions">
        <button type="button" disabled={ui.busy} onClick={() => ui.apply({ kind: "remove", id: agent.id })}>
          {t("common.remove")}
        </button>
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open(null)}>
          {t("common.keep")}
        </button>
      </div>
    </div>
  );
}

function Node({ node, ui }: { node: OrgNode<AgentInfo>; ui: OrgUi }) {
  const { agent, children } = node;
  const { panel } = ui;
  const validTarget = ui.dragging !== null && moveEdit(ui.agents, ui.dragging, agent.id) !== null;
  return (
    <li className="org-node">
      {/* Dragging lives on the card, not on the <li>: the subordinates' `dragover` must not bubble up to the parent's card. */}
      <div
        className={`org-card${ui.dragging === agent.id ? " org-card--dragging" : ""}${validTarget ? " org-card--target" : ""}`}
        draggable={!ui.busy}
        onDragStart={(e) => {
          e.dataTransfer.setData("text/plain", agent.id);
          e.dataTransfer.effectAllowed = "move";
          ui.drag(agent.id);
        }}
        onDragEnd={() => ui.drag(null)}
        onDragOver={(e) => {
          if (validTarget) e.preventDefault();
        }}
        onDrop={(e) => {
          e.preventDefault();
          onDrop(ui, agent.id);
        }}
      >
        <span className="org-name">{agent.id}</span>
        {agent.role && <span className="org-role">{agent.role}</span>}
        <span className="org-badges">
          {badges(agent).map((badge) => (
            <span key={badge} className="org-badge">
              {badge}
            </span>
          ))}
        </span>
        {agent.canDelegateToAgents && <span className="skills-hint org-limit">{delegationSummary(agent.delegationModels)}</span>}
        {ui.activity(agent.id) && <span className="skills-hint org-limit">{ui.activity(agent.id)}</span>}
        {children.length > 0 && <span className="org-count">{children.length === 1 ? t("org.oneSubordinate") : t("org.nSubordinates", { count: children.length })}</span>}
        <span className="org-actions">
          <button type="button" className="link-button" onClick={() => ui.onOpenChat(agent.id)} title={t("org.startChat", { id: agent.id })}>
            {t("org.chat")}
          </button>
          <button type="button" className="link-button" onClick={() => ui.onOpenTasks(agent.id)} title={t("org.tasksOf", { id: agent.id })}>
            {t("org.tasks")}
          </button>
          <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "edit", id: agent.id })}>
            {t("common.edit")}
          </button>
          <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "powers", id: agent.id })} title={t("org.powersOf", { id: agent.id })}>
            {t("org.powers")}
          </button>
          {agent.canDelegateToAgents && (
            <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "limit", id: agent.id })} title={t("org.modelsOf", { id: agent.id })}>
              {t("org.models")}
            </button>
          )}
          <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "add", under: agent.id })}>
            {t("org.addSubordinate")}
          </button>
          <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "remove", id: agent.id })}>
            {t("common.remove")}
          </button>
        </span>
      </div>
      {panel?.kind === "edit" && panel.id === agent.id && <RoleForm key={`edit-${agent.id}`} agent={agent} ui={ui} />}
      {panel?.kind === "remove" && panel.id === agent.id && <ConfirmRemoval node={node} ui={ui} />}
      {panel?.kind === "powers" && panel.id === agent.id && <PowersForm key={`powers-${agent.id}`} agent={agent} ui={ui} />}
      {panel?.kind === "limit" && panel.id === agent.id && <LimitForm key={`limit-${agent.id}`} agent={agent} ui={ui} />}
      {panel?.kind === "add" && panel.under === agent.id && <NewAgentForm key={`add-${agent.id}`} under={agent.id} ui={ui} />}
      {children.length > 0 && (
        <ul className="org-children">
          {children.map((child) => (
            <Node key={child.agent.id} node={child} ui={ui} />
          ))}
        </ul>
      )}
    </li>
  );
}

/** P120, P123 — who reports to whom among the agents, and where that is changed: give an agent a role, move it under another, add a
 * subordinate, remove one. Each change is written on its own (nothing else in the settings is touched) and asks for the hub's pairing key. The
 * model policies and each agent's model limit (P123) are also edited here, through the same narrow operation, without the whole form. */
export default function OrgView({ onOpenChat, onOpenTasks }: { onOpenChat: (id: string) => void; onOpenTasks: (id: string) => void }) {
  const [hub, setHub] = useState<HubAgents | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [panel, setPanel] = useState<Panel | null>(null);
  const [asking, setAsking] = useState<OrgEdit | null>(null);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState<string | null>(null);
  const [tasks, setTasks] = useState<AgentTask[]>([]);
  // What each agent spent and when it last had a conversation (P123): extras of the line, like the tasks.
  const [extras, setExtras] = useState<Pick<AgentActivityExtrasResponse, "usage" | "conversations">>({ usage: null, conversations: [] });

  function load() {
    chrome.runtime.sendMessage({ type: "listHubAgents" }).then((res: HubAgentsResponse) => {
      if (res.ok && res.hub) {
        setHub(res.hub);
        setError(null);
      } else {
        setError(res.error ?? t("org.readFailed"));
      }
    });
    // Each node's activity is an extra: without the tasks, the card just does not show the line.
    chrome.runtime.sendMessage({ type: "listAgentTasks" }).then((res: AgentTasksResponse) => {
      if (res.ok) setTasks(res.tasks);
    });
    chrome.runtime.sendMessage({ type: "listAgentActivityExtras" }).then((res: AgentActivityExtrasResponse) => {
      if (res.ok) setExtras({ usage: res.usage, conversations: res.conversations });
    });
  }

  useEffect(load, []);

  function cancelKey() {
    setAsking(null);
    setKeyError(null);
  }

  function confirm(pairingKey: string) {
    if (!asking) return;
    setBusy(true);
    setKeyError(null);
    chrome.runtime.sendMessage({ type: "editAgentOrg", pairingKey, edit: asking }).then((res: HubAgentsResponse) => {
      setBusy(false);
      if (res.ok && res.hub) {
        setHub(res.hub);
        setPanel(null);
        setError(null);
        cancelKey();
      } else if (res.authRejected) {
        setKeyError(t("pairing.wrong"));
      } else {
        cancelKey();
        setError(res.error ?? t("org.changeFailed"));
      }
    });
  }

  if (hub === null) {
    return <div className="agents-pane">{error ? <p className="error-banner">{error}</p> : <p className="skills-hint">{t("common.loading")}</p>}</div>;
  }

  const { agents, modelPolicies, modelIds } = hub;
  const ui: OrgUi = {
    onOpenChat,
    onOpenTasks,
    agents,
    candidates: delegationCandidates(modelIds, modelPolicies),
    modelIds,
    policies: modelPolicies,
    panel,
    busy: busy || asking !== null,
    open: (next) => {
      setError(null);
      setPanel(next);
    },
    apply: (edit) => {
      setError(null);
      setKeyError(null);
      setAsking(edit);
    },
    dragging,
    drag: setDragging,
    activity: (id) => {
      const activity = activityOf(tasks, id, { spend: spendOf(extras.usage, id), lastConversationMs: lastConversationOf(extras.conversations, id) });
      return activity ? activityLine(activity, Date.now()) : null;
    },
  };
  const topIsValid = dragging !== null && moveEdit(agents, dragging, null) !== null;
  const tree = buildOrg(agents);
  const nobodyReports = tree.every((node) => node.children.length === 0);

  return (
    <div className="agents-pane">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("org.intro")}</span>
        <button type="button" className="link-button" onClick={load} disabled={busy}>
          {t("org.refresh")}
        </button>
      </div>
      {error && <p className="error-banner">{error}</p>}
      {asking && (
        <PairingKeyForm busy={busy} error={keyError} onConfirm={confirm} onCancel={cancelKey}>
          {t("org.pairingHint")}
        </PairingKeyForm>
      )}
      {tree.length === 0 ? (
        <p className="skills-hint">{t("org.noAgents")}</p>
      ) : (
        <ul className="org-tree">
          {tree.map((node) => (
            <Node key={node.agent.id} node={node} ui={ui} />
          ))}
        </ul>
      )}
      {topIsValid && (
        <div
          className="org-top"
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault();
            onDrop(ui, null);
          }}
        >
          {t("org.dropHere")}
        </div>
      )}
      {tree.length > 0 && nobodyReports && <p className="skills-hint">{t("org.nobodyReports")}</p>}
      {panel?.kind === "add" && panel.under === null && <NewAgentForm key="add-top" under={null} ui={ui} />}
      <div className="skills-actions">
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "add", under: null })}>
          {t("org.addAtTop")}
        </button>
      </div>
      <h2 className="agents-subtitle">{t("org.policies")}</h2>
      {modelPolicies.length === 0 ? (
        <p className="skills-hint">{t("org.noPolicies")}</p>
      ) : (
        <ul className="skills-list">
          {modelPolicies.map((policy) => (
            <li key={policy.id} className="skills-item">
              <span className="skills-item-name">{policy.id}</span> <span className="skills-hint">→ {policy.model}</span>
              {policy.description && <p className="skills-item-description">{policy.description}</p>}
              <span className="org-actions">
                <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "policy", id: policy.id })}>
                  {t("common.edit")}
                </button>
                <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.apply(policiesEdit(policiesWithout(modelPolicies, policy.id)))}>
                  {t("common.remove")}
                </button>
              </span>
              {panel?.kind === "policy" && panel.id === policy.id && <PolicyForm key={`policy-${policy.id}`} original={policy} ui={ui} />}
            </li>
          ))}
        </ul>
      )}
      {panel?.kind === "policy" && panel.id === null && <PolicyForm key="policy-new" original={null} ui={ui} />}
      <div className="skills-actions">
        <button type="button" className="link-button" disabled={ui.busy || modelIds.length === 0} onClick={() => ui.open({ kind: "policy", id: null })}>
          {t("org.newPolicy")}
        </button>
      </div>
    </div>
  );
}

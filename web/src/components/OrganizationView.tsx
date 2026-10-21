import { useCallback, useEffect, useState } from "react";
import { SettingsError, type ServerConnection } from "../hub/connection";
import type { AgentSettings, AgentTask, ConversationSummary, UsageReport } from "../hub/messages";
import { activityLine, activityOf, lastConversationOf, spendOf } from "../hub/agentTasks";
import { approvalCategoryLabel } from "../hub/approvalCategories";
import { addReportEdit, buildOrg, moveEdit, positionEdit, superiorChoices, type OrgAccess, type OrgAgent, type OrgEdit, type OrgNode } from "../hub/org";
import { t, type Key } from "../i18n";
import { rich } from "../i18n/rich";

const AUTONOMY_LABELS: Record<number, Key> = { 1: "org.autonomy.1", 2: "org.autonomy.2", 3: "org.autonomy.3", 4: "org.autonomy.4", 5: "org.autonomy.5" };

/** The agent has the Settings powers (the owner receives them; a member only receives the id, the role and the superior). */
function hasPowers(agent: OrgAgent): agent is AgentSettings {
  return "autonomy" in agent;
}

/** What the agent can do, as the Settings screen defines it. */
function badges(agent: OrgAgent): string[] {
  if (!hasPowers(agent)) return [];
  const list: string[] = [];
  if (agent.canDelegateToAgents) list.push(t("org.badge.delegates"));
  if (agent.canManageAgents) list.push(t("org.badge.manages"));
  if (agent.canMessageAgents) list.push(t("org.badge.notes"));
  if (agent.canManageTasks) list.push(t("org.badge.schedules"));
  if (agent.autonomy !== 4) list.push(t("org.badge.autonomy", { level: agent.autonomy, what: AUTONOMY_LABELS[agent.autonomy] ? t(AUTONOMY_LABELS[agent.autonomy]) : "" }).trim());
  if (agent.approvalRequired.length > 0) list.push(t("org.badge.asksFirst", { what: agent.approvalRequired.map(approvalCategoryLabel).join(", ").toLowerCase() }));
  return list;
}

/** What is open in the tree: an agent's role, a new subordinate under an agent (`null`: at the top) or the removal of one. */
type Panel = { kind: "edit"; id: string } | { kind: "add"; under: string | null } | { kind: "remove"; id: string };

interface OrgUi {
  /** Opens a new conversation with this agent, or the tasks it received and delegated. */
  onOpenChat: (id: string) => void;
  onOpenTasks: (id: string) => void;
  agents: OrgAgent[];
  /** Whoever looks at the tree can change it: the owner always, a member only with `edit` access. */
  editable: boolean;
  /** A member: no conversation or tasks per node (the owner's agents are not always theirs) and no powers for each agent. */
  member: boolean;
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

function RoleForm({ agent, ui }: { agent: OrgAgent; ui: OrgUi }) {
  const [role, setRole] = useState(agent.role ?? "");
  const [superior, setSuperior] = useState(agent.reportsTo ?? "");
  const options = superiorChoices(ui.agents, agent.id);
  return (
    <div className="org-form">
      <label className="settings-field">
        {t("org.role")}
        <input type="text" value={role} maxLength={80} placeholder={t("org.rolePlaceholder")} onChange={(e) => setRole(e.target.value)} />
      </label>
      <label className="settings-field">
        {t("org.reportsTo")}
        <select value={superior} onChange={(e) => setSuperior(e.target.value)}>
          <option value="">{t("org.nobodyTop")}</option>
          {options.map((a) => (
            <option key={a.id} value={a.id}>
              {a.id}
            </option>
          ))}
        </select>
        <span className="field-hint">{t("org.reportsHint", { id: agent.id })}</span>
      </label>
      <div className="skills-actions">
        <button type="button" className="primary-button" disabled={ui.busy} onClick={() => ui.apply(positionEdit(agent.id, role, superior))}>
          {t("common.save")}
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
    <div className="org-form">
      <label className="settings-field">
        {t("org.name")}
        <input type="text" value={id} maxLength={64} placeholder={t("org.namePlaceholder")} onChange={(e) => setId(e.target.value)} />
      </label>
      <label className="settings-field">
        {t("org.role")}
        <input type="text" value={role} maxLength={80} placeholder={t("org.optional")} onChange={(e) => setRole(e.target.value)} />
      </label>
      <label className="settings-field">
        {t("org.whatItDoes")}
        <textarea value={persona} rows={3} placeholder={t("org.personaPlaceholder")} onChange={(e) => setPersona(e.target.value)} />
        <span className="field-hint">
          {under ? t("org.reportsToWho", { who: under }) : t("org.startsAtTop")}
          {t("org.startsCareful")}
        </span>
      </label>
      <div className="skills-actions">
        <button type="button" className="primary-button" disabled={ui.busy || !ready} onClick={() => ui.apply(addReportEdit(id, persona, role, under))}>
          {t("org.addAgent")}
        </button>
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

function ConfirmRemoval({ node, ui }: { node: OrgNode<OrgAgent>; ui: OrgUi }) {
  const { agent, children } = node;
  return (
    <div className="org-form">
      <p className="skills-hint">
        {rich("org.removeQuestion", { id: <strong>{agent.id}</strong> })}
        {children.length > 0 &&
          ` ${t(children.length === 1 ? "org.reportOne" : "org.reportMany", { count: children.length })}${t("org.reportTo", { who: agent.reportsTo ?? t("org.nobodyTopParen") })}`}
        {t("org.conversationsStay")}
      </p>
      <div className="skills-actions">
        <button type="button" className="primary-button" disabled={ui.busy} onClick={() => ui.apply({ kind: "remove", id: agent.id })}>
          {t("common.remove")}
        </button>
        <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open(null)}>
          {t("common.keep")}
        </button>
      </div>
    </div>
  );
}

function Node({ node, ui }: { node: OrgNode<OrgAgent>; ui: OrgUi }) {
  const { agent, children } = node;
  const { panel } = ui;
  const validTarget = ui.dragging !== null && moveEdit(ui.agents, ui.dragging, agent.id) !== null;
  return (
    <li className="org-node">
      {/* Dragging lives on the card, not on the <li>: the subordinates' `dragover` must not bubble up to the parent's card. */}
      <div
        className={`org-card${ui.dragging === agent.id ? " org-card--dragging" : ""}${validTarget ? " org-card--target" : ""}`}
        draggable={ui.editable && !ui.busy}
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
        {children.length > 0 && <span className="org-count">{children.length === 1 ? t("org.oneSubordinate") : t("org.nSubordinates", { count: children.length })}</span>}
        {ui.activity(agent.id) && <span className="org-activity">{ui.activity(agent.id)}</span>}
        <span className="org-actions">
          {!ui.member && (
            <>
              <button type="button" className="link-button" onClick={() => ui.onOpenChat(agent.id)} title={t("org.startChat", { id: agent.id })}>
                {t("org.chat")}
              </button>
              <button type="button" className="link-button" onClick={() => ui.onOpenTasks(agent.id)} title={t("org.tasksOf", { id: agent.id })}>
                {t("tab.tasks")}
              </button>
            </>
          )}
          {ui.editable && (
            <>
              <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "edit", id: agent.id })}>
                {t("common.edit")}
              </button>
              <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "add", under: agent.id })}>
                {t("org.addSubordinate")}
              </button>
              <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "remove", id: agent.id })}>
                {t("common.remove")}
              </button>
            </>
          )}
        </span>
      </div>
      {panel?.kind === "edit" && panel.id === agent.id && <RoleForm key={`edit-${agent.id}`} agent={agent} ui={ui} />}
      {panel?.kind === "remove" && panel.id === agent.id && <ConfirmRemoval node={node} ui={ui} />}
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

/** P120 — who reports to whom among the agents, and where that is changed: give an agent a role, move it under another, add a
 * subordinate, remove one. Each change is written on its own (nothing else in the settings is touched) and asks for the hub's pairing key;
 * whoever manages (`manage_agents`) or delegates (`delegate_to_agent`) reaches only those below them, so the tree defines the reach. */
export default function OrganizationView({
  conn,
  onEdit,
  onOpenChat,
  onOpenTasks,
  memberAccess,
}: {
  conn: ServerConnection | null;
  onEdit: () => void;
  onOpenChat: (id: string) => void;
  onOpenTasks: (id: string) => void;
  /** A member (P120): the access the owner gave, `view` or `edit`. Absent for the owner. The member reads and edits through their own session,
   * with no pairing key, and only receives the id, the role and the superior of each agent. */
  memberAccess?: OrgAccess;
}) {
  const member = memberAccess !== undefined;
  const editable = memberAccess === undefined || memberAccess === "edit";
  const [agents, setAgents] = useState<OrgAgent[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [panel, setPanel] = useState<Panel | null>(null);
  const [asking, setAsking] = useState<OrgEdit | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState<string | null>(null);
  const [tasks, setTasks] = useState<AgentTask[]>([]);
  // Owner only: each agent's spend and the conversations it has had (P123). A member neither asks for nor sees them.
  const [usage, setUsage] = useState<UsageReport | null>(null);
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      if (member) {
        setAgents((await conn.listAgentOrg()).agents);
      } else {
        const loaded = await conn.requestSettings();
        setAgents(loaded.settings.agents);
      }
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
    if (member) return;
    // Each node's activity is an extra: without the tasks, the card just does not show the line.
    try {
      setTasks(await conn.listAgentTasks());
    } catch {
      /* ignored */
    }
    try {
      setUsage(await conn.requestUsage());
    } catch {
      /* ignored */
    }
    try {
      setConversations(await conn.listConversations());
    } catch {
      /* ignored */
    }
  }, [conn, member]);

  useEffect(() => {
    void load();
  }, [load]);

  function cancelKey() {
    setAsking(null);
    setPairingKey("");
    setKeyError(null);
  }

  /** A member with `edit` access: their session is the authorization, so the change goes straight through, with no key prompt. */
  async function applyAsMember(edit: OrgEdit) {
    if (!conn) return;
    setBusy(true);
    setError(null);
    try {
      setAgents((await conn.editAgentOrgAsMember(edit)).agents);
      setPanel(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function confirm() {
    if (!conn || !asking) return;
    setBusy(true);
    setKeyError(null);
    try {
      setAgents(await conn.editAgentOrg(pairingKey, asking));
      setPanel(null);
      setError(null);
      cancelKey();
    } catch (err) {
      if (err instanceof SettingsError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        cancelKey();
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      setBusy(false);
    }
  }

  if (agents === null) {
    return <div className="usage-view">{error ? <p className="error-banner">{error}</p> : <p className="skills-hint">{t("common.loading")}</p>}</div>;
  }

  const ui: OrgUi = {
    onOpenChat,
    onOpenTasks,
    agents,
    editable,
    member,
    panel,
    busy: busy || asking !== null,
    open: (next) => {
      setError(null);
      setPanel(next);
    },
    apply: (edit) => {
      setError(null);
      setKeyError(null);
      if (member) {
        void applyAsMember(edit);
      } else {
        setAsking(edit);
      }
    },
    dragging,
    drag: setDragging,
    activity: (id) => {
      const activity = activityOf(tasks, id, { spend: spendOf(usage, id), lastConversationMs: lastConversationOf(conversations, id) });
      return activity ? activityLine(activity, Date.now()) : null;
    },
  };
  const topIsValid = dragging !== null && moveEdit(agents, dragging, null) !== null;
  const tree = buildOrg(agents);
  const nobodyReports = tree.every((node) => node.children.length === 0);

  const keyPrompt = asking && (
    <form
      className="settings-confirm devices-confirm"
      onSubmit={(e) => {
        e.preventDefault();
        void confirm();
      }}
    >
      <label className="settings-field">
        {t("pairingKey.label")}
        <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
        <span className="field-hint">{t("org.pairingHint")}</span>
      </label>
      {keyError && <p className="error-banner">{keyError}</p>}
      <div className="skills-actions">
        <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
          {busy ? t("common.wait") : t("common.confirm")}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={cancelKey}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );

  return (
    <div className="usage-view">
      <div className="skills-toolbar">
        <span className="skills-hint">{member ? (editable ? t("org.introMemberEdit") : t("org.introMemberView")) : t("org.introOwner")}</span>
        <button type="button" className="link-button" onClick={() => void load()} disabled={!conn || busy}>
          {t("common.refresh")}
        </button>
      </div>
      {error && <p className="error-banner">{error}</p>}
      {keyPrompt}
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
      {tree.length > 0 && nobodyReports && !ui.editable && <p className="skills-hint">{t("org.nobodyReports")}</p>}
      {tree.length > 0 && nobodyReports && ui.editable && <p className="skills-hint">{t("org.nobodyReportsEdit")}</p>}
      {panel?.kind === "add" && panel.under === null && <NewAgentForm key="add-top" under={null} ui={ui} />}
      {editable && (
        <div className="skills-actions">
          <button type="button" className="link-button" disabled={ui.busy} onClick={() => ui.open({ kind: "add", under: null })}>
            {t("org.addAtTop")}
          </button>
          {!member && (
            <button type="button" className="link-button" onClick={onEdit}>
              {t("org.moreSettings")}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

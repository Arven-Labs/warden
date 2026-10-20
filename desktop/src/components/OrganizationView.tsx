import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t, type Key } from "../i18n";
import { rich } from "../i18n/rich";
import type { AgentEntry, AgentTask, SpendStatus } from "../types";
import { activityLine, activityOf, lastConversationOf, spendOf } from "../lib/agentTasks";
import { approvalCategoryLabel } from "../lib/approvalCategories";
import { hubAgentTasks, hubEditAgentOrg, hubEditAgentOrgAsMember, hubListAgentOrg, hubListConversations, hubUsage } from "../lib/hub";
import { addReportEdit, buildOrg, moveEdit, positionEdit, superiorChoices, type OrgAccess, type OrgAgent, type OrgEdit, type OrgNode } from "../lib/org";
import { KeyCancelled, usePairingKey } from "./PairingKeyDialog";

const autonomyLabel = (level: number): string => (level >= 1 && level <= 5 ? t(`org.autonomy${level}` as Key) : "");

/** The agent has the powers of the Settings screen (the owner gets them; a member gets only the id, the role and the superior). */
function hasPowers(agent: OrgAgent): agent is AgentEntry {
  return "autonomy" in agent;
}

/** The small facts shown next to an agent: what it is allowed to do, as the Settings screen sets it. */
function badges(agent: OrgAgent): string[] {
  const list: string[] = [];
  if (!hasPowers(agent)) return list;
  if (agent.canDelegateToAgents) list.push(t("org.badgeDelegates"));
  if (agent.canManageAgents) list.push(t("org.badgeManages"));
  if (agent.canMessageAgents) list.push(t("org.badgeNotes"));
  if (agent.canManageTasks) list.push(t("org.badgeTasks"));
  if (agent.autonomy !== 4) list.push(t("org.badgeAutonomy", { level: agent.autonomy, label: autonomyLabel(agent.autonomy) }).trim());
  if (agent.approvalRequired.length > 0) list.push(t("org.badgeAsksBefore", { list: agent.approvalRequired.map(approvalCategoryLabel).join(", ").toLowerCase() }));
  return list;
}

/** What is open on the tree: the position of an agent, a new report under an agent (`null`: at the top), or the removal of one. */
type Panel = { kind: "edit"; id: string } | { kind: "add"; under: string | null } | { kind: "remove"; id: string };

interface Editing {
  /** Opens a new conversation with this agent, or the tasks it was given and delegated. */
  onOpenChat: (id: string) => void;
  onOpenTasks: (id: string) => void;
  agents: OrgAgent[];
  /** A member (P120): no chat or tasks per node (the owner's agents are not always theirs) and no powers to show. */
  member: boolean;
  /** Who looks at the tree may change it: the owner always, a member only with the `edit` access. */
  editable: boolean;
  panel: Panel | null;
  busy: boolean;
  open: (panel: Panel | null) => void;
  apply: (edit: OrgEdit) => Promise<void>;
  /** The agent whose card is being dragged (P120): another card, or the top, takes the drop when the change is valid. */
  dragging: string | null;
  drag: (id: string | null) => void;
  /** What the agent has been up to, on one line, taken from the hub's tasks; `null` with no task of its own. */
  activity: (id: string) => string | null;
}

/** Dropping the dragged card on `target` (`null`: the top): the change, if there is one. A card dropped under one of its own reports would close a circle. */
function onDropOn(editing: Editing, target: string | null): void {
  const edit = editing.dragging === null ? null : moveEdit(editing.agents, editing.dragging, target);
  editing.drag(null);
  if (edit) void editing.apply(edit);
}

function PositionForm({ agent, editing }: { agent: OrgAgent; editing: Editing }) {
  const [role, setRole] = useState(agent.role ?? "");
  const [superior, setSuperior] = useState(agent.reportsTo ?? "");
  const choices = superiorChoices(editing.agents, agent.id);
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
          {choices.map((a) => (
            <option key={a.id} value={a.id}>
              {a.id}
            </option>
          ))}
        </select>
        <span className="field-hint">{t("org.movesWith", { id: agent.id })}</span>
      </label>
      <div className="org-form-actions">
        <button type="button" className="settings-save-btn" disabled={editing.busy} onClick={() => void editing.apply(positionEdit(agent.id, role, superior))}>
          {t("org.save")}
        </button>
        <button type="button" className="settings-browse-btn" disabled={editing.busy} onClick={() => editing.open(null)}>
          {t("org.cancel")}
        </button>
      </div>
    </div>
  );
}

function AddForm({ under, editing }: { under: string | null; editing: Editing }) {
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
          {under ? t("org.reportsUnder", { under }) : t("org.startsTop")}
          {editing.member ? t("org.carefulMember") : t("org.carefulOwner")}
        </span>
      </label>
      <div className="org-form-actions">
        <button type="button" className="settings-save-btn" disabled={editing.busy || !ready} onClick={() => void editing.apply(addReportEdit(id, persona, role, under))}>
          {t("org.addAgent")}
        </button>
        <button type="button" className="settings-browse-btn" disabled={editing.busy} onClick={() => editing.open(null)}>
          {t("org.cancel")}
        </button>
      </div>
    </div>
  );
}

function RemoveConfirm({ node, editing }: { node: OrgNode<OrgAgent>; editing: Editing }) {
  const { agent, children } = node;
  const superior = agent.reportsTo;
  return (
    <div className="org-form">
      <p className="settings-hint">
        {rich("org.removeAsk", { id: <strong>{agent.id}</strong> })}
        {children.length > 0 &&
          (children.length === 1
            ? t("org.removeOneReport", { superior: superior ?? t("org.nobodyTopLower") })
            : t("org.removeManyReports", { count: children.length, superior: superior ?? t("org.nobodyTopLower") }))}
        {t("org.removeNote")}
      </p>
      <div className="org-form-actions">
        <button type="button" className="provider-delete-btn" disabled={editing.busy} onClick={() => void editing.apply({ kind: "remove", id: agent.id })}>
          {t("org.remove")}
        </button>
        <button type="button" className="settings-browse-btn" disabled={editing.busy} onClick={() => editing.open(null)}>
          {t("org.keepIt")}
        </button>
      </div>
    </div>
  );
}

function Node({ node, editing }: { node: OrgNode<OrgAgent>; editing: Editing }) {
  const { agent, children } = node;
  const { panel } = editing;
  const validTarget = editing.dragging !== null && moveEdit(editing.agents, editing.dragging, agent.id) !== null;
  return (
    <li className="org-node">
      {/* The drag sits on the card, not on the <li>: the `dragover` of the reports must not bubble up to their superior's card. */}
      <div
        className={`org-card${editing.dragging === agent.id ? " org-card--dragging" : ""}${validTarget ? " org-card--target" : ""}`}
        draggable={editing.editable && !editing.busy}
        onDragStart={(e) => {
          e.dataTransfer.setData("text/plain", agent.id);
          e.dataTransfer.effectAllowed = "move";
          editing.drag(agent.id);
        }}
        onDragEnd={() => editing.drag(null)}
        onDragOver={(e) => {
          if (validTarget) e.preventDefault();
        }}
        onDrop={(e) => {
          e.preventDefault();
          onDropOn(editing, agent.id);
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
        {children.length > 0 && <span className="org-count">{children.length === 1 ? t("org.oneReport") : t("org.reports", { count: children.length })}</span>}
        {editing.activity(agent.id) && <span className="org-activity">{editing.activity(agent.id)}</span>}
        <span className="org-actions">
          {!editing.member && (
            <>
              <button type="button" className="settings-browse-btn" onClick={() => editing.onOpenChat(agent.id)} title={t("org.chatTitle", { id: agent.id })}>
                {t("org.chat")}
              </button>
              <button type="button" className="settings-browse-btn" onClick={() => editing.onOpenTasks(agent.id)} title={t("org.tasksTitle", { id: agent.id })}>
                {t("org.tasks")}
              </button>
            </>
          )}
          {editing.editable && (
            <>
              <button type="button" className="settings-browse-btn" disabled={editing.busy} onClick={() => editing.open({ kind: "edit", id: agent.id })}>
                {t("org.edit")}
              </button>
              <button type="button" className="settings-browse-btn" disabled={editing.busy} onClick={() => editing.open({ kind: "add", under: agent.id })}>
                {t("org.addReport")}
              </button>
              <button type="button" className="settings-browse-btn" disabled={editing.busy} onClick={() => editing.open({ kind: "remove", id: agent.id })}>
                {t("org.remove")}
              </button>
            </>
          )}
        </span>
      </div>
      {panel?.kind === "edit" && panel.id === agent.id && <PositionForm key={`edit-${agent.id}`} agent={agent} editing={editing} />}
      {panel?.kind === "remove" && panel.id === agent.id && <RemoveConfirm node={node} editing={editing} />}
      {panel?.kind === "add" && panel.under === agent.id && <AddForm key={`add-${agent.id}`} under={agent.id} editing={editing} />}
      {children.length > 0 && (
        <ul className="org-children">
          {children.map((child) => (
            <Node key={child.agent.id} node={child} editing={editing} />
          ))}
        </ul>
      )}
    </li>
  );
}

/** P120 — who reports to whom, and the place to change it: give an agent a role, move it under another, add a report, remove one.
 * Each change is written on its own (nothing else of the settings is touched) and an agent that manages or delegates reaches only the
 * ones below it, so the tree is what sets its reach. On a hub it asks for the pairing key, like any change to it. */
function OrganizationView({
  agents: ownerAgents,
  remote = false,
  memberAccess,
  onChanged,
  onEdit,
  onOpenChat,
  onOpenTasks,
}: {
  agents: AgentEntry[];
  remote?: boolean;
  /** A member of the hub (P120): the access the owner gave, `view` or `edit`. Absent for the owner and on this computer. The member reads
   * and edits by their session, with no pairing key, and gets only the id, the role and the superior of each agent. */
  memberAccess?: OrgAccess;
  onChanged: () => void;
  onEdit: () => void;
  onOpenChat: (id: string) => void;
  onOpenTasks: (id: string) => void;
}) {
  const member = memberAccess !== undefined;
  const editable = memberAccess === undefined || memberAccess === "edit";
  const [memberAgents, setMemberAgents] = useState<OrgAgent[]>([]);
  const agents: OrgAgent[] = member ? memberAgents : ownerAgents;
  const tree = buildOrg(agents);
  const nobodyReports = tree.every((node) => node.children.length === 0);
  const [panel, setPanel] = useState<Panel | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [tasks, setTasks] = useState<AgentTask[]>([]);
  // The owner's only (P123): what each agent spent and the conversations it has had. A member asks for neither and sees neither.
  const [spend, setSpend] = useState<SpendStatus | null>(null);
  const [conversations, setConversations] = useState<{ agentId?: string; updatedAt: number; parent?: unknown }[]>([]);
  const { askKey, dialog } = usePairingKey();

  // A member's tree is the owner's, read from the hub as the access they were given allows.
  useEffect(() => {
    if (!member) return;
    let alive = true;
    hubListAgentOrg().then(
      (loaded) => alive && (setMemberAgents(loaded.agents), setError(null)),
      (err) => alive && setError(String(err instanceof Error ? err.message : err)),
    );
    return () => {
      alive = false;
    };
  }, [member]);

  // The activity of each node is an extra: without the tasks, the card just shows no line.
  useEffect(() => {
    if (member) return;
    let alive = true;
    (remote ? hubAgentTasks() : invoke<AgentTask[]>("list_agent_tasks")).then(
      (loaded) => alive && setTasks(loaded),
      () => {},
    );
    return () => {
      alive = false;
    };
  }, [remote, member, ownerAgents]);

  // Also extras of the line: without them it just has less to say.
  useEffect(() => {
    if (member) return;
    let alive = true;
    (remote ? hubUsage().spend() : invoke<SpendStatus>("spend_status")).then(
      (loaded) => alive && setSpend(loaded),
      () => {},
    );
    (remote ? hubListConversations() : invoke<{ agentId?: string; updatedAt: number; parent?: unknown }[]>("list_conversations")).then(
      (loaded) => alive && setConversations(loaded),
      () => {},
    );
    return () => {
      alive = false;
    };
  }, [remote, member, ownerAgents]);

  const editing: Editing = {
    dragging,
    drag: setDragging,
    activity: (id) => {
      const activity = activityOf(tasks, id, { spend: spendOf(spend, id), lastConversationMs: lastConversationOf(conversations, id) });
      return activity ? activityLine(activity, Date.now()) : null;
    },
    onOpenChat,
    onOpenTasks,
    agents,
    member,
    editable,
    panel,
    busy,
    open: (next) => {
      setError(null);
      setPanel(next);
    },
    apply: async (edit) => {
      setBusy(true);
      setError(null);
      try {
        if (member) setMemberAgents((await hubEditAgentOrgAsMember(edit)).agents);
        else if (remote) await hubEditAgentOrg(askKey, edit);
        else await invoke("edit_agent_org", { edit });
        setPanel(null);
        if (!member) onChanged();
      } catch (err) {
        if (!(err instanceof KeyCancelled)) setError(String(err));
      } finally {
        setBusy(false);
      }
    },
  };

  return (
    <div className="settings-view">
      <h2 className="settings-title">{t("org.title")}</h2>
      <p className="settings-hint">{member ? (editable ? t("org.hintMemberEdit") : t("org.hintMemberView")) : t("org.hintOwner")}</p>
      {error && <p className="usage-error">{error}</p>}

      {tree.length === 0 ? (
        <p className="settings-hint">{t("org.noAgents")}</p>
      ) : (
        <ul className="org-tree">
          {tree.map((node) => (
            <Node key={node.agent.id} node={node} editing={editing} />
          ))}
        </ul>
      )}
      {editable && dragging !== null && moveEdit(agents, dragging, null) !== null && (
        <div
          className="org-top"
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault();
            onDropOn(editing, null);
          }}
        >
          {t("org.dropHere")}
        </div>
      )}
      {editable && tree.length > 0 && nobodyReports && <p className="settings-hint">{t("org.nobodyReports")}</p>}
      {panel?.kind === "add" && panel.under === null && <AddForm key="add-top" under={null} editing={editing} />}
      {editable && (
        <div className="org-form-actions">
          <button type="button" className="settings-browse-btn" disabled={busy} onClick={() => editing.open({ kind: "add", under: null })}>
            {t("org.addTop")}
          </button>
          {!member && (
            <button type="button" className="settings-browse-btn" onClick={onEdit}>
              {t("org.moreSettings")}
            </button>
          )}
        </div>
      )}
      {dialog}
    </div>
  );
}

export default OrganizationView;

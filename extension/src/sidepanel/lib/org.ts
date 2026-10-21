import { t, type Key } from "../../i18n/index.ts";
import type { OrgEdit } from "../../protocol/messages";

/** P120 — the agents' organization: each can have a role and a superior (`reportsTo`, another agent's id).
 * For now it only shows: it changes nothing about what an agent can do. Mirrors `warden_bootstrap::org`. */
export interface OrgAgent {
  id: string;
  role?: string | null;
  reportsTo?: string | null;
}

export interface OrgNode<T extends OrgAgent = OrgAgent> {
  agent: T;
  children: OrgNode<T>[];
}

/** The agents as a forest: those with no superior sit at the top, in the order they came, with their subordinates
 * below. One whose superior is gone shows at the top instead of getting lost, and a cycle (which the hub refuses) never hangs the tree. */
export function buildOrg<T extends OrgAgent>(agents: T[]): OrgNode<T>[] {
  const ids = new Set(agents.map((a) => a.id));
  const seen = new Set<string>();
  const nodeOf = (agent: T): OrgNode<T> => {
    seen.add(agent.id);
    const children: OrgNode<T>[] = [];
    for (const report of agents) {
      if (report.reportsTo === agent.id && !seen.has(report.id)) children.push(nodeOf(report));
    }
    return { agent, children };
  };
  return agents.filter((a) => !a.reportsTo || !ids.has(a.reportsTo)).map(nodeOf);
}

/** Everyone below `id`, at any level: the ones it cannot start reporting to. */
export function descendantsOf(agents: OrgAgent[], id: string): Set<string> {
  const below = new Set<string>();
  const queue = [id];
  while (queue.length > 0) {
    const current = queue.pop()!;
    for (const a of agents) {
      if (a.reportsTo === current && !below.has(a.id)) {
        below.add(a.id);
        queue.push(a.id);
      }
    }
  }
  return below;
}

/** Who `id` can start reporting to: everyone except itself and those below it (that would close a cycle). */
export function superiorChoices<T extends OrgAgent>(agents: T[], id: string): T[] {
  const below = descendantsOf(agents, id);
  return agents.filter((a) => a.id !== id && !below.has(a.id));
}

// A change made through the tree, as the hub receives it (`AgentOrgEdit`): the type lives with the protocol, in `protocol/messages.ts`.
export type { OrgEdit };

/** A blank field is no value: the change leaves it out instead of sending an empty string. */
export function positionEdit(id: string, role: string, reportsTo: string): OrgEdit {
  return { kind: "setPosition", id, ...(role.trim() ? { role: role.trim() } : {}), ...(reportsTo ? { reportsTo } : {}) };
}

/** P122 — the kinds of action an agent can be required to have approved even when it acts alone. The ids are those of
 * `warden_core::autonomy::Category`, as the hub and `approvalRequired` write them (the same list as the web's). */
const category = (id: string, key: Key): { id: string; label: string } => ({
  id,
  get label() {
    return t(key);
  },
});

export const APPROVAL_CATEGORIES: { id: string; label: string }[] = [
  category("delete_data", "approvalCategory.delete_data"),
  category("spend_money", "approvalCategory.spend_money"),
  category("critical_infra", "approvalCategory.critical_infra"),
  category("external_message", "approvalCategory.external_message"),
  category("publish_code", "approvalCategory.publish_code"),
  category("important_config", "approvalCategory.important_config"),
  category("elevated_agent", "approvalCategory.elevated_agent"),
];

/** A category's name, or the id itself when a newer hub sends one this screen does not know. */
export function approvalCategoryLabel(id: string): string {
  return APPROVAL_CATEGORIES.find((c) => c.id === id)?.label ?? id;
}

/** What each autonomy level means, in one line (P122). */
const autonomyLevel = (level: number, key: Key): { level: number; label: string } => ({
  level,
  get label() {
    return t(key);
  },
});

export const AUTONOMY_LEVELS: { level: number; label: string }[] = [
  autonomyLevel(1, "autonomyLevel.1"),
  autonomyLevel(2, "autonomyLevel.2"),
  autonomyLevel(3, "autonomyLevel.3"),
  autonomyLevel(4, "autonomyLevel.4"),
  autonomyLevel(5, "autonomyLevel.5"),
];

export interface Powers {
  autonomy: number;
  canStartTasks: boolean;
  canCreateWorkers: boolean;
  canMessageUser: boolean;
  canChooseModels: boolean;
  /** The checked ids, in any order. */
  approvalRequired: Iterable<string>;
}

/** P122 — the powers change for `id`. The categories come out in the list's fixed order (the checked ones the screen knows, and those only the hub
 * knows after them), with no repeats, so the same set always gives the same change. */
export function powersEdit(id: string, powers: Powers): OrgEdit {
  const marked = new Set(powers.approvalRequired);
  const known = APPROVAL_CATEGORIES.map((c) => c.id).filter((c) => marked.has(c));
  const unknown = [...marked].filter((c) => !APPROVAL_CATEGORIES.some((k) => k.id === c));
  return {
    kind: "setPowers",
    id,
    autonomy: powers.autonomy,
    canStartTasks: powers.canStartTasks,
    canCreateWorkers: powers.canCreateWorkers,
    canMessageUser: powers.canMessageUser,
    canChooseModels: powers.canChooseModels,
    approvalRequired: [...known, ...unknown],
  };
}

export function addReportEdit(id: string, persona: string, role: string, reportsTo: string | null): OrgEdit {
  return { kind: "addReport", id: id.trim(), persona: persona.trim(), ...(role.trim() ? { role: role.trim() } : {}), ...(reportsTo ? { reportsTo } : {}) };
}

/** Dropping the card of `draggedId` onto that of `targetId` (or `null`: the top of the tree): the position change the hub receives, with the role
 * it already has. `null` when there is nothing to change: the agent does not exist, the target is itself or someone below it (that would close a
 * cycle, which the hub also refuses) or the current superior already is the target. */
export function moveEdit(agents: OrgAgent[], draggedId: string, targetId: string | null): OrgEdit | null {
  const dragged = agents.find((a) => a.id === draggedId);
  if (!dragged) return null;
  if (targetId !== null && !superiorChoices(agents, draggedId).some((a) => a.id === targetId)) return null;
  if ((dragged.reportsTo ?? null) === targetId) return null;
  return positionEdit(draggedId, dragged.role ?? "", targetId ?? "");
}

/** An agent was renamed: whoever reported to it follows the new name. */
export function renameInReports<T extends OrgAgent>(agents: T[], from: string, to: string): T[] {
  if (from === to) return agents;
  return agents.map((a) => (a.reportsTo === from ? { ...a, reportsTo: to } : a));
}

/** `removed` is leaving: whoever reported to it now reports to its superior (or to nobody). The list comes back without it. */
export function removeFromOrg<T extends OrgAgent>(agents: T[], removed: string): T[] {
  const superior = agents.find((a) => a.id === removed)?.reportsTo ?? null;
  return agents.filter((a) => a.id !== removed).map((a) => (a.reportsTo === removed ? { ...a, reportsTo: superior } : a));
}

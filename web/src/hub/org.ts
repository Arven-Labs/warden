import { t } from "../i18n/index.ts";

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

/** A change made through the tree, as the hub receives it (`AgentOrgEdit`). An absent `role` or `reportsTo` means none. */
export type OrgEdit =
  | { kind: "setPosition"; id: string; role?: string; reportsTo?: string }
  | { kind: "addReport"; id: string; persona: string; role?: string; reportsTo?: string }
  | { kind: "remove"; id: string }
  /** P140 — adds an agent of the gallery (careful by default) and the skills it works with; one of that id that differs is only replaced with `overwrite`. The owner's only. */
  | { kind: "installCatalogAgent"; id: string; overwrite: boolean }
  /** P123 — what the light clients (phone and extension) send; the web edits the limit and the policies through the Settings form. */
  | { kind: "setDelegationModels"; id: string; models: string[] }
  | { kind: "setModelPolicies"; policies: Array<{ id: string; model: string; description: string }> };

/** Installing the agent `id` of the gallery. */
export function installAgentEdit(id: string, overwrite: boolean): OrgEdit {
  return { kind: "installCatalogAgent", id, overwrite };
}

/** What a gallery row offers, from the entry's state and whether the person already asked to replace theirs: `installed` (nothing to do),
 * `install` (one tap), `conflict` (an agent of that id that differs: say so and offer to use the gallery's) or `confirm` (asked once, so
 * replacing takes a second, explicit tap). */
export type CatalogAgentAction = "installed" | "install" | "conflict" | "confirm";

export function catalogAgentAction(status: "notInstalled" | "installed" | "modified", confirming: boolean): CatalogAgentAction {
  if (status === "installed") return "installed";
  if (status === "modified") return confirming ? "confirm" : "conflict";
  return "install";
}

/** A blank field is no value: the change leaves it out instead of sending an empty string. */
export function positionEdit(id: string, role: string, reportsTo: string): OrgEdit {
  return { kind: "setPosition", id, ...(role.trim() ? { role: role.trim() } : {}), ...(reportsTo ? { reportsTo } : {}) };
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

/** What a member can do with the agents' organization (P120): the owner chooses per member. There is only one tree, the owner's. */
export type OrgAccess = "none" | "view" | "edit";

/** The access the hub sent (the member's `orgAccess`, absent when it is none); a value we do not know counts as none, so as not to
 * show a screen the hub will refuse. */
export function orgAccessOf(value: string | null | undefined): OrgAccess {
  return value === "view" || value === "edit" ? value : "none";
}

/** The member with the access the hub just reported (the hub's notice, or the answer to a query). The same object when nothing changes, so as
 * not to render again; `none` drops the field, like the hub, which sends nothing to someone with no access. */
export function userWithOrgAccess<T extends { orgAccess?: string }>(user: T, access: string): T {
  const next = orgAccessOf(access);
  if (orgAccessOf(user.orgAccess) === next) return user;
  const { orgAccess: _before, ...rest } = user;
  return (next === "none" ? rest : { ...rest, orgAccess: next }) as T;
}

/** The three levels, in the order the owner picks them (getters, so each read has the current language). */
export const ORG_ACCESS_CHOICES: Array<{ value: OrgAccess; label: string; hint: string }> = [
  {
    value: "none",
    get label() {
      return t("orgAccess.none");
    },
    get hint() {
      return t("orgAccess.noneHint");
    },
  },
  {
    value: "view",
    get label() {
      return t("orgAccess.view");
    },
    get hint() {
      return t("orgAccess.viewHint");
    },
  },
  {
    value: "edit",
    get label() {
      return t("orgAccess.edit");
    },
    get hint() {
      return t("orgAccess.editHint");
    },
  },
];

/** The short phrase on the member's card. */
export function orgAccessLabel(access: OrgAccess): string {
  return t(access === "edit" ? "orgAccess.cardEdit" : access === "view" ? "orgAccess.cardView" : "orgAccess.cardNone");
}

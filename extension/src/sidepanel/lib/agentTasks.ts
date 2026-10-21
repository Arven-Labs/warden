// P123 — the work the agents hand to each other, as the "Agent work" tab shows it: the tasks one turn
// started form a group, with a progress count over them. Mirrors `desktop/src/lib/agentTasks.ts`.

import { intlTag, t } from "../../i18n/index.ts";
import type { AgentTask, AgentTaskAction, AgentTaskState } from "../../protocol/messages";

/** A task as the list shows it: how many levels below the turn's agent it sits (0 for the ones the agent itself started). */
export interface TaskRow {
  task: AgentTask;
  depth: number;
}

export interface TaskGroup {
  group: string;
  /** The agent that delegated, if any. */
  owner: string | null;
  /** Oldest to newest, in the order the turn started them. */
  tasks: AgentTask[];
  /** The same tasks as a flattened tree for display: each one followed by its subtasks. */
  rows: TaskRow[];
  total: number;
  /** Done, failed or cancelled: nothing left to wait for. */
  finished: number;
  counts: Record<AgentTaskState, number>;
  /** Whole percentage of the tasks that finished. */
  percent: number;
  /** Tokens used by the tasks that reported them. */
  totalTokens: number;
  /** Something in the group is still pending or running. */
  active: boolean;
  createdAtMs: number;
}

// Getters, so each read says the words of the language the person has now.
export const STATE_LABEL: Record<AgentTaskState, string> = {
  get pending() {
    return t("taskState.pending");
  },
  get running() {
    return t("taskState.running");
  },
  get waiting() {
    return t("taskState.waiting");
  },
  get paused() {
    return t("taskState.paused");
  },
  get done() {
    return t("taskState.done");
  },
  get failed() {
    return t("taskState.failed");
  },
  get cancelled() {
    return t("taskState.cancelled");
  },
};

export const STATE_MARK: Record<AgentTaskState, string> = {
  pending: "○",
  running: "◐",
  waiting: "◉",
  paused: "⏸",
  done: "✓",
  failed: "⚠",
  cancelled: "⏹",
};

const emptyCounts = (): Record<AgentTaskState, number> => ({ pending: 0, running: 0, waiting: 0, paused: 0, done: 0, failed: 0, cancelled: 0 });

/** What can be done to this task now: only one that runs in the process of the hub that answered. One that has not started can only be stopped;
 * a paused one can be resumed; a delegation the agent is waiting on can also only be stopped. */
export function actionsFor(task: AgentTask): AgentTaskAction[] {
  if (!task.controllable) return [];
  switch (task.state) {
    case "pending":
      return ["cancel"];
    case "running":
    case "waiting":
      return task.pausable ? ["pause", "cancel"] : ["cancel"];
    case "paused":
      return ["resume", "cancel"];
    default:
      return [];
  }
}

export const ACTION_LABEL: Record<AgentTaskAction, string> = {
  get pause() {
    return t("taskAction.pause");
  },
  get resume() {
    return t("taskAction.resume");
  },
  get cancel() {
    return t("taskAction.cancel");
  },
};

/** A group's tasks as a tree: each task followed by its subtasks, one level down. A task whose parent is not in the group
 * (or a cycle that should not exist) shows at the top instead of getting lost. */
function treeRows(ordered: AgentTask[]): TaskRow[] {
  const ids = new Set(ordered.map((t) => t.id));
  const rows: TaskRow[] = [];
  const seen = new Set<string>();
  const add = (task: AgentTask, depth: number) => {
    seen.add(task.id);
    rows.push({ task, depth });
    for (const child of ordered) if (child.parentId === task.id && !seen.has(child.id)) add(child, depth + 1);
  };
  for (const task of ordered) if (!task.parentId || !ids.has(task.parentId)) add(task, 0);
  for (const task of ordered) if (!seen.has(task.id)) add(task, 0);
  return rows;
}

/** The tasks grouped by the turn that started them, newest group first. An unknown state counts as pending. */
export function groupTasks(tasks: AgentTask[]): TaskGroup[] {
  const byGroup = new Map<string, AgentTask[]>();
  for (const task of tasks) {
    const list = byGroup.get(task.group);
    if (list) list.push(task);
    else byGroup.set(task.group, [task]);
  }
  const groups: TaskGroup[] = [];
  for (const [group, members] of byGroup) {
    const ordered = [...members].sort((a, b) => a.createdAtMs - b.createdAtMs);
    const counts = emptyCounts();
    for (const task of ordered) counts[task.state in counts ? task.state : "pending"] += 1;
    const finished = counts.done + counts.failed + counts.cancelled;
    groups.push({
      group,
      owner: ordered.find((t) => t.owner)?.owner ?? null,
      tasks: ordered,
      rows: treeRows(ordered),
      total: ordered.length,
      finished,
      counts,
      percent: Math.round((finished / ordered.length) * 100),
      totalTokens: ordered.reduce((sum, t) => sum + (t.totalTokens ?? 0), 0),
      active: counts.pending + counts.running + counts.waiting + counts.paused > 0,
      createdAtMs: ordered[0].createdAtMs,
    });
  }
  return groups.sort((a, b) => b.createdAtMs - a.createdAtMs);
}

/** The tasks that involve an agent: the ones it received and the ones it delegated (P120, from a node of the tree). */
export function involvingAgent(tasks: AgentTask[], agent: string): AgentTask[] {
  return tasks.filter((t) => t.assignee === agent || t.owner === agent);
}

/** What an agent has been doing, taken only from the tasks the hub keeps (P120): what it received (`assignee`), how many it delegated (`owner`) and
 * when it last touched something. The log keeps only the most recent ones, and direct conversations and notes do not count. */
export interface AgentActivity {
  /** Tasks it received, by where they stand. */
  done: number;
  failed: number;
  cancelled: number;
  /** Pending, running, waiting on an agent or paused. */
  active: number;
  /** Tokens of the tasks it received that reported them. */
  tokens: number;
  /** How many tasks it delegated to others. */
  delegated: number;
  /** The last thing that happened: on one of its tasks (created, started or finished) or in a conversation with it. `null` when only the spend is known. */
  lastActiveMs: number | null;
  /** What it spent in the hub's ledger window. Only shows when the screen reports it. */
  spend?: AgentSpend;
}

/** An agent's spend in the hub's ledger, which only keeps the last `windowHours` (the largest limit). */
export interface AgentSpend {
  costUsd: number;
  /** Calls with no known price: the dollar amount leaves them out. */
  unpricedCalls: number;
  windowHours: number;
}

/** What the activity uses besides the tasks (P123): the agent's spend and most recent conversation. */
export interface ActivityExtras {
  spend?: AgentSpend;
  lastConversationMs?: number;
}

/** The activity of `agent`, or `null` when nothing involves it: no task, no recent conversation, no spend. */
export function activityOf(tasks: AgentTask[], agent: string, extras: ActivityExtras = {}): AgentActivity | null {
  const received = tasks.filter((t) => t.assignee === agent);
  const delegated = tasks.filter((t) => t.owner === agent);
  const spend = extras.spend && (extras.spend.costUsd > 0 || extras.spend.unpricedCalls > 0) ? extras.spend : undefined;
  if (received.length === 0 && delegated.length === 0 && extras.lastConversationMs === undefined && !spend) return null;
  const counts = emptyCounts();
  for (const task of received) counts[task.state in counts ? task.state : "pending"] += 1;
  const stamp = (t: AgentTask) => t.finishedAtMs ?? t.startedAtMs ?? t.createdAtMs;
  const stamps = [...received, ...delegated].map(stamp);
  if (extras.lastConversationMs !== undefined) stamps.push(extras.lastConversationMs);
  return {
    done: counts.done,
    failed: counts.failed,
    cancelled: counts.cancelled,
    active: counts.pending + counts.running + counts.waiting + counts.paused,
    tokens: received.reduce((sum, t) => sum + (t.totalTokens ?? 0), 0),
    delegated: delegated.length,
    lastActiveMs: stamps.length > 0 ? Math.max(...stamps) : null,
    ...(spend && { spend }),
  };
}

/** The spend of `agent` in the hub's usage report, or `undefined` when the hub does not count it (older hub, or an agent with no spend in the window). */
export function spendOf(
  report: { recent?: { windowHours: number; byAgent?: { key: string; costUsd: number; unpricedCalls: number }[] } | null } | null,
  agent: string,
): AgentSpend | undefined {
  const bucket = report?.recent?.byAgent?.find((b) => b.key === agent);
  if (!bucket || !report?.recent) return undefined;
  return { costUsd: bucket.costUsd, unpricedCalls: bucket.unpricedCalls, windowHours: report.recent.windowHours };
}

/** When the last conversation of `agent` was (the most recent one that talked to it, not counting threads), or `undefined` when there was none. */
export function lastConversationOf(conversations: { agentId?: string | null; updatedAt: number; parent?: unknown }[], agent: string): number | undefined {
  const times = conversations.filter((c) => c.agentId === agent && !c.parent).map((c) => c.updatedAt);
  return times.length > 0 ? Math.max(...times) : undefined;
}

/** How long ago, in a few words: `now`, `4 min ago`, `2 h ago`, `3 d ago`. */
export function agoLabel(thenMs: number, nowMs: number): string {
  const minutes = Math.max(0, Math.round((nowMs - thenMs) / 60000));
  if (minutes < 1) return t("ago.now");
  if (minutes < 60) return t("ago.minutes", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("ago.hours", { count: hours });
  return t("ago.days", { count: Math.floor(hours / 24) });
}

/** The spend in a few words: `$1.20 in the last 24 h`, with a note of the calls the value leaves out. */
export function spendLabel(spend: AgentSpend): string {
  const window = t("spend.window", { hours: spend.windowHours });
  const unpriced = spend.unpricedCalls > 0 ? t(spend.unpricedCalls > 1 ? "spend.unpricedMany" : "spend.unpricedOne", { count: spend.unpricedCalls }) : "";
  if (spend.costUsd <= 0) return `${unpriced} ${window}`;
  const dollars = (n: number) => new Intl.NumberFormat(intlTag(), { style: "currency", currency: "USD" }).format(n);
  const money = spend.costUsd < 0.01 ? t("spend.lessThan", { amount: dollars(0.01) }) : dollars(spend.costUsd);
  return unpriced ? t("spend.withUnpriced", { money, window, unpriced }) : `${money} ${window}`;
}

export interface ComboSpendRow {
  combo: string;
  /** `$1.20 in the last 24 h`, with a note of the calls the value leaves out. */
  label: string;
}

/** P126 — what each combo spent in the ledger window, biggest first. The window is the ledger's, not the per-combo tokens' (which has no time
 * limit), hence the separate rows. An older hub, or no call through a combo, gives no row at all. */
export function comboSpendRows(report: { recent?: { windowHours: number; byCombo?: { key: string; costUsd: number; unpricedCalls: number }[] } | null }): ComboSpendRow[] {
  const recent = report.recent;
  if (!recent?.byCombo) return [];
  return [...recent.byCombo]
    .sort((a, b) => b.costUsd - a.costUsd)
    .map((b) => ({ combo: b.key, label: spendLabel({ costUsd: b.costUsd, unpricedCalls: b.unpricedCalls, windowHours: recent.windowHours }) }));
}

/** The activity on one line of the tree card: `3 done, 1 failed, 2 in progress · 12.9K tokens · $1.20 in the last 24 h · delegated 5 · active 4 min ago`. */
export function activityLine(a: AgentActivity, nowMs: number): string {
  const parts = [
    a.done > 0 && t(a.done > 1 ? "tally.doneMany" : "tally.doneOne", { count: a.done }),
    a.failed > 0 && t("tally.failed", { count: a.failed }),
    a.cancelled > 0 && t(a.cancelled > 1 ? "tally.cancelledMany" : "tally.cancelledOne", { count: a.cancelled }),
    a.active > 0 && t("tally.active", { count: a.active }),
  ].filter(Boolean);
  const line = [parts.join(", ")];
  if (a.tokens > 0) line.push(t("tally.tokens", { count: formatTokens(a.tokens) }));
  if (a.spend) line.push(spendLabel(a.spend));
  if (a.delegated > 0) line.push(t("tally.delegated", { count: a.delegated }));
  if (a.lastActiveMs !== null) line.push(t("tally.activeAgo", { ago: agoLabel(a.lastActiveMs, nowMs) }));
  return line.filter(Boolean).join(" · ");
}

export function formatTokens(n: number): string {
  return new Intl.NumberFormat(intlTag(), { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

/** How long a task took (or is taking): `4s`, `2m 05s`, `1h 03m`. `null` before it starts. */
export function durationLabel(task: AgentTask, now: number): string | null {
  if (task.startedAtMs == null) return null;
  const seconds = Math.max(0, Math.round(((task.finishedAtMs ?? now) - task.startedAtMs) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}

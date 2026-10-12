// P123 — the work agents delegate to each other, as the "Agent work" screen shows it: the tasks one turn started are a group,
// with a progress counted over them. Pure, so it can be tested without a window.

import type { AgentTask, AgentTaskAction, AgentTaskState } from "../types";

/** A task as the list shows it: how many levels under the turn's own agent it sits (0 for the tasks the agent started itself). */
export interface TaskRow {
  task: AgentTask;
  depth: number;
}

export interface TaskGroup {
  group: string;
  /** The agent that delegated, if there was one. */
  owner: string | null;
  /** Oldest first, in the order the turn started them. */
  tasks: AgentTask[];
  /** The same tasks as a tree flattened for display: each one followed by its subtasks. */
  rows: TaskRow[];
  total: number;
  /** Done, failed or cancelled: no longer waiting for anything. */
  finished: number;
  counts: Record<AgentTaskState, number>;
  /** Whole percent of the tasks that are finished. */
  percent: number;
  /** Tokens the tasks that reported any used. */
  totalTokens: number;
  /** Something in the group is still pending, running, waiting for another agent or paused. */
  active: boolean;
  createdAtMs: number;
}

export const STATE_LABEL: Record<AgentTaskState, string> = {
  pending: "Pending",
  running: "Running",
  waiting: "Waiting for agent",
  paused: "Paused",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

/** The mark shown next to a state. */
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

/** What a person can do to this task right now: only a task running in the process that answered can be controlled. A task that
 * hasn't started can only be stopped; a paused one can be resumed. */
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

export const ACTION_LABEL: Record<AgentTaskAction, string> = { pause: "Pause", resume: "Resume", cancel: "Stop" };

/** The tasks of one group as a tree: each task followed by its subtasks, the subtasks one level deeper. A task whose parent isn't in the
 * group (or a loop that shouldn't exist) is shown at the top rather than lost. */
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

/** The tasks grouped by the turn that started them, the newest group first. A state this app doesn't know counts as pending. */
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

/** The tasks that involve one agent: the ones it was given to do and the ones it delegated (P120, from a node of the tree). The tasks of
 * the subtasks it started keep their place under it because they are the ones it delegated. */
export function involvingAgent(tasks: AgentTask[], agent: string): AgentTask[] {
  return tasks.filter((t) => t.assignee === agent || t.owner === agent);
}

/** What an agent has been up to, taken only from the tasks the hub keeps (P120): what it was given (`assignee`), how many it delegated
 * (`owner`) and when it last did anything. The log keeps only the most recent ones, and direct chats and notes are not in it. */
export interface AgentActivity {
  /** Tasks it was given, by where they are. */
  done: number;
  failed: number;
  cancelled: number;
  /** Pending, running, waiting for an agent or paused. */
  active: number;
  /** Tokens of the tasks it was given that reported any. */
  tokens: number;
  /** How many tasks it delegated to others. */
  delegated: number;
  /** The last thing that happened: to a task of its own (created, started or finished) or in a conversation with it. `null` when only the spend is known. */
  lastActiveMs: number | null;
  /** What it spent in the ledger's window. Only there when the owner supplies it; never for a member. */
  spend?: AgentSpend;
}

/** An agent's spending in the ledger, which only keeps the last `windowHours` (the longest limit). */
export interface AgentSpend {
  costUsd: number;
  /** Calls with no known price: the dollar figure leaves them out. */
  unpricedCalls: number;
  windowHours: number;
}

/** What the activity uses besides the tasks (P123): the agent's spending and its most recent conversation. */
export interface ActivityExtras {
  spend?: AgentSpend;
  lastConversationMs?: number;
}

/** The activity of `agent`, or `null` when nothing involves it: no task, no recent conversation, no spending. */
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

/** `agent`'s spending in the hub's usage report, or `undefined` when the hub doesn't count it (an older hub, or no spending in the window). */
export function spendOf(
  report: { recent?: { windowHours: number; byAgent?: { key: string; costUsd: number; unpricedCalls: number }[] } | null } | null,
  agent: string,
): AgentSpend | undefined {
  const bucket = report?.recent?.byAgent?.find((b) => b.key === agent);
  if (!bucket || !report?.recent) return undefined;
  return { costUsd: bucket.costUsd, unpricedCalls: bucket.unpricedCalls, windowHours: report.recent.windowHours };
}

/** When `agent` last had a conversation (the newest one that spoke with it, threads left out), or `undefined` if it never did. */
export function lastConversationOf(conversations: { agentId?: string | null; updatedAt: number; parent?: unknown }[], agent: string): number | undefined {
  const times = conversations.filter((c) => c.agentId === agent && !c.parent).map((c) => c.updatedAt);
  return times.length > 0 ? Math.max(...times) : undefined;
}

/** How long ago, in a few words: `just now`, `4 min ago`, `2 h ago`, `3 d ago`. */
export function agoLabel(thenMs: number, nowMs: number): string {
  const minutes = Math.max(0, Math.round((nowMs - thenMs) / 60000));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}

/** The spending in a few words: `$1.20 in the last 24 h`, with a note of the calls the figure leaves out. */
export function spendLabel(spend: AgentSpend): string {
  const window = `in the last ${spend.windowHours} h`;
  const unpriced = spend.unpricedCalls > 0 ? `${spend.unpricedCalls} unpriced call${spend.unpricedCalls > 1 ? "s" : ""}` : "";
  if (spend.costUsd <= 0) return `${unpriced} ${window}`;
  const dollars = (n: number) => new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(n);
  const money = spend.costUsd < 0.01 ? `under ${dollars(0.01)}` : dollars(spend.costUsd);
  return unpriced ? `${money} ${window} (and ${unpriced})` : `${money} ${window}`;
}

/** The activity on one line of the tree card: `3 done, 1 failed, 2 running · 12.9K tokens · $1.20 in the last 24 h · delegated 5 · active 4 min ago`. */
export function activityLine(a: AgentActivity, nowMs: number): string {
  const parts = [
    a.done > 0 && `${a.done} done`,
    a.failed > 0 && `${a.failed} failed`,
    a.cancelled > 0 && `${a.cancelled} stopped`,
    a.active > 0 && `${a.active} running`,
  ].filter(Boolean);
  const line = [parts.join(", ")];
  if (a.tokens > 0) line.push(`${formatTokens(a.tokens)} tokens`);
  if (a.spend) line.push(spendLabel(a.spend));
  if (a.delegated > 0) line.push(`delegated ${a.delegated}`);
  if (a.lastActiveMs !== null) line.push(`active ${agoLabel(a.lastActiveMs, nowMs)}`);
  return line.filter(Boolean).join(" · ");
}

/** `1,284` up to 999, `12.9K` beyond. */
export function formatTokens(n: number): string {
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

/** How long a task ran (or has been running), in a short form: `4s`, `2m 05s`, `1h 03m`. `null` before it started. */
export function durationLabel(task: AgentTask, now: number): string | null {
  if (task.startedAtMs == null) return null;
  const seconds = Math.max(0, Math.round(((task.finishedAtMs ?? now) - task.startedAtMs) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}

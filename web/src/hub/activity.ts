// P121 — the activity feed: who did what among the agents, newest to oldest. Pure; the screen only draws what comes out of here.

import { intlTag, t } from "../i18n/index.ts";
import type { ActivityEvent } from "./messages";

/** What the screen calls the agent that answers when nobody was picked (an event with no `actor`). */
const who = (name: string) => (name.trim() === "" ? t("activity.noAgent") : name);

/** The runs of scheduled tasks and webhooks: their target is the task's or webhook's id, not an agent. */
const RUN_KINDS = new Set(["scheduled_ran", "scheduled_failed", "webhook_ran", "webhook_failed"]);

/** The agent the event reaches, when its target is an agent. */
const targetAgent = (event: ActivityEvent) => (RUN_KINDS.has(event.kind) ? undefined : event.target ?? undefined);

/** An event's sentence, without its text (the text is the objective, the reply or the message). */
export function headline(event: ActivityEvent): string {
  const actor = who(event.actor);
  const target = event.target ? who(event.target) : "";
  switch (event.kind) {
    case "created_agent":
      return t("activity.agent_created", { actor, target });
    case "removed_agent":
      return t("activity.agent_deleted", { actor, target });
    case "updated_agent":
      return t("activity.agent_updated", { actor, target });
    case "scheduled_ran":
      return t("activity.scheduled_ran", { actor, target });
    case "scheduled_failed":
      return t("activity.scheduled_failed", { target });
    case "webhook_ran":
      return t("activity.webhook_ran", { actor, target });
    case "webhook_failed":
      return t("activity.webhook_failed", { target });
    case "delegated":
      return t("activity.task_delegated", { actor, target });
    case "started":
      return t("activity.task_started", { actor });
    case "done":
      return t("activity.task_done", { actor });
    case "failed":
      return t("activity.task_failed", { actor });
    case "cancelled":
      return t("activity.task_cancelled", { actor });
    case "note":
      return t("activity.note_left", { actor, target });
    case "reply":
      return t("activity.note_answered", { actor, target });
    case "messaged_user":
      return t("activity.note_to_you", { actor });
    default:
      return `${actor}: ${event.kind}`;
  }
}

/** A short mark per kind, to scan the list by eye. */
export function mark(kind: string): string {
  switch (kind) {
    case "delegated":
      return "→";
    case "started":
      return "▶";
    case "done":
      return "✓";
    case "failed":
    case "scheduled_failed":
    case "webhook_failed":
      return "✕";
    case "cancelled":
      return "■";
    case "created_agent":
      return "+";
    case "removed_agent":
      return "−";
    case "updated_agent":
      return "~";
    case "scheduled_ran":
      return "⏱";
    case "webhook_ran":
      return "⚡";
    case "note":
    case "reply":
      return "✉";
    case "messaged_user":
      return "●";
    default:
      return "·";
  }
}

/** The events the agent appears in, doing or receiving. */
export function involving(events: ActivityEvent[], agent: string): ActivityEvent[] {
  return events.filter((e) => e.actor === agent || targetAgent(e) === agent);
}

/** The agents that appear in the events, in alphabetical order, without the unnamed assistant (nor the id of a scheduled task or a webhook). */
export function agentsIn(events: ActivityEvent[]): string[] {
  const names = new Set<string>();
  for (const e of events) {
    if (e.actor.trim() !== "") names.add(e.actor);
    const target = targetAgent(e);
    if (target && target.trim() !== "") names.add(target);
  }
  return [...names].sort((a, b) => a.localeCompare(b));
}

/** What a click on an event opens: the agent's tasks, the conversation between two agents or the agent's channel. */
export type Destination = { kind: "tasks"; agent: string } | { kind: "conversation"; id: string } | { kind: "channel"; agent: string };

export function destination(event: ActivityEvent): Destination | null {
  if (event.kind === "messaged_user") return { kind: "channel", agent: event.actor };
  if (event.conversationId) return { kind: "conversation", id: event.conversationId };
  if (event.taskId) {
    const agent = event.kind === "delegated" ? event.target : event.actor;
    return agent ? { kind: "tasks", agent } : null;
  }
  return null;
}

export interface DayGroup {
  /** "Today", "Yesterday" or the date. */
  label: string;
  events: ActivityEvent[];
}

const dayKey = (ms: number) => {
  const d = new Date(ms);
  return d.getFullYear() * 10000 + (d.getMonth() + 1) * 100 + d.getDate();
};

/** The events (already newest to oldest) split by day, with each day's label. */
export function groupByDay(events: ActivityEvent[], now: number): DayGroup[] {
  const formatter = new Intl.DateTimeFormat(intlTag(), { day: "2-digit", month: "long", year: "numeric" });
  const today = dayKey(now);
  const yesterday = dayKey(now - 24 * 60 * 60 * 1000);
  const groups: DayGroup[] = [];
  for (const event of events) {
    const key = dayKey(event.atMs);
    const label = key === today ? t("activity.today") : key === yesterday ? t("activity.yesterday") : formatter.format(event.atMs);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.events.push(event);
    else groups.push({ label, events: [event] });
  }
  return groups;
}

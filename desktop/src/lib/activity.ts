// P121 — the feed of activity: who did what among the agents, newest first. Pure; the screen only draws what comes out of here.
// Mirrors `web/src/hub/activity.ts`, in English.

import { intlTag, t, type Key } from "../i18n/index.ts";
import type { ActivityEvent } from "../types";

/** What the screen calls the assistant that answers with no agent picked (an event with no `actor`). */
const who = (name: string) => (name.trim() === "" ? t("activity.noAgent") : name);

/** The runs of scheduled tasks and webhooks: their target is the id of the task or the webhook, not an agent. */
const RUN_KINDS = new Set(["scheduled_ran", "scheduled_failed", "webhook_ran", "webhook_failed"]);

/** The agent the event is done to, when its target is an agent. */
const targetAgent = (event: ActivityEvent) => (RUN_KINDS.has(event.kind) ? undefined : event.target ?? undefined);

/** The sentence of an event, without its text (the text is the objective, the answer or the message). */
export function headline(event: ActivityEvent): string {
  const actor = who(event.actor);
  const target = event.target ? who(event.target) : "";
  if (KINDS.has(event.kind)) return t(`activity.${event.kind}` as Key, { actor, target });
  return `${actor}: ${event.kind}`;
}

/** The kinds that have a sentence of their own in the catalog (`activity.<kind>`). */
const KINDS = new Set([
  "created_agent",
  "removed_agent",
  "updated_agent",
  "scheduled_ran",
  "scheduled_failed",
  "webhook_ran",
  "webhook_failed",
  "delegated",
  "started",
  "done",
  "failed",
  "cancelled",
  "note",
  "reply",
  "messaged_user",
]);

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

/** The events an agent appears in, doing or receiving. */
export function involving(events: ActivityEvent[], agent: string): ActivityEvent[] {
  return events.filter((e) => e.actor === agent || targetAgent(e) === agent);
}

/** The agents that appear in the events, alphabetical, without the nameless assistant (nor the id of a scheduled task or a webhook). */
export function agentsIn(events: ActivityEvent[]): string[] {
  const names = new Set<string>();
  for (const e of events) {
    if (e.actor.trim() !== "") names.add(e.actor);
    const target = targetAgent(e);
    if (target && target.trim() !== "") names.add(target);
  }
  return [...names].sort((a, b) => a.localeCompare(b));
}

/** What a click on an event opens: the agent's tasks, the conversation between two agents, or the agent's channel. */
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

/** The events (already newest first) split by day, each day with its label. */
export function groupByDay(events: ActivityEvent[], now: number): DayGroup[] {
  const formatter = new Intl.DateTimeFormat(intlTag(), { day: "numeric", month: "long", year: "numeric" });
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

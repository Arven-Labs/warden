// P125 — threads: a child conversation tied to a message of another. Pure, to test without a window. Mirrors `web/src/hub/threads.ts`.

import { t } from "../i18n/index.ts";
import type { ConversationSummary } from "./messages";

/** The start of an agent's channel id (P121): the fixed conversation it keeps with the person. Same as the hub's `CHANNEL_PREFIX`. */
export const CHANNEL_PREFIX = "channel-";

/** Whether `id` is an agent's channel. */
export function isAgentChannel(id: string): boolean {
  return id.startsWith(CHANNEL_PREFIX);
}

/** The conversations the list shows: threads are left out, they only appear from the message they came from, and so are the agents'
 * channels, which have their own tab. */
export function visibleConversations<T extends Pick<ConversationSummary, "parent" | "id">>(conversations: T[]): T[] {
  return conversations.filter((c) => !c.parent && !isAgentChannel(c.id));
}

/** A message's thread: its conversation and how many messages the person sent in it. */
export interface ThreadInfo {
  conversationId: string;
  replies: number;
}

/** The threads of a conversation, by the id of the message they came from. A message has at most one; if a race between two devices
 * produces two, the one with more replies wins. */
export function threadsOf(conversations: ConversationSummary[], conversationId: string): Record<string, ThreadInfo> {
  const found: Record<string, ThreadInfo> = {};
  for (const c of conversations) {
    if (c.parent?.conversationId !== conversationId) continue;
    const info = { conversationId: c.id, replies: c.replies ?? 0 };
    const known = found[c.parent.messageId];
    if (!known || info.replies > known.replies) found[c.parent.messageId] = info;
  }
  return found;
}

/** Attaches the hub history's ids to the messages already on screen that do not have them yet (the reply that just arrived). Only applies when
 * the messages on screen, minus the errors (which the hub does not keep), are as many as the history's: otherwise a turn is in flight
 * and the positions do not line up, so the list stays as it is. */
export function withMessageIds<T extends { role: string; id?: string }>(entries: T[], history: Array<{ id?: string }>): T[] {
  if (entries.filter((e) => e.role !== "error").length !== history.length) return entries;
  let next = 0;
  return entries.map((entry) => {
    if (entry.role === "error") return entry;
    const id = history[next++].id;
    return id && !entry.id ? { ...entry, id } : entry;
  });
}

/** What the chip of a message says: `1 reply`, `3 replies`. */
export function repliesLabel(replies: number): string {
  return t(replies === 1 ? "replies.one" : "replies.many", { count: replies });
}

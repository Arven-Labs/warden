// P157 — how the sidebar lists the conversations it keeps loose (not in a project): the pinned ones first, then by how recently
// they were used, with the archived ones apart. Pure, so it can be tested without a window.

import type { Conversation } from "../types";

/** One block of the list: a label and its conversations, newest first. */
export interface ConversationBucket {
  label: string;
  items: Conversation[];
}

export interface GroupedConversations {
  pinned: Conversation[];
  buckets: ConversationBucket[];
  archived: Conversation[];
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** The start of `now`'s day on this computer's clock. */
function startOfDay(now: number): number {
  const date = new Date(now);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** Which block a conversation last used at `updatedAt` goes in, with the day as the computer's clock sees it (a day is counted
 * on the calendar, not as 24 hours back, so "Yesterday" is the whole of yesterday). */
export function bucketLabel(updatedAt: number, now: number): string {
  const today = startOfDay(now);
  if (updatedAt >= today) return "Today";
  const yesterday = startOfDay(today - DAY_MS / 2);
  if (updatedAt >= yesterday) return "Yesterday";
  const week = startOfDay(today - 6.5 * DAY_MS);
  if (updatedAt >= week) return "Previous 7 days";
  const month = startOfDay(today - 29.5 * DAY_MS);
  if (updatedAt >= month) return "Previous 30 days";
  return "Older";
}

/** Splits `conversations` into the pinned, the blocks by date and the archived. The order each came in is kept inside a block, and
 * a block with nothing in it is left out. A conversation that is archived is only ever in `archived`, whatever else it says. */
export function groupConversations(conversations: Conversation[], now: number): GroupedConversations {
  const pinned: Conversation[] = [];
  const archived: Conversation[] = [];
  const byLabel = new Map<string, Conversation[]>();
  for (const conversation of conversations) {
    if (conversation.archived) archived.push(conversation);
    else if (conversation.pinned) pinned.push(conversation);
    else {
      const label = bucketLabel(conversation.updatedAt, now);
      byLabel.set(label, [...(byLabel.get(label) ?? []), conversation]);
    }
  }
  const order = ["Today", "Yesterday", "Previous 7 days", "Previous 30 days", "Older"];
  const buckets = order.filter((label) => byLabel.has(label)).map((label) => ({ label, items: byLabel.get(label)! }));
  return { pinned, buckets, archived };
}

// P157 — how the list groups the loose conversations (outside a project): the pinned ones first, then by how recent they are, and the archived
// ones apart. Pure, to test without a window. Mirrors `desktop/src/lib/conversationGroups.ts`.

import { t } from "../i18n/index.ts";
import type { ConversationSummary } from "./messages";

type Groupable = Pick<ConversationSummary, "updatedAt" | "pinned" | "archived">;

/** A block of the list: its name and its conversations, in the order they came. */
export interface ConversationBucket<T> {
  label: string;
  items: T[];
}

export interface GroupedConversations<T> {
  pinned: T[];
  buckets: ConversationBucket<T>[];
  archived: T[];
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** The date blocks, from the most recent to the oldest. */
const LABELS = ["bucket.today", "bucket.yesterday", "bucket.last7", "bucket.last30", "bucket.older"] as const;

/** The start of the day of `now` on this computer's clock. */
function startOfDay(now: number): number {
  const date = new Date(now);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** Which block a conversation last used at `updatedAt` falls into. The day counts on the calendar, not 24 hours back: "Yesterday" is the whole
 * of yesterday. */
export function bucketLabel(updatedAt: number, now: number): string {
  const today = startOfDay(now);
  if (updatedAt >= today) return t(LABELS[0]);
  if (updatedAt >= startOfDay(today - DAY_MS / 2)) return t(LABELS[1]);
  if (updatedAt >= startOfDay(today - 6.5 * DAY_MS)) return t(LABELS[2]);
  if (updatedAt >= startOfDay(today - 29.5 * DAY_MS)) return t(LABELS[3]);
  return t(LABELS[4]);
}

/** Splits `conversations` into pinned, date blocks and archived. Within each part the arrival order is kept, and an empty block
 * is left out. An archived conversation only appears in `archived`, whatever else says otherwise. */
export function groupConversations<T extends Groupable>(conversations: T[], now: number): GroupedConversations<T> {
  const pinned: T[] = [];
  const archived: T[] = [];
  const byLabel = new Map<string, T[]>();
  for (const conversation of conversations) {
    if (conversation.archived) archived.push(conversation);
    else if (conversation.pinned) pinned.push(conversation);
    else {
      const label = bucketLabel(conversation.updatedAt, now);
      byLabel.set(label, [...(byLabel.get(label) ?? []), conversation]);
    }
  }
  const buckets = LABELS.map((key) => t(key))
    .filter((label) => byLabel.has(label))
    .map((label) => ({ label, items: byLabel.get(label)! }));
  return { pinned, buckets, archived };
}

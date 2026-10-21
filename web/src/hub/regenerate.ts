// P157 — the pure part of "regenerate the reply" and "edit my message": which message the redo starts from and what the trimmed list looks like.
// Pure, to test without a window. Mirrors `desktop/src/lib/regenerate.ts`, but over the conversation's entries. Redoing sends the turn again
// with the id of the message it starts from (`replaceFrom`); the turn takes the place of that message and of everything after it. The id only exists
// on an entry the hub already stored (`withMessageIds`), so an entry with no id cannot be redone.

import { t } from "../i18n/index.ts";
import type { ChatEntry } from "./connection";

/** The last message the person sent that the hub already stored, or `undefined`. */
export function lastUserEntry(entries: ChatEntry[]): ChatEntry | undefined {
  for (let i = entries.length - 1; i >= 0; i--) {
    if (entries[i].role === "user" && entries[i].id) return entries[i];
  }
  return undefined;
}

/** Whether `entry` is the assistant's last answer (error ones do not count) and can be regenerated. */
export function isLastAnswer(entries: ChatEntry[], index: number): boolean {
  const entry = entries[index];
  if (!entry || entry.role !== "assistant" || !entry.id) return false;
  return !entries.slice(index + 1).some((e) => e.role !== "error");
}

/** The entries before the one with id `from`; all of them, if it is not in the list. */
export function entriesBefore(entries: ChatEntry[], from: string): ChatEntry[] {
  const at = entries.findIndex((e) => e.id === from);
  return at === -1 ? entries : entries.slice(0, at);
}

/** How many messages come after the one with id `from` (0 if it is the last or is not there): what an edit also deletes. Errors do not count, the hub does not keep them. */
export function messagesAfter(entries: ChatEntry[], from: string): number {
  const at = entries.findIndex((e) => e.id === from);
  return at === -1 ? 0 : entries.slice(at + 1).filter((e) => e.role !== "error").length;
}

/** Why a conversation cannot be redone now, or `null` when it can: it is not on the hub yet, it is a thread (its context belongs to another
 * conversation), it belongs to a code project (the engine's session cannot go back), a turn is in progress or the hub is offline. */
export function redoBlock(state: { onHub: boolean; thread: boolean; code: boolean; busy: boolean; offline: boolean }): string | null {
  if (state.offline) return t("redo.offline");
  if (state.busy) return t("redo.busy");
  if (!state.onHub) return t("redo.notOnHub");
  if (state.thread) return t("redo.thread");
  if (state.code) return t("redo.code");
  return null;
}

// P157 — the pure half of "regenerate the answer" and "edit a message of mine": which message a redo starts from, and what the
// list looks like once it has been cut. Pure, so it can be tested without a window. A redo sends the turn again with the id of the
// message it starts from; the turn takes the place of that message and of everything after it.

import type { ChatMessage, Conversation } from "../types";

/** The last message the person sent, or `undefined` when there is none. */
export function lastUserMessage(messages: ChatMessage[]): ChatMessage | undefined {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === "user") return messages[i];
  }
  return undefined;
}

/** The messages before the one with id `from`; all of them when it isn't there. */
export function historyBefore(messages: ChatMessage[], from: string): ChatMessage[] {
  const at = messages.findIndex((m) => m.id === from);
  return at === -1 ? messages : messages.slice(0, at);
}

/** How many messages come after the one with id `from` (0 when it is the last, or isn't there): what an edit would also drop. */
export function messagesAfter(messages: ChatMessage[], from: string): number {
  const at = messages.findIndex((m) => m.id === from);
  return at === -1 ? 0 : messages.length - at - 1;
}

/** Whether a conversation can be redone from a message now: not a thread (its context is another conversation's), not a code
 * project's (the engine's session can't be rewound), and not while a turn is being answered. */
export function canRedo(conversation: Conversation | undefined, inCodeProject: boolean, isSending: boolean): boolean {
  return conversation !== undefined && !conversation.parent && !inCodeProject && !isSending;
}

/** The conversations with `conversationId` cut at message `from` (it and what followed are gone) and the threads opened from those
 * messages gone with them, as the save does. Anything else is left as it is. */
export function withoutFrom(conversations: Conversation[], conversationId: string, from: string): Conversation[] {
  const target = conversations.find((c) => c.id === conversationId);
  if (!target) return conversations;
  const at = target.messages.findIndex((m) => m.id === from);
  if (at === -1) return conversations;
  const cut = new Set(target.messages.slice(at).map((m) => m.id));
  return conversations
    .filter((c) => !(c.parent?.conversationId === conversationId && cut.has(c.parent.messageId)))
    .map((c) => (c.id === conversationId ? { ...c, messages: historyBefore(c.messages, from) } : c));
}

// The sidebar's search box: keeps the conversations whose title or messages have what the person typed. Pure, so it can be
// tested without a window. It looks at the messages in `messages`: on this computer they all are, but on a hub a conversation
// has none until it is opened, so there the others are found by their title only (P157).

import type { Conversation } from "../types";

/** Lower case with the accents taken off, so "reuniao" finds "Reunião". */
function plain(text: string): string {
  return text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
}

// The normalized text of each message, kept while the message object lives: the list is searched at every key, and taking the
// accents off a long history each time was measured at ~200 ms for 500 conversations of 100 messages.
const normalized = new WeakMap<object, string>();

function plainMessage(message: { content: string }): string {
  let text = normalized.get(message);
  if (text === undefined) {
    text = plain(message.content);
    normalized.set(message, text);
  }
  return text;
}

/** The conversations that have every word of `query` in the title or in the messages (the words may be split between them),
 * in any order and case, ignoring accents; all of them when the query is empty. The title is checked first, which is cheap,
 * and the messages only for the words it is missing. */
export function searchConversations(conversations: Conversation[], query: string): Conversation[] {
  const words = plain(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return conversations;
  return conversations.filter((c) => {
    const title = plain(c.title);
    let missing = words.filter((word) => !title.includes(word));
    for (const message of c.messages) {
      if (missing.length === 0) break;
      const text = plainMessage(message);
      missing = missing.filter((word) => !text.includes(word));
    }
    return missing.length === 0;
  });
}

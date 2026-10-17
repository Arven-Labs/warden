// The sidebar's search box: keeps the conversations whose title has what the person typed. Pure, so it can be tested without
// a window. Only the title: on a hub the messages of a conversation are not loaded until it is opened.

import type { Conversation } from "../types";

/** Lower case with the accents taken off, so "reuniao" finds "Reunião". */
function plain(text: string): string {
  return text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
}

/** The conversations whose title has every word of `query`, in any order and case, ignoring accents; all of them when the
 * query is empty. */
export function searchConversations(conversations: Conversation[], query: string): Conversation[] {
  const words = plain(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return conversations;
  return conversations.filter((c) => {
    const title = plain(c.title);
    return words.every((word) => title.includes(word));
  });
}

// The sidebar's search over the titles (`src/lib/conversationSearch.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { searchConversations } from "../src/lib/conversationSearch.ts";

const conversation = (id, title, extra = {}) => ({ id, title, messages: [], createdAt: 1, updatedAt: 1, ...extra });
const list = [conversation("a", "Reunião de planejamento"), conversation("b", "Trip to Rome", { projectId: "travel" }), conversation("c", "Receita de pão")];
const ids = (found) => found.map((c) => c.id);

describe("the search over the titles", () => {
  test("an empty or blank query keeps every conversation, in order", () => {
    assert.deepEqual(ids(searchConversations(list, "")), ["a", "b", "c"]);
    assert.deepEqual(ids(searchConversations(list, "   ")), ["a", "b", "c"]);
  });

  test("ignores case and accents, both ways", () => {
    assert.deepEqual(ids(searchConversations(list, "REUNIAO")), ["a"]);
    assert.deepEqual(ids(searchConversations(list, "pão")), ["c"]);
    assert.deepEqual(ids(searchConversations(list, "pao")), ["c"]);
  });

  test("wants every word, in any order, and finds nothing when one is missing", () => {
    assert.deepEqual(ids(searchConversations(list, "rome trip")), ["b"]);
    assert.deepEqual(ids(searchConversations(list, "rome pão")), []);
  });

  test("hands back the conversations themselves, project included", () => {
    assert.equal(searchConversations(list, "rome")[0], list[1]);
    assert.equal(searchConversations(list, "rome")[0].projectId, "travel");
  });
});

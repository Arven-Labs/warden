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

describe("the search over the messages (P157)", () => {
  const message = (content) => ({ id: "m", role: "user", content, createdAt: 1 });
  const withMessages = [
    conversation("a", "Planejamento", { messages: [message("Preciso de uma receita de lasanha")] }),
    conversation("b", "Viagem", { messages: [message("Passagem para Roma"), message("Hotel perto do Coliseu")] }),
    conversation("c", "Sem mensagens carregadas"),
  ];

  test("finds a conversation by a word that is only in a message", () => {
    assert.deepEqual(ids(searchConversations(withMessages, "lasanha")), ["a"]);
    assert.deepEqual(ids(searchConversations(withMessages, "coliseu")), ["b"]);
  });

  test("ignores case and accents in the messages too, both ways", () => {
    assert.deepEqual(ids(searchConversations(withMessages, "ROMA")), ["b"]);
    assert.deepEqual(ids(searchConversations(withMessages, "preciso receita")), ["a"]);
    assert.deepEqual(ids(searchConversations([conversation("x", "t", { messages: [message("Ação rápida")] })], "acao")), ["x"]);
  });

  test("wants every word, and they may be split between the title and different messages", () => {
    assert.deepEqual(ids(searchConversations(withMessages, "viagem roma")), ["b"]);
    assert.deepEqual(ids(searchConversations(withMessages, "roma hotel")), ["b"]);
    assert.deepEqual(ids(searchConversations(withMessages, "roma lasanha")), []);
  });

  test("a conversation without loaded messages is still found by its title, and a missing word finds nothing", () => {
    assert.deepEqual(ids(searchConversations(withMessages, "carregadas")), ["c"]);
    assert.deepEqual(ids(searchConversations(withMessages, "inexistente")), []);
  });

  test("does not look at anything but the text: roles and ids are not searched", () => {
    assert.deepEqual(ids(searchConversations(withMessages, "user")), []);
  });
});

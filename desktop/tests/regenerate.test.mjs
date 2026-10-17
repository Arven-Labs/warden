// P157 — regenerate and edit, the pure half (`src/lib/regenerate.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { canRedo, historyBefore, lastUserMessage, messagesAfter, withoutFrom } from "../src/lib/regenerate.ts";

const message = (id, role) => ({ id, role, content: id, createdAt: 1 });
const messages = [message("u1", "user"), message("a1", "assistant"), message("u2", "user"), message("a2", "assistant")];
const conversation = (id, extra = {}) => ({ id, title: id, messages, createdAt: 1, updatedAt: 1, ...extra });

describe("the message a redo starts from", () => {
  test("regenerate starts from the last message the person sent", () => {
    assert.equal(lastUserMessage(messages).id, "u2");
    assert.equal(lastUserMessage([message("a1", "assistant")]), undefined);
    assert.equal(lastUserMessage([]), undefined);
  });

  test("the model sees what came before that message, and all of it when the message isn't there", () => {
    assert.deepEqual(historyBefore(messages, "u2").map((m) => m.id), ["u1", "a1"]);
    assert.deepEqual(historyBefore(messages, "u1"), []);
    assert.equal(historyBefore(messages, "ghost"), messages);
  });

  test("an edit drops what came after the message too, and counts it for the warning", () => {
    assert.equal(messagesAfter(messages, "u1"), 3);
    assert.equal(messagesAfter(messages, "a2"), 0);
    assert.equal(messagesAfter(messages, "ghost"), 0);
  });
});

describe("when a conversation can be redone", () => {
  test("not a thread, not a code project's, not while a turn is answered, and not one that isn't there", () => {
    assert.equal(canRedo(conversation("c1"), false, false), true);
    assert.equal(canRedo(conversation("t1", { parent: { conversationId: "c1", messageId: "a1" } }), false, false), false);
    assert.equal(canRedo(conversation("c1"), true, false), false);
    assert.equal(canRedo(conversation("c1"), false, true), false);
    assert.equal(canRedo(undefined, false, false), false);
  });
});

describe("the list once a conversation is cut", () => {
  const thread = (id, messageId, conversationId = "c1") => conversation(id, { messages: [], parent: { conversationId, messageId } });

  test("drops the message and what followed, and the threads opened from them, and keeps the rest", () => {
    const list = [conversation("c1"), thread("t-kept", "a1"), thread("t-cut", "a2"), thread("t-cut-too", "u2"), thread("t-other", "a2", "c2"), conversation("c2")];
    const cut = withoutFrom(list, "c1", "u2");
    assert.deepEqual(cut.map((c) => c.id), ["c1", "t-kept", "t-other", "c2"]);
    assert.deepEqual(cut[0].messages.map((m) => m.id), ["u1", "a1"]);
    assert.equal(cut[3], list[5], "another conversation is the same object");
  });

  test("changes nothing when the conversation or the message isn't there", () => {
    const list = [conversation("c1")];
    assert.equal(withoutFrom(list, "ghost", "u2"), list);
    assert.equal(withoutFrom(list, "c1", "ghost"), list);
  });
});

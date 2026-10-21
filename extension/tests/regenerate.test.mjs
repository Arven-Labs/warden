// P157 — regenerate and edit, the pure part (`src/protocol/regenerate.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { entriesBefore, isLastAnswer, lastUserEntry, messagesAfter, redoBlock } from "../src/protocol/regenerate.ts";

const entry = (role, id) => ({ ...(id && { id }), role, content: id ?? role });
const saved = [entry("user", "u1"), entry("assistant", "a1"), entry("user", "u2"), entry("assistant", "a2")];

describe("which message the redo starts from", () => {
  test("regenerating starts from the person's last message that the hub already stored", () => {
    assert.equal(lastUserEntry(saved).id, "u2");
    assert.equal(lastUserEntry([entry("assistant", "a1")]), undefined);
    assert.equal(lastUserEntry([]), undefined);
    assert.equal(lastUserEntry([entry("user", "u1"), entry("user")]).id, "u1", "a message with no id yet does not count");
  });

  test("only the last stored answer is regenerated, and the errors after it do not count", () => {
    assert.deepEqual(saved.map((_, i) => isLastAnswer(saved, i)), [false, false, false, true]);
    const withError = [...saved, entry("error")];
    assert.equal(isLastAnswer(withError, 3), true);
    assert.equal(isLastAnswer([entry("user", "u1"), entry("assistant")], 1), false, "an answer with no id yet does not either");
    assert.equal(isLastAnswer(saved, 9), false);
  });

  test("the model sees what came before the message, and everything if the message is not there", () => {
    assert.deepEqual(entriesBefore(saved, "u2").map((e) => e.id), ["u1", "a1"]);
    assert.deepEqual(entriesBefore(saved, "u1"), []);
    assert.equal(entriesBefore(saved, "ghost"), saved);
  });

  test("an edit also deletes what came after, and that is counted for the warning, without the errors", () => {
    assert.equal(messagesAfter(saved, "u1"), 3);
    assert.equal(messagesAfter(saved, "a2"), 0);
    assert.equal(messagesAfter(saved, "ghost"), 0);
    assert.equal(messagesAfter([entry("user", "u1"), entry("error"), entry("assistant", "a1")], "u1"), 1);
  });
});

describe("when a conversation can be redone", () => {
  const free = { onHub: true, thread: false, code: false, busy: false, offline: false };

  test("free when it is on the hub, is neither a thread nor a code conversation, and nothing is in flight", () => {
    assert.equal(redoBlock(free), null);
  });

  test("each reason blocks and says which", () => {
    for (const [why, state] of [
      ["offline", { offline: true }],
      ["reply in flight", { busy: true }],
      ["not on the hub yet", { onHub: false }],
      ["thread", { thread: true }],
      ["code project", { code: true }],
    ]) {
      assert.equal(typeof redoBlock({ ...free, ...state }), "string", why);
    }
  });
});

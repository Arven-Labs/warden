// P124 — the live bubble of a turn (`src/protocol/liveTurn.ts`, the same as the web's): what the engine says and does while a turn runs. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { applyEvent, LIVE_TEXT_LIMIT } from "../src/protocol/liveTurn.ts";
import { decode } from "../src/protocol/messages.ts";

const say = (turn, text, source) => applyEvent(turn, { type: "text", text, ...(source ? { source } : {}) });
const tool = (turn, callId, status) => applyEvent(turn, { type: "tool", callId, tool: "bash", title: "cargo test", status });

describe("the words of a turn", () => {
  test("a lone engine's words run on in one block", () => {
    const turn = say(say(undefined, "Hello "), "there");
    assert.deepEqual(turn.items, [{ kind: "text", text: "Hello there" }]);
  });

  test("the words of one task run on, and another task's open a block of their own", () => {
    let turn = say(undefined, "A: one ", "Repo");
    turn = say(turn, "two", "Repo");
    turn = say(turn, "B: first", "Site");
    assert.deepEqual(turn.items, [
      { kind: "text", text: "A: one two", source: "Repo" },
      { kind: "text", text: "B: first", source: "Site" },
    ]);
  });

  test("a tool in between starts a new block even for the same task", () => {
    let turn = say(undefined, "before", "Repo");
    turn = tool(turn, "repo:c1", "running");
    turn = say(turn, "after", "Repo");
    assert.deepEqual(turn.items.map((item) => item.kind), ["text", "tool", "text"]);
  });

  test("a task that writes a lot keeps only the latest words of it", () => {
    let turn = say(undefined, "x".repeat(LIVE_TEXT_LIMIT), "Repo");
    turn = say(turn, "END", "Repo");
    assert.equal(turn.items[0].text.length, LIVE_TEXT_LIMIT);
    assert.ok(turn.items[0].text.endsWith("END"));
  });
});

describe("the tools and notices of a turn", () => {
  test("a later stage of the same call updates its line instead of adding one", () => {
    let turn = tool(undefined, "c1", "running");
    turn = say(turn, "words");
    turn = tool(turn, "c1", "completed");
    assert.deepEqual(turn.items.map((item) => item.kind === "tool" ? item.status : item.kind), ["completed", "text"]);
  });

  test("a notice is shown under the work and replaced by the next", () => {
    let turn = applyEvent(undefined, { type: "notice", text: "retrying" });
    turn = applyEvent(turn, { type: "notice", text: "waiting" });
    assert.equal(turn.notice, "waiting");
    assert.deepEqual(turn.items, []);
  });
});

describe("the chatEvent message", () => {
  test("decodes as it came, so the background can build the turn from it", () => {
    const event = { type: "tool", callId: "c1", tool: "bash", title: "ls", status: "running" };
    const message = decode(JSON.stringify({ type: "chatEvent", conversationId: "conv", event }));
    assert.deepEqual(message, { type: "chatEvent", conversationId: "conv", event });
  });
});

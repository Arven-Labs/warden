// P124 — the live bubble of a turn (`src/lib/liveTurn.ts`): the words of tasks side by side are kept apart. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { applyEvent, LIVE_TEXT_LIMIT } from "../src/lib/liveTurn.ts";

const say = (turn, text, source) => applyEvent(turn, { type: "text", text, ...(source ? { source } : {}) });
const tool = (turn, callId, status) => applyEvent(turn, { type: "tool", callId, tool: "bash", title: "cargo test", status });

describe("the words of a turn", () => {
  test("a lone engine's words run on in one block, as they always did", () => {
    const turn = say(say(undefined, "Hello "), "there");
    assert.deepEqual(turn.items, [{ kind: "text", text: "Hello there" }]);
  });

  test("the words of one task run on, and another task's open a block of their own", () => {
    let turn = say(undefined, "A: one ", "Repo [wt a1b2c3]");
    turn = say(turn, "two", "Repo [wt a1b2c3]");
    turn = say(turn, "B: first", "Site");
    turn = say(turn, " second", "Site");
    assert.deepEqual(turn.items, [
      { kind: "text", text: "A: one two", source: "Repo [wt a1b2c3]" },
      { kind: "text", text: "B: first second", source: "Site" },
    ]);
  });

  test("two tasks writing by turns each get a block every time they switch, and no words are lost or mixed", () => {
    let turn = say(undefined, "a1", "A");
    turn = say(turn, "b1", "B");
    turn = say(turn, "a2", "A");
    assert.deepEqual(
      turn.items.map((item) => [item.source, item.text]),
      [["A", "a1"], ["B", "b1"], ["A", "a2"]],
    );
  });

  test("a tool in between starts a new block even for the same task", () => {
    let turn = say(undefined, "before", "Repo");
    turn = tool(turn, "repo:c1", "running");
    turn = say(turn, "after", "Repo");
    assert.deepEqual(turn.items.map((item) => item.kind), ["text", "tool", "text"]);
  });

  test("words with a source and words without one are not run together", () => {
    const turn = say(say(undefined, "plain"), "from a task", "Repo");
    assert.equal(turn.items.length, 2);
  });

  test("a long chat answer written bit by bit keeps its start (P157)", () => {
    // The model's words come with no source, a few hundred characters at a time; a long answer must not lose its beginning.
    const piece = "word ".repeat(100);
    let turn;
    for (let i = 0; i < 60; i++) turn = say(turn, piece);
    assert.equal(turn.items.length, 1);
    assert.equal(turn.items[0].text.length, piece.length * 60, "6000 characters, past the old 4000 limit");
    assert.ok(turn.items[0].text.startsWith("word word"));
  });

  test("a task that writes a lot keeps only the latest words of it", () => {
    let turn = say(undefined, "x".repeat(LIVE_TEXT_LIMIT), "Repo");
    turn = say(turn, "END", "Repo");
    assert.equal(turn.items.length, 1);
    assert.equal(turn.items[0].text.length, LIVE_TEXT_LIMIT);
    assert.ok(turn.items[0].text.endsWith("END"));
    assert.equal(say(undefined, "y".repeat(LIVE_TEXT_LIMIT + 10), "Repo").items[0].text.length, LIVE_TEXT_LIMIT, "even a first, long chunk");
  });
});

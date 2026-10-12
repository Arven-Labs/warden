// P123 — what a node of the organization tree says about an agent's activity (`src/hub/agentTasks.ts`): the tasks it was given, what it
// spent in the ledger's window and when it last talked. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { activityLine, activityOf, lastConversationOf, spendLabel, spendOf } from "../src/hub/agentTasks.ts";

const task = (id, state, extra = {}) => ({ id, group: "g", owner: "chief", assignee: id, objective: `faça ${id}`, channel: "web", state, createdAtMs: 1000, ...extra });
// The currency format puts a no-break space after "US$"; the tests read it as a plain one.
const plain = (text) => text.replace(/ /g, " ");

const tasks = [
  task("a", "done", { assignee: "dev", totalTokens: 100, createdAtMs: 1000, startedAtMs: 1100, finishedAtMs: 5000 }),
  task("b", "running", { assignee: "dev", createdAtMs: 3000, startedAtMs: 3100 }),
];
const now = 5000 + 4 * 60000;

describe("the activity of an agent besides its tasks", () => {
  test("without anything extra it is what it always was", () => {
    assert.deepEqual(activityOf(tasks, "dev"), { done: 1, failed: 0, cancelled: 0, active: 1, tokens: 100, delegated: 0, lastActiveMs: 5000 });
    assert.equal(activityLine(activityOf(tasks, "dev"), now), "1 concluída, 1 em andamento · 100 tokens · ativo há 4 min");
    assert.equal(activityOf(tasks, "ghost"), null);
  });

  test("a conversation more recent than any task moves the last activity, and an older one does not", () => {
    assert.equal(activityOf(tasks, "dev", { lastConversationMs: 9000 }).lastActiveMs, 9000);
    assert.equal(activityOf(tasks, "dev", { lastConversationMs: 2000 }).lastActiveMs, 5000);
  });

  test("an agent with no task but a conversation still has a line, and one with nothing at all has none", () => {
    const talked = activityOf(tasks, "writer", { lastConversationMs: 5000 });
    assert.deepEqual(talked, { done: 0, failed: 0, cancelled: 0, active: 0, tokens: 0, delegated: 0, lastActiveMs: 5000 });
    assert.equal(activityLine(talked, now), "ativo há 4 min");
    assert.equal(activityOf(tasks, "writer", {}), null);
  });

  test("the spend is said with the window the hub counts, and an agent that spent nothing says nothing of it", () => {
    const spend = { costUsd: 1.2, unpricedCalls: 0, windowHours: 24 };
    assert.equal(plain(activityLine(activityOf(tasks, "dev", { spend }), now)), "1 concluída, 1 em andamento · 100 tokens · US$ 1,20 nas últimas 24 h · ativo há 4 min");
    assert.equal(activityOf(tasks, "dev", { spend: { costUsd: 0, unpricedCalls: 0, windowHours: 24 } }).spend, undefined);
  });

  test("only the spend is enough to show a line, with no time of activity", () => {
    const only = activityOf([], "writer", { spend: { costUsd: 0.5, unpricedCalls: 0, windowHours: 6 } });
    assert.equal(only.lastActiveMs, null);
    assert.equal(plain(activityLine(only, now)), "US$ 0,50 nas últimas 6 h");
  });
});

describe("saying the spend", () => {
  test("a tiny amount, and calls the price list does not know", () => {
    assert.equal(plain(spendLabel({ costUsd: 0.004, unpricedCalls: 0, windowHours: 24 })), "menos de US$ 0,01 nas últimas 24 h");
    assert.equal(plain(spendLabel({ costUsd: 2, unpricedCalls: 3, windowHours: 24 })), "US$ 2,00 nas últimas 24 h (e 3 chamadas sem preço)");
    assert.equal(spendLabel({ costUsd: 0, unpricedCalls: 1, windowHours: 24 }), "1 chamada sem preço nas últimas 24 h");
  });
});

describe("reading the hub's reports", () => {
  const report = { recent: { windowHours: 12, byAgent: [{ key: "dev", costUsd: 3, unpricedCalls: 1 }, { key: "", costUsd: 9, unpricedCalls: 0 }] } };

  test("the spend of an agent comes from the ledger's by-agent split, with its window", () => {
    assert.deepEqual(spendOf(report, "dev"), { costUsd: 3, unpricedCalls: 1, windowHours: 12 });
    assert.equal(spendOf(report, "writer"), undefined, "an agent the ledger does not know");
  });

  test("an older hub, or no report yet, has no spend", () => {
    assert.equal(spendOf(null, "dev"), undefined);
    assert.equal(spendOf({}, "dev"), undefined);
    assert.equal(spendOf({ recent: { windowHours: 12 } }, "dev"), undefined);
  });

  test("the last conversation of an agent is the newest one that spoke with it, threads left out", () => {
    const conversations = [
      { id: "1", agentId: "dev", updatedAt: 100 },
      { id: "2", agentId: "dev", updatedAt: 300 },
      { id: "3", agentId: "dev", updatedAt: 900, parent: { conversationId: "2", messageId: "m" } },
      { id: "4", agentId: "writer", updatedAt: 500 },
      { id: "5", updatedAt: 700 },
    ];
    assert.equal(lastConversationOf(conversations, "dev"), 300);
    assert.equal(lastConversationOf(conversations, "ghost"), undefined);
  });
});

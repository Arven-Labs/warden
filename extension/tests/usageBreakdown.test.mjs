// P126 — the pure half of the token breakdowns (`src/sidepanel/lib/usageBreakdown.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { breakdownRows } from "../src/sidepanel/lib/usageBreakdown.ts";

const entry = (key, messageCount, totalTokens) => ({ key, messageCount, usage: { totalTokens } });

describe("the rows of a breakdown", () => {
  test("come biggest first, with the bar against the biggest", () => {
    const rows = breakdownRows([entry("small", 1, 5), entry("big", 3, 20), entry("mid", 2, 10)], "Sem agente");
    assert.deepEqual(rows.map((r) => [r.key, r.tokens, r.calls, r.share]), [["big", 20, 3, 1], ["mid", 10, 2, 0.5], ["small", 5, 1, 0.25]]);
  });

  test("call the bucket with no key by the label the screen gives it", () => {
    const rows = breakdownRows([entry(null, 1, 8), entry("writer", 1, 2)], "Sem agente");
    assert.deepEqual(rows.map((r) => [r.key, r.label]), [["", "Sem agente"], ["writer", "writer"]]);
  });

  test("are none for an older hub or an empty list", () => {
    assert.deepEqual(breakdownRows(undefined, "x"), []);
    assert.deepEqual(breakdownRows([], "x"), []);
  });

  test("keep a bar of zero when nobody used a token", () => {
    assert.deepEqual(breakdownRows([entry("a", 1, 0)], "x").map((r) => r.share), [0]);
  });
});

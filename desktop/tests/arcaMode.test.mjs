// Warden P183 — which controls the arca mode section draws and what a finished cycle says. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { summaryOf, viewOf } from "../src/lib/arcaMode.ts";

const status = (over) => ({ hasKey: true, on: false, pendingSend: false, ...over });
const report = (over) => ({ snapshotCreated: false, pulled: 0, merges: 0, conflicts: 0, filesChanged: 0, pushed: 0, ...over });

describe("which controls the section draws", () => {
  test("until the backend answers there is nothing to press", () => {
    assert.equal(viewOf(null), "loading");
  });

  test("a computer with a key and nothing pending can turn the mode on", () => {
    assert.equal(viewOf(status({})), "ready");
  });

  test("a computer without a key is told to pair first, and cannot turn it on", () => {
    assert.equal(viewOf(status({ hasKey: false })), "needsKey");
  });

  test("a Send waiting for the phone blocks turning it on", () => {
    assert.equal(viewOf(status({ pendingSend: true })), "pendingSend");
  });

  test("once it is on, the rest does not matter: the screen only offers to sync", () => {
    assert.equal(viewOf(status({ on: true })), "on");
    assert.equal(viewOf(status({ on: true, hasKey: false, pendingSend: true })), "on");
  });
});

describe("what a finished cycle says", () => {
  test("nothing moved: one short sentence", () => {
    assert.deepEqual(summaryOf(report({})), { key: "arca.syncNothing", conflicts: 0 });
  });

  test("files came in and objects went out: both numbers are told", () => {
    assert.deepEqual(summaryOf(report({ filesChanged: 2, pushed: 5, pulled: 4 })), { key: "arca.syncDone", values: { received: 2, sent: 5 }, conflicts: 0 });
  });

  test("a cycle that only published this computer's changes still counts as something", () => {
    assert.equal(summaryOf(report({ snapshotCreated: true, pushed: 3 })).key, "arca.syncDone");
  });

  test("a cycle that only pulled objects (no file changed here) still counts as something", () => {
    assert.equal(summaryOf(report({ pulled: 2 })).key, "arca.syncDone");
  });

  test("conflicts are told apart, so the screen can show them beside the sentence", () => {
    assert.equal(summaryOf(report({ merges: 1, conflicts: 2, filesChanged: 1, pushed: 1 })).conflicts, 2);
  });
});

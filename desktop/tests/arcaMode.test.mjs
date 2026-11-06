// Warden P183 — which controls the arca mode section draws and what a finished cycle says. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { hubChoice, summaryOf, viewOf } from "../src/lib/arcaMode.ts";

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

describe("which hub the selector shows for the automatic sync", () => {
  const home = { id: "hub-1", name: "Home hub" };
  const work = { id: "hub-2", name: "Work hub" };

  test("the hub the backend recorded is the one selected, not the first of the list", () => {
    assert.deepEqual(hubChoice("hub-2", [home, work]), { selected: "hub-2", removed: false });
  });

  test("with no hub recorded nothing is selected, even when there is only one saved", () => {
    assert.deepEqual(hubChoice(null, [home, work]), { selected: "", removed: false });
    assert.deepEqual(hubChoice(null, [home]), { selected: "", removed: false });
  });

  test("a recorded hub that is no longer saved selects nothing and says it was removed", () => {
    assert.deepEqual(hubChoice("hub-9", [home, work]), { selected: "", removed: true });
    assert.deepEqual(hubChoice("hub-9", []), { selected: "", removed: true });
  });

  test("before the list of saved hubs arrives there is no warning to give yet", () => {
    assert.deepEqual(hubChoice("hub-2", null), { selected: "", removed: false });
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

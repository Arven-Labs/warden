// Warden P183 — which controls the arca mode section draws and what a finished cycle says. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { hubChoice, loginsView, noticesOf, summaryOf, viewOf } from "../src/lib/arcaMode.ts";

const status = (over) => ({ hasKey: true, on: false, pendingSend: false, loginsAccepted: false, ...over });
const report = (over) => ({
  snapshotCreated: false,
  pulled: 0,
  merges: 0,
  conflicts: 0,
  filesChanged: 0,
  pushed: 0,
  secretLosersSaved: 0,
  secretIgnored: 0,
  secretSkipped: [],
  recoveredKept: 0,
  ...over,
});

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

describe("what the screen says about the MCP logins (class secret), which travel only after the person accepted", () => {
  test("before the mode is on there is nothing to say: the choice is made in the confirmation", () => {
    assert.equal(loginsView(null), "hidden");
    assert.equal(loginsView(status({})), "hidden");
    assert.equal(loginsView(status({ loginsAccepted: true })), "hidden");
  });

  test("a mode that is on and was never told to take the logins offers to take them (the case of whoever turned it on before)", () => {
    assert.equal(loginsView(status({ on: true, loginsAccepted: false })), "offer");
  });

  test("once accepted it says they are syncing, and offers nothing", () => {
    assert.equal(loginsView(status({ on: true, loginsAccepted: true })), "syncing");
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

describe("what a cycle says about the logins (the secret class)", () => {
  test("a clean cycle has nothing to warn about", () => {
    assert.deepEqual(noticesOf(report({})), { counts: [], skipped: [] });
  });

  test("a cycle that only has notices is still 'nothing new' for the sentence, so the notices are told apart", () => {
    const r = report({ secretIgnored: 1 });
    assert.equal(summaryOf(r).key, "arca.syncNothing");
    assert.equal(noticesOf(r).counts.length, 1);
  });

  test("each counter that is not zero gets its own line, with its number, and zero ones are left out", () => {
    const r = report({ secretIgnored: 2, recoveredKept: 1, secretLosersSaved: 3 });
    assert.deepEqual(noticesOf(r).counts, [
      { key: "arca.noticeIgnored", count: 2 },
      { key: "arca.noticeKept", count: 1 },
      { key: "arca.noticeLosers", count: 3 },
    ]);
    assert.deepEqual(noticesOf(report({ recoveredKept: 1 })).counts, [{ key: "arca.noticeKept", count: 1 }]);
  });

  test("the three reasons the arca gives for leaving a login out are translated, keeping the path", () => {
    const r = report({
      secretSkipped: [
        { path: "notes.txt", reason: "not accepted by this device" },
        { path: "con.json", reason: "reserved name" },
        { path: "GitHub.json", reason: "differs only by case from another file" },
      ],
    });
    assert.deepEqual(noticesOf(r).skipped, [
      { path: "notes.txt", reasonKey: "arca.skipNotAccepted", reason: "not accepted by this device" },
      { path: "con.json", reasonKey: "arca.skipReserved", reason: "reserved name" },
      { path: "GitHub.json", reasonKey: "arca.skipCase", reason: "differs only by case from another file" },
    ]);
  });

  test("a reason the arca may add later is shown as it came, never hidden", () => {
    const r = report({ secretSkipped: [{ path: "x.json", reason: "something new" }] });
    assert.deepEqual(noticesOf(r).skipped, [{ path: "x.json", reasonKey: null, reason: "something new" }]);
  });
});

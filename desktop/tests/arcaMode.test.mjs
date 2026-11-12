// Warden P183 — which controls the arca mode section draws and what a finished cycle says. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { hubChoice, loginsView, loopView, noticesOf, summaryOf, viewOf } from "../src/lib/arcaMode.ts";

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
  secretFolderMissing: false,
  vaultEmptied: false,
  secretEmptied: false,
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
    assert.deepEqual(noticesOf(report({})), { counts: [], skipped: [], folderMissing: false, vaultEmptied: false, loginsEmptied: false });
  });

  test("an emptied class is told apart, per class: it is not a counter and does not change the sentence", () => {
    const vault = report({ vaultEmptied: true });
    assert.deepEqual([noticesOf(vault).vaultEmptied, noticesOf(vault).loginsEmptied], [true, false]);
    const logins = report({ secretEmptied: true });
    assert.deepEqual([noticesOf(logins).vaultEmptied, noticesOf(logins).loginsEmptied], [false, true]);
    assert.deepEqual(noticesOf(logins).counts, []);
    assert.equal(summaryOf(logins).key, "arca.syncNothing");
  });

  test("a logins folder that is gone is told apart: the logins are not syncing and nothing was removed", () => {
    const r = report({ secretFolderMissing: true });
    assert.equal(noticesOf(r).folderMissing, true);
    assert.deepEqual(noticesOf(r).counts, [], "it is not a counter");
    assert.equal(summaryOf(r).key, "arca.syncNothing", "it does not change the sentence either");
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

describe("what the section says about the last automatic cycle", () => {
  const loop = (over) => ({ atMs: 1000, report: report({}), error: null, stopped: false, ...over });

  test("before any cycle ran there is nothing to say", () => {
    assert.deepEqual(loopView(null), { notices: null, error: null });
  });

  test("a clean cycle says nothing, so it clears an older warning", () => {
    assert.deepEqual(loopView(loop({})), { notices: null, error: null });
  });

  test("a cycle with a warning hands over the same notices the manual sync shows", () => {
    const view = loopView(loop({ report: report({ vaultEmptied: true, recoveredKept: 2 }) }));
    assert.equal(view.notices.vaultEmptied, true);
    assert.deepEqual(view.notices.counts, [{ key: "arca.noticeKept", count: 2 }]);
    assert.equal(view.error, null);
  });

  test("every kind of notice counts as something to say: a folder gone, logins emptied, a counter, a file left out", () => {
    for (const over of [{ secretFolderMissing: true }, { secretEmptied: true }, { secretLosersSaved: 1 }, { secretSkipped: [{ path: "a.bak", reason: "not accepted by this device" }] }]) {
      assert.notEqual(loopView(loop({ report: report(over) })).notices, null, JSON.stringify(over));
    }
  });

  test("a rejected token says the automatic sync stopped, with the detail", () => {
    const view = loopView(loop({ report: null, error: { kind: "authRejected", message: "401" }, stopped: true }));
    assert.deepEqual(view, { notices: null, error: { key: "arca.loopStopped", message: "401" } });
  });

  test("a hub that was only down says the last automatic sync failed, not that it stopped", () => {
    const view = loopView(loop({ report: null, error: { kind: "other", message: "connection refused" } }));
    assert.deepEqual(view, { notices: null, error: { key: "arca.loopFailed", message: "connection refused" } });
  });
});

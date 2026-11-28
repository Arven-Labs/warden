// Warden P183 — which controls the arca mode section draws and what a finished cycle says. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";
import { clearSummary, deletableIds, hubChoice, loginsView, loopView, noticesOf, rotationHalfDoneShown, sizeLabel, summaryOf, verifyErrorMessage, verifyView, viewOf } from "../src/lib/arcaMode.ts";

const status = (over) => ({ hasKey: true, on: false, pendingSend: false, loginsAccepted: false, rotationHalfDone: false, ...over });
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

describe("the line that says the automatic sync is paused by a key change left half way", () => {
  test("it shows only when the backend says a rotation was left half done and the mode is on (the only place the rotation block lives)", () => {
    assert.equal(rotationHalfDoneShown(status({ on: true, rotationHalfDone: true })), true);
    assert.equal(rotationHalfDoneShown(status({ on: true, rotationHalfDone: true, hasKey: false, pendingSend: true, loginsAccepted: true })), true);
  });

  test("it does not show while nothing is half done, whatever else the status says (a key that merely waits is not this)", () => {
    assert.equal(rotationHalfDoneShown(status({ on: true, rotationHalfDone: false })), false);
    assert.equal(rotationHalfDoneShown(status({ on: true, rotationHalfDone: false, pendingSend: true, loginsAccepted: true, hubId: "hub-1" })), false);
  });

  test("it does not show before the status arrives, nor with the mode off", () => {
    assert.equal(rotationHalfDoneShown(null), false);
    assert.equal(rotationHalfDoneShown(status({ on: false, rotationHalfDone: true })), false);
  });

  test("an answer that does not carry the field (an older backend) shows nothing", () => {
    const old = { hasKey: true, on: true, pendingSend: false, loginsAccepted: false };
    assert.equal(rotationHalfDoneShown(old), false);
  });

  // The words name the buttons the person has to click: `arca.rotationResume` opens the box for the 24 words, and `arca.cancel` is
  // there (ArcaRotationBlock.tsx); in the idle step the Cancel button does not exist yet.
  for (const [lang, texts, paused, words] of [
    ["English", en, /paused/, /24 words/],
    ["Portuguese", ptBR, /pausada/, /24 palavras/],
  ]) {
    test(`the text in ${lang} says it is paused, asks for the same 24 words and names the real buttons`, () => {
      const text = texts["arca.rotationHalfDone"];
      assert.ok(text, "the key exists");
      assert.match(text, paused);
      assert.match(text, words);
      assert.ok(text.includes(`"${texts["arca.rotationResume"].replace("…", "")}`), `names the button that resumes: ${text}`);
      assert.ok(text.includes(`"${texts["arca.cancel"]}"`), `names the button that gives up: ${text}`);
      assert.doesNotMatch(text, /\{/, "has no placeholder: nothing is passed to it");
    });
  }
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

describe("the copies of the losers of conflicts", () => {
  const copy = (id, over) => ({ id, class: "secret", path: "github.json", device: "dev-a", shaPrefix: "0011223344556677", edited: false, sizeBytes: 10, modifiedAtMs: 5, ...over });

  test("'delete all' sends every copy but the edited ones", () => {
    const items = [copy("a"), copy("b", { edited: true }), copy("c")];
    assert.deepEqual(deletableIds(items), ["a", "c"]);
    assert.deepEqual(deletableIds([]), []);
    assert.deepEqual(deletableIds([copy("x", { edited: true })]), [], "a list with only edited copies sends nothing");
  });

  test("what deleting did: the count, each copy that stayed with its translated reason, and the work files that may hold a login", () => {
    const summary = clearSummary({
      removed: 2,
      skipped: [
        { id: "a", reason: "edited" },
        { id: "b", reason: "notACopy" },
        { id: "c", reason: "gone" },
        { id: "d", reason: "failed" },
      ],
      keptTmp: 1,
    });
    assert.deepEqual(summary, {
      removed: 2,
      notRemoved: [
        { id: "a", reasonKey: "arca.loserSkipEdited" },
        { id: "b", reasonKey: "arca.loserSkipNotACopy" },
        { id: "c", reasonKey: "arca.loserSkipGone" },
        { id: "d", reasonKey: "arca.loserSkipFailed" },
      ],
      keptTmp: 1,
    });
  });

  test("a clean deletion has nothing that stayed and no work file", () => {
    assert.deepEqual(clearSummary({ removed: 1, skipped: [], keptTmp: 0 }), { removed: 1, notRemoved: [], keptTmp: 0 });
  });

  test("a size reads in bytes below 1 KB and with one decimal above, and an unknown size is a dash", () => {
    assert.equal(sizeLabel(0), "0 B");
    assert.equal(sizeLabel(10), "10 B");
    assert.equal(sizeLabel(1023), "1023 B");
    assert.equal(sizeLabel(1536), "1.5 KB");
    assert.equal(sizeLabel(1048576), "1.0 MB");
    assert.equal(sizeLabel(null), "—");
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

describe("what the section says about the integrity of the copy on the hub", () => {
  const counts = { manifestsOk: 2, rejected: 1, referencedBlobs: 9, missing: 2, damaged: 3, sampled: 9 };
  const verify = (over) => ({ atMs: 5000, clean: true, counts, error: null, stopped: false, ...over });
  const date = (ms) => `D${ms}`;

  test("nothing is said before the first answer", () => {
    assert.equal(verifyView(null, date), null);
  });

  test("a copy that was never checked says so", () => {
    assert.deepEqual(verifyView(verify({ atMs: 0, clean: false, counts: null }), date), { key: "arca.verifyNever", params: {} });
  });

  test("a clean round says all good, with its date", () => {
    assert.deepEqual(verifyView(verify(), date), { key: "arca.verifyClean", params: { date: "D5000" } });
  });

  test("a round with a problem gives the counts", () => {
    assert.deepEqual(verifyView(verify({ clean: false }), date), { key: "arca.verifyProblem", params: { date: "D5000", missing: 2, damaged: 3, rejected: 1 } });
  });

  test("a problem remembered from before this launch has no counts and does not invent them", () => {
    assert.deepEqual(verifyView(verify({ clean: false, counts: null }), date), { key: "arca.verifyProblemNoCounts", params: { date: "D5000" } });
  });

  test("a failed check says why, but a refused token is left to the sync banner", () => {
    assert.equal(verifyErrorMessage(verify({ error: { kind: "other", message: "connection refused" } })), "connection refused");
    assert.equal(verifyErrorMessage(verify({ error: { kind: "authRejected", message: "401" }, stopped: true })), null);
    assert.equal(verifyErrorMessage(verify()), null);
    assert.equal(verifyErrorMessage(null), null);
  });
});

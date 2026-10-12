// P47 — the words of the SSH usage log (`src/lib/sshAudit.ts`): what a call was about, whether a person approved it and how it ended.
// Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { actionLabel, approvalLabel, detailOf, outcomeOf, wasRefused } from "../src/lib/sshAudit.ts";

const entry = (extra = {}) => ({ atMs: 1000, hostId: "prod", action: "exec", approval: "not_required", overwrite: false, timedOut: false, ...extra });

describe("the SSH usage log", () => {
  test("a command is its own detail and a copy reads from where it comes to where it goes", () => {
    assert.equal(detailOf(entry({ command: "uptime" })), "uptime");
    assert.equal(detailOf(entry({ action: "upload", localPath: "/tmp/a", remotePath: "/srv/a", overwrite: true })), "/tmp/a → /srv/a (overwrite)");
    assert.equal(detailOf(entry({ action: "download", localPath: "/tmp/a", remotePath: "/srv/a" })), "/srv/a → /tmp/a", "a download leaves the server");
  });

  test("how a call ended: an error first, then the timeout, the exit code and the size of a copy", () => {
    assert.equal(outcomeOf(entry({ exitCode: 0 })), "exit 0");
    assert.equal(outcomeOf(entry({ timedOut: true })), "timed out");
    assert.equal(outcomeOf(entry({ error: "refused: not approved" })), "error: refused: not approved");
    assert.equal(outcomeOf(entry({ action: "upload", bytes: 512 })), "512 B");
    assert.equal(outcomeOf(entry({ action: "upload", bytes: 2048 })), "2 KB");
    assert.equal(outcomeOf(entry({ action: "download", bytes: 5 * 1024 * 1024 })), "5 MB");
    assert.equal(outcomeOf(entry()), "", "nothing to say when nothing ran");
  });

  test("the approval and the action read as a person would, and an unknown one shows as it came", () => {
    assert.equal(approvalLabel("approved"), "approved");
    assert.equal(approvalLabel("unavailable"), "nobody to ask");
    assert.equal(approvalLabel("something_new"), "something_new");
    assert.equal(actionLabel("exec"), "command");
    assert.equal(actionLabel("sync"), "sync");
  });

  test("only a denied call or one with nobody to ask counts as refused", () => {
    assert.equal(wasRefused(entry({ approval: "denied" })), true);
    assert.equal(wasRefused(entry({ approval: "unavailable" })), true);
    assert.equal(wasRefused(entry({ approval: "approved" })), false);
  });
});

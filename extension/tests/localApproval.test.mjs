// The yes the person gives, in the side panel, before `browser_type_text` writes anything (`src/protocol/localApproval.ts`). It has to fail closed:
// no answer, a dropped connection or a stray id never turns into a yes. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, mock, test } from "node:test";
import { LocalApprovals, isLocalApprovalId } from "../src/protocol/localApproval.ts";

const prompt = { target: "#q @ https://example.com", action: "browser_type_text", detail: "olá" };

describe("asking the person", () => {
  test("the question waits in the list the panel shows, with an id the hub's ids cannot be", () => {
    const approvals = new LocalApprovals();
    void approvals.request(prompt, 60_000);
    const [pending] = approvals.pending();
    assert.equal(pending.target, prompt.target);
    assert.equal(pending.action, prompt.action);
    assert.equal(isLocalApprovalId(pending.approvalId), true);
    assert.equal(isLocalApprovalId(0), false);
    assert.equal(isLocalApprovalId(17), false);
  });

  test("a yes resolves the question with true, and it leaves the list", async () => {
    const approvals = new LocalApprovals();
    const answer = approvals.request(prompt, 60_000);
    assert.equal(approvals.resolve(approvals.pending()[0].approvalId, true), true);
    assert.equal(await answer, true);
    assert.equal(approvals.pending().length, 0);
  });

  test("a no resolves it with false", async () => {
    const approvals = new LocalApprovals();
    const answer = approvals.request(prompt, 60_000);
    approvals.resolve(approvals.pending()[0].approvalId, false);
    assert.equal(await answer, false);
  });

  test("two questions at once get two ids and are answered apart, oldest first in the list", async () => {
    const approvals = new LocalApprovals();
    const first = approvals.request({ ...prompt, target: "a" }, 60_000);
    const second = approvals.request({ ...prompt, target: "b" }, 60_000);
    const [one, two] = approvals.pending();
    assert.notEqual(one.approvalId, two.approvalId);
    assert.deepEqual([one.target, two.target], ["a", "b"]);
    approvals.resolve(two.approvalId, true);
    approvals.resolve(one.approvalId, false);
    assert.deepEqual([await first, await second], [false, true]);
  });
});

describe("failing closed", () => {
  test("no answer in time is a no, and the question leaves the list", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const approvals = new LocalApprovals();
    const answer = approvals.request(prompt, 60_000);
    t.mock.timers.tick(59_999);
    assert.equal(approvals.pending().length, 1);
    t.mock.timers.tick(1);
    assert.equal(await answer, false);
    assert.equal(approvals.pending().length, 0);
  });

  test("an answer after the time is over changes nothing", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const approvals = new LocalApprovals();
    const answer = approvals.request(prompt, 1_000);
    const id = approvals.pending()[0].approvalId;
    t.mock.timers.tick(1_000);
    assert.equal(approvals.resolve(id, true), false);
    assert.equal(await answer, false);
  });

  test("an id that is not waiting, or one answered twice, is not a yes for anything", async () => {
    const approvals = new LocalApprovals();
    const answer = approvals.request(prompt, 60_000);
    const id = approvals.pending()[0].approvalId;
    assert.equal(approvals.resolve(id + 1, true), false);
    assert.equal(approvals.resolve(5, true), false);
    assert.equal(approvals.resolve(id, false), true);
    assert.equal(approvals.resolve(id, true), false);
    assert.equal(await answer, false);
  });

  test("denying everything (the connection dropped, so the answer could not be delivered) answers every question with false", async () => {
    const approvals = new LocalApprovals();
    const answers = [approvals.request(prompt, 60_000), approvals.request(prompt, 60_000)];
    approvals.denyAll();
    assert.deepEqual(await Promise.all(answers), [false, false]);
    assert.equal(approvals.pending().length, 0);
  });

  test("a time that makes no sense is a no at once rather than waiting forever", async () => {
    const approvals = new LocalApprovals();
    for (const ms of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      assert.equal(await approvals.request(prompt, ms), false, String(ms));
    }
    assert.equal(approvals.pending().length, 0);
  });
});

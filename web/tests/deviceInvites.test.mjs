// P36 — what the browser reads of the invites that wait (`src/hub/messages.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { decode } from "../src/hub/messages.ts";

describe("decoding the invites that wait", () => {
  test("a list keeps each name and its deadline, and has no code to keep", () => {
    const reply = decode(JSON.stringify({ type: "deviceInviteList", requestId: 7, invites: [{ name: "Ana's phone", createdMs: 1, expiresMs: 9 }] }));
    assert.deepEqual(reply, { type: "deviceInviteList", requestId: 7, invites: [{ name: "Ana's phone", createdMs: 1, expiresMs: 9 }] });
  });

  test("a cancel that the hub refuses comes through as a deviceError with authRejected", () => {
    const reply = decode(JSON.stringify({ type: "deviceError", requestId: 8, message: "wrong pairing key", authRejected: true }));
    assert.equal(reply.type, "deviceError");
    assert.equal(reply.authRejected, true);
  });
});

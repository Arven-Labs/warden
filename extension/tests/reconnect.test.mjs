// The pure half of the extension's automatic reconnect (`src/protocol/reconnect.ts`): how long to wait before the next try, whether a failure
// is worth trying again, and whether a freshly started service worker should connect on its own. Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { AUTH_REJECTED_PREFIX, BASE_DELAY_MS, MAX_DELAY_MS, canAutoConnect, nextDelayMs, retryDelayMs } from "../src/protocol/reconnect.ts";

const always = (value) => () => value;

describe("the wait before the next try", () => {
  test("doubles with each failed try, from the base", () => {
    const delays = [0, 1, 2, 3].map((attempt) => nextDelayMs(attempt, always(1)));
    assert.deepEqual(delays, [BASE_DELAY_MS, BASE_DELAY_MS * 2, BASE_DELAY_MS * 4, BASE_DELAY_MS * 8]);
  });

  test("never passes the ceiling, however many tries failed", () => {
    for (const attempt of [10, 20, 100, 10_000]) {
      assert.equal(nextDelayMs(attempt, always(1)), MAX_DELAY_MS, `attempt ${attempt}`);
    }
  });

  test("the jitter only shortens the wait, to at most half of it, so it is never zero", () => {
    assert.equal(nextDelayMs(0, always(0)), BASE_DELAY_MS / 2);
    assert.equal(nextDelayMs(20, always(0)), MAX_DELAY_MS / 2);
    for (const random of [0, 0.25, 0.5, 0.999]) {
      const delay = nextDelayMs(3, always(random));
      assert.ok(delay >= (BASE_DELAY_MS * 8) / 2 && delay <= BASE_DELAY_MS * 8, `random ${random} gave ${delay}`);
    }
  });

  test("a try number that makes no sense counts as the first", () => {
    for (const attempt of [-1, Number.NaN, Number.NEGATIVE_INFINITY]) {
      assert.equal(nextDelayMs(attempt, always(1)), BASE_DELAY_MS, `attempt ${attempt}`);
    }
  });
});

describe("whether a failure is tried again", () => {
  const failure = (message) => ({ kind: "failure", message });

  test("a dropped or unreachable hub is tried again, with the wait of that try", () => {
    for (const message of ["Connection closed unexpectedly", "WebSocket error", "connection error", "No response to Hello within 10s"]) {
      assert.equal(retryDelayMs(true, failure(message), 2, always(1)), nextDelayMs(2, always(1)), message);
    }
  });

  test("never when the person did not ask to be connected (they disconnected, or never connected)", () => {
    assert.equal(retryDelayMs(false, failure("Connection closed unexpectedly"), 0, always(1)), null);
  });

  test("never when the hub refused the key: the hub counts tries per address, so insisting would lock the person out", () => {
    assert.equal(retryDelayMs(true, failure(`${AUTH_REJECTED_PREFIX}: wrong pairing key`), 0, always(1)), null);
    assert.equal(retryDelayMs(true, failure(`${AUTH_REJECTED_PREFIX}: too many attempts, try again later`), 0, always(1)), null);
  });

  test("the words the person already sees for a refusal stay as they are", () => {
    assert.equal(AUTH_REJECTED_PREFIX, "authentication rejected");
  });

  test("only a failure is retried: connecting, connected and a disconnect the person chose are not", () => {
    for (const status of [{ kind: "connecting" }, { kind: "connected", serverName: "hub" }, { kind: "disconnected" }, { kind: "disconnected", reason: "bye" }]) {
      assert.equal(retryDelayMs(true, status, 0, always(1)), null, status.kind);
    }
  });
});

describe("a service worker that has just started", () => {
  const saved = { host: "192.168.0.5", port: 4021, deviceName: "Chrome", authKey: "k", secure: false };

  test("connects on its own when the person was connected and the address is saved", () => {
    assert.equal(canAutoConnect(true, saved), true);
  });

  test("does not when the person had disconnected, or had never connected", () => {
    assert.equal(canAutoConnect(false, saved), false);
  });

  test("does not without a saved address, or with one that cannot be dialled", () => {
    assert.equal(canAutoConnect(true, {}), false);
    assert.equal(canAutoConnect(true, { ...saved, host: "" }), false);
    assert.equal(canAutoConnect(true, { ...saved, port: 0 }), false);
    assert.equal(canAutoConnect(true, { ...saved, port: 70_000 }), false);
    assert.equal(canAutoConnect(true, { ...saved, deviceName: "" }), false);
  });
});

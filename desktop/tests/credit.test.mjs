import assert from "node:assert/strict";
import { test } from "node:test";
import { creditUsd, formatUsd, hasHolds, isOutOfCredit, topUpUrl } from "../src/lib/credit.ts";

test("only a plain https address with no login in it is opened to top up", () => {
  assert.equal(topUpUrl("https://example.com/credits"), "https://example.com/credits");
  assert.equal(topUpUrl("https://example.com"), "https://example.com/");
  // A hub that sends nothing, or something that can run code or leak a login, reads as "no link": the usage screen opens instead.
  for (const bad of [undefined, null, "", "http://example.com/credits", "javascript:alert(1)", "file:///etc/passwd", "https://user:pass@example.com/", "not a url"]) {
    assert.equal(topUpUrl(bad), null, String(bad));
  }
});

test("dollars get more decimals under one dollar", () => {
  assert.equal(formatUsd(25), "$25.00");
  assert.equal(formatUsd(0.5), "$0.5000");
  assert.equal(formatUsd(0.012345), "$0.0123");
});

test("micro-USD turn into dollars for display", () => {
  assert.equal(creditUsd(25_000_000), 25);
  assert.equal(creditUsd(1_500_000), 1.5);
  assert.equal(creditUsd(1), 0.000001);
  assert.equal(creditUsd(0), 0);
});

test("a call is refused only when nothing is available, whatever the balance says", () => {
  assert.equal(isOutOfCredit({ balanceMicroUsd: 5_000_000, availableMicroUsd: 4_000_000 }), false);
  assert.equal(isOutOfCredit({ balanceMicroUsd: 0, availableMicroUsd: 0 }), true);
  // Everything is held by calls in progress: there is a balance, but no new call can start.
  assert.equal(isOutOfCredit({ balanceMicroUsd: 3_000_000, availableMicroUsd: 0 }), true);
  assert.equal(isOutOfCredit({ balanceMicroUsd: 10, availableMicroUsd: -5 }), true);
});

test("the available amount is shown only when calls in progress make it differ from the balance", () => {
  assert.equal(hasHolds({ balanceMicroUsd: 5_000_000, availableMicroUsd: 5_000_000 }), false);
  assert.equal(hasHolds({ balanceMicroUsd: 5_000_000, availableMicroUsd: 4_000_000 }), true);
});

// P157 — what a turn may carry (`src/lib/attachments.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { checkAttachmentLimits, isPdf, MAX_ATTACHMENTS_BASE64_BYTES, MAX_ATTACHMENTS_PER_TURN } from "../src/lib/attachments.ts";

const attachment = (mimeType, size = 10) => ({ mimeType, data: "x".repeat(size) });

describe("an attachment", () => {
  test("is a PDF only when its type says so", () => {
    assert.equal(isPdf(attachment("application/pdf")), true);
    assert.equal(isPdf(attachment("image/png")), false);
    assert.equal(isPdf(attachment("audio/wav")), false);
  });
});

describe("the limits of a message", () => {
  test("lets one more in while it fits", () => {
    assert.equal(checkAttachmentLimits([], attachment("application/pdf")), null);
    const nine = Array.from({ length: MAX_ATTACHMENTS_PER_TURN - 1 }, () => attachment("image/png"));
    assert.equal(checkAttachmentLimits(nine, attachment("image/png")), null);
  });

  test("refuses an eleventh", () => {
    const ten = Array.from({ length: MAX_ATTACHMENTS_PER_TURN }, () => attachment("image/png"));
    assert.match(checkAttachmentLimits(ten, attachment("image/png")), /at most 10/);
  });

  test("counts the base64 of what is already there together with what arrives, to the hub's cap", () => {
    const half = attachment("application/pdf", MAX_ATTACHMENTS_BASE64_BYTES / 2);
    assert.equal(checkAttachmentLimits([half], half), null, "exactly the cap still goes");
    const over = checkAttachmentLimits([half], attachment("image/png", MAX_ATTACHMENTS_BASE64_BYTES / 2 + 1));
    assert.match(over, /too big \(12\.0 MB; the most a message can carry is 12 MB\)/);
  });
});

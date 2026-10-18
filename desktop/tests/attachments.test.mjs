// P157 — what a turn may carry (`src/lib/attachments.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import {
  checkAttachmentLimits,
  fileToAttachment,
  isAttachmentFile,
  isPdf,
  MAX_ATTACHMENT_FILE_BYTES,
  MAX_ATTACHMENTS_BASE64_BYTES,
  MAX_ATTACHMENTS_PER_TURN,
} from "../src/lib/attachments.ts";

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

describe("a file pasted from the clipboard", () => {
  const file = (type, bytes) => new File([bytes], "pasted", { type });

  test("is one of the types a message carries, and nothing else", () => {
    for (const type of ["image/png", "image/jpeg", "image/webp", "image/gif", "application/pdf"]) assert.equal(isAttachmentFile({ type }), true, type);
    for (const type of ["", "text/plain", "image/svg+xml", "audio/wav"]) assert.equal(isAttachmentFile({ type }), false, type);
  });

  test("becomes an attachment with its type and its bytes in base64", async () => {
    assert.deepEqual(await fileToAttachment(file("application/pdf", "%PDF-1.4")), { mimeType: "application/pdf", data: "JVBERi0xLjQ=" });
  });

  test("takes a big file in pieces, without overflowing the stack, and gives back the same bytes", async () => {
    const bytes = Uint8Array.from({ length: 3 * 1024 * 1024 }, (_, i) => i % 251);
    const { data } = await fileToAttachment(file("image/png", bytes));
    assert.deepEqual(new Uint8Array(Buffer.from(data, "base64")), bytes);
  });

  test("refuses another type, and a file over the most a message can carry", async () => {
    await assert.rejects(fileToAttachment(file("text/plain", "hi")), /Unsupported file type/);
    const big = { type: "application/pdf", size: MAX_ATTACHMENT_FILE_BYTES + 1, arrayBuffer: async () => new ArrayBuffer(0) };
    await assert.rejects(fileToAttachment(big), /the most a message can carry is 9 MB/);
    const exactly = { type: "application/pdf", size: MAX_ATTACHMENT_FILE_BYTES, arrayBuffer: async () => new ArrayBuffer(0) };
    assert.deepEqual(await fileToAttachment(exactly), { mimeType: "application/pdf", data: "" }, "exactly the cap still goes");
  });
});

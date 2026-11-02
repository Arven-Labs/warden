// P157 — what a turn may carry (`src/lib/attachments.ts`). Run with `npm test`.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import {
  canShrink,
  checkAttachmentLimits,
  fileToAttachment,
  fitAttachment,
  SHRINK_STEPS,
  shrunkMimeType,
  targetSize,
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

describe("shrinking a picture that would not fit (P157)", () => {
  const cap = MAX_ATTACHMENTS_BASE64_BYTES;

  test("only a still picture can be shrunk: not a GIF (it would lose its frames), not a PDF", () => {
    for (const type of ["image/png", "image/jpeg", "image/webp"]) assert.equal(canShrink(attachment(type)), true, type);
    for (const type of ["image/gif", "application/pdf"]) assert.equal(canShrink(attachment(type)), false, type);
  });

  test("the steps go from the biggest side down", () => {
    assert.deepEqual(SHRINK_STEPS, [2048, 1536, 1024]);
  });

  test("the target keeps the proportion with the longest side at the step, and never makes a picture bigger", () => {
    assert.deepEqual(targetSize(4000, 3000, 2048), { width: 2048, height: 1536 });
    assert.deepEqual(targetSize(3000, 4000, 2048), { width: 1536, height: 2048 });
    assert.deepEqual(targetSize(1000, 500, 2048), { width: 1000, height: 500 });
    assert.deepEqual(targetSize(2048, 2048, 2048), { width: 2048, height: 2048 });
    assert.deepEqual(targetSize(10000, 1, 2048), { width: 2048, height: 1 }, "a thin strip keeps at least one pixel");
  });

  test("a PNG stays PNG (it may be transparent); the other pictures come out as JPEG", () => {
    assert.equal(shrunkMimeType("image/png"), "image/png");
    assert.equal(shrunkMimeType("image/jpeg"), "image/jpeg");
    assert.equal(shrunkMimeType("image/webp"), "image/jpeg");
  });

  test("what already fits is handed back untouched, without shrinking anything", async () => {
    let calls = 0;
    const incoming = attachment("image/jpeg", cap);
    const result = await fitAttachment([], incoming, async () => (calls++, incoming));
    assert.equal(result, incoming);
    assert.equal(calls, 0);
  });

  test("a picture that does not fit is shrunk by the first step, and stops there when that is enough", async () => {
    const steps = [];
    const big = attachment("image/jpeg", cap + 1);
    const small = attachment("image/jpeg", cap / 2);
    const result = await fitAttachment([], big, async (_, side) => (steps.push(side), small));
    assert.equal(result, small);
    assert.deepEqual(steps, [2048]);
  });

  test("goes to the next step while it still does not fit, counting what is already in the box", async () => {
    const steps = [];
    const inBox = [attachment("image/png", cap / 2)];
    const sizes = { 2048: cap, 1536: cap / 2 + 1, 1024: cap / 4 };
    const result = await fitAttachment(inBox, attachment("image/png", cap), async (_, side) => (steps.push(side), attachment("image/png", sizes[side])));
    assert.deepEqual(steps, [2048, 1536, 1024]);
    assert.equal(result.data.length, cap / 4);
    assert.equal(checkAttachmentLimits(inBox, result), null);
  });

  test("when not even the smallest step fits, hands back the original so the usual refusal is the one shown", async () => {
    const big = attachment("image/png", cap + 1);
    const result = await fitAttachment([], big, async () => attachment("image/png", cap + 1));
    assert.equal(result, big);
    assert.match(checkAttachmentLimits([], result), /too big/);
  });

  test("never tries to shrink a GIF or a PDF, nor when the box is already full of attachments", async () => {
    let calls = 0;
    const shrink = async (a) => (calls++, a);
    const gif = attachment("image/gif", cap + 1);
    const pdf = attachment("application/pdf", cap + 1);
    assert.equal(await fitAttachment([], gif, shrink), gif);
    assert.equal(await fitAttachment([], pdf, shrink), pdf);
    const full = Array.from({ length: MAX_ATTACHMENTS_PER_TURN }, () => attachment("image/png"));
    const png = attachment("image/png", cap + 1);
    assert.equal(await fitAttachment(full, png, shrink), png);
    assert.equal(calls, 0);
  });
});

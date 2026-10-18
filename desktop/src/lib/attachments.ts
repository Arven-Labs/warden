// P157 — what a turn may carry: images and PDFs. Pure, so it can be tested without a window. The limits are the hub's
// (`chat_input.rs`: `MAX_ATTACHMENTS_PER_TURN`, `MAX_ATTACHMENTS_BASE64_BYTES`), so a message the hub would refuse is refused
// here, before it is sent. Mirrors `web/src/hub/attachments.ts`.

import type { Attachment } from "../types";

export const MAX_ATTACHMENTS_PER_TURN = 10;
/** The hub's cap on a turn's attachments, in base64. */
export const MAX_ATTACHMENTS_BASE64_BYTES = 12 * 1024 * 1024;

/** The biggest file a message takes: what fits in the hub's base64 cap. The same 9 MiB `read_attachment` (Rust) refuses above. */
export const MAX_ATTACHMENT_FILE_BYTES = 9 * 1024 * 1024;

/** The types a message carries: the hub's (`USER_ATTACHMENT_MIME_TYPES`). */
export const ATTACHMENT_MIME_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif", "application/pdf"];

/** Whether a pasted file is one of the types a message carries. */
export function isAttachmentFile(file: { type: string }): boolean {
  return ATTACHMENT_MIME_TYPES.includes(file.type);
}

/** A file from the clipboard as an attachment (a screenshot or a copied picture has no path on disk to hand to `read_attachment`).
 * Throws the reason when the type isn't one a message carries or the file is too big. */
export async function fileToAttachment(file: { type: string; size: number; arrayBuffer: () => Promise<ArrayBuffer> }): Promise<Attachment> {
  if (!isAttachmentFile(file)) throw new Error("Unsupported file type");
  if (file.size > MAX_ATTACHMENT_FILE_BYTES) {
    throw new Error(`This file is ${(file.size / 1024 / 1024).toFixed(1)} MB; the most a message can carry is ${MAX_ATTACHMENT_FILE_BYTES / 1024 / 1024} MB`);
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  // In pieces: spreading a whole file into `String.fromCharCode` overflows the stack.
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return { mimeType: file.type, data: btoa(binary) };
}

/** Whether the attachment is a PDF, which is shown as a file and not as a picture. */
export function isPdf(attachment: Attachment): boolean {
  return attachment.mimeType === "application/pdf";
}

/** Why `incoming` can't join `current` in one message, or `null` when it can. */
export function checkAttachmentLimits(current: Attachment[], incoming: Attachment): string | null {
  if (current.length + 1 > MAX_ATTACHMENTS_PER_TURN) return `A message can carry at most ${MAX_ATTACHMENTS_PER_TURN} images or PDFs`;
  const total = [...current, incoming].reduce((sum, a) => sum + a.data.length, 0);
  if (total > MAX_ATTACHMENTS_BASE64_BYTES) {
    return `The attachments are too big (${(total / 1024 / 1024).toFixed(1)} MB; the most a message can carry is ${MAX_ATTACHMENTS_BASE64_BYTES / 1024 / 1024} MB)`;
  }
  return null;
}

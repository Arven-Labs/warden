// P157 — what a turn may carry: images and PDFs. Pure, so it can be tested without a window. The limits are the hub's
// (`chat_input.rs`: `MAX_ATTACHMENTS_PER_TURN`, `MAX_ATTACHMENTS_BASE64_BYTES`), so a message the hub would refuse is refused
// here, before it is sent. Mirrors `web/src/hub/attachments.ts`.

import type { Attachment } from "../types";

export const MAX_ATTACHMENTS_PER_TURN = 10;
/** The hub's cap on a turn's attachments, in base64. */
export const MAX_ATTACHMENTS_BASE64_BYTES = 12 * 1024 * 1024;

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

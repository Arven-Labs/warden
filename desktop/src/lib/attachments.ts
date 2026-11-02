// P157 — what a turn may carry: images and PDFs. Pure, so it can be tested without a window. The limits are the hub's
// (`chat_input.rs`: `MAX_ATTACHMENTS_PER_TURN`, `MAX_ATTACHMENTS_BASE64_BYTES`), so a message the hub would refuse is refused
// here, before it is sent. Mirrors `web/src/hub/attachments.ts`.

import { t } from "../i18n/index.ts";
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
  if (!isAttachmentFile(file)) throw new Error(t("attach.unsupported"));
  if (file.size > MAX_ATTACHMENT_FILE_BYTES) {
    throw new Error(t("attach.fileTooBig", { mb: (file.size / 1024 / 1024).toFixed(1), max: MAX_ATTACHMENT_FILE_BYTES / 1024 / 1024 }));
  }
  return { mimeType: file.type, data: bytesToBase64(new Uint8Array(await file.arrayBuffer())) };
}

function bytesToBase64(bytes: Uint8Array): string {
  // In pieces: spreading a whole file into `String.fromCharCode` overflows the stack.
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

/** Whether the attachment is a PDF, which is shown as a file and not as a picture. */
export function isPdf(attachment: Attachment): boolean {
  return attachment.mimeType === "application/pdf";
}

/** Why `incoming` can't join `current` in one message, or `null` when it can. */
export function checkAttachmentLimits(current: Attachment[], incoming: Attachment): string | null {
  if (current.length + 1 > MAX_ATTACHMENTS_PER_TURN) return t("attach.tooMany", { max: MAX_ATTACHMENTS_PER_TURN });
  const total = [...current, incoming].reduce((sum, a) => sum + a.data.length, 0);
  if (total > MAX_ATTACHMENTS_BASE64_BYTES) {
    return t("attach.tooLarge", { mb: (total / 1024 / 1024).toFixed(1), max: MAX_ATTACHMENTS_BASE64_BYTES / 1024 / 1024 });
  }
  return null;
}

// P157 — a picture that would not fit in the message is shrunk here instead of refused. The decisions are pure (and tested);
// only `shrinkImage` draws, and it needs a window's canvas.

/** The longest side a shrunk picture gets, biggest first: the next one is tried only while the picture still does not fit. */
export const SHRINK_STEPS = [2048, 1536, 1024];

/** JPEG quality of a shrunk photo. */
const SHRINK_JPEG_QUALITY = 0.85;

/** Whether the attachment is a still picture that can be redrawn smaller. A GIF would lose its frames, and a PDF is not a picture. */
export function canShrink(attachment: Attachment): boolean {
  return ["image/png", "image/jpeg", "image/webp"].includes(attachment.mimeType);
}

/** The size of a picture whose longest side is brought down to `longestSide`, keeping the proportion; never bigger than it is. */
export function targetSize(width: number, height: number, longestSide: number): { width: number; height: number } {
  const scale = Math.min(1, longestSide / Math.max(width, height));
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) };
}

/** What a shrunk picture is saved as: a PNG stays PNG (it may be transparent), the others become JPEG. */
export function shrunkMimeType(mimeType: string): string {
  return mimeType === "image/png" ? "image/png" : "image/jpeg";
}

/** Redraws the picture with its longest side at `longestSide` (the browser's canvas; not testable without a window). */
export async function shrinkImage(attachment: Attachment, longestSide: number): Promise<Attachment> {
  const source = Uint8Array.from(atob(attachment.data), (c) => c.charCodeAt(0));
  // "from-image" applies the photo's rotation (EXIF) now, because re-encoding drops it.
  const bitmap = await createImageBitmap(new Blob([source], { type: attachment.mimeType }), { imageOrientation: "from-image" });
  try {
    const { width, height } = targetSize(bitmap.width, bitmap.height, longestSide);
    // A canvas in the page, not an `OffscreenCanvas`: the webview on Linux (WebKitGTK) may be too old for the latter.
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) throw new Error(t("attach.shrinkFailed"));
    const mimeType = shrunkMimeType(attachment.mimeType);
    if (mimeType === "image/jpeg") {
      // JPEG has no transparency: without this a transparent area would come out black.
      context.fillStyle = "#fff";
      context.fillRect(0, 0, width, height);
    }
    context.drawImage(bitmap, 0, 0, width, height);
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, mimeType, SHRINK_JPEG_QUALITY));
    if (!blob) throw new Error(t("attach.shrinkFailed"));
    return { mimeType, data: bytesToBase64(new Uint8Array(await blob.arrayBuffer())) };
  } finally {
    bitmap.close();
  }
}

/** The attachment as it should join `current`: itself when it fits (or when shrinking cannot help: not a still picture, or the
 * box already has the most attachments), else the first of `SHRINK_STEPS` that makes it fit. When none does, the original, so the
 * usual refusal of `checkAttachmentLimits` is the one shown. `shrink` is `shrinkImage` in the window. */
export async function fitAttachment(
  current: Attachment[],
  incoming: Attachment,
  shrink: (attachment: Attachment, longestSide: number) => Promise<Attachment>,
): Promise<Attachment> {
  if (checkAttachmentLimits(current, incoming) === null) return incoming;
  if (!canShrink(incoming) || current.length + 1 > MAX_ATTACHMENTS_PER_TURN) return incoming;
  for (const side of SHRINK_STEPS) {
    const shrunk = await shrink(incoming, side);
    if (checkAttachmentLimits(current, shrunk) === null) return shrunk;
  }
  return incoming;
}

// P157 — a text or code file goes into the message text, not as an attachment: the hub and the providers take only images and PDFs.

/** The most a text file may be: it becomes part of the prompt. The same 1 MiB `read_text_file` (Rust) refuses above. */
export const MAX_TEXT_FILE_BYTES = 1024 * 1024;

/** The extensions of the files that go into the message text: the list of `read_text_file` (Rust). */
export const TEXT_FILE_EXTENSIONS = [
  "txt", "md", "markdown", "csv", "tsv", "json", "yaml", "yml", "toml", "xml", "html", "css", "js", "ts", "tsx", "jsx", "py", "rs", "go", "java", "c", "h", "cpp", "sh",
  "sql", "log", "ini",
];

/** The extension of the file name at the end of `path` (either kind of slash), in lower case; empty when it has none. */
function extensionOf(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot <= 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/** Whether the path is a text or code file that goes into the message text (by its extension). */
export function isTextPath(path: string): boolean {
  return TEXT_FILE_EXTENSIONS.includes(extensionOf(path));
}

/** The file as a block of the message: its name, then its text fenced with the language of its extension. The fence is one backtick
 * longer than the longest run of them in the text (three at least), or a file with ``` inside would close it early. */
export function textFileBlock(name: string, text: string): string {
  const longestRun = Math.max(0, ...(text.match(/`+/g) ?? []).map((run) => run.length));
  const fence = "`".repeat(Math.max(3, longestRun + 1));
  const body = text.endsWith("\n") ? text.slice(0, -1) : text;
  return `${name}\n${fence}${extensionOf(name)}\n${body}\n${fence}`;
}

/** The draft with `block` at its end after a blank line, and a blank line after it to keep typing. */
export function appendToDraft(draft: string, block: string): string {
  const before = draft.trimEnd();
  return `${before === "" ? "" : `${before}\n\n`}${block}\n\n`;
}

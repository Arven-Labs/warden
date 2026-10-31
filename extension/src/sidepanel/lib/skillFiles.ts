// P72 — the pure half of a skill's attached files in the skills tab (`SkillsView.tsx`). The checks mirror the hub's
// (`warden_core::skill::validate_file_name`, 64 KB, 20 per skill) so a bad file fails before it is sent; the hub stays the real
// barrier. Run its tests with `npm test`.

import type { SkillFileInfo } from "../../protocol/messages";

export const MAX_FILE_BYTES = 64 * 1024;
export const MAX_FILES_PER_SKILL = 20;
export const MAX_FILE_NAME_LEN = 64;

export type FileNameProblem = "length" | "chars" | "dot";

/** Why `file` can't be an attachment's name, or `null` when it can: 1 to 64 of `[A-Za-z0-9._-]`, not starting with a dot. The
 * panel turns the problem into a message. */
export function validateFileName(file: string): FileNameProblem | null {
  if (file.length === 0 || file.length > MAX_FILE_NAME_LEN) return "length";
  if (!/^[A-Za-z0-9._-]+$/.test(file)) return "chars";
  if (file.startsWith(".")) return "dot";
  return null;
}

export type DecodedAttachment = { ok: true; text: string } | { ok: false; reason: "tooBig" | "notText" };

/** The text of a chosen file, or why it can't be one: more than 64 KB (counted in bytes, as the hub does), not valid UTF-8, or
 * holding a NUL. */
export function decodeAttachment(data: Uint8Array): DecodedAttachment {
  if (data.byteLength > MAX_FILE_BYTES) return { ok: false, reason: "tooBig" };
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(data);
  } catch {
    return { ok: false, reason: "notText" };
  }
  if (text.includes("\0")) return { ok: false, reason: "notText" };
  return { ok: true, text };
}

/** Whether `name` can be saved on a skill that has `files` already: replacing one always fits, a new name only below twenty. */
export function canAttach(files: SkillFileInfo[] | undefined, name: string): boolean {
  const current = files ?? [];
  return current.some((f) => f.name === name) || current.length < MAX_FILES_PER_SKILL;
}

// The list of keys whose skill packages the hub's owner trusts (P160), as the screen edits it. Kept out of the component so the rules are
// testable without a DOM. The hub checks the list again when it is saved (`validate_publishers`); these are the same
// rules, so the person hears about a bad key before a round trip, not instead of it.

export interface Publisher {
  /** The Ed25519 public key, 64 lowercase hex characters. */
  key: string;
  name: string;
}

export const MAX_PUBLISHERS = 50;
export const MAX_NAME_CHARS = 80;
const SHORT_KEY_CHARS = 16;

/** What is wrong with a key or name about to be added; the text for each is `skills.trustProblem.<problem>`. */
export type DraftProblem = "badKey" | "duplicate" | "nameLong" | "nameLines" | "tooMany";

/** The key as it is compared (trimmed, lowercase), or `null` when it is not 64 hex characters. */
export function normalizeKey(raw: string): string | null {
  const key = raw.trim().toLowerCase();
  return /^[0-9a-f]{64}$/.test(key) ? key : null;
}

/** The first problem with adding `key` and `name` to `list`, or `null` when it can go in. */
export function checkDraft(list: readonly Publisher[], key: string, name: string): DraftProblem | null {
  const normal = normalizeKey(key);
  if (normal === null) return "badKey";
  if (list.some((p) => p.key === normal)) return "duplicate";
  if (/[\r\n]/.test(name)) return "nameLines";
  if ([...name.trim()].length > MAX_NAME_CHARS) return "nameLong";
  if (list.length >= MAX_PUBLISHERS) return "tooMany";
  return null;
}

/** `list` with the key added (a draft that `checkDraft` refuses leaves it as it is). */
export function addPublisher(list: readonly Publisher[], key: string, name: string): Publisher[] {
  if (checkDraft(list, key, name) !== null) return [...list];
  return [...list, { key: normalizeKey(key) as string, name: name.trim() }];
}

/** Trusting one key from the import preview: the list to save with it added, or why it cannot be (`duplicate` means it is already
 * listed, so there is nothing to save). */
export function trustKey(list: readonly Publisher[], key: string, name: string): { list: Publisher[] } | { problem: DraftProblem } {
  const problem = checkDraft(list, key, name);
  return problem === null ? { list: addPublisher(list, key, name) } : { problem };
}

/** `list` without `key`. */
export function removePublisher(list: readonly Publisher[], key: string): Publisher[] {
  return list.filter((p) => p.key !== key);
}

/** Whether two lists hold the same keys with the same names, in the same order: what says there is something to save. */
export function samePublishers(a: readonly Publisher[], b: readonly Publisher[]): boolean {
  return a.length === b.length && a.every((p, i) => p.key === b[i].key && p.name === b[i].name);
}

/** The first characters of a key, enough to tell two keys apart in a line of text. */
export function shortKey(key: string): string {
  return key.slice(0, SHORT_KEY_CHARS);
}

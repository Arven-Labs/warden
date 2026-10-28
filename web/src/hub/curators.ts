// The list of curators the hub's owner takes skill packages from (P160), as the screen edits it. Kept out of the component so the rules are
// testable without a DOM. The hub checks the list again when it is saved (`validate_curators`); these are the same rules, so the person
// hears about a bad entry before a round trip, not instead of it.

import { normalizeKey } from "./trustedPublishers.ts";

export interface Curator {
  name: string;
  /** The Ed25519 public key that signs their index, 64 lowercase hex characters. */
  key: string;
  /** Where the index is: `https://…`, `ar://<id>`, or plain `http://` to this machine only. */
  url: string;
}

/** The first source (P160), kept in this repository under `community/`: offered by an optional button, never added by itself. Its key is
 * `community/PUBLIC_KEY` (a test checks they agree) and the private half stays with its owner. */
export const FABIO_SOURCE: Curator = {
  name: "Fabio",
  key: "538d066ae6595f27f4048d7ef6c42ae3b9aefbc0b5471ece23b68c0dbbb34b25",
  url: "https://raw.githubusercontent.com/Arven-Labs/warden/main/community/index.json",
};

/** Whether `list` already has the first source, by its key (the name may have been changed). */
export function hasFabioSource(list: readonly Curator[]): boolean {
  return list.some((c) => c.key === FABIO_SOURCE.key);
}

export const MAX_CURATORS = 20;
export const MAX_NAME_CHARS = 80;
export const MAX_URL_CHARS = 500;
const ARWEAVE_ID = /^[A-Za-z0-9_-]{43}$/;

/** What is wrong with an entry about to be added; the text for each is `skills.curatorProblem.<problem>`. */
export type CuratorProblem = "noName" | "nameLong" | "nameLines" | "badKey" | "badUrl" | "duplicateKey" | "duplicateName" | "tooMany";

/** Whether `url` is somewhere an index can be read from: `https://`, `ar://` and a 43-character id, or `http://` to this machine (the
 * hub's own check, `is_source`, is the one that counts). */
export function isSource(url: string): boolean {
  const text = url.trim();
  if (text.length > MAX_URL_CHARS || /[\s\u0000-\u001f]/.test(text)) return false;
  if (text.startsWith("ar://")) return ARWEAVE_ID.test(text.slice("ar://".length));
  if (text.startsWith("http://")) {
    const host = text.slice("http://".length).split(/[/?#]/)[0];
    const port = /:(\d+)$/.exec(host);
    const name = port ? host.slice(0, port.index) : host;
    return name === "127.0.0.1" || name === "localhost" || name === "[::1]";
  }
  return text.startsWith("https://") && text.length > "https://".length && text[("https://").length] !== "/";
}

/** The first problem with adding `draft` to `list`, or `null` when it can go in. */
export function checkCurator(list: readonly Curator[], draft: Curator): CuratorProblem | null {
  const name = draft.name.trim();
  if (/[\r\n]/.test(draft.name)) return "nameLines";
  if (name === "") return "noName";
  if ([...name].length > MAX_NAME_CHARS) return "nameLong";
  const key = normalizeKey(draft.key);
  if (key === null) return "badKey";
  if (!isSource(draft.url)) return "badUrl";
  if (list.some((c) => c.key === key)) return "duplicateKey";
  if (list.some((c) => c.name.toLowerCase() === name.toLowerCase())) return "duplicateName";
  if (list.length >= MAX_CURATORS) return "tooMany";
  return null;
}

/** `list` with the curator added (a draft that `checkCurator` refuses leaves it as it is). */
export function addCurator(list: readonly Curator[], draft: Curator): Curator[] {
  if (checkCurator(list, draft) !== null) return [...list];
  return [...list, { name: draft.name.trim(), key: normalizeKey(draft.key) as string, url: draft.url.trim() }];
}

/** `list` without the curator called `name`. */
export function removeCurator(list: readonly Curator[], name: string): Curator[] {
  return list.filter((c) => c.name !== name);
}

/** Whether two lists hold the same curators in the same order: what says there is something to save. */
export function sameCurators(a: readonly Curator[], b: readonly Curator[]): boolean {
  return a.length === b.length && a.every((c, i) => c.name === b[i].name && c.key === b[i].key && c.url === b[i].url);
}

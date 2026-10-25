// Which key file signs the skills this person shares (P160). Only the path is remembered, on this computer's browser storage and
// nowhere else: the key itself is read by the app for each signature and never kept, and the path is no secret. Kept out of the
// component so the rules are testable without a DOM.

const STORAGE_KEY = "warden.signingKeyPath";

/** The part of `localStorage` used here, so a test can hand in a stand-in. */
export interface PathStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function defaultStorage(): PathStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null; // blocked storage: the screen works, it just asks again next time
  }
}

/** The key file chosen last time, or `null` when there is none (or storage is not available). */
export function rememberedKeyPath(storage: PathStorage | null = defaultStorage()): string | null {
  try {
    const path = storage?.getItem(STORAGE_KEY)?.trim();
    return path ? path : null;
  } catch {
    return null;
  }
}

/** Remembers `path` as the key file to sign with; `null` forgets it. */
export function rememberKeyPath(path: string | null, storage: PathStorage | null = defaultStorage()): void {
  try {
    if (path === null || path.trim() === "") storage?.removeItem(STORAGE_KEY);
    else storage?.setItem(STORAGE_KEY, path.trim());
  } catch {
    // nothing to do: not remembering is not an error
  }
}

/** The first characters of a public key, enough to tell two keys apart in a line of text. */
export function shortSigner(key: string): string {
  return key.slice(0, 16);
}

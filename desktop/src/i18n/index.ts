// P136 — the words of the app. `en.ts` is the source of truth (every key is there, in English); `pt-BR.ts` has to have every key too, so
// `tsc` names a missing one. A key missing at run time still reads in English. The language is the one the person picked (kept in this
// webview), else the system's, else English.

import { useSyncExternalStore } from "react";
import { en, type Key } from "./en.ts";
import { ptBR } from "./pt-BR.ts";

export type { Key };
export type Locale = "en" | "pt-BR";

export const LOCALES: readonly { id: Locale; label: string }[] = [
  { id: "en", label: "English" },
  { id: "pt-BR", label: "Português (Brasil)" },
];

const STORAGE_KEY = "warden.lang";
const catalogs: Record<Locale, Record<string, string>> = { en, "pt-BR": ptBR };
const listeners = new Set<() => void>();

function isLocale(value: unknown): value is Locale {
  return value === "en" || value === "pt-BR";
}

/** What the person picked, or what the system asks for: any Portuguese reads as `pt-BR`, everything else as English. */
export function detectLocale(stored: string | null, browser: readonly string[]): Locale {
  if (isLocale(stored)) return stored;
  return browser.some((tag) => tag.toLowerCase().startsWith("pt")) ? "pt-BR" : "en";
}

function readStored(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

let current: Locale = detectLocale(readStored(), typeof navigator === "undefined" ? [] : navigator.languages ?? []);

export function locale(): Locale {
  return current;
}

/** The tag `Intl` and `toLocale…String` take for dates, times and numbers in the current language. */
export function intlTag(): string {
  return current === "pt-BR" ? "pt-BR" : "en-US";
}

export function setLocale(next: Locale): void {
  current = next;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // The choice holds for this run even when the webview won't keep it.
  }
  if (typeof document !== "undefined") document.documentElement.lang = next;
  for (const listener of listeners) listener();
}

if (typeof document !== "undefined") document.documentElement.lang = current;

/** The text for `key` in the current language, with each `{name}` replaced by its value in `vars`. */
export function t(key: Key, vars?: Record<string, string | number>): string {
  const text = catalogs[current][key] ?? en[key];
  return vars ? text.replace(/\{(\w+)\}/g, (whole, name: string) => (name in vars ? String(vars[name]) : whole)) : text;
}

/** `one` when `count` is 1, else `other` (both keys, so each language can spell its own plural). */
export function plural(count: number, one: Key, other: Key): string {
  return t(count === 1 ? one : other, { count });
}

/** The current language for a component, which draws again when the person switches it. */
export function useLocale(): Locale {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}

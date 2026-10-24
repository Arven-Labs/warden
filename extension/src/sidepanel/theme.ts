// The theme of the panel: dark by default (it does not follow the system), light when the person picks it. It is kept in this browser
// under the same key as the site (`site/components/ThemeToggle.tsx`), and `theme-init.js` applies it before the first paint.
import { useSyncExternalStore } from "react";

export type Theme = "dark" | "light";

const STORAGE_KEY = "warden-theme";
const listeners = new Set<() => void>();

export function theme(): Theme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

export function setTheme(next: Theme): void {
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // storage can be blocked; the choice then lasts until the page is closed
  }
  listeners.forEach((fn) => fn());
}

export function useTheme(): Theme {
  return useSyncExternalStore(
    (fn) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    theme,
  );
}

"use client";

import { useEffect, useState } from "react";

type Theme = "dark" | "light";

/** Dark is the default; the choice is kept per browser and applied before paint by the script in `layout.tsx`. */
export function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>("dark");

  useEffect(() => {
    setTheme(document.documentElement.dataset.theme === "light" ? "light" : "dark");
  }, []);

  function flip() {
    const next: Theme = theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    try {
      localStorage.setItem("warden-theme", next);
    } catch {
      // storage can be blocked; the choice then lasts for this page only
    }
    setTheme(next);
  }

  return (
    <button type="button" className="icon-btn" onClick={flip} aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} theme`}>
      {theme === "dark" ? "☾" : "☀"}
    </button>
  );
}

import { t } from "../i18n/index.ts";
import { useLocale } from "../i18n/react.tsx";
import { setTheme, useTheme } from "./theme.ts";

/** The theme of the panel, dark or light, kept in the panel's own storage. Dark until the person picks light. */
export default function ThemeSwitch() {
  useLocale(); // so the label follows the language
  const current = useTheme();
  const next = current === "dark" ? "light" : "dark";
  const label = t(next === "light" ? "theme.toLight" : "theme.toDark");
  return (
    <button type="button" className="theme-switch" aria-label={label} title={label} onClick={() => setTheme(next)}>
      {current === "dark" ? "☾" : "☀"}
    </button>
  );
}

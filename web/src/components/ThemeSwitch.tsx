import { t } from "../i18n";
import { setTheme, useTheme } from "../theme";

/** The theme of the page, dark or light, kept in this browser. Dark until the person picks light. */
export default function ThemeSwitch() {
  const current = useTheme();
  const next = current === "dark" ? "light" : "dark";
  const label = t(next === "light" ? "theme.toLight" : "theme.toDark");
  return (
    <button type="button" className="theme-switch" aria-label={label} title={label} onClick={() => setTheme(next)}>
      {current === "dark" ? "☾" : "☀"}
    </button>
  );
}

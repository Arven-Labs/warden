import { LOCALES, setLocale, t, type Locale } from "../i18n/index.ts";
import { useLocale } from "../i18n/react.tsx";

/** The language of the panel, kept in the panel's storage; until the person picks one it follows the browser's. */
export default function LanguageSwitch() {
  const current = useLocale();
  return (
    <select className="language-switch" value={current} aria-label={t("language.label")} title={t("language.label")} onChange={(e) => setLocale(e.target.value as Locale)}>
      {LOCALES.map((l) => (
        <option key={l.id} value={l.id}>
          {l.label}
        </option>
      ))}
    </select>
  );
}

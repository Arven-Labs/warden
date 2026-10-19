import { LOCALES, setLocale, t, useLocale, type Locale } from "../i18n";

/** The language of the page, kept in this browser; until the person picks one it follows the browser's. */
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

import { LOCALES, setLocale, t, useLocale, type Locale } from "../i18n";

/** The language of the app, kept in this webview; until the person picks one it follows the system's. */
export default function LanguageSwitch() {
  const current = useLocale();
  return (
    <select className="settings-select language-switch" value={current} aria-label={t("language.label")} title={t("language.label")} onChange={(e) => setLocale(e.target.value as Locale)}>
      {LOCALES.map((l) => (
        <option key={l.id} value={l.id}>
          {l.label}
        </option>
      ))}
    </select>
  );
}

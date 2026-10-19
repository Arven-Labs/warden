//! Text the terminal shows, in more than one language (P136). English is the default; Brazilian
//! Portuguese comes from a second catalog. Same design as `web/src/i18n/index.ts`: a key looks up
//! a template, `{name}` is replaced from the arguments, and a key a catalog lacks falls back to
//! English instead of printing nothing.
//!
//! The language the program speaks is one process-wide value (`init`, also what `/lang` calls),
//! because tokio may build a string on any worker thread. A thread can override it for itself
//! (`set_locale`): the unit tests run in parallel threads and some of them pin the language they
//! assert on.

mod en;
mod pt_br;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    En,
    PtBr,
}

impl Locale {
    /// The tag people type (`--lang pt-BR`) and the value saved in `cli_lang.txt`.
    pub fn tag(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::PtBr => "pt-BR",
        }
    }

    /// Accepts `en`, `pt`, `pt-BR`, `pt_BR.UTF-8`, `en_US`... — the shapes `LANG` and a flag come in.
    pub fn parse(value: &str) -> Option<Locale> {
        let lower = value.trim().to_ascii_lowercase();
        if lower.starts_with("pt") {
            Some(Locale::PtBr)
        } else if lower.starts_with("en") {
            Some(Locale::En)
        } else {
            None
        }
    }
}

/// What `detect` reads, kept as plain values so the order of precedence is testable without
/// touching the real environment.
#[derive(Default, Clone, Debug)]
pub struct Sources {
    pub flag: Option<String>,
    pub warden_lang: Option<String>,
    pub saved: Option<String>,
    pub lc_all: Option<String>,
    pub lc_messages: Option<String>,
    pub lang: Option<String>,
}

impl Sources {
    pub fn from_env(flag: Option<String>, saved: Option<String>) -> Sources {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        Sources { flag, warden_lang: var("WARDEN_LANG"), saved, lc_all: var("LC_ALL"), lc_messages: var("LC_MESSAGES"), lang: var("LANG") }
    }
}

/// `--lang`, then `WARDEN_LANG`, then the saved choice, then the system locale; English when none
/// of them names a language we have (`C`, `POSIX`, nothing at all).
pub fn detect(sources: &Sources) -> Locale {
    [&sources.flag, &sources.warden_lang, &sources.saved, &sources.lc_all, &sources.lc_messages, &sources.lang]
        .into_iter()
        .flatten()
        .find_map(|value| Locale::parse(value))
        .unwrap_or(Locale::En)
}

/// Where the choice made with `/lang` is kept: a one-line file beside `cli_history.txt`, so the
/// shared `config.toml` (which rejects keys it doesn't know) is left alone.
pub fn saved_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("warden").join("cli_lang.txt"))
}

pub fn load_saved(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|text| text.trim().to_string()).filter(|text| !text.is_empty())
}

pub fn save(path: &Path, locale: Locale) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}\n", locale.tag()))
}

static PROCESS: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static CURRENT: Cell<Option<Locale>> = const { Cell::new(None) };
}

/// Sets the language for the whole process: what `main` decided, and what `/lang` changes.
pub fn init(locale: Locale) {
    PROCESS.store(if locale == Locale::PtBr { 1 } else { 0 }, Ordering::Relaxed);
}

/// Sets the language for this thread only — what the tests use.
#[cfg_attr(not(test), allow(dead_code))]
pub fn set_locale(locale: Locale) {
    CURRENT.with(|current| current.set(Some(locale)));
}

pub fn current() -> Locale {
    CURRENT.with(|current| current.get()).unwrap_or_else(|| if PROCESS.load(Ordering::Relaxed) == 1 { Locale::PtBr } else { Locale::En })
}

fn lookup(catalog: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    catalog.iter().find(|(name, _)| *name == key).map(|(_, text)| *text)
}

fn template(key: &str) -> &'static str {
    let localized = match current() {
        Locale::En => None,
        Locale::PtBr => lookup(pt_br::CATALOG, key),
    };
    localized.or_else(|| lookup(en::CATALOG, key)).unwrap_or_else(|| {
        // A key nobody wrote: show it, so it is seen and fixed, rather than a blank line.
        Box::leak(key.to_string().into_boxed_str())
    })
}

/// The text for `key` with each `{name}` replaced by its value in `vars`. A `{name}` with no
/// value stays as written.
pub fn t(key: &str, vars: &[(&str, String)]) -> String {
    let text = template(key);
    if vars.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if vars.iter().any(|(name, _)| *name == &after[..end]) => {
                let name = &after[..end];
                let (_, value) = vars.iter().find(|(n, _)| *n == name).expect("checked above");
                out.push_str(value);
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `t` with the `{name}` arguments written inline: `tr!("key")` or `tr!("key", name = value, ...)`.
#[macro_export]
macro_rules! tr {
    ($key:expr) => {
        $crate::i18n::t($key, &[])
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::t($key, &[$((stringify!($name), ($value).to_string())),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(text: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            if let Some(end) = after.find('}') {
                let name = &after[..end];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    found.insert(name.to_string());
                }
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
        found
    }

    #[test]
    fn the_two_catalogs_have_the_same_keys_and_the_same_placeholders() {
        let english: std::collections::BTreeMap<_, _> = en::CATALOG.iter().copied().collect();
        let portuguese: std::collections::BTreeMap<_, _> = pt_br::CATALOG.iter().copied().collect();
        assert_eq!(english.len(), en::CATALOG.len(), "a key is written twice in en.rs");
        assert_eq!(portuguese.len(), pt_br::CATALOG.len(), "a key is written twice in pt_br.rs");
        let missing: Vec<_> = english.keys().filter(|key| !portuguese.contains_key(*key)).collect();
        let extra: Vec<_> = portuguese.keys().filter(|key| !english.contains_key(*key)).collect();
        assert!(missing.is_empty(), "keys missing from pt_br.rs: {missing:?}");
        assert!(extra.is_empty(), "keys only in pt_br.rs: {extra:?}");
        for (key, text) in &english {
            assert_eq!(placeholders(text), placeholders(portuguese[key]), "the {{names}} of {key} differ between the catalogs");
        }
    }

    #[test]
    fn the_locale_comes_from_the_first_source_that_names_one() {
        let mut sources = Sources { lang: Some("pt_BR.UTF-8".into()), ..Sources::default() };
        assert_eq!(detect(&sources), Locale::PtBr);
        sources.saved = Some("en".into());
        assert_eq!(detect(&sources), Locale::En, "the saved choice beats the system locale");
        sources.warden_lang = Some("pt".into());
        assert_eq!(detect(&sources), Locale::PtBr, "WARDEN_LANG beats the saved choice");
        sources.flag = Some("en-US".into());
        assert_eq!(detect(&sources), Locale::En, "--lang beats everything");
    }

    #[test]
    fn an_unknown_or_missing_locale_is_english() {
        assert_eq!(detect(&Sources::default()), Locale::En);
        assert_eq!(detect(&Sources { lang: Some("C".into()), lc_all: Some("POSIX".into()), ..Sources::default() }), Locale::En);
        assert_eq!(detect(&Sources { lang: Some("fr_FR.UTF-8".into()), ..Sources::default() }), Locale::En);
    }

    #[test]
    fn a_name_is_replaced_and_an_unknown_one_is_left_as_written() {
        set_locale(Locale::En);
        assert_eq!(t("lang.set", &[("language", "English".to_string())]), "Language set to English.");
        assert_eq!(t("lang.set", &[("other", "x".to_string())]), "Language set to {language}.");
    }

    #[test]
    fn a_key_without_a_translation_falls_back_to_english_and_an_unknown_key_shows_itself() {
        set_locale(Locale::PtBr);
        assert_eq!(t("lang.set", &[("language", "inglês".to_string())]), "Idioma alterado para inglês.");
        assert_eq!(t("no.such.key", &[]), "no.such.key");
        set_locale(Locale::En);
    }

    #[test]
    fn the_tag_round_trips_through_the_saved_file() {
        let dir = std::env::temp_dir().join(format!("warden-cli-lang-{}", std::process::id()));
        let path = dir.join("cli_lang.txt");
        save(&path, Locale::PtBr).unwrap();
        assert_eq!(load_saved(&path).as_deref(), Some("pt-BR"));
        assert_eq!(Locale::parse(&load_saved(&path).unwrap()), Some(Locale::PtBr));
        let _ = std::fs::remove_dir_all(dir);
    }
}

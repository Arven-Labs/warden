//! The model catalog: which models the gateway sells, where each one is served from, and what it costs and sells for.
//!
//! The file is TOML, next to the ledger. Prices are written in USD per million tokens (what the providers publish) and
//! turned into integer micro-USD per million tokens once, on load, so that no float touches a charge. Every amount is
//! rounded **up** to the next micro-USD: the gateway never under-charges by rounding.
//!
//! ```toml
//! margin_percent = 30            # required: sale = cost + 30%, unless a model sets its own sale price
//! default_max_tokens = 4096      # the answer ceiling when the caller sets none
//!
//! [[upstreams]]
//! id = "gemini"
//! base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
//! api_key_env = "GEMINI_API_KEY"  # the key is read from the environment, never stored in this file
//! # api_key_file = "/run/secrets/gemini"  # or from a secret file (absolute path), read again when it changes
//!
//! [[models]]
//! name = "warden-fast"            # what the customer asks for
//! upstream = "gemini"
//! upstream_model = "gemini-3.5-flash"
//! cost_in = 0.30                  # USD per million tokens
//! cost_out = 2.50
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::apikey::ApiKey;
use crate::store::Micros;

const MICROS_PER_USD: f64 = 1_000_000.0;
const TOKENS_PER_MILLION: i128 = 1_000_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    margin_percent: Option<u32>,
    #[serde(default = "default_max_tokens")]
    default_max_tokens: u32,
    #[serde(default)]
    upstreams: Vec<RawUpstream>,
    #[serde(default)]
    models: Vec<RawModel>,
}

fn default_max_tokens() -> u32 {
    4096
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUpstream {
    id: String,
    base_url: String,
    api_key_env: Option<String>,
    api_key_file: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    name: String,
    upstream: String,
    upstream_model: String,
    cost_in: f64,
    cost_out: f64,
    sale_in: Option<f64>,
    sale_out: Option<f64>,
    max_tokens: Option<u32>,
}

/// A provider endpoint the gateway forwards to (an OpenAI-compatible `base_url`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub id: String,
    pub base_url: String,
    /// The key for this provider: `api_key.get()` is the one to send now (a key file may have been rotated since the start).
    pub api_key: ApiKey,
}

/// Prices are micro-USD per million tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEntry {
    pub name: String,
    pub upstream: String,
    pub upstream_model: String,
    pub cost_in: Micros,
    pub cost_out: Micros,
    pub sale_in: Micros,
    pub sale_out: Micros,
    /// The most an answer may have; the hold for a call is sized with it.
    pub max_tokens: u32,
}

/// What a call used, in tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tokens {
    pub prompt: u64,
    pub completion: u64,
}

/// What a call cost the gateway and what it charges the customer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priced {
    pub cost: Micros,
    pub charged: Micros,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub upstreams: Vec<Upstream>,
    pub models: Vec<ModelEntry>,
}

/// USD → micro-USD per million tokens, rounded to the nearest micro (the price list is written with at most six decimals).
fn usd_to_micros(field: &str, usd: f64) -> Result<Micros, String> {
    if !usd.is_finite() || usd < 0.0 {
        return Err(format!("{field} must be a number of USD, zero or more (got {usd})"));
    }
    Ok((usd * MICROS_PER_USD).round() as Micros)
}

/// `tokens × price_per_million`, rounded up to the next micro-USD, in 128-bit so nothing overflows.
fn charge(tokens: u64, price_per_million: Micros) -> Micros {
    let total = tokens as i128 * price_per_million as i128;
    ((total + TOKENS_PER_MILLION - 1) / TOKENS_PER_MILLION) as Micros
}

/// `cost + margin_percent%`, rounded up to the next micro-USD.
fn with_margin(cost: Micros, margin_percent: u32) -> Micros {
    let total = cost as i128 * (100 + margin_percent as i128);
    ((total + 99) / 100) as Micros
}

impl Catalog {
    /// Parses the TOML. `env` resolves `api_key_env` names (the process environment in production, a map in tests).
    pub fn parse(text: &str, env: impl Fn(&str) -> Option<String>) -> Result<Catalog, String> {
        let raw: RawCatalog = toml::from_str(text).map_err(|e| format!("catalog: {e}"))?;
        let margin = raw
            .margin_percent
            .ok_or("catalog: `margin_percent` is required (a forgotten one would sell at cost)")?;

        let mut upstreams = Vec::new();
        let mut seen = HashSet::new();
        for u in raw.upstreams {
            if !seen.insert(u.id.clone()) {
                return Err(format!("catalog: upstream `{}` is defined twice", u.id));
            }
            let api_key = match (u.api_key_env.as_deref(), u.api_key_file.as_deref()) {
                (Some(_), Some(_)) => {
                    return Err(format!("catalog: upstream `{}` sets both `api_key_env` and `api_key_file`; choose one", u.id));
                }
                (Some(name), None) => ApiKey::from_env(name, env(name).filter(|k| !k.is_empty())),
                (None, Some(file)) => {
                    let path = PathBuf::from(file);
                    // A relative path would mean the catalog's directory to one reader and the process's to another.
                    if !path.is_absolute() {
                        return Err(format!("catalog: upstream `{}`: `api_key_file` must be an absolute path (got `{file}`)", u.id));
                    }
                    ApiKey::from_file(path).map_err(|e| format!("catalog: upstream `{}`: {e}", u.id))?
                }
                (None, None) => ApiKey::none(),
            };
            upstreams.push(Upstream { id: u.id, base_url: u.base_url.trim_end_matches('/').to_string(), api_key });
        }

        let mut models = Vec::new();
        let mut names = HashSet::new();
        for m in raw.models {
            if !names.insert(m.name.clone()) {
                return Err(format!("catalog: model `{}` is defined twice", m.name));
            }
            if !upstreams.iter().any(|u| u.id == m.upstream) {
                return Err(format!("catalog: model `{}` uses the unknown upstream `{}`", m.name, m.upstream));
            }
            let cost_in = usd_to_micros("cost_in", m.cost_in)?;
            let cost_out = usd_to_micros("cost_out", m.cost_out)?;
            let sale_in = match m.sale_in {
                Some(v) => usd_to_micros("sale_in", v)?,
                None => with_margin(cost_in, margin),
            };
            let sale_out = match m.sale_out {
                Some(v) => usd_to_micros("sale_out", v)?,
                None => with_margin(cost_out, margin),
            };
            if sale_in < cost_in || sale_out < cost_out {
                return Err(format!("catalog: model `{}` would sell below its cost", m.name));
            }
            let max_tokens = m.max_tokens.unwrap_or(raw.default_max_tokens);
            if max_tokens == 0 {
                return Err(format!("catalog: model `{}` has max_tokens = 0", m.name));
            }
            models.push(ModelEntry {
                name: m.name,
                upstream: m.upstream,
                upstream_model: m.upstream_model,
                cost_in,
                cost_out,
                sale_in,
                sale_out,
                max_tokens,
            });
        }
        Ok(Catalog { upstreams, models })
    }

    pub fn load(path: &Path) -> Result<Catalog, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("catalog: cannot read {}: {e}", path.display()))?;
        Catalog::parse(&text, |name| std::env::var(name).ok())
    }

    pub fn model(&self, name: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.name == name)
    }

    pub fn upstream(&self, id: &str) -> Option<&Upstream> {
        self.upstreams.iter().find(|u| u.id == id)
    }
}

impl ModelEntry {
    /// The answer ceiling for a call: what the caller asked for, capped by this model's own limit.
    pub fn effective_max_tokens(&self, requested: Option<u32>) -> u32 {
        requested.filter(|&n| n > 0).map_or(self.max_tokens, |n| n.min(self.max_tokens))
    }

    /// The worst case of a call, to hold before it runs: the estimated prompt plus a full answer at the sale price.
    pub fn estimate_hold(&self, prompt_tokens: u64, max_tokens: u32) -> Micros {
        charge(prompt_tokens, self.sale_in) + charge(max_tokens as u64, self.sale_out)
    }

    /// What a call really used, priced both ways.
    pub fn price(&self, used: Tokens) -> Priced {
        Priced {
            cost: charge(used.prompt, self.cost_in) + charge(used.completion, self.cost_out),
            charged: charge(used.prompt, self.sale_in) + charge(used.completion, self.sale_out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
margin_percent = 30

[[upstreams]]
id = "gemini"
base_url = "https://example.test/v1/"
api_key_env = "GEMINI_KEY"

[[models]]
name = "warden-fast"
upstream = "gemini"
upstream_model = "gemini-3.5-flash"
cost_in = 0.30
cost_out = 2.50
"#;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn parse(text: &str) -> Result<Catalog, String> {
        Catalog::parse(text, no_env)
    }

    #[test]
    fn the_sale_price_is_the_cost_plus_the_margin() {
        let cat = parse(SAMPLE).unwrap();
        let m = cat.model("warden-fast").unwrap();
        assert_eq!((m.cost_in, m.cost_out), (300_000, 2_500_000));
        assert_eq!((m.sale_in, m.sale_out), (390_000, 3_250_000));
        assert_eq!(m.max_tokens, 4096);
    }

    #[test]
    fn the_margin_rounds_up_to_the_next_micro() {
        // 0.000001 USD of cost + 30% is 1.3 micro, so it sells for 2.
        assert_eq!(with_margin(1, 30), 2);
        assert_eq!(with_margin(0, 30), 0);
        assert_eq!(with_margin(100, 0), 100);
    }

    #[test]
    fn a_model_can_set_its_own_sale_price() {
        let text = format!("{SAMPLE}sale_in = 1.0\nsale_out = 5.0\nmax_tokens = 1000\n");
        let m = parse(&text).unwrap().models.remove(0);
        assert_eq!((m.sale_in, m.sale_out, m.max_tokens), (1_000_000, 5_000_000, 1000));
    }

    #[test]
    fn a_charge_for_a_million_tokens_is_exact_with_no_float_drift() {
        let m = parse(SAMPLE).unwrap().models.remove(0);
        let p = m.price(Tokens { prompt: 1_000_000, completion: 1_000_000 });
        assert_eq!(p.cost, 2_800_000);
        assert_eq!(p.charged, 3_640_000);
    }

    #[test]
    fn a_charge_rounds_up_so_a_tiny_call_is_never_free() {
        let m = parse(SAMPLE).unwrap().models.remove(0);
        // 1 prompt token at 390_000 micro per million is 0.39 micro: it costs 1.
        let p = m.price(Tokens { prompt: 1, completion: 0 });
        assert_eq!(p.charged, 1);
        assert_eq!(p.cost, 1);
        assert_eq!(m.price(Tokens::default()), Priced { cost: 0, charged: 0 });
    }

    #[test]
    fn the_hold_covers_the_prompt_plus_a_full_answer() {
        let m = parse(SAMPLE).unwrap().models.remove(0);
        let hold = m.estimate_hold(1_000, 4_096);
        let worst = m.price(Tokens { prompt: 1_000, completion: 4_096 });
        assert_eq!(hold, worst.charged);
        // Anything a call can really use, up to the ceiling, fits inside the hold.
        assert!(m.price(Tokens { prompt: 1_000, completion: 100 }).charged <= hold);
    }

    #[test]
    fn the_answer_ceiling_is_the_smaller_of_the_ask_and_the_model_limit() {
        let m = parse(SAMPLE).unwrap().models.remove(0);
        assert_eq!(m.effective_max_tokens(None), 4096);
        assert_eq!(m.effective_max_tokens(Some(0)), 4096);
        assert_eq!(m.effective_max_tokens(Some(100)), 100);
        assert_eq!(m.effective_max_tokens(Some(1_000_000)), 4096);
    }

    #[test]
    fn the_api_key_comes_from_the_environment_and_the_url_loses_its_trailing_slash() {
        let cat = Catalog::parse(SAMPLE, |n| (n == "GEMINI_KEY").then(|| "secret".to_string())).unwrap();
        let u = cat.upstream("gemini").unwrap();
        assert_eq!(u.api_key.get().as_deref(), Some("secret"));
        assert_eq!(u.base_url, "https://example.test/v1");
        assert_eq!(parse(SAMPLE).unwrap().upstream("gemini").unwrap().api_key.get(), None);
    }

    #[test]
    fn the_api_key_can_come_from_a_file_but_not_from_both_a_file_and_the_environment() {
        let dir = std::env::temp_dir().join(format!("warden-gateway-catalog-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gemini");
        std::fs::write(&file, "file-secret\n").unwrap();

        let from_file = SAMPLE.replace("api_key_env = \"GEMINI_KEY\"", &format!("api_key_file = {:?}", file.display().to_string()));
        let cat = parse(&from_file).unwrap();
        assert_eq!(cat.upstream("gemini").unwrap().api_key.get().as_deref(), Some("file-secret"));

        let both = SAMPLE.replace("api_key_env = \"GEMINI_KEY\"", &format!("api_key_env = \"K\"\napi_key_file = {:?}", file.display().to_string()));
        assert!(parse(&both).unwrap_err().contains("choose one"));

        let relative = SAMPLE.replace("api_key_env = \"GEMINI_KEY\"", "api_key_file = \"secrets/gemini\"");
        assert!(parse(&relative).unwrap_err().contains("absolute"));

        let missing = SAMPLE.replace("api_key_env = \"GEMINI_KEY\"", &format!("api_key_file = {:?}", dir.join("nope").display().to_string()));
        let err = parse(&missing).unwrap_err();
        assert!(err.contains("gemini") && err.contains("nope"), "{err}");

        assert!(!format!("{:?}", cat.upstream("gemini").unwrap()).contains("file-secret"), "Debug must not print the key");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_margin_is_an_error() {
        let text = SAMPLE.replace("margin_percent = 30", "");
        assert!(parse(&text).unwrap_err().contains("margin_percent"));
    }

    #[test]
    fn bad_catalogs_are_refused() {
        let unknown_upstream = SAMPLE.replace("upstream = \"gemini\"", "upstream = \"nope\"");
        assert!(parse(&unknown_upstream).unwrap_err().contains("unknown upstream"));
        let negative = SAMPLE.replace("cost_in = 0.30", "cost_in = -1.0");
        assert!(parse(&negative).unwrap_err().contains("cost_in"));
        let below_cost = format!("{SAMPLE}sale_out = 1.0\n");
        assert!(parse(&below_cost).unwrap_err().contains("below its cost"));
        let twice = format!("{SAMPLE}\n[[models]]\nname = \"warden-fast\"\nupstream = \"gemini\"\nupstream_model = \"x\"\ncost_in = 0\ncost_out = 0\n");
        assert!(parse(&twice).unwrap_err().contains("defined twice"));
        assert!(parse("margin_percent = 30\nsurprise = 1").is_err());
    }
}

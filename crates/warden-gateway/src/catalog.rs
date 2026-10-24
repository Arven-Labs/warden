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
//!
//! [[models.fallback]]             # optional, in order: where a call goes when the one above fails on the provider's side
//! upstream = "openrouter"         # (down, rate-limiting us, our key refused...); each has its own cost and sale price, and the
//! upstream_model = "deepseek/deepseek-v4.1-flash"  # customer pays the price of the target that answered
//! cost_in = 0.20
//! cost_out = 0.80
//! ```
//!
//! With fallbacks, `strategy` on the model says who starts a call: `ordered` (the default, the order written), `cheapest` (least
//! cost to the gateway), `round_robin`, `weighted` (a random start by each target's `weight`, on the model and on each
//! `[[models.fallback]]`) or `fastest` (the one answering quickest lately). The rest follow as fallbacks, and a provider over its
//! `daily_budget_usd` (or `hourly_budget_usd`, over any rolling hour) is out of it altogether. A `[[models.fallback]]` may set a
//! `max_tokens` of its own under the model's, and a model a `daily_budget_usd` of its own: once the gateway has spent that on
//! it today, whichever provider answered, calls to it are refused with a 503.

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
    /// What the gateway may spend on this provider per UTC day, in USD, at its own cost. Left out: no limit.
    daily_budget_usd: Option<f64>,
    /// The same over any rolling hour.
    hourly_budget_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    name: String,
    /// What the gateway may spend on this model per UTC day, in USD, at its own cost, whichever target answers. Left out: no limit.
    daily_budget_usd: Option<f64>,
    upstream: String,
    upstream_model: String,
    cost_in: f64,
    cost_out: f64,
    sale_in: Option<f64>,
    sale_out: Option<f64>,
    max_tokens: Option<u32>,
    /// Which target a call tries first: `ordered` (the default), `cheapest`, `round_robin`, `weighted` or `fastest`.
    strategy: Option<String>,
    /// The main target's share under `weighted`.
    weight: Option<u32>,
    /// Places to try, in order, when the main one fails in a way another provider could fix (`[[models.fallback]]`).
    #[serde(default)]
    fallback: Vec<RawFallback>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFallback {
    upstream: String,
    upstream_model: String,
    cost_in: f64,
    cost_out: f64,
    sale_in: Option<f64>,
    sale_out: Option<f64>,
    weight: Option<u32>,
    /// A ceiling on the answer for this target, under the model's own.
    max_tokens: Option<u32>,
}

/// A provider endpoint the gateway forwards to (an OpenAI-compatible `base_url`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub id: String,
    pub base_url: String,
    /// The key for this provider: `api_key.get()` is the one to send now (a key file may have been rotated since the start).
    pub api_key: ApiKey,
    /// The most the gateway spends on this provider in a UTC day (micro-USD of cost). Once the day's spend has reached it, the
    /// provider is skipped and the next target of the model answers. It is a soft limit: calls already under way finish.
    pub daily_budget: Option<Micros>,
    /// The same over any rolling hour, so a burst can be held down without waiting for the day's total.
    pub hourly_budget: Option<Micros>,
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
    /// The most the gateway spends on this model, whichever target answers, in a UTC day (micro-USD of cost). Once reached, calls
    /// to it are refused (503) until the next day. Soft, like a provider's.
    pub daily_budget: Option<Micros>,
    /// Which target a call tries first; with no fallbacks it makes no difference.
    pub strategy: Strategy,
    /// The main target's `weight` (the fallbacks' are in their `Target`s).
    pub weight: u32,
    /// Where a call goes when the main target (the fields above) fails in a way another provider could fix, in order. Each has
    /// its own cost and sale price; the customer pays the price of the target that answered.
    pub fallbacks: Vec<Target>,
}

/// One place a model's calls can go: its own provider, the provider's name for the model, and what that costs and sells for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub upstream: String,
    pub upstream_model: String,
    pub cost_in: Micros,
    pub cost_out: Micros,
    pub sale_in: Micros,
    pub sale_out: Micros,
    /// Its share of the calls under the `weighted` strategy (1 when not written; it means nothing to the other strategies).
    pub weight: u32,
    /// A ceiling on the answer for this target alone, under the model's (a fallback that is cheap because it is short). The main
    /// target has none of its own: the model's `max_tokens` is its ceiling.
    pub max_tokens: Option<u32>,
}

/// Which target of a model a call tries first; the rest follow as fallbacks. A provider that is out of budget never takes part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// The main target, then the fallbacks in the order written.
    #[default]
    Ordered,
    /// The target that costs the gateway least for the call, then the next cheapest.
    Cheapest,
    /// Takes turns: each call starts one target further along.
    RoundRobin,
    /// A random start, by the targets' `weight`s.
    Weighted,
    /// The target that has been answering fastest lately; one not yet measured goes first, so all get measured.
    Fastest,
}

impl Strategy {
    pub const NAMES: &'static str = "ordered, cheapest, round_robin, weighted, fastest";

    fn parse(text: &str) -> Option<Strategy> {
        Some(match text {
            "ordered" => Strategy::Ordered,
            "cheapest" => Strategy::Cheapest,
            "round_robin" => Strategy::RoundRobin,
            "weighted" => Strategy::Weighted,
            "fastest" => Strategy::Fastest,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Strategy::Ordered => "ordered",
            Strategy::Cheapest => "cheapest",
            Strategy::RoundRobin => "round_robin",
            Strategy::Weighted => "weighted",
            Strategy::Fastest => "fastest",
        }
    }
}

impl Target {
    /// The answer ceiling for a call to this target: the call's, or the target's own when that is lower.
    pub fn cap(&self, requested: u32) -> u32 {
        self.max_tokens.map_or(requested, |own| requested.min(own))
    }

    /// What a call really used, priced both ways.
    pub fn price(&self, used: Tokens) -> Priced {
        Priced {
            cost: charge(used.prompt, self.cost_in) + charge(used.completion, self.cost_out),
            charged: charge(used.prompt, self.sale_in) + charge(used.completion, self.sale_out),
        }
    }

    /// `upstream/upstream_model`: how the ledger and the logs name where a call went.
    pub fn label(&self) -> String {
        format!("{}/{}", self.upstream, self.upstream_model)
    }
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

/// A spending ceiling written in USD, in micro-USD; left out means no ceiling, and zero or less is an error (a ceiling of nothing
/// would be a provider or model nobody can use, which is what leaving it out of the catalog is for).
fn budget(owner: &str, field: &str, usd: Option<f64>) -> Result<Option<Micros>, String> {
    match usd {
        None => Ok(None),
        Some(usd) => match usd_to_micros(field, usd) {
            Ok(0) => Err(format!("catalog: {owner}: `{field}` must be more than zero (leave it out for no limit)")),
            Ok(micros) => Ok(Some(micros)),
            Err(e) => Err(format!("catalog: {owner}: {e}")),
        },
    }
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

/// A target's costs in micro-USD and its sale prices (written, or the cost plus the margin), refusing one that would sell below cost.
/// `label` says which target in the error.
fn priced_target(
    label: &str,
    upstream: String,
    upstream_model: String,
    (cost_in, cost_out): (f64, f64),
    (sale_in, sale_out): (Option<f64>, Option<f64>),
    margin: u32,
) -> Result<Target, String> {
    let cost_in = usd_to_micros("cost_in", cost_in)?;
    let cost_out = usd_to_micros("cost_out", cost_out)?;
    let sale_in = match sale_in {
        Some(v) => usd_to_micros("sale_in", v)?,
        None => with_margin(cost_in, margin),
    };
    let sale_out = match sale_out {
        Some(v) => usd_to_micros("sale_out", v)?,
        None => with_margin(cost_out, margin),
    };
    if sale_in < cost_in || sale_out < cost_out {
        return Err(format!("catalog: {label} would sell below its cost"));
    }
    Ok(Target { upstream, upstream_model, cost_in, cost_out, sale_in, sale_out, weight: 1, max_tokens: None })
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
            // A target is labelled `<upstream>/<model>` and the budget sums by that prefix, so a `/` in an id would blur two.
            if u.id.contains('/') {
                return Err(format!("catalog: upstream id `{}` cannot contain `/`", u.id));
            }
            let owner = format!("upstream `{}`", u.id);
            let daily_budget = budget(&owner, "daily_budget_usd", u.daily_budget_usd)?;
            let hourly_budget = budget(&owner, "hourly_budget_usd", u.hourly_budget_usd)?;
            upstreams.push(Upstream { id: u.id, base_url: u.base_url.trim_end_matches('/').to_string(), api_key, daily_budget, hourly_budget });
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
            let main = priced_target(
                &format!("model `{}`", m.name),
                m.upstream,
                m.upstream_model,
                (m.cost_in, m.cost_out),
                (m.sale_in, m.sale_out),
                margin,
            )?;
            let max_tokens = m.max_tokens.unwrap_or(raw.default_max_tokens);
            if max_tokens == 0 {
                return Err(format!("catalog: model `{}` has max_tokens = 0", m.name));
            }
            let daily_budget = budget(&format!("model `{}`", m.name), "daily_budget_usd", m.daily_budget_usd)?;
            let strategy = match m.strategy.as_deref() {
                None => Strategy::Ordered,
                Some(text) => Strategy::parse(text)
                    .ok_or_else(|| format!("catalog: model `{}` has the unknown strategy `{text}` (use one of: {})", m.name, Strategy::NAMES))?,
            };
            // A weight under any other strategy would look like it counted and not: better an error than a quiet surprise.
            let weighted = strategy == Strategy::Weighted;
            if !weighted && (m.weight.is_some() || m.fallback.iter().any(|f| f.weight.is_some())) {
                return Err(format!("catalog: model `{}` sets a `weight`, which only counts with strategy = \"weighted\"", m.name));
            }
            if m.weight == Some(0) || m.fallback.iter().any(|f| f.weight == Some(0)) {
                return Err(format!("catalog: model `{}` has a `weight` of 0 (leave the target out instead)", m.name));
            }
            let weight = m.weight.unwrap_or(1);
            let mut fallbacks: Vec<Target> = Vec::new();
            for f in m.fallback {
                if !upstreams.iter().any(|u| u.id == f.upstream) {
                    return Err(format!("catalog: model `{}` has a fallback on the unknown upstream `{}`", m.name, f.upstream));
                }
                let label = format!("model `{}` fallback `{}/{}`", m.name, f.upstream, f.upstream_model);
                let mut target = priced_target(&label, f.upstream, f.upstream_model, (f.cost_in, f.cost_out), (f.sale_in, f.sale_out), margin)?;
                target.weight = f.weight.unwrap_or(1);
                if f.max_tokens == Some(0) {
                    return Err(format!("catalog: {label} has max_tokens = 0"));
                }
                target.max_tokens = f.max_tokens;
                if target.label() == main.label() || fallbacks.iter().any(|t| t.label() == target.label()) {
                    return Err(format!("catalog: model `{}` lists `{}` twice", m.name, target.label()));
                }
                fallbacks.push(target);
            }
            models.push(ModelEntry {
                name: m.name,
                upstream: main.upstream,
                upstream_model: main.upstream_model,
                cost_in: main.cost_in,
                cost_out: main.cost_out,
                sale_in: main.sale_in,
                sale_out: main.sale_out,
                max_tokens,
                daily_budget,
                strategy,
                weight,
                fallbacks,
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

    /// What a call really used, priced both ways, at the main target.
    pub fn price(&self, used: Tokens) -> Priced {
        self.main_target().price(used)
    }

    /// The main target as a `Target`: the fields above.
    fn main_target(&self) -> Target {
        Target {
            upstream: self.upstream.clone(),
            upstream_model: self.upstream_model.clone(),
            cost_in: self.cost_in,
            cost_out: self.cost_out,
            sale_in: self.sale_in,
            sale_out: self.sale_out,
            weight: self.weight,
            max_tokens: None,
        }
    }

    /// Where a call can go, in the order to try: the main target, then the fallbacks.
    pub fn targets(&self) -> Vec<Target> {
        std::iter::once(self.main_target()).chain(self.fallbacks.iter().cloned()).collect()
    }

    /// What a call really used, priced at target number `index` of `targets()` (`0` is the main one).
    pub fn price_at(&self, index: usize, used: Tokens) -> Priced {
        match index {
            0 => self.main_target().price(used),
            n => self.fallbacks.get(n - 1).expect("an index of `targets()`").price(used),
        }
    }

    /// The worst a call could cost and charge: each figure is the largest over the targets, because any of them might answer.
    /// This is what is held before the call; the customer then pays the price of the one that did.
    pub fn worst_price(&self, used: Tokens) -> Priced {
        let all: Vec<usize> = (0..=self.fallbacks.len()).collect();
        self.worst_price_among(&all, used)
    }

    /// `worst_price` over only some of the targets (by their index in `targets()`): the ones a provider's budget left standing.
    pub fn worst_price_among(&self, among: &[usize], used: Tokens) -> Priced {
        among.iter().map(|&i| self.worst_case_at(i, used)).fold(Priced { cost: 0, charged: 0 }, |worst, p| Priced {
            cost: worst.cost.max(p.cost),
            charged: worst.charged.max(p.charged),
        })
    }

    /// What `used` would cost at target `index` if its answer is held to that target's own ceiling: the price a call that runs
    /// to the limit has there (`used.completion` is the call's ceiling; a target with a lower one cannot go past it).
    pub fn worst_case_at(&self, index: usize, used: Tokens) -> Priced {
        let capped = Tokens { prompt: used.prompt, completion: self.target_cap(index, used.completion as u32).into() };
        self.price_at(index, capped)
    }

    /// The answer ceiling for target `index`: the call's, or the target's own when that is lower.
    pub fn target_cap(&self, index: usize, requested: u32) -> u32 {
        index.checked_sub(1).and_then(|n| self.fallbacks.get(n)).map_or(requested, |t| t.cap(requested))
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

    const WITH_BUDGET: &str = r#"
margin_percent = 30

[[upstreams]]
id = "a"
base_url = "http://a.test/v1"
daily_budget_usd = 2.5

[[upstreams]]
id = "b"
base_url = "http://b.test/v1"

[[models]]
name = "fast"
upstream = "a"
upstream_model = "real-a"
cost_in = 1.0
cost_out = 2.0
"#;

    #[test]
    fn a_providers_daily_budget_is_read_in_micro_usd_and_left_out_means_no_limit() {
        let catalog = parse(WITH_BUDGET).unwrap();
        assert_eq!(catalog.upstream("a").unwrap().daily_budget, Some(2_500_000));
        assert_eq!(catalog.upstream("b").unwrap().daily_budget, None);
    }

    #[test]
    fn a_budget_of_zero_or_less_is_an_error_naming_the_provider() {
        for bad in ["0", "0.0", "-1"] {
            let err = parse(&WITH_BUDGET.replace("2.5", bad)).unwrap_err();
            assert!(err.contains("upstream `a`") && err.contains("daily_budget_usd"), "{bad}: {err}");
        }
    }

    #[test]
    fn an_hourly_budget_for_a_provider_and_a_daily_one_for_a_model_are_read_in_micro_usd() {
        let text = WITH_BUDGET
            .replace("daily_budget_usd = 2.5", "daily_budget_usd = 2.5\nhourly_budget_usd = 0.4")
            .replace("name = \"fast\"", "name = \"fast\"\ndaily_budget_usd = 7.25");
        let catalog = parse(&text).unwrap();
        let a = catalog.upstream("a").unwrap();
        assert_eq!((a.daily_budget, a.hourly_budget), (Some(2_500_000), Some(400_000)));
        assert_eq!(catalog.upstream("b").unwrap().hourly_budget, None);
        assert_eq!(catalog.model("fast").unwrap().daily_budget, Some(7_250_000));
        assert_eq!(parse(WITH_BUDGET).unwrap().model("fast").unwrap().daily_budget, None);
    }

    #[test]
    fn a_zero_or_negative_hourly_or_model_budget_is_an_error_naming_who_has_it() {
        for bad in ["0", "-3"] {
            let hourly = parse(&WITH_BUDGET.replace("daily_budget_usd = 2.5", &format!("hourly_budget_usd = {bad}"))).unwrap_err();
            assert!(hourly.contains("upstream `a`") && hourly.contains("hourly_budget_usd"), "{bad}: {hourly}");
            let model = parse(&WITH_BUDGET.replace("name = \"fast\"", &format!("name = \"fast\"\ndaily_budget_usd = {bad}"))).unwrap_err();
            assert!(model.contains("model `fast`") && model.contains("daily_budget_usd"), "{bad}: {model}");
        }
    }

    #[test]
    fn a_slash_in_a_providers_id_is_refused_because_targets_are_told_apart_by_it() {
        let err = parse(&WITH_BUDGET.replace("id = \"b\"", "id = \"b/c\"")).unwrap_err();
        assert!(err.contains("`b/c`") && err.contains("cannot contain `/`"), "{err}");
    }

    fn with_two_targets(strategy_line: &str, main_weight: &str, fallback_weight: &str) -> String {
        format!(
            r#"
margin_percent = 30
[[upstreams]]
id = "a"
base_url = "http://a.test/v1"
[[upstreams]]
id = "b"
base_url = "http://b.test/v1"

[[models]]
name = "m"
{strategy_line}
{main_weight}
upstream = "a"
upstream_model = "ra"
cost_in = 1.0
cost_out = 2.0
[[models.fallback]]
upstream = "b"
upstream_model = "rb"
cost_in = 1.0
cost_out = 2.0
{fallback_weight}
"#
        )
    }

    #[test]
    fn a_model_without_a_strategy_is_ordered_and_a_named_one_is_read() {
        assert_eq!(parse(&with_two_targets("", "", "")).unwrap().models[0].strategy, Strategy::Ordered);
        for (name, want) in [
            ("ordered", Strategy::Ordered),
            ("cheapest", Strategy::Cheapest),
            ("round_robin", Strategy::RoundRobin),
            ("fastest", Strategy::Fastest),
        ] {
            let m = parse(&with_two_targets(&format!("strategy = \"{name}\""), "", "")).unwrap().models.remove(0);
            assert_eq!((m.strategy, m.strategy.name()), (want, name));
        }
    }

    #[test]
    fn weights_are_read_for_the_weighted_strategy_and_default_to_one() {
        let m = parse(&with_two_targets("strategy = \"weighted\"", "weight = 4", "")).unwrap().models.remove(0);
        assert_eq!((m.strategy, m.weight, m.fallbacks[0].weight), (Strategy::Weighted, 4, 1));
        assert_eq!(m.targets().iter().map(|t| t.weight).collect::<Vec<_>>(), vec![4, 1]);
        let both = parse(&with_two_targets("strategy = \"weighted\"", "weight = 2", "weight = 5")).unwrap().models.remove(0);
        assert_eq!(both.targets().iter().map(|t| t.weight).collect::<Vec<_>>(), vec![2, 5]);
    }

    #[test]
    fn a_bad_strategy_or_weight_is_an_error_naming_the_model() {
        let unknown = parse(&with_two_targets("strategy = \"lucky\"", "", "")).unwrap_err();
        assert!(unknown.contains("model `m`") && unknown.contains("`lucky`") && unknown.contains("round_robin"), "{unknown}");

        let zero = parse(&with_two_targets("strategy = \"weighted\"", "weight = 0", "")).unwrap_err();
        assert!(zero.contains("model `m`") && zero.contains("weight"), "{zero}");
        let zero_fallback = parse(&with_two_targets("strategy = \"weighted\"", "", "weight = 0")).unwrap_err();
        assert!(zero_fallback.contains("weight"), "{zero_fallback}");

        // A weight that no strategy would use is refused rather than quietly ignored.
        for strategy_line in ["", "strategy = \"cheapest\""] {
            let stray = parse(&with_two_targets(strategy_line, "weight = 3", "")).unwrap_err();
            assert!(stray.contains("model `m`") && stray.contains("weighted"), "{stray}");
        }
    }

    #[test]
    fn the_worst_price_among_some_targets_ignores_the_others() {
        let m = parse(WITH_FALLBACK).unwrap().models.remove(0);
        let used = Tokens { prompt: 1_000_000, completion: 1_000_000 };
        assert_eq!(m.worst_price_among(&[0, 1], used), m.worst_price(used));
        assert_eq!(m.worst_price_among(&[0], used), m.price_at(0, used), "the dear fallback is out, so the hold is the main target's");
        assert_eq!(m.worst_price_among(&[1], used), m.price_at(1, used));
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

    const WITH_FALLBACK: &str = r#"
margin_percent = 30

[[upstreams]]
id = "a"
base_url = "https://a.test/v1"

[[upstreams]]
id = "b"
base_url = "https://b.test/v1"

[[models]]
name = "smart"
upstream = "a"
upstream_model = "real-a"
cost_in = 1.0
cost_out = 2.0

[[models.fallback]]
upstream = "b"
upstream_model = "real-b"
cost_in = 3.0
cost_out = 6.0
sale_out = 9.0
"#;

    #[test]
    fn a_model_lists_its_fallbacks_with_their_own_prices() {
        let cat = parse(WITH_FALLBACK).unwrap();
        let m = cat.model("smart").unwrap();
        let targets = m.targets();
        assert_eq!(targets.iter().map(Target::label).collect::<Vec<_>>(), vec!["a/real-a", "b/real-b"]);
        // The main target: cost + 30%. The fallback: the margin on the input, the written price on the output.
        assert_eq!((targets[0].sale_in, targets[0].sale_out), (1_300_000, 2_600_000));
        assert_eq!((targets[1].cost_in, targets[1].sale_in, targets[1].cost_out, targets[1].sale_out), (3_000_000, 3_900_000, 6_000_000, 9_000_000));
        // A model without fallbacks has the main target alone, as before.
        assert_eq!(parse(SAMPLE).unwrap().model("warden-fast").unwrap().targets().len(), 1);
    }

    #[test]
    fn prices_follow_the_target_asked_for_and_the_worst_case_is_the_dearest() {
        let m = parse(WITH_FALLBACK).unwrap().model("smart").unwrap().clone();
        let used = Tokens { prompt: 1_000_000, completion: 1_000_000 };
        assert_eq!(m.price(used), m.price_at(0, used));
        assert_eq!(m.price_at(0, used), Priced { cost: 3_000_000, charged: 3_900_000 });
        assert_eq!(m.price_at(1, used), Priced { cost: 9_000_000, charged: 12_900_000 });
        assert_eq!(m.worst_price(used), m.price_at(1, used));
        // With nothing dearer than the main target, the worst case is the main target.
        let single = parse(SAMPLE).unwrap().model("warden-fast").unwrap().clone();
        assert_eq!(single.worst_price(used), single.price(used));
    }

    #[test]
    fn a_fallback_can_have_a_lower_answer_ceiling_of_its_own() {
        let m = parse(&format!("{WITH_FALLBACK}max_tokens = 100\n")).unwrap().models.remove(0);
        assert_eq!(m.targets().iter().map(|t| t.max_tokens).collect::<Vec<_>>(), vec![None, Some(100)]);
        assert_eq!((m.target_cap(0, 4096), m.target_cap(1, 4096), m.target_cap(1, 50)), (4096, 100, 50), "a ceiling only lowers");
        // The hold sizes each target's answer to its own ceiling: here the fallback is dearer per token but short.
        let used = Tokens { prompt: 1_000_000, completion: 1_000_000 };
        assert_eq!(m.worst_case_at(1, used), m.price_at(1, Tokens { prompt: 1_000_000, completion: 100 }));
        assert_eq!(m.worst_case_at(0, used), m.price_at(0, used));
        // The fallback's prompt is dearer (3.0 against 1.0), so it is still the worst case; but held to 100 tokens its worst case is
        // lower than the model's whole ceiling would make it, which is what a smaller hold is for.
        assert_eq!(m.worst_price(used), m.worst_case_at(1, used));
        assert!(m.worst_price(used).charged < m.price_at(1, used).charged);
    }

    #[test]
    fn a_ceiling_of_zero_on_a_fallback_is_an_error_naming_it() {
        let err = parse(&format!("{WITH_FALLBACK}max_tokens = 0\n")).unwrap_err();
        assert!(err.contains("fallback `b/real-b`") && err.contains("max_tokens = 0"), "{err}");
    }

    #[test]
    fn a_bad_fallback_is_an_error_naming_it() {
        let unknown = WITH_FALLBACK.replace("upstream = \"b\"", "upstream = \"nope\"");
        assert!(parse(&unknown).unwrap_err().contains("unknown upstream `nope`"));

        let twice = WITH_FALLBACK.replace("upstream = \"b\"\nupstream_model = \"real-b\"", "upstream = \"a\"\nupstream_model = \"real-a\"");
        assert!(parse(&twice).unwrap_err().contains("twice"));

        let below_cost = WITH_FALLBACK.replace("sale_out = 9.0", "sale_out = 1.0");
        let err = parse(&below_cost).unwrap_err();
        assert!(err.contains("fallback `b/real-b`") && err.contains("below its cost"), "{err}");
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

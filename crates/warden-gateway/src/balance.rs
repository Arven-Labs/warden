//! Who starts a call, when a model has more than one target (`catalog::Strategy`).
//!
//! A call walks a list of target indices (`Reservation::available`) until one answers. Balancing is only the order of that
//! list: `Balancer::order` takes the targets whose provider is within its budget and puts them in the order the model's strategy
//! asks for. The rest of the gateway does not know: the hold is the dearest target's, the customer pays the price of the one
//! that answered, and `X-Warden-Target` says which it was.
//!
//! What it remembers lives in memory and starts empty after a restart: a turn counter per model for `round_robin`, and a moving
//! average of the time each target took to answer for `fastest`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use rand::Rng;

use crate::catalog::{ModelEntry, Strategy, Tokens};

/// How much a new measurement counts against the average so far: recent enough to follow a provider that slowed down, steady
/// enough that one odd answer does not flip the order.
const LATENCY_WEIGHT: f64 = 0.3;

#[derive(Default)]
pub struct Balancer {
    /// Calls so far per model, for `round_robin`.
    turns: Mutex<HashMap<String, u64>>,
    /// Moving average of the milliseconds a target took to answer, by `Target::label`.
    latency: Mutex<HashMap<String, f64>>,
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Balancer {
    pub fn new() -> Balancer {
        Balancer::default()
    }

    /// `available` (indices into `model.targets()`, in declared order) in the order this call should try them. `used` is the
    /// call's worst case, which `cheapest` prices.
    pub fn order(&self, model: &ModelEntry, available: Vec<usize>, used: Tokens) -> Vec<usize> {
        // One target, or the order as written: nothing to decide, and nothing to remember.
        if available.len() < 2 || model.strategy == Strategy::Ordered {
            return available;
        }
        let turn = if model.strategy == Strategy::RoundRobin {
            let mut turns = locked(&self.turns);
            let turn = turns.entry(model.name.clone()).or_insert(0);
            let this = *turn;
            *turn = turn.wrapping_add(1);
            this
        } else {
            0
        };
        let mut rng = rand::thread_rng();
        let latency = locked(&self.latency).clone();
        order(model, available, used, turn, &mut || rng.gen::<f64>(), &|label| latency.get(label).copied())
    }

    /// A target answered after `elapsed` (for a stream: until it opened). Feeds `fastest`.
    pub fn record(&self, label: &str, elapsed: Duration) {
        let sample = elapsed.as_secs_f64() * 1000.0;
        let mut latency = locked(&self.latency);
        latency.entry(label.to_string()).and_modify(|avg| *avg = *avg * (1.0 - LATENCY_WEIGHT) + sample * LATENCY_WEIGHT).or_insert(sample);
    }
}

/// The ordering itself, with the turn counter, the dice and the measured latencies handed in so a test can fix them.
/// `roll` gives a number in `[0, 1)`; `latency` the average milliseconds of a target by its label, if it has been measured.
pub fn order(
    model: &ModelEntry,
    available: Vec<usize>,
    used: Tokens,
    turn: u64,
    roll: &mut dyn FnMut() -> f64,
    latency: &dyn Fn(&str) -> Option<f64>,
) -> Vec<usize> {
    let targets = model.targets();
    let mut list = available;
    match model.strategy {
        Strategy::Ordered => {}
        // A stable sort: targets that cost the same keep the order they were written in.
        Strategy::Cheapest => list.sort_by_key(|&i| model.price_at(i, used).cost),
        Strategy::RoundRobin => {
            let by = (turn % list.len().max(1) as u64) as usize;
            list.rotate_left(by);
        }
        Strategy::Weighted => {
            // Drawing without putting back, so the fallbacks after the first follow the weights too.
            let mut left = list;
            list = Vec::with_capacity(left.len());
            while !left.is_empty() {
                let total: u64 = left.iter().map(|&i| targets[i].weight as u64).sum();
                let mut pick = (roll() * total as f64) as u64;
                let at = left
                    .iter()
                    .position(|&i| {
                        let w = targets[i].weight as u64;
                        if pick < w {
                            true
                        } else {
                            pick -= w;
                            false
                        }
                    })
                    .unwrap_or(left.len() - 1);
                list.push(left.remove(at));
            }
        }
        Strategy::Fastest => {
            // Not measured yet counts as instant: it is tried, and then it is known.
            let speed = |i: usize| latency(&targets[i].label()).unwrap_or(0.0);
            list.sort_by(|&a, &b| speed(a).partial_cmp(&speed(b)).unwrap_or(std::cmp::Ordering::Equal));
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    const USED: Tokens = Tokens { prompt: 1_000_000, completion: 1_000_000 };

    /// A model with three targets: `a` (cost 2.0 + 4.0), `b` (cost 1.0 + 2.0, the cheapest), `c` (cost 3.0 + 6.0).
    fn model(strategy: &str, weights: [u32; 3]) -> ModelEntry {
        let text = format!(
            r#"
margin_percent = 30
[[upstreams]]
id = "a"
base_url = "http://a.test/v1"
[[upstreams]]
id = "b"
base_url = "http://b.test/v1"
[[upstreams]]
id = "c"
base_url = "http://c.test/v1"

[[models]]
name = "m"
strategy = "{strategy}"
{w0}
upstream = "a"
upstream_model = "ra"
cost_in = 2.0
cost_out = 4.0
[[models.fallback]]
upstream = "b"
upstream_model = "rb"
cost_in = 1.0
cost_out = 2.0
{w1}
[[models.fallback]]
upstream = "c"
upstream_model = "rc"
cost_in = 3.0
cost_out = 6.0
{w2}
"#,
            w0 = weight_line(strategy, weights[0]),
            w1 = weight_line(strategy, weights[1]),
            w2 = weight_line(strategy, weights[2]),
        );
        Catalog::parse(&text, |_| None).unwrap().models.remove(0)
    }

    fn weight_line(strategy: &str, weight: u32) -> String {
        if strategy == "weighted" {
            format!("weight = {weight}")
        } else {
            String::new()
        }
    }

    fn none(_: &str) -> Option<f64> {
        None
    }

    fn run(model: &ModelEntry, available: &[usize], turn: u64, roll: f64) -> Vec<usize> {
        order(model, available.to_vec(), USED, turn, &mut || roll, &none)
    }

    #[test]
    fn ordered_leaves_the_order_as_written() {
        assert_eq!(run(&model("ordered", [1; 3]), &[0, 1, 2], 7, 0.5), vec![0, 1, 2]);
    }

    #[test]
    fn cheapest_goes_by_what_the_gateway_pays_and_keeps_the_written_order_on_a_tie() {
        let m = model("cheapest", [1; 3]);
        assert_eq!(run(&m, &[0, 1, 2], 0, 0.0), vec![1, 0, 2]);
        assert_eq!(run(&m, &[0, 2], 0, 0.0), vec![0, 2], "a target left out of the list stays out");
        // Equal costs: a model whose three targets cost the same keeps its order.
        let same = Catalog::parse(
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
strategy = "cheapest"
upstream = "b"
upstream_model = "x"
cost_in = 1.0
cost_out = 1.0
[[models.fallback]]
upstream = "a"
upstream_model = "y"
cost_in = 1.0
cost_out = 1.0
"#,
            |_| None,
        )
        .unwrap()
        .models
        .remove(0);
        assert_eq!(run(&same, &[0, 1], 0, 0.0), vec![0, 1]);
    }

    #[test]
    fn round_robin_starts_one_further_along_each_call_and_comes_back_round() {
        let m = model("round_robin", [1; 3]);
        let starts: Vec<Vec<usize>> = (0..4).map(|turn| run(&m, &[0, 1, 2], turn, 0.0)).collect();
        assert_eq!(starts, vec![vec![0, 1, 2], vec![1, 2, 0], vec![2, 0, 1], vec![0, 1, 2]]);
        assert_eq!(run(&m, &[0, 2], 1, 0.0), vec![2, 0], "with one target out, the rest take turns");
    }

    #[test]
    fn weighted_starts_by_the_weights_and_never_repeats_a_target() {
        let m = model("weighted", [6, 3, 1]); // 60%, 30%, 10%
        assert_eq!(run(&m, &[0, 1, 2], 0, 0.0)[0], 0);
        assert_eq!(run(&m, &[0, 1, 2], 0, 0.59)[0], 0);
        assert_eq!(run(&m, &[0, 1, 2], 0, 0.61)[0], 1);
        assert_eq!(run(&m, &[0, 1, 2], 0, 0.89)[0], 1);
        assert_eq!(run(&m, &[0, 1, 2], 0, 0.91)[0], 2);
        for roll in [0.0, 0.3, 0.61, 0.95, 0.999] {
            let mut got = run(&m, &[0, 1, 2], 0, roll);
            got.sort();
            assert_eq!(got, vec![0, 1, 2], "every target once, at roll {roll}");
        }
    }

    #[test]
    fn weighted_follows_the_weights_over_many_draws() {
        let m = model("weighted", [8, 1, 1]);
        let mut firsts = [0u32; 3];
        for n in 0..1000 {
            firsts[run(&m, &[0, 1, 2], 0, n as f64 / 1000.0)[0]] += 1;
        }
        assert_eq!(firsts, [800, 100, 100]);
    }

    #[test]
    fn fastest_tries_the_unmeasured_first_and_then_the_quickest() {
        let m = model("fastest", [1; 3]);
        let avg = |label: &str| match label {
            "a/ra" => Some(300.0),
            "b/rb" => Some(120.0),
            _ => None, // c has never answered
        };
        let got = order(&m, vec![0, 1, 2], USED, 0, &mut || 0.0, &avg);
        assert_eq!(got, vec![2, 1, 0]);
        let measured = |label: &str| avg(label).or(Some(500.0));
        assert_eq!(order(&m, vec![0, 1, 2], USED, 0, &mut || 0.0, &measured), vec![1, 0, 2]);
    }

    #[test]
    fn the_balancer_alone_does_nothing_for_one_target_or_for_ordered() {
        let balancer = Balancer::new();
        assert_eq!(balancer.order(&model("cheapest", [1; 3]), vec![0], USED), vec![0]);
        assert_eq!(balancer.order(&model("ordered", [1; 3]), vec![0, 1, 2], USED), vec![0, 1, 2]);
    }

    #[test]
    fn the_balancer_takes_turns_per_model() {
        let balancer = Balancer::new();
        let m = model("round_robin", [1; 3]);
        let firsts: Vec<usize> = (0..5).map(|_| balancer.order(&m, vec![0, 1, 2], USED)[0]).collect();
        assert_eq!(firsts, vec![0, 1, 2, 0, 1]);
    }

    #[test]
    fn the_latency_average_follows_recent_answers_without_jumping_on_one() {
        let balancer = Balancer::new();
        balancer.record("a/ra", Duration::from_millis(100));
        assert_eq!(locked(&balancer.latency)["a/ra"], 100.0);
        balancer.record("a/ra", Duration::from_millis(200));
        let avg = locked(&balancer.latency)["a/ra"];
        assert!((avg - 130.0).abs() < 1e-9, "100 * 0.7 + 200 * 0.3, got {avg}");
    }

    #[test]
    fn the_balancer_puts_the_quicker_target_first_for_fastest() {
        let balancer = Balancer::new();
        let m = model("fastest", [1; 3]);
        balancer.record("a/ra", Duration::from_millis(400));
        balancer.record("b/rb", Duration::from_millis(100));
        balancer.record("c/rc", Duration::from_millis(250));
        assert_eq!(balancer.order(&m, vec![0, 1, 2], USED), vec![1, 2, 0]);
    }
}

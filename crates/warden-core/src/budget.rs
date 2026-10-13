use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::model::Usage;
use crate::spend::{meter_notice, LimitStatus, SpendContext, SpendGuard};
use crate::tool::{ApprovalRequest, Approver};

/// The tokens one agent spent through one provider within a turn (P126). A turn that fell back to a reserve partway, or whose
/// sub-agents run another model, is more than one slice; they always add up to the turn's `usage`. `None` in a field means the
/// call did not say: an agent that was not picked, or a model that carries no provider id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSlice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// The combo (P90) the calls were made through, when the model was picked as one: the usage screens group a combo's calls under it,
    /// whichever member answered. `None` for a model picked by itself, and for what was saved before combos were told.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combo_id: Option<String>,
    pub usage: Usage,
}

impl UsageSlice {
    /// Adds `usage` to the slice for `(agent_id, provider_id)` in `slices`, opening it the first time that pair is seen. The order
    /// is the order the pairs first appeared.
    pub fn add(slices: &mut Vec<UsageSlice>, agent_id: Option<&str>, provider_id: Option<&str>, usage: &Usage) {
        Self::add_in_combo(slices, agent_id, provider_id, None, usage);
    }

    /// `add` for the trio `(agent_id, provider_id, combo_id)`: the same provider reached through a combo and by itself are two slices.
    pub fn add_in_combo(slices: &mut Vec<UsageSlice>, agent_id: Option<&str>, provider_id: Option<&str>, combo_id: Option<&str>, usage: &Usage) {
        let same = |s: &&mut UsageSlice| s.agent_id.as_deref() == agent_id && s.provider_id.as_deref() == provider_id && s.combo_id.as_deref() == combo_id;
        match slices.iter_mut().find(|s| same(s)) {
            Some(slice) => slice.usage += usage,
            None => slices.push(UsageSlice {
                agent_id: agent_id.map(str::to_string),
                provider_id: provider_id.map(str::to_string),
                combo_id: combo_id.map(str::to_string),
                usage: *usage,
            }),
        }
    }
}

/// What one turn is allowed to spend on sub-agents (P46/P60), shared by the whole tree of
/// orchestrators a turn spawns (`delegate_task`, `delegate_to_agent`, and whatever those delegate
/// to in turn) — one budget per turn, however deep the tree goes.
///
/// It counts model calls made by *sub-agents* only. The orchestrator the turn started in is never
/// charged: it is already bounded by `MAX_TOOL_ITERATIONS`, and leaving it out means that once the
/// budget runs out the caller can still answer with what its sub-agents produced (their refusal
/// reaches it as an ordinary tool error) instead of the whole turn failing.
///
/// It also adds up the tokens those calls report, so the turn's `MessageOutcome.usage` includes
/// work that `Tool::call`'s plain JSON result can't carry back (P18).
///
/// The same object carries the turn's `SpendTurn` (the time-window limits of P4), because it is the
/// one thing every orchestrator in the tree already shares — the root and each sub-agent.
#[derive(Debug)]
pub struct TurnBudget {
    max_calls: Option<u32>,
    calls: AtomicU32,
    usage: Mutex<Option<Usage>>,
    /// The same tokens as `usage`, told per agent and provider (P126).
    slices: Mutex<Vec<UsageSlice>>,
    spend: Option<SpendTurn>,
}

impl TurnBudget {
    pub fn new(max_calls: u32) -> Arc<Self> {
        Self::for_turn(Some(max_calls), None)
    }

    /// A budget with an optional sub-agent call cap (`None` = uncapped) and the turn's spending
    /// limits, when there are any.
    pub fn for_turn(max_calls: Option<u32>, spend: Option<SpendTurn>) -> Arc<Self> {
        Arc::new(Self { max_calls, calls: AtomicU32::new(0), usage: Mutex::new(None), slices: Mutex::new(Vec::new()), spend })
    }

    /// Counts one sub-agent model call, or says why it can't be made. The refused call is not
    /// counted, so `calls()` never exceeds the limit.
    pub fn charge(&self) -> anyhow::Result<()> {
        let Some(max) = self.max_calls else { return Ok(()) };
        let taken = self
            .calls
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < max).then_some(n + 1))
            .is_ok();
        if taken {
            Ok(())
        } else {
            anyhow::bail!(
                "this turn's limit of {max} model calls by sub-agents was reached — stop delegating and answer with what you already have"
            )
        }
    }

    /// Adds one sub-agent call's reported usage, under the agent and provider that made it (P126). A provider that reports none
    /// adds nothing.
    pub fn record(&self, agent_id: Option<&str>, provider_id: Option<&str>, usage: Option<&Usage>) {
        self.record_in_combo(agent_id, provider_id, None, usage);
    }

    /// `record` for a call made through the combo `combo_id` (P126).
    pub fn record_in_combo(&self, agent_id: Option<&str>, provider_id: Option<&str>, combo_id: Option<&str>, usage: Option<&Usage>) {
        if let Some(usage) = usage {
            *self.usage.lock().unwrap().get_or_insert_with(Usage::default) += usage;
            UsageSlice::add_in_combo(&mut self.slices.lock().unwrap(), agent_id, provider_id, combo_id, usage);
        }
    }

    /// What `usage()` holds, told per agent and provider; empty when no sub-agent reported any.
    pub fn slices(&self) -> Vec<UsageSlice> {
        self.slices.lock().unwrap().clone()
    }

    /// Sub-agent model calls made so far.
    pub fn calls(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }

    /// Tokens reported by sub-agents so far; `None` when none was reported.
    pub fn usage(&self) -> Option<Usage> {
        *self.usage.lock().unwrap()
    }

    /// The turn's spending limits, when any are configured.
    pub fn spend(&self) -> Option<&SpendTurn> {
        self.spend.as_ref()
    }
}

/// The turn was stopped because a spending limit has no room left and nobody allowed more.
#[derive(Debug, Clone, PartialEq)]
pub struct SpendLimitReached(pub Box<LimitStatus>);

impl std::fmt::Display for SpendLimitReached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spending limit reached — {}. The turn was stopped before calling the model again. \
             Allow more from the desktop, the CLI or the web UI's Usage tab, or raise the limit in config.toml.",
            self.0.describe()
        )
    }
}

impl std::error::Error for SpendLimitReached {}

/// The spending limits as one turn sees them: who is spending, the ledger to check and book to,
/// and — when the channel can ask a person — who to ask for more room.
pub struct SpendTurn {
    guard: Arc<SpendGuard>,
    ctx: SpendContext,
    approver: Option<Arc<dyn Approver>>,
    /// One question at a time: background jobs that all hit the same wall must not stack up a
    /// prompt each — the first answer usually settles the rest.
    ask: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for SpendTurn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpendTurn").field("ctx", &self.ctx).field("can_ask", &self.approver.is_some()).finish()
    }
}

impl SpendTurn {
    pub fn new(guard: Arc<SpendGuard>, ctx: SpendContext, approver: Option<Arc<dyn Approver>>) -> Self {
        Self { guard, ctx, approver, ask: tokio::sync::Mutex::new(()) }
    }

    /// Where the limits that apply to this turn stand.
    pub fn status(&self) -> Vec<LimitStatus> {
        self.guard.status(Some(&self.ctx))
    }

    /// Why the ledger couldn't be written, when it couldn't.
    pub fn ledger_error(&self) -> Option<String> {
        self.guard.last_error()
    }

    /// Books a finished model call to the ledger, with the provider that answered ("" when it isn't labelled).
    pub fn record(&self, model: &str, provider: &str, usage: &Usage) {
        self.guard.record_served(&self.ctx, model, provider, usage);
    }

    /// Called before every model call. Returns what to tell the model when a limit is close
    /// (`None` when none is), or — when a limit has no room left — pauses: asks a person to allow
    /// one more step and carries on if they do, or stops the turn with `SpendLimitReached` if they
    /// don't or there is nobody to ask.
    pub async fn gate(&self) -> Result<Option<String>, SpendLimitReached> {
        loop {
            let check = self.guard.check(&self.ctx);
            if check.exceeded.is_none() {
                return Ok(meter_notice(&check.warnings));
            }
            let _asking = self.ask.lock().await;
            // Another job may have been allowed more while this one waited its turn to ask.
            let Some(exceeded) = self.guard.check(&self.ctx).exceeded else { continue };
            let Some(approver) = &self.approver else { return Err(SpendLimitReached(Box::new(exceeded))) };
            let Some((tokens, dollars)) = self.guard.extension_size(&exceeded.id) else { return Err(SpendLimitReached(Box::new(exceeded))) };
            let mut allow = Vec::new();
            if tokens > 0 {
                allow.push(format!("{tokens} more tokens"));
            }
            if dollars > 0.0 {
                allow.push(format!("${dollars:.2} more"));
            }
            let request = ApprovalRequest {
                target: exceeded.id.clone(),
                action: "extend_limit".to_string(),
                detail: format!(
                    "{}\n\nAllow {} until the window moves on? The paused turn continues where it stopped. Say no to stop it.",
                    exceeded.describe(),
                    allow.join(" and ")
                ),
                category: Some(crate::autonomy::Category::SpendMoney),
            };
            if !approver.approve(request).await || self.guard.extend(&exceeded.id).is_err() {
                return Err(SpendLimitReached(Box::new(exceeded)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charges_up_to_the_limit_then_refuses_without_counting() {
        let budget = TurnBudget::new(2);
        assert!(budget.charge().is_ok() && budget.charge().is_ok());
        let err = budget.charge().unwrap_err().to_string();
        assert!(err.contains("limit of 2 model calls"), "{err}");
        assert!(budget.charge().is_err());
        assert_eq!(budget.calls(), 2);
    }

    #[test]
    fn a_zero_limit_refuses_everything() {
        assert!(TurnBudget::new(0).charge().is_err());
    }

    #[test]
    fn an_uncapped_budget_never_refuses() {
        let budget = TurnBudget::for_turn(None, None);
        assert!((0..1000).all(|_| budget.charge().is_ok()));
    }

    #[test]
    fn records_usage_only_when_reported() {
        let budget = TurnBudget::new(5);
        assert_eq!(budget.usage(), None);
        budget.record(None, None, None);
        assert_eq!(budget.usage(), None);
        assert!(budget.slices().is_empty());
        budget.record(None, None, Some(&Usage { prompt_tokens: 3, completion_tokens: 2, total_tokens: 5 }));
        budget.record(None, None, Some(&Usage { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 }));
        assert_eq!(budget.usage(), Some(Usage { prompt_tokens: 4, completion_tokens: 3, total_tokens: 7 }));
    }

    #[test]
    fn usage_is_told_per_agent_and_provider_and_adds_up_to_the_total() {
        let budget = TurnBudget::new(9);
        let usage = |prompt, completion| Usage { prompt_tokens: prompt, completion_tokens: completion, total_tokens: prompt + completion };
        budget.record(Some("poet"), Some("main"), Some(&usage(3, 2)));
        budget.record(Some("poet"), Some("spare"), Some(&usage(10, 1)));
        budget.record(Some("poet"), Some("main"), Some(&usage(1, 1)));
        budget.record(None, None, Some(&usage(2, 2)));

        let slices = budget.slices();
        let key = |s: &UsageSlice| (s.agent_id.clone(), s.provider_id.clone());
        assert_eq!(
            slices.iter().map(key).collect::<Vec<_>>(),
            vec![(Some("poet".into()), Some("main".into())), (Some("poet".into()), Some("spare".into())), (None, None)],
            "one slice per pair, in the order the pairs first appeared"
        );
        assert_eq!(slices[0].usage, usage(4, 3));
        let mut sum = Usage::default();
        for slice in &slices {
            sum += &slice.usage;
        }
        assert_eq!(Some(sum), budget.usage(), "the slices always add up to the total");
    }

    #[test]
    fn a_slice_without_an_agent_or_a_provider_leaves_them_out_of_the_json() {
        let slice = UsageSlice { agent_id: None, provider_id: Some("main".into()), combo_id: None, usage: Usage { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3 } };
        let json = serde_json::to_string(&slice).unwrap();
        assert_eq!(json, r#"{"providerId":"main","usage":{"promptTokens":1,"completionTokens":2,"totalTokens":3}}"#);
        assert_eq!(serde_json::from_str::<UsageSlice>(&json).unwrap(), slice);

        // The combo is written only when there is one, and a slice saved before combos were told reads back without it.
        let in_combo = UsageSlice { combo_id: Some("fast".into()), ..slice.clone() };
        let json = serde_json::to_string(&in_combo).unwrap();
        assert!(json.contains(r#""comboId":"fast""#), "{json}");
        assert_eq!(serde_json::from_str::<UsageSlice>(&json).unwrap(), in_combo);
    }

    #[test]
    fn the_same_provider_reached_through_a_combo_and_by_itself_are_two_slices() {
        let budget = TurnBudget::new(9);
        let usage = |n| Usage { prompt_tokens: n, completion_tokens: 0, total_tokens: n };
        budget.record_in_combo(Some("a"), Some("main"), Some("fast"), Some(&usage(3)));
        budget.record_in_combo(Some("a"), Some("main"), Some("fast"), Some(&usage(4)));
        budget.record_in_combo(Some("a"), Some("spare"), Some("fast"), Some(&usage(5)));
        budget.record(Some("a"), Some("main"), Some(&usage(6)));

        let slices = budget.slices();
        let seen: Vec<(Option<&str>, Option<&str>, u32)> = slices.iter().map(|s| (s.provider_id.as_deref(), s.combo_id.as_deref(), s.usage.total_tokens)).collect();
        assert_eq!(seen, [(Some("main"), Some("fast"), 7), (Some("spare"), Some("fast"), 5), (Some("main"), None, 6)]);
        assert_eq!(budget.usage().unwrap().total_tokens, 18, "the slices always add up to the total");
    }

    mod gate {
        use super::*;
        use crate::spend::{Limit, MemoryStore, PriceTable, Scope};
        use std::sync::atomic::AtomicUsize;

        struct Answers {
            yes: bool,
            asked: AtomicUsize,
            last: Mutex<Option<ApprovalRequest>>,
        }

        #[async_trait::async_trait]
        impl Approver for Answers {
            async fn approve(&self, request: ApprovalRequest) -> bool {
                self.asked.fetch_add(1, Ordering::SeqCst);
                *self.last.lock().unwrap() = Some(request);
                self.yes
            }
        }

        fn answers(yes: bool) -> Arc<Answers> {
            Arc::new(Answers { yes, asked: AtomicUsize::new(0), last: Mutex::new(None) })
        }

        fn spent(max: u64, used: u32, approver: Option<Arc<dyn Approver>>) -> SpendTurn {
            let guard = Arc::new(SpendGuard::new(
                Arc::new(MemoryStore::default()),
                vec![Limit::new("day", Scope::Global, 24).with_max_tokens(max)],
                PriceTable::default(),
            ));
            let ctx = SpendContext::new("cli");
            guard.record(&ctx, "m", &Usage { prompt_tokens: used, completion_tokens: 0, total_tokens: used });
            SpendTurn::new(guard, ctx, approver)
        }

        #[tokio::test]
        async fn far_from_the_limit_it_says_nothing() {
            assert_eq!(spent(1000, 100, None).gate().await, Ok(None));
        }

        #[tokio::test]
        async fn close_to_the_limit_it_hands_back_the_meter_without_asking() {
            let who = answers(true);
            let notice = spent(1000, 900, Some(who.clone())).gate().await.unwrap().expect("90% used");
            assert!(notice.contains("900 of 1000 tokens (100 left)"), "{notice}");
            assert_eq!(who.asked.load(Ordering::SeqCst), 0);
        }

        #[tokio::test]
        async fn out_of_room_and_nobody_to_ask_stops_the_turn() {
            let err = spent(1000, 1000, None).gate().await.unwrap_err();
            assert_eq!(err.0.id, "day");
            assert!(err.to_string().contains("spending limit reached"));
        }

        #[tokio::test]
        async fn out_of_room_asks_and_a_yes_lets_the_turn_go_on_with_more_room() {
            let who = answers(true);
            let turn = spent(1000, 1000, Some(who.clone()));
            let notice = turn.gate().await.expect("allowed").expect("1000 of 1250 is past the warning mark");
            assert!(notice.contains("1000 of 1250"), "{notice}");
            assert_eq!(who.asked.load(Ordering::SeqCst), 1);
            let card = who.last.lock().unwrap().clone().unwrap();
            assert_eq!((card.target.as_str(), card.action.as_str()), ("day", "extend_limit"));
            assert!(card.detail.contains("250 more tokens"), "{}", card.detail);
        }

        #[tokio::test]
        async fn a_no_stops_the_turn_and_leaves_the_ceiling_alone() {
            let who = answers(false);
            let turn = spent(1000, 1000, Some(who.clone()));
            assert!(turn.gate().await.is_err());
            assert_eq!(turn.status()[0].max_tokens, Some(1000));
        }

        #[tokio::test]
        async fn jobs_hitting_the_wall_together_share_one_question() {
            let who = answers(true);
            let turn = Arc::new(spent(1000, 1000, Some(who.clone())));
            let gates = (0..4).map(|_| {
                let turn = turn.clone();
                tokio::spawn(async move { turn.gate().await })
            });
            for gate in futures_util::future::join_all(gates).await {
                assert!(gate.unwrap().is_ok());
            }
            assert_eq!(who.asked.load(Ordering::SeqCst), 1);
        }
    }
}

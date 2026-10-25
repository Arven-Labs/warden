//! Usage over the wire (P78) — what `RequestUsage`/`ExtendLimit` answer. Tokens come from every
//! conversation this hub keeps, for every device and all time (`aggregate_usage`/`daily_usage`
//! from `warden-bootstrap`); limits and dollars come from the P4 ledger through the `SpendGuard`,
//! which is shared with every other channel on this machine. Pure functions over paths and the
//! guard, like `skills.rs`, so they're testable without a socket.

use std::collections::HashMap;
use std::path::Path;

use warden_bootstrap::usage::{daily_cost, daily_usage, UsageByKey};
use warden_bootstrap::{aggregate_usage, list_conversations, Conversation};
use warden_core::spend::SpendGuard;
use warden_server_protocol::protocol::{CreditDto, DailyCostDto, DailyUsageDto, DeviceUsage, LimitStatusDto, UsageByKeyDto, UsageReportDto};
use warden_server_protocol::ServerMessage;

use crate::device_registry::PairingStore;

/// How many days the daily series covers, today included.
pub const USAGE_DAYS: u32 = 30;

/// The row scheduled tasks (P92) get in `by_device`: their conversations live apart from any device.
pub const TASKS_USAGE_ID: &str = "tasks";
/// The webhooks' conversations (P105) live in the same folder, so they are counted in the same row.
const TASKS_USAGE_NAME: &str = "Tarefas e webhooks";
/// P84: the owner's conversations, every device of theirs together.
const ROOT_USAGE_NAME: &str = "Owner";

/// Everything `RequestUsage` reports. `conversations_root` holds one folder per device (P78) and,
/// for devices that haven't reconnected since, their old single `<device_id>.json`. `tasks_dir`
/// holds the scheduled tasks' conversations, reported as one more row.
pub fn build_usage_report(
    conversations_root: &Path,
    tasks_dir: Option<&Path>,
    pairing: &PairingStore,
    guard: Option<&SpendGuard>,
    tz_offset_minutes: i32,
    now_ms: i64,
) -> anyhow::Result<UsageReportDto> {
    let mut names: HashMap<String, String> = pairing.list().unwrap_or_default().into_iter().map(|(id, d)| (id, d.device_name)).collect();

    let mut per_device: Vec<(String, Vec<Conversation>)> = Vec::new();
    match std::fs::read_dir(conversations_root) {
        Ok(entries) => {
            for entry in entries {
                let path = entry?.path();
                if path.is_dir() {
                    let device_id = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if device_id == "users" {
                        // P84: one row per member, `user:<id>`.
                        for member in std::fs::read_dir(&path)? {
                            let member = member?.path();
                            if member.is_dir() {
                                let id = format!("user:{}", member.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                                names.insert(id.clone(), id.trim_start_matches("user:").to_string());
                                // P84 slice 4: a member whose data is locked (the hub restarted since they
                                // signed in) has no row until they do, instead of failing the whole report.
                                per_device.push((id, list_conversations(&member).unwrap_or_default()));
                            }
                        }
                        continue;
                    }
                    if device_id == warden_bootstrap::users::ROOT_ID {
                        names.insert(device_id.clone(), ROOT_USAGE_NAME.to_string());
                    }
                    per_device.push((device_id, list_conversations(&path)?));
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    for legacy in list_conversations(conversations_root)? {
        match per_device.iter_mut().find(|(id, _)| *id == legacy.id) {
            Some((_, conversations)) => conversations.push(legacy),
            None => per_device.push((legacy.id.clone(), vec![legacy])),
        }
    }

    if let Some(tasks_dir) = tasks_dir {
        per_device.push((TASKS_USAGE_ID.to_string(), list_conversations(tasks_dir)?));
        names.insert(TASKS_USAGE_ID.to_string(), TASKS_USAGE_NAME.to_string());
    }

    let mut by_device: Vec<DeviceUsage> = per_device
        .iter()
        .map(|(device_id, conversations)| {
            let summary = aggregate_usage(conversations);
            DeviceUsage {
                device_id: device_id.clone(),
                name: names.get(device_id).cloned(),
                conversation_count: summary.conversation_count,
                message_count: summary.message_count,
                usage: summary.total,
            }
        })
        .filter(|d| d.conversation_count > 0)
        .collect();
    by_device.sort_by_key(|d| std::cmp::Reverse(d.usage.total_tokens));

    let all: Vec<Conversation> = per_device.into_iter().flat_map(|(_, c)| c).collect();
    let summary = aggregate_usage(&all);
    let daily = daily_usage(&all, USAGE_DAYS, tz_offset_minutes, now_ms)
        .into_iter()
        .map(|d| DailyUsageDto { date: d.date, calls: d.calls, tokens: d.tokens })
        .collect();
    // Dollars per day (P10) from what the ledger still holds: with limits off there is no guard, so no ledger.
    let daily_cost = guard
        .map(|g| {
            let since = now_ms.saturating_sub(g.longest_window_hours() as i64 * 3_600_000).max(0) as u64;
            daily_cost(&g.events_since(since), USAGE_DAYS, tz_offset_minutes, now_ms)
                .into_iter()
                .map(|d| DailyCostDto { date: d.date, calls: d.calls, cost_usd: d.cost_usd, unpriced_calls: d.unpriced_calls })
                .collect()
        })
        .unwrap_or_default();

    Ok(UsageReportDto {
        total: summary.total,
        conversation_count: summary.conversation_count,
        message_count: summary.message_count,
        by_device,
        daily,
        daily_cost,
        limits_enabled: guard.is_some(),
        limits: guard.map(LimitStatusDto::all).unwrap_or_default(),
        recent: guard.map(|g| g.breakdown().into()),
        ledger_error: guard.and_then(SpendGuard::last_error),
        by_agent: usage_by_key(summary.by_agent),
        by_provider: usage_by_key(summary.by_provider),
        by_combo: usage_by_key(summary.by_combo),
        // The hub adds it afterwards (`with_credit`): asking the gateway is a network call, not part of reading the folders.
        credit: None,
        topup_url: None,
    })
}

/// A usage reply with the hosted credit put in, when there is one, and the address to buy more of it (`topup_url`) alongside:
/// without a credit there is nothing to top up, so the address stays out too. Anything but a report passes through untouched.
pub fn with_credit(
    reply: ServerMessage,
    credit: Option<warden_core::model::cloud_balance::CloudBalance>,
    topup_url: Option<String>,
) -> ServerMessage {
    match (reply, credit) {
        (ServerMessage::UsageReport { request_id, mut report }, Some(c)) => {
            report.credit = Some(CreditDto { balance_micro_usd: c.balance_micro_usd, available_micro_usd: c.available_micro_usd });
            report.topup_url = topup_url;
            ServerMessage::UsageReport { request_id, report }
        }
        (reply, _) => reply,
    }
}

/// What `RequestUsage` reports to a workspace member (P99): their own conversations, their own calls in the ledger and the
/// limits written for them (`person <id>`), and nothing of anyone else's. A global or channel limit adds up what everybody
/// spent, so showing its meter would show the others' use; the member hears of it only when it stops a turn. No device rows
/// (a member's devices share one folder), no ledger error, and no size for an extension: they can't allow more.
/// Fails when the member's data is locked (the hub restarted since they signed in).
pub fn build_member_usage_report(
    member_conversations: &Path,
    guard: Option<&SpendGuard>,
    person: &str,
    tz_offset_minutes: i32,
    now_ms: i64,
) -> anyhow::Result<UsageReportDto> {
    let conversations = list_conversations(member_conversations)?;
    let summary = aggregate_usage(&conversations);
    let daily = daily_usage(&conversations, USAGE_DAYS, tz_offset_minutes, now_ms)
        .into_iter()
        .map(|d| DailyUsageDto { date: d.date, calls: d.calls, tokens: d.tokens })
        .collect();
    let own_scope = warden_core::spend::Scope::Person(person.to_string()).to_string();
    let daily_cost = guard
        .map(|g| {
            let since = now_ms.saturating_sub(g.longest_window_hours() as i64 * 3_600_000).max(0) as u64;
            let own: Vec<_> = g.events_since(since).into_iter().filter(|e| e.person.as_deref() == Some(person)).collect();
            daily_cost(&own, USAGE_DAYS, tz_offset_minutes, now_ms)
                .into_iter()
                .map(|d| DailyCostDto { date: d.date, calls: d.calls, cost_usd: d.cost_usd, unpriced_calls: d.unpriced_calls })
                .collect()
        })
        .unwrap_or_default();
    let limits = guard
        .map(|g| {
            LimitStatusDto::all(g)
                .into_iter()
                .filter(|limit| limit.scope == own_scope)
                .map(|limit| LimitStatusDto { extend_tokens: 0, extend_cost_usd: 0.0, ..limit })
                .collect()
        })
        .unwrap_or_default();
    Ok(UsageReportDto {
        total: summary.total,
        conversation_count: summary.conversation_count,
        message_count: summary.message_count,
        by_device: Vec::new(),
        daily,
        daily_cost,
        limits_enabled: guard.is_some(),
        limits,
        recent: guard.map(|g| g.breakdown_for_person(person).into()),
        ledger_error: None,
        by_agent: usage_by_key(summary.by_agent),
        by_provider: usage_by_key(summary.by_provider),
        by_combo: usage_by_key(summary.by_combo),
        // The hosted credit belongs to the instance, not to a member.
        credit: None,
        topup_url: None,
    })
}

/// A token breakdown as the report carries it (P126).
fn usage_by_key(buckets: Vec<UsageByKey>) -> Vec<UsageByKeyDto> {
    buckets.into_iter().map(|b| UsageByKeyDto { key: b.key, message_count: b.message_count, usage: b.usage }).collect()
}

/// Answers `RequestUsage`.
pub fn handle_usage_request(
    conversations_root: &Path,
    tasks_dir: Option<&Path>,
    pairing: &PairingStore,
    guard: Option<&SpendGuard>,
    request_id: u64,
    tz_offset_minutes: i32,
) -> ServerMessage {
    let now = warden_core::spend::now_millis() as i64;
    match build_usage_report(conversations_root, tasks_dir, pairing, guard, tz_offset_minutes, now) {
        Ok(report) => ServerMessage::UsageReport { request_id, report },
        Err(err) => ServerMessage::UsageError { request_id, message: format!("{err:#}") },
    }
}

/// Answers `RequestUsage` for a workspace member: `build_member_usage_report` over their own folder.
pub fn handle_member_usage_request(member_conversations: &Path, guard: Option<&SpendGuard>, person: &str, request_id: u64, tz_offset_minutes: i32) -> ServerMessage {
    let now = warden_core::spend::now_millis() as i64;
    match build_member_usage_report(member_conversations, guard, person, tz_offset_minutes, now) {
        Ok(report) => ServerMessage::UsageReport { request_id, report },
        Err(err) => ServerMessage::UsageError { request_id, message: format!("{err:#}") },
    }
}

/// Answers `ExtendLimit`: one `extend_step` more for that limit, and where it stands after.
pub fn handle_extend_limit(guard: Option<&SpendGuard>, request_id: u64, limit_id: &str) -> ServerMessage {
    let Some(guard) = guard else {
        return ServerMessage::UsageError { request_id, message: "spending limits are switched off on this hub".into() };
    };
    let extended = guard.extend(limit_id).and_then(|_| {
        LimitStatusDto::all(guard).into_iter().find(|l| l.id == limit_id).ok_or_else(|| anyhow::anyhow!("no limit named '{limit_id}'"))
    });
    match extended {
        Ok(limit) => ServerMessage::LimitExtended { request_id, limit },
        Err(err) => ServerMessage::UsageError { request_id, message: format!("{err:#}") },
    }
}

/// The id of the spending limit a failed turn stopped on, if that's why it failed (P4) — so the
/// client can offer `ExtendLimit`.
pub fn spend_limit_id(err: &anyhow::Error) -> Option<String> {
    err.downcast_ref::<warden_core::budget::SpendLimitReached>().map(|reached| reached.0.id.clone())
}

/// What a member is told when a limit stops their turn (P84). `SpendLimitReached`'s own text sends the
/// reader to the desktop, the CLI or the Usage tab — none of which a member has — so theirs says to
/// ask whoever runs this Warden, and carries no limit id (nothing for them to extend).
pub fn member_limit_message(err: &anyhow::Error) -> Option<String> {
    let reached = err.downcast_ref::<warden_core::budget::SpendLimitReached>()?;
    Some(format!(
        "spending limit reached — {}. The turn was stopped before calling the model again. Ask whoever runs this Warden to allow more.",
        reached.0.describe()
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use warden_bootstrap::{save_conversation, ChatRole, ConversationMessage};
    use warden_core::model::Usage;
    use warden_core::spend::{Limit, MemoryStore, Price, PriceTable, Scope, SpendContext};

    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "warden-server-usage-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    fn conversation(id: &str, tokens: &[u32], created_at: i64) -> Conversation {
        let messages = tokens
            .iter()
            .map(|&t| ConversationMessage {
                id: "m".into(),
                role: ChatRole::Assistant,
                content: String::new(),
                created_at,
                usage: Some(Usage { prompt_tokens: t, completion_tokens: 0, total_tokens: t }),
                answered_by: None,
                attachments: Vec::new(),
                generated_files: Vec::new(), tools_used: Vec::new(),
            })
            .collect();
        Conversation { id: id.into(), title: id.into(), messages, created_at, updated_at: created_at, agent_id: None, provider_id: None, project_id: None, engine_session_id: None, workdir: None, parent: None, pinned: false, archived: false }
    }

    fn guard(max_tokens: u64) -> SpendGuard {
        SpendGuard::new(
            Arc::new(MemoryStore::default()),
            vec![Limit::new("day", Scope::Global, 24).with_max_tokens(max_tokens)],
            PriceTable::new(vec![Price { model: "m".into(), input_per_mtok: 1.0, output_per_mtok: 1.0 }]),
        )
    }

    #[test]
    fn reports_every_device_with_its_registry_name_including_an_old_single_file() {
        let root = temp_dir("root");
        let now = warden_core::spend::now_millis() as i64;
        save_conversation(&root.join("phone"), &conversation("default", &[10, 20], now)).unwrap();
        save_conversation(&root.join("phone"), &conversation("trip", &[5], now)).unwrap();
        save_conversation(&root.join("web-1"), &conversation("default", &[100], now)).unwrap();
        save_conversation(&root, &conversation("old-laptop", &[1], now)).unwrap(); // pre-P78 layout
        std::fs::create_dir_all(root.join("empty")).unwrap();

        let pairing = PairingStore::new(temp_dir("devices").join("devices.json"));
        pairing.authenticate("web-1", "Browser", None, true).unwrap().unwrap();

        let report = build_usage_report(&root, None, &pairing, None, 0, now).unwrap();
        assert_eq!(report.total.total_tokens, 136);
        assert_eq!((report.conversation_count, report.message_count), (4, 5));
        let devices: Vec<_> = report.by_device.iter().map(|d| (d.device_id.as_str(), d.name.as_deref(), d.conversation_count, d.usage.total_tokens)).collect();
        assert_eq!(devices, vec![("web-1", Some("Browser"), 1, 100), ("phone", None, 2, 35), ("old-laptop", None, 1, 1)]);
        assert_eq!(report.daily.len(), USAGE_DAYS as usize);
        assert_eq!(report.daily.last().unwrap().tokens, 136);
        assert!(!report.limits_enabled && report.limits.is_empty() && report.recent.is_none());
    }

    fn answered(conversation: &mut Conversation, agent: Option<&str>, provider: Option<&str>, combo: Option<&str>) {
        for message in &mut conversation.messages {
            message.answered_by = Some(warden_bootstrap::AnsweredBy {
                agent_id: agent.map(str::to_string),
                provider_id: provider.map(str::to_string),
                combo_id: combo.map(str::to_string),
                slices: Vec::new(),
            });
        }
    }

    #[test]
    fn tokens_are_told_by_agent_provider_and_combo_for_the_owner_and_for_a_member() {
        let root = temp_dir("by-key");
        let now = warden_core::spend::now_millis() as i64;
        let mut writer = conversation("w", &[10, 20], now);
        answered(&mut writer, Some("writer"), Some("main"), Some("fast"));
        let mut plain = conversation("p", &[5], now);
        answered(&mut plain, None, Some("main"), None);
        save_conversation(&root.join("phone"), &writer).unwrap();
        save_conversation(&root.join("web-1"), &plain).unwrap();
        let pairing = PairingStore::new(temp_dir("devices").join("devices.json"));

        let report = build_usage_report(&root, None, &pairing, None, 0, now).unwrap();
        let tokens = |buckets: &[UsageByKeyDto]| buckets.iter().map(|b| (b.key.clone(), b.message_count, b.usage.total_tokens)).collect::<Vec<_>>();
        assert_eq!(tokens(&report.by_agent), vec![(Some("writer".to_string()), 2, 30), (None, 1, 5)], "biggest first, no agent as a key of its own");
        assert_eq!(tokens(&report.by_provider), vec![(Some("main".to_string()), 3, 35)]);
        assert_eq!(tokens(&report.by_combo), vec![(Some("fast".to_string()), 2, 30)], "only what went through a combo");

        // A member's report counts their own conversations only.
        let member = build_member_usage_report(&root.join("phone"), None, "ana", 0, now).unwrap();
        assert_eq!(tokens(&member.by_agent), vec![(Some("writer".to_string()), 2, 30)]);
        assert_eq!(tokens(&member.by_combo), vec![(Some("fast".to_string()), 2, 30)]);
    }

    #[test]
    fn a_hub_with_no_conversations_yet_reports_zeros() {
        let pairing = PairingStore::new(temp_dir("devices").join("devices.json"));
        let report = build_usage_report(&temp_dir("missing"), None, &pairing, None, 0, 0).unwrap();
        assert_eq!((report.conversation_count, report.by_device.len()), (0, 0));
    }

    #[test]
    fn limits_and_recent_spending_come_from_the_guard_and_extend_raises_the_ceiling() {
        let guard = guard(100);
        guard.record(&SpendContext::new("server").with_user("web-1"), "m", &Usage { prompt_tokens: 150, completion_tokens: 0, total_tokens: 150 });
        let pairing = PairingStore::new(temp_dir("devices").join("devices.json"));

        let report = build_usage_report(&temp_dir("none"), None, &pairing, Some(&guard), 0, 0).unwrap();
        let day = &report.limits[0];
        assert!(report.limits_enabled && day.exceeded);
        assert_eq!((day.used_tokens, day.max_tokens, day.extend_tokens), (150, Some(100), 25));
        let recent = report.recent.unwrap();
        assert_eq!((recent.by_model[0].key.as_str(), recent.by_channel[0].key.as_str()), ("m", "server"));

        match handle_extend_limit(Some(&guard), 7, "day") {
            ServerMessage::LimitExtended { request_id: 7, limit } => assert_eq!(limit.max_tokens, Some(125)),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(handle_extend_limit(Some(&guard), 8, "nope"), ServerMessage::UsageError { request_id: 8, .. }));
        assert!(matches!(handle_extend_limit(None, 9, "day"), ServerMessage::UsageError { request_id: 9, .. }));
    }

    /// P10: the report also carries dollars per day, and the ledger's split by provider, agent and person.
    #[test]
    fn the_report_carries_dollars_per_day_and_the_split_by_provider_agent_and_person() {
        let guard = guard(10_000_000);
        let now = warden_core::spend::now_millis() as i64;
        let usage = |prompt: u32| Usage { prompt_tokens: prompt, completion_tokens: 0, total_tokens: prompt };
        guard.record_served(&SpendContext::new("server").with_agent(Some("writer".to_string())), "m", "main", &usage(1_000_000)); // $1.00
        guard.record_served(&SpendContext::new("server").with_person("ana"), "m", "spare", &usage(500_000)); // $0.50
        guard.record(&SpendContext::new("cli"), "unpriced", &usage(10));
        guard.record_in_combo(&SpendContext::new("server"), "m", "main", "fast", &usage(250_000)); // $0.25
        let pairing = PairingStore::new(temp_dir("devices").join("devices.json"));

        let report = build_usage_report(&temp_dir("none"), None, &pairing, Some(&guard), 0, now).unwrap();
        let today = report.daily_cost.last().unwrap();
        assert_eq!((today.calls, today.unpriced_calls), (4, 1));
        assert!((today.cost_usd - 1.75).abs() < 1e-9, "{}", today.cost_usd);
        assert_eq!(report.daily_cost.len(), USAGE_DAYS as usize);
        assert!(report.daily_cost[..USAGE_DAYS as usize - 1].iter().all(|d| d.calls == 0), "only today has spending");

        let recent = report.recent.unwrap();
        let keys = |buckets: &[warden_server_protocol::protocol::SpendBucketDto]| buckets.iter().map(|b| b.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&recent.by_provider), ["main", "spare", ""]);
        assert_eq!(keys(&recent.by_agent), ["writer", ""], "largest first: the writer's 1,000,000 tokens against the other calls' 750,010");
        assert_eq!(keys(&recent.by_person), ["", "ana"], "the owner's 1,250,010 tokens against Ana's 500,000");
        // P126: dollars per combo, with no bucket for the calls that weren't through one.
        assert_eq!(recent.by_combo.len(), 1);
        assert_eq!(recent.by_combo[0].key, "fast");
        assert!((recent.by_combo[0].cost_usd - 0.25).abs() < 1e-9, "{}", recent.by_combo[0].cost_usd);

        // With limits off there is no guard, so no ledger and no dollars.
        let off = build_usage_report(&temp_dir("none"), None, &pairing, None, 0, now).unwrap();
        assert!(off.daily_cost.is_empty() && off.recent.is_none());
    }

    #[test]
    fn spend_limit_id_is_found_only_on_a_spend_limit_error() {
        let guard = guard(1);
        guard.record(&SpendContext::new("server"), "m", &Usage { prompt_tokens: 5, completion_tokens: 0, total_tokens: 5 });
        let status = guard.status(None).remove(0);
        let err = anyhow::Error::new(warden_core::budget::SpendLimitReached(Box::new(status))).context("turn failed");
        assert_eq!(spend_limit_id(&err).as_deref(), Some("day"));
        assert_eq!(spend_limit_id(&anyhow::anyhow!("provider error")), None);
    }

    #[test]
    fn the_credit_goes_into_a_usage_report_and_only_into_one() {
        let root = temp_dir("credit-root");
        let pairing = PairingStore::new(temp_dir("credit-devices").join("devices.json"));
        let reply = handle_usage_request(&root, None, &pairing, None, 7, 0);
        let balance = warden_core::model::cloud_balance::CloudBalance { balance_micro_usd: 5_000_000, available_micro_usd: 4_000_000 };

        match with_credit(reply.clone(), Some(balance), Some("https://example.com/credits".into())) {
            ServerMessage::UsageReport { request_id, report } => {
                assert_eq!(request_id, 7);
                assert_eq!(report.credit, Some(CreditDto { balance_micro_usd: 5_000_000, available_micro_usd: 4_000_000 }));
                assert_eq!(report.topup_url.as_deref(), Some("https://example.com/credits"));
            }
            other => panic!("{other:?}"),
        }
        // No top-up address configured: the credit is there and the address is not.
        match with_credit(reply.clone(), Some(balance), None) {
            ServerMessage::UsageReport { report, .. } => assert!(report.credit.is_some() && report.topup_url.is_none()),
            other => panic!("{other:?}"),
        }
        // No gateway answer: the report is as it was, with no credit, and an address with nothing to top up stays out too.
        match with_credit(reply, None, Some("https://example.com/credits".into())) {
            ServerMessage::UsageReport { report, .. } => assert!(report.credit.is_none() && report.topup_url.is_none()),
            other => panic!("{other:?}"),
        }
        // A reply that isn't a report is not touched.
        let error = ServerMessage::UsageError { request_id: 1, message: "x".into() };
        assert_eq!(with_credit(error.clone(), Some(balance), None), error);
    }

    #[test]
    fn scheduled_tasks_are_one_more_row() {
        let root = temp_dir("root");
        let tasks = temp_dir("tasks");
        let now = warden_core::spend::now_millis() as i64;
        save_conversation(&root.join("phone"), &conversation("default", &[10], now)).unwrap();
        save_conversation(&tasks, &conversation("task-daily", &[7, 8], now)).unwrap();
        let pairing = PairingStore::new(temp_dir("devices").join("devices.json"));

        let report = build_usage_report(&root, Some(&tasks), &pairing, None, 0, now).unwrap();
        assert_eq!(report.total.total_tokens, 25);
        let row = report.by_device.iter().find(|d| d.device_id == TASKS_USAGE_ID).unwrap();
        assert_eq!((row.name.as_deref(), row.conversation_count, row.usage.total_tokens), (Some("Tarefas e webhooks"), 1, 15));
    }
}

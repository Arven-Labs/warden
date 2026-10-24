//! Sending the outbox to the site mãe.
//!
//! The events are made by the store (`credits.low`, `credits.depleted`, `usage.daily_summary`) and wait in its outbox.
//! The dispatcher takes the ones that are due, signs each (`warden-hosted-protocol`, with the **webhook secret**, which is
//! not the secret the site mãe's own calls are checked with) and posts it. A 2xx means delivered; anything else counts a
//! failed attempt and schedules a retry with a growing wait, up to `MAX_DELIVERY_ATTEMPTS`. Delivery is at least once: the
//! `Idempotency-Key` header carries the event id, which is what the site mãe dedupes on.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use warden_hosted_protocol::{sign, WebhookEvent, WebhookKind, IDEMPOTENCY_HEADER, SIGNATURE_HEADER};

use crate::store::{OutboxEvent, Store, StoreError};

/// How many days back the daily summaries are (re)queued: it covers a gateway that was down over midnight.
pub const SUMMARY_DAYS_BACK: i64 = 7;
/// How long delivered rows stay in the outbox. It must outlast `SUMMARY_DAYS_BACK`: a summary's row is what keeps it from
/// being queued (and sent) a second time.
pub const DELIVERED_KEEP_DAYS: i64 = 14;
/// How long the rows given up on stay, for the operator to look at.
pub const GIVEN_UP_KEEP_DAYS: i64 = 30;
const DAY_SECS: i64 = 86_400;
const PRUNE_EVERY_SECS: i64 = 3600;
const BATCH: u32 = 50;
const TICK: Duration = Duration::from_secs(30);

/// The HTTP side of a delivery, behind a trait so the dispatcher is tested with `fake::FakeSender`.
#[async_trait]
pub trait WebhookSender: Send + Sync {
    /// Posts `body` to `url` with `headers`; the response status, or why it could not be sent.
    async fn send(&self, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<u16, String>;
}

pub struct HttpSender {
    client: reqwest::Client,
}

impl HttpSender {
    pub fn new() -> HttpSender {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("a reqwest client with plain timeouts builds");
        HttpSender { client }
    }
}

impl Default for HttpSender {
    fn default() -> Self {
        HttpSender::new()
    }
}

#[async_trait]
impl WebhookSender for HttpSender {
    async fn send(&self, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<u16, String> {
        let mut request = self.client.post(url).body(body.to_vec());
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = request.send().await.map_err(|e| e.without_url().to_string())?;
        Ok(response.status().as_u16())
    }
}

/// A request ready to send: the body and its headers.
type SignedRequest = (Vec<u8>, Vec<(String, String)>);

/// What one pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TickReport {
    /// New daily summaries queued.
    pub queued: usize,
    pub delivered: usize,
    pub failed: usize,
    /// Old outbox rows deleted (the pass that prunes is at most one an hour).
    pub pruned: usize,
}

pub struct Dispatcher {
    store: Arc<Store>,
    sender: Arc<dyn WebhookSender>,
    url: String,
    secret: String,
    /// When the outbox was last pruned (unix seconds); 0 until the first pass.
    last_prune: AtomicI64,
}

/// The path and query of a URL (`http://host:1234/hooks/warden?x=1` → `/hooks/warden?x=1`), which is what gets signed.
fn path_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.find('/').map_or("/".to_string(), |i| rest[i..].to_string())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, StoreError> + Send + 'static) -> Result<T, StoreError> {
    tokio::task::spawn_blocking(f).await.map_err(|_| StoreError::Sqlite(rusqlite::Error::InvalidQuery))?
}

impl Dispatcher {
    pub fn new(store: Arc<Store>, sender: Arc<dyn WebhookSender>, url: &str, secret: &str) -> Dispatcher {
        Dispatcher { store, sender, url: url.to_string(), secret: secret.to_string(), last_prune: AtomicI64::new(0) }
    }

    /// The signed request for one event: the body and the headers.
    fn request_for(&self, event: &OutboxEvent, at: i64) -> Result<SignedRequest, String> {
        let kind: WebhookKind = serde_json::from_value(serde_json::Value::String(event.kind.clone()))
            .map_err(|e| format!("event {} has an unknown kind: {e}", event.id))?;
        let data = serde_json::from_str(&event.payload).map_err(|e| format!("event {} has a bad payload: {e}", event.id))?;
        let body = serde_json::to_vec(&WebhookEvent { id: event.id.clone(), kind, created_at: event.created_at, data })
            .map_err(|e| e.to_string())?;
        let signature = sign(&self.secret, at, "POST", &path_of(&self.url), &body);
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            (SIGNATURE_HEADER.to_string(), signature),
            (IDEMPOTENCY_HEADER.to_string(), event.id.clone()),
        ];
        Ok((body, headers))
    }

    /// One pass: queue the daily summaries that are missing, then send every event that is due.
    pub async fn tick(&self, at: i64) -> Result<TickReport, StoreError> {
        let store = self.store.clone();
        let prune = at - self.last_prune.load(Ordering::Relaxed) >= PRUNE_EVERY_SECS;
        if prune {
            self.last_prune.store(at, Ordering::Relaxed);
        }
        let (queued, pruned, due) = blocking(move || {
            // Pruning goes before the summaries are queued; the kept window is longer than theirs, so nothing comes back.
            let pruned = if prune { store.prune_outbox(at, DELIVERED_KEEP_DAYS * DAY_SECS, GIVEN_UP_KEEP_DAYS * DAY_SECS)? } else { 0 };
            let queued = store.enqueue_daily_summaries(at, SUMMARY_DAYS_BACK)?;
            Ok((queued, pruned, store.due_events(at, BATCH)?))
        })
        .await?;

        let mut report = TickReport { queued, pruned, ..TickReport::default() };
        for event in due {
            let outcome = match self.request_for(&event, at) {
                Ok((body, headers)) => self.sender.send(&self.url, &headers, &body).await,
                Err(why) => Err(why),
            };
            let store = self.store.clone();
            let id = event.id.clone();
            match outcome {
                Ok(status) if (200..300).contains(&status) => {
                    blocking(move || store.mark_delivered(&id, at)).await?;
                    report.delivered += 1;
                }
                other => {
                    let why = match other {
                        Ok(status) => format!("the site answered {status}"),
                        Err(why) => why,
                    };
                    let attempts = blocking(move || store.mark_failed(&id, at)).await?;
                    tracing::warn!(event = %event.id, attempts, "webhook not delivered: {why}");
                    report.failed += 1;
                }
            }
        }
        Ok(report)
    }

    /// Runs `tick` every 30 seconds, forever.
    pub async fn run(self: Arc<Self>) {
        loop {
            if let Err(e) = self.tick(now()).await {
                tracing::error!("the webhook dispatcher failed: {e}");
            }
            tokio::time::sleep(TICK).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSender;
    use crate::store::{EntryKind, UsageRecord};
    use warden_hosted_protocol::verify;

    const NOW: i64 = 1_800_000_000;
    const URL: &str = "http://site.test:9000/hooks/warden";

    fn drained_account() -> (Arc<Store>, String) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let (account, _) = store.create_account("acme", NOW).unwrap();
        store.credit(&account.id, EntryKind::Topup, 1_000, Some("t"), "", NOW).unwrap();
        store.place_hold(&account.id, "r1", 1_000, 60, NOW).unwrap();
        let record = UsageRecord {
            request_id: "r1".into(),
            account_id: account.id.clone(),
            model: "m".into(),
            target: "u/real-m".into(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cost: 500,
            charged: 1_000,
            ok: true,
        };
        store.settle(&record, NOW).unwrap();
        (store, account.id)
    }

    #[test]
    fn the_signed_path_is_the_urls_path_and_query() {
        assert_eq!(path_of("http://site.test:9000/hooks/warden"), "/hooks/warden");
        assert_eq!(path_of("https://site.test/hooks?x=1"), "/hooks?x=1");
        assert_eq!(path_of("https://site.test"), "/");
    }

    #[tokio::test]
    async fn the_outbox_is_pruned_at_most_once_an_hour() {
        let (store, _) = drained_account();
        let sender = Arc::new(FakeSender::new());
        let dispatcher = Dispatcher::new(store.clone(), sender, URL, "hook-secret");
        dispatcher.tick(NOW).await.unwrap(); // delivers the one event, at NOW

        let long_after = NOW + (DELIVERED_KEEP_DAYS + 1) * DAY_SECS;
        // The first pass after the first tick is within the hour of it: no pruning yet.
        assert_eq!(dispatcher.tick(NOW + 60).await.unwrap().pruned, 0);
        assert_eq!(store.outbox_stats().unwrap().delivered, 1);
        // Past the retention and past the hour, the delivered row goes; and straight after, nothing is left to go.
        assert_eq!(dispatcher.tick(long_after).await.unwrap().pruned, 1);
        assert_eq!(store.outbox_stats().unwrap().delivered, 0);
        assert_eq!(dispatcher.tick(long_after + 60).await.unwrap().pruned, 0);
    }

    #[tokio::test]
    async fn an_event_is_sent_signed_with_its_id_and_marked_delivered() {
        let (store, account) = drained_account();
        let sender = Arc::new(FakeSender::new());
        let dispatcher = Dispatcher::new(store.clone(), sender.clone(), URL, "hook-secret");

        let report = dispatcher.tick(NOW).await.unwrap();
        assert_eq!(report, TickReport { queued: 0, delivered: 1, failed: 0, pruned: 0 });

        let sent = &sender.deliveries()[0];
        assert_eq!(sent.url, URL);
        verify("hook-secret", sent.header(SIGNATURE_HEADER), "POST", "/hooks/warden", &sent.body, NOW).unwrap();
        let event: WebhookEvent = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(sent.header(IDEMPOTENCY_HEADER), Some(event.id.as_str()));
        assert_eq!(event.kind, WebhookKind::CreditsDepleted);
        assert_eq!(event.data["account_id"], account.as_str());
        assert_eq!(store.outbox_stats().unwrap().delivered, 1);

        // Nothing is sent twice.
        assert_eq!(dispatcher.tick(NOW + 1).await.unwrap(), TickReport::default());
        assert_eq!(sender.deliveries().len(), 1);
    }

    #[tokio::test]
    async fn a_failing_site_is_retried_after_the_backoff_and_then_delivered() {
        let (store, _) = drained_account();
        let sender = Arc::new(FakeSender::scripted([Ok(500), Err("connection refused".to_string())]));
        let dispatcher = Dispatcher::new(store.clone(), sender.clone(), URL, "hook-secret");

        assert_eq!(dispatcher.tick(NOW).await.unwrap().failed, 1);
        assert_eq!(dispatcher.tick(NOW + 10).await.unwrap(), TickReport::default(), "30 s of backoff");
        assert_eq!(dispatcher.tick(NOW + 30).await.unwrap().failed, 1);
        assert_eq!(dispatcher.tick(NOW + 60).await.unwrap(), TickReport::default(), "60 s of backoff now");
        // The script is over, so the site is back.
        assert_eq!(dispatcher.tick(NOW + 90).await.unwrap().delivered, 1);

        let ids: Vec<_> = sender.deliveries().iter().map(|d| d.header(IDEMPOTENCY_HEADER).unwrap().to_string()).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|id| *id == ids[0]), "every retry carries the same event id");
        assert_eq!(store.outbox_stats().unwrap().pending, 0);
    }

    #[tokio::test]
    async fn the_daily_summary_is_queued_and_sent_once() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let (account, _) = store.create_account("acme", NOW).unwrap();
        store.credit(&account.id, EntryKind::Topup, 10_000, Some("t"), "", NOW).unwrap();
        let yesterday = NOW - 28_800 - 43_200;
        store.place_hold(&account.id, "r1", 100, 60, yesterday).unwrap();
        store
            .settle(
                &UsageRecord {
                    request_id: "r1".into(),
                    account_id: account.id.clone(),
                    model: "m".into(),
                    target: "u/real-m".into(),
                    prompt_tokens: 5,
                    completion_tokens: 6,
                    cost: 50,
                    charged: 100,
                    ok: true,
                },
                yesterday,
            )
            .unwrap();
        let sender = Arc::new(FakeSender::new());
        let dispatcher = Dispatcher::new(store, sender.clone(), URL, "hook-secret");

        assert_eq!(dispatcher.tick(NOW).await.unwrap(), TickReport { queued: 1, delivered: 1, failed: 0, pruned: 0 });
        assert_eq!(dispatcher.tick(NOW + 60).await.unwrap(), TickReport::default());
        let event: WebhookEvent = serde_json::from_slice(&sender.deliveries()[0].body).unwrap();
        assert_eq!(event.kind, WebhookKind::UsageDailySummary);
        assert_eq!(event.data["total_charged_micro_usd"], 100);
    }

    #[tokio::test]
    async fn a_wrong_secret_is_what_the_site_would_reject() {
        let (store, _) = drained_account();
        let sender = Arc::new(FakeSender::new());
        Dispatcher::new(store, sender.clone(), URL, "hook-secret").tick(NOW).await.unwrap();
        let sent = &sender.deliveries()[0];
        assert!(verify("another-secret", sent.header(SIGNATURE_HEADER), "POST", "/hooks/warden", &sent.body, NOW).is_err());
        assert!(verify("hook-secret", sent.header(SIGNATURE_HEADER), "POST", "/elsewhere", &sent.body, NOW).is_err());
    }
}

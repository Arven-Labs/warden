//! The gateway against a real model provider (P168 h): what the fake upstream of every other test can't confirm. That the
//! `Authorization: Bearer` and the `base_url` + `/chat/completions` URL are what a provider takes, that it accepts
//! `max_tokens` and `stream_options.include_usage`, that the `usage` really comes back (in the answer and in the last
//! chunk of a stream), that a real answer's `model` is swapped for the catalog's, and what happens on a bad key or model.
//! It also lines the prompt estimate (`estimate_prompt_tokens`, P168 f) up against what a real tokenizer counts.
//!
//! `#[ignore]`d: it calls a paid API (a few cents at the very most: every call is a handful of tokens). The key comes only
//! from the environment of the one command, the same variables as `warden-bootstrap`'s `real_model.rs`:
//!
//! ```text
//! WARDEN_EVAL_BASE_URL=https://openrouter.ai/api/v1 WARDEN_EVAL_MODEL=<model id> WARDEN_EVAL_KEY=<key> \
//!   cargo test -p warden-gateway --test real_provider -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `WARDEN_EVAL_BASE_URL` defaults to OpenRouter's. A model is not deterministic and a reasoning model may spend its whole
//! `max_tokens` thinking, so nothing here asserts on the words of an answer, only on what the gateway did around it.

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use warden_gateway::http::GatewayServer;
use warden_gateway::store::EntryKind;
use warden_gateway::upstream::HttpUpstream;
use warden_gateway::{estimate_prompt_tokens, Catalog, Gateway, Store, Tokens};

const START_CREDIT: i64 = 10_000_000;

fn need(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set: this test calls a real provider (see the header of the file)"))
}

struct Rig {
    addr: SocketAddr,
    store: Arc<Store>,
    catalog: Arc<Catalog>,
    account: String,
    token: String,
    /// The provider's own id for the model, which must never reach the caller.
    real_model: String,
}

/// A gateway over the real provider. `key_env` names the variable the key is read from; `model` is the provider's id.
async fn rig(key_env: &str, model: &str) -> Rig {
    let base_url = std::env::var("WARDEN_EVAL_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
    let text = format!(
        r#"
margin_percent = 30
default_max_tokens = 64

[[upstreams]]
id = "p"
base_url = "{base_url}"
api_key_env = "{key_env}"

[[models]]
name = "fast"
upstream = "p"
upstream_model = "{model}"
cost_in = 1.0
cost_out = 2.0
"#
    );
    let catalog = Arc::new(Catalog::parse(&text, |name| std::env::var(name).ok()).unwrap());
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (account, token) = store.create_account("real", 0).unwrap();
    store.credit(&account.id, EntryKind::Topup, START_CREDIT, Some("t"), "", 0).unwrap();
    let server = GatewayServer::bind("127.0.0.1:0".parse().unwrap(), Gateway::new(store.clone(), catalog.clone()), Arc::new(HttpUpstream::new()))
        .await
        .unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Rig { addr, store, catalog, account: account.id, token, real_model: model.to_string() }
}

/// The rig with the key and model from the environment.
async fn real_rig() -> Rig {
    rig("WARDEN_EVAL_KEY", &need("WARDEN_EVAL_MODEL")).await
}

async fn post_chat(rig: &Rig, body: &Value) -> (u16, String, String) {
    let body = body.to_string();
    let mut stream = TcpStream::connect(rig.addr).await.unwrap();
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        rig.addr,
        rig.token,
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), head.to_string(), body.to_string())
}

fn tokens_of(usage: &Value) -> Tokens {
    Tokens { prompt: usage["prompt_tokens"].as_u64().expect("usage.prompt_tokens"), completion: usage["completion_tokens"].as_u64().expect("usage.completion_tokens") }
}

/// The debit the ledger holds for the one call this rig made.
fn debit(rig: &Rig) -> i64 {
    let entries = rig.store.entries(&rig.account, 10).unwrap();
    let debit = entries.iter().find(|e| e.kind == EntryKind::Debit).expect("the call was charged");
    -debit.amount
}

fn no_hold_left(rig: &Rig) {
    assert!(rig.store.holds(&rig.account, 0).unwrap().is_empty(), "a hold was left behind");
}

#[tokio::test]
#[ignore = "calls a paid API; see the header"]
async fn a_whole_answer_comes_back_under_the_catalog_name_and_is_charged_from_real_usage() {
    let rig = real_rig().await;
    let body = json!({"model": "fast", "max_tokens": 64, "messages": [{"role": "user", "content": "Reply with the single word: pong"}]});
    let (status, head, reply) = post_chat(&rig, &body).await;
    assert_eq!(status, 200, "{reply}");

    let answer: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(answer["model"], "fast", "the caller sees the catalog's name, not the provider's: {reply}");
    assert!(!reply.contains(&rig.real_model), "the provider's model id reached the caller: {reply}");
    let used = tokens_of(&answer["usage"]);
    println!("usage: {} prompt + {} completion tokens", used.prompt, used.completion);
    assert!(used.prompt > 0, "{reply}");

    let charged = rig.catalog.model("fast").unwrap().price(used).charged;
    assert_eq!(debit(&rig), charged, "the ledger is charged from the provider's own usage");
    assert!(head.to_lowercase().contains(&format!("x-warden-charged-micros: {charged}")), "{head}");
    no_hold_left(&rig);
}

#[tokio::test]
#[ignore = "calls a paid API; see the header"]
async fn a_stream_is_forwarded_under_the_catalog_name_and_charged_from_real_usage() {
    let rig = real_rig().await;
    let body = json!({"model": "fast", "stream": true, "max_tokens": 64, "messages": [{"role": "user", "content": "Count from one to five."}]});
    let (status, head, reply) = post_chat(&rig, &body).await;
    assert_eq!(status, 200, "{reply}");
    assert!(head.to_lowercase().contains("text/event-stream"), "{head}");
    assert!(!reply.contains(&rig.real_model), "the provider's model id reached the caller in the stream: {reply}");
    assert!(reply.contains("[DONE]"), "the stream did not end with [DONE]: {reply}");

    let chunks: Vec<Value> =
        reply.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim).filter(|d| *d != "[DONE]").filter_map(|d| serde_json::from_str(d).ok()).collect();
    assert!(!chunks.is_empty(), "no data chunk in: {reply}");
    for chunk in &chunks {
        if let Some(model) = chunk.get("model") {
            assert_eq!(model, "fast", "a chunk carries the provider's model name: {chunk}");
        }
    }
    let usage = chunks.iter().rev().find_map(|c| c.get("usage").filter(|u| u.is_object())).expect("include_usage: no chunk carries `usage`");
    let used = tokens_of(usage);
    println!("stream usage: {} prompt + {} completion tokens in {} chunks", used.prompt, used.completion, chunks.len());

    assert_eq!(debit(&rig), rig.catalog.model("fast").unwrap().price(used).charged, "the stream is charged from the provider's own usage");
    no_hold_left(&rig);
}

/// About 300 words of English prose.
fn english() -> String {
    let sentences = [
        "The committee met on Tuesday to discuss the budget for the coming year, and several members raised concerns about rising costs.",
        "After a long debate, they agreed to postpone the decision until more detailed figures were available from the finance department.",
        "Meanwhile, the engineers continued their work on the bridge, which had been delayed twice because of unusually heavy rain.",
        "Local residents, who had hoped to use the new crossing by the summer, were told that the opening would probably slip to autumn.",
        "A spokesperson explained that safety inspections could not be rushed, and that every cable had to be tested before traffic was allowed.",
    ];
    sentences.iter().cycle().take(12).copied().collect::<Vec<_>>().join(" ")
}

/// Portuguese with plenty of accents.
fn portuguese() -> String {
    let sentences = [
        "A reunião do conselho aconteceu na terça-feira, e os conselheiros discutiram o orçamento do próximo ano com bastante cautela.",
        "Depois de um longo debate, ficou decidido que a votação seria adiada até que o departamento financeiro apresentasse números mais detalhados.",
        "Enquanto isso, os engenheiros seguiram trabalhando na ponte, que já tinha sido adiada duas vezes por causa das chuvas fortes.",
        "Os moradores, que esperavam atravessar a nova ponte antes do verão, foram informados de que a inauguração ficará para o outono.",
    ];
    sentences.iter().cycle().take(14).copied().collect::<Vec<_>>().join(" ")
}

fn code() -> String {
    let snippet = r#"
fn spent_since(&self, account_id: &str, since: i64) -> Result<Micros> {
    let conn = self.lock();
    let spent: Micros = conn.query_row(
        "SELECT COALESCE(SUM(charged), 0) FROM usage_events WHERE account_id = ?1 AND created_at >= ?2",
        params![account_id, since],
        |row| row.get(0),
    )?;
    Ok(spent)
}
"#;
    snippet.repeat(8)
}

#[tokio::test]
#[ignore = "calls a paid API; see the header"]
async fn the_prompt_estimate_does_not_fall_below_what_the_provider_counts() {
    let rig = real_rig().await;
    let tools = json!([
        {"type": "function", "function": {"name": "get_weather", "description": "Looks up the current weather for a city and returns the temperature, the wind and a short description of the sky.",
            "parameters": {"type": "object", "properties": {"city": {"type": "string", "description": "The name of the city, for example Lisbon"}, "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}}, "required": ["city"]}}},
        {"type": "function", "function": {"name": "search_notes", "description": "Searches the user's notes by keyword and returns the titles of the ten best matches, newest first.",
            "parameters": {"type": "object", "properties": {"query": {"type": "string", "description": "Words to look for"}, "limit": {"type": "integer"}}, "required": ["query"]}}},
        {"type": "function", "function": {"name": "create_event", "description": "Adds an event to the calendar with a title, a start time and an optional list of guests.",
            "parameters": {"type": "object", "properties": {"title": {"type": "string"}, "start": {"type": "string", "description": "ISO 8601 date and time"}, "guests": {"type": "array", "items": {"type": "string"}}}, "required": ["title", "start"]}}},
    ]);
    let cases: Vec<(&str, Value)> = vec![
        ("english", json!({"model": "fast", "max_tokens": 16, "messages": [{"role": "user", "content": english()}]})),
        ("portuguese", json!({"model": "fast", "max_tokens": 16, "messages": [{"role": "user", "content": portuguese()}]})),
        ("code", json!({"model": "fast", "max_tokens": 16, "messages": [{"role": "user", "content": code()}]})),
        (
            "tools",
            json!({"model": "fast", "max_tokens": 16, "tool_choice": "none", "tools": tools,
                "messages": [{"role": "system", "content": "You are a helpful assistant."}, {"role": "user", "content": "What is the weather in Lisbon, and what is on my calendar?"}]}),
        ),
    ];

    let mut below = Vec::new();
    for (name, body) in &cases {
        let estimate = estimate_prompt_tokens(body);
        let (status, _, reply) = post_chat(&rig, body).await;
        assert_eq!(status, 200, "{name}: {reply}");
        let real = tokens_of(&serde_json::from_str::<Value>(&reply).unwrap()["usage"]).prompt;
        println!("estimate {estimate:>6}  real {real:>6}  ratio {:.2}  {name}", estimate as f64 / real as f64);
        // A short prompt is dominated by the provider's own template tokens, which no content estimate can see: read, don't assert.
        if real >= 150 && estimate < real {
            below.push(format!("{name}: estimate {estimate} < real {real}"));
        }
    }
    assert!(below.is_empty(), "the estimate fell below what the provider counted: {below:?}");
}

/// The gateway's answer to a provider that refused: a 502 in its own words, nothing charged, no hold left.
async fn assert_refusal_costs_nothing(rig: &Rig, what: &str) {
    let body = json!({"model": "fast", "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]});
    let (status, _, reply) = post_chat(rig, &body).await;
    assert_eq!(status, 502, "{what}: {reply}");
    let error: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(error["error"]["code"], "upstream_error", "{what}: {reply}");
    assert!(error["error"]["message"].as_str().unwrap().contains("could not answer"), "{what}: {reply}");
    assert_eq!(rig.store.balance(&rig.account).unwrap(), START_CREDIT, "{what}: a refused call must not be charged");
    no_hold_left(rig);
}

#[tokio::test]
#[ignore = "calls a paid API; see the header"]
async fn a_provider_refusal_is_a_502_that_costs_nothing() {
    // No key at all: the provider says 401, in its own words, which the caller must not see.
    let no_key = rig("WARDEN_NO_SUCH_KEY_VARIABLE", &need("WARDEN_EVAL_MODEL")).await;
    assert_refusal_costs_nothing(&no_key, "no API key").await;

    // A real key and a model that doesn't exist.
    let no_model = rig("WARDEN_EVAL_KEY", "warden-test/no-such-model-12345").await;
    assert_refusal_costs_nothing(&no_model, "unknown model").await;
}

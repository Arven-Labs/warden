//! `code_task` (P89, P124) on a real hub: a model calls the tool in an ordinary chat turn, the engine's ask goes to the
//! person as an approval card and the answer goes back to the engine, and the model gets what the engine did. The engine is
//! a script (the opencode itself is covered in `warden-core`), and the tool is registered the way the hub's `main` does it
//! (`register_code_task`), with the same slot the engine is put in afterwards.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use warden_core::code_engine::{CodeEngine, CodeEvent, CodeMode, TurnOutcome, TurnRequest};
use warden_core::memory::Vault;
use warden_core::model::{response_stream, ChatStream, Message, ModelProvider, Response, Role, ToolCall};
use warden_core::orchestrator::Orchestrator;
use warden_core::tool::code_task::CodeEngineSlot;
use warden_core::tool::{ApprovalRequest, Approver, ToolSpec};
use warden_server::code_turns::register_code_task;
use warden_server::{ClientMessage, Server, ServerConnection, ServerMessage};
use warden_server_protocol::protocol::ProjectDto;

const KEY: &str = "test-key";

/// Asks for `code_task` on the first call and, once the tool answered, says what it answered.
struct Manager {
    offered: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ModelProvider for Manager {
    async fn chat_stream(&self, messages: Vec<Message>, tools: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
        *self.offered.lock().unwrap() = tools.iter().map(|t| t.name.clone()).collect();
        let last = messages.last().unwrap();
        if last.role == Role::Tool {
            return Ok(response_stream(Response { content: format!("tool said: {}", last.content), tool_calls: Vec::new(), usage: None }));
        }
        let call = ToolCall { id: "call-1".into(), name: "code_task".into(), arguments: json!({ "project": "repo", "task": "add a test" }), thought_signature: None };
        Ok(response_stream(Response { content: String::new(), tool_calls: vec![call], usage: None }))
    }
}

/// A task that wants one yes (to run `cargo test`) and says whether it got it.
#[derive(Default)]
struct Scripted {
    asked: Mutex<Vec<(String, CodeMode)>>,
    approvals: Mutex<Vec<bool>>,
}

#[async_trait]
impl CodeEngine for Scripted {
    async fn run_turn(&self, request: TurnRequest, approver: Option<Arc<dyn Approver>>, _: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
        self.asked.lock().unwrap().push((request.workdir.clone(), *request.mode.borrow()));
        let allowed = match approver {
            Some(approver) => approver.approve(ApprovalRequest::new("Repo", "bash", "cargo test")).await,
            None => false,
        };
        self.approvals.lock().unwrap().push(allowed);
        let text = if allowed { "tests added" } else { "the person refused" };
        Ok(TurnOutcome { session_id: "ses_9".into(), text: text.into(), tools_used: vec!["bash".into()] })
    }

    async fn abort(&self, _: &str, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct Hub {
    url: String,
    offered: Arc<Mutex<Vec<String>>>,
}

/// A hub whose engine is `engine` — or, with `None`, one whose slot is never filled.
async fn spin_up(engine: Option<Arc<Scripted>>) -> Hub {
    std::env::set_var(warden_core::memory::embed::OFF_SWITCH, "1");
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("warden-server-code-task-{}-{n}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let offered = Arc::new(Mutex::new(Vec::new()));
    let mut orchestrator = Orchestrator::new(Arc::new(Manager { offered: offered.clone() }), Arc::new(Vault::new(dir.join("vault"))));
    let slot: CodeEngineSlot = Arc::new(OnceLock::new());
    register_code_task(&mut orchestrator, &slot, None);
    if let Some(engine) = engine {
        let engine: Arc<dyn CodeEngine> = engine;
        slot.set(engine).ok();
    }
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), KEY, "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json")).await.unwrap().with_users_dir(dir.join("users"));
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Hub { url: format!("ws://{addr}"), offered }
}

async fn owner(hub: &Hub) -> ServerConnection {
    ServerConnection::connect(&hub.url, "laptop", "Laptop", KEY).await.unwrap()
}

async fn next(conn: &mut ServerConnection) -> ServerMessage {
    loop {
        match tokio::time::timeout(Duration::from_secs(10), conn.recv()).await.expect("no message").unwrap().expect("connection closed") {
            ServerMessage::ConversationsChanged { .. } | ServerMessage::Pong { .. } | ServerMessage::ChatEvent { .. } => continue,
            msg => return msg,
        }
    }
}

async fn save_code_project(conn: &mut ServerConnection) {
    let project = ProjectDto { id: "repo".into(), name: "Repo".into(), description: "The main repo".into(), instructions: "Use tabs.".into(), workdir: Some("/home/me/repo".into()), code: true };
    conn.send(&ClientMessage::SaveProject { request_id: 1, project, overwrite: false }).await.unwrap();
    assert!(matches!(next(conn).await, ServerMessage::ProjectOk { .. }));
}

async fn ask_for_the_task(conn: &mut ServerConnection) {
    conn.send(&ClientMessage::Chat { message: "get the repo tested".into(), conversation_id: Some("c1".into()), attachments: Vec::new(), agent_id: None, project_id: None, workdir: None, thread_of: None }).await.unwrap();
}

#[tokio::test]
async fn the_engines_ask_reaches_the_person_and_the_model_gets_what_the_engine_did() {
    let engine = Arc::new(Scripted::default());
    let hub = spin_up(Some(engine.clone())).await;
    let mut me = owner(&hub).await;
    save_code_project(&mut me).await;

    ask_for_the_task(&mut me).await;
    let approval_id = match next(&mut me).await {
        ServerMessage::ApprovalRequest { approval_id, target, action, detail, .. } => {
            assert_eq!((target.as_str(), action.as_str(), detail.as_str()), ("Repo", "bash", "cargo test"));
            approval_id
        }
        other => panic!("{other:?}"),
    };
    me.send(&ClientMessage::ResolveApproval { approval_id, approved: true, always: false }).await.unwrap();
    match next(&mut me).await {
        ServerMessage::ChatResponse { content, .. } => {
            assert!(content.starts_with("tool said: "), "{content}");
            assert!(content.contains("tests added") && content.contains("ses_9"), "{content}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(*engine.asked.lock().unwrap(), [("/home/me/repo".to_string(), CodeMode::Manual)]);
    assert_eq!(*engine.approvals.lock().unwrap(), [true]);
    assert!(hub.offered.lock().unwrap().iter().any(|t| t == "code_task"), "the model was offered the tool");
}

/// A manager that opens two code tasks in the background (one per project), reads both with `jobs`, and says what came back.
struct BackgroundManager;

#[async_trait]
impl ModelProvider for BackgroundManager {
    async fn chat_stream(&self, messages: Vec<Message>, _: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
        let results: Vec<&Message> = messages.iter().filter(|m| m.role == Role::Tool).collect();
        let calls = |calls: Vec<(&str, serde_json::Value)>| {
            let tool_calls = calls.into_iter().enumerate().map(|(i, (name, arguments))| ToolCall { id: format!("call-{i}"), name: name.into(), arguments, thought_signature: None }).collect();
            Ok(response_stream(Response { content: String::new(), tool_calls, usage: None }))
        };
        match results.len() {
            0 => calls(vec![
                ("code_task", json!({ "project": "repo", "task": "add a test", "background": true })),
                ("code_task", json!({ "project": "site", "task": "fix the footer", "background": true })),
            ]),
            2 => calls(vec![("jobs", json!({ "action": "result", "job_id": "job-1" })), ("jobs", json!({ "action": "result", "job_id": "job-2" }))]),
            _ => Ok(response_stream(Response { content: results[2..].iter().map(|m| m.content.clone()).collect::<Vec<_>>().join(" || "), tool_calls: Vec::new(), usage: None })),
        }
    }
}

/// Answers at once, with the session of its folder, and never needs the person.
struct Instant;

#[async_trait]
impl CodeEngine for Instant {
    async fn run_turn(&self, request: TurnRequest, _: Option<Arc<dyn Approver>>, _: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
        Ok(TurnOutcome { session_id: format!("ses_{}", request.workdir.rsplit('/').next().unwrap()), text: format!("done: {}", request.prompt), tools_used: Vec::new() })
    }

    async fn abort(&self, _: &str, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_manager_opens_two_code_tasks_in_the_background_and_both_are_on_the_tasks_screen() {
    std::env::set_var(warden_core::memory::embed::OFF_SWITCH, "1");
    let dir = std::env::temp_dir().join(format!("warden-server-code-bg-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("agent_tasks.jsonl");
    let mut orchestrator = Orchestrator::new(Arc::new(BackgroundManager), Arc::new(Vault::new(dir.join("vault"))));
    orchestrator.register_tool(Arc::new(warden_core::tool::job_tools::JobsTool::new()));
    let slot: CodeEngineSlot = Arc::new(OnceLock::new());
    register_code_task(&mut orchestrator, &slot, None);
    let engine: Arc<dyn CodeEngine> = Arc::new(Instant);
    slot.set(engine).ok();
    let orchestrator = orchestrator.with_parallel_jobs(2).with_task_recorder(Arc::new(warden_bootstrap::agent_tasks::FileTaskRecorder::new(&log)));
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), KEY, "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_users_dir(dir.join("users"))
        .with_agent_tasks(Some(log));
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    let mut me = ServerConnection::connect(&format!("ws://{addr}"), "laptop", "Laptop", KEY).await.unwrap();

    for (id, name, workdir) in [("repo", "Repo", "/home/me/repo"), ("site", "Site", "/home/me/site")] {
        let project = ProjectDto { id: id.into(), name: name.into(), description: String::new(), instructions: String::new(), workdir: Some(workdir.into()), code: true };
        me.send(&ClientMessage::SaveProject { request_id: 1, project, overwrite: false }).await.unwrap();
        assert!(matches!(next(&mut me).await, ServerMessage::ProjectOk { .. }));
    }

    ask_for_the_task(&mut me).await;
    match next(&mut me).await {
        ServerMessage::ChatResponse { content, .. } => {
            assert!(content.contains("done: add a test") && content.contains("session_id: ses_repo"), "{content}");
            assert!(content.contains("done: fix the footer") && content.contains("session_id: ses_site"), "{content}");
        }
        other => panic!("{other:?}"),
    }

    me.send(&ClientMessage::ListAgentTasks { request_id: 7 }).await.unwrap();
    let tasks = loop {
        if let ServerMessage::AgentTaskList { tasks, .. } = next(&mut me).await {
            break tasks;
        }
    };
    let mut assignees: Vec<_> = tasks.iter().map(|t| (t.assignee.as_str(), t.state.as_str())).collect();
    assignees.sort();
    assert_eq!(assignees, [("opencode · Repo", "done"), ("opencode · Site", "done")]);
    assert_eq!(tasks[0].group, tasks[1].group, "one turn, one group: the screen shows them with a progress");
}

/// Narrates every task: its session, some text of its own, and an edit that starts and finishes.
struct Narrates;

#[async_trait]
impl CodeEngine for Narrates {
    async fn run_turn(&self, request: TurnRequest, _: Option<Arc<dyn Approver>>, on_event: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
        let edit = |status| CodeEvent::Tool(warden_core::code_engine::ToolEvent { call_id: "c1".into(), tool: "edit".into(), title: "src/lib.rs".into(), status });
        on_event(CodeEvent::Session("ses_n".into()));
        on_event(CodeEvent::Text("the engine's own words".into()));
        on_event(edit(warden_core::code_engine::ToolStatus::Running));
        tokio::task::yield_now().await;
        on_event(edit(warden_core::code_engine::ToolStatus::Completed));
        Ok(TurnOutcome { session_id: "ses_n".into(), text: format!("done: {}", request.prompt), tools_used: vec!["edit".into()] })
    }

    async fn abort(&self, _: &str, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn the_chat_sees_each_tasks_tool_rows_and_words_live_told_apart() {
    use warden_server_protocol::protocol::{ChatEventDto, ToolStatusDto};
    std::env::set_var(warden_core::memory::embed::OFF_SWITCH, "1");
    let dir = std::env::temp_dir().join(format!("warden-server-code-live-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut orchestrator = Orchestrator::new(Arc::new(BackgroundManager), Arc::new(Vault::new(dir.join("vault"))));
    orchestrator.register_tool(Arc::new(warden_core::tool::job_tools::JobsTool::new()));
    let slot: CodeEngineSlot = Arc::new(OnceLock::new());
    register_code_task(&mut orchestrator, &slot, None);
    let engine: Arc<dyn CodeEngine> = Arc::new(Narrates);
    slot.set(engine).ok();
    let orchestrator = orchestrator.with_parallel_jobs(2);
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), KEY, "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json")).await.unwrap().with_users_dir(dir.join("users"));
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    let mut me = ServerConnection::connect(&format!("ws://{addr}"), "laptop", "Laptop", KEY).await.unwrap();
    for (id, name, workdir) in [("repo", "Repo", "/home/me/repo"), ("site", "Site", "/home/me/site")] {
        let project = ProjectDto { id: id.into(), name: name.into(), description: String::new(), instructions: String::new(), workdir: Some(workdir.into()), code: true };
        me.send(&ClientMessage::SaveProject { request_id: 1, project, overwrite: false }).await.unwrap();
        assert!(matches!(next(&mut me).await, ServerMessage::ProjectOk { .. }));
    }

    ask_for_the_task(&mut me).await;
    let mut events = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(10), me.recv()).await.expect("no message").unwrap().expect("connection closed") {
            ServerMessage::ChatEvent { event, .. } => events.push(event),
            ServerMessage::ChatResponse { content, .. } => {
                assert!(content.contains("done: add a test") && content.contains("done: fix the footer"), "{content}");
                break;
            }
            ServerMessage::ChatError { message, .. } => panic!("{message}"),
            _ => {}
        }
    }

    let rows: Vec<(String, String, ToolStatusDto)> = events
        .iter()
        .filter_map(|e| match e {
            ChatEventDto::Tool { call_id, title, status, .. } => Some((call_id.clone(), title.clone(), *status)),
            _ => None,
        })
        .collect();
    for (id, title) in [("repo:c1", "Repo: src/lib.rs"), ("site:c1", "Site: src/lib.rs")] {
        let mine: Vec<_> = rows.iter().filter(|(call, _, _)| call == id).collect();
        assert_eq!(mine.iter().map(|(_, t, s)| (t.as_str(), *s)).collect::<Vec<_>>(), [(title, ToolStatusDto::Running), (title, ToolStatusDto::Completed)], "{id}: {rows:?}");
    }
    // The words come too, each marked with the task they are from, so two tasks side by side are not run together.
    let mut words: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|e| match e {
            ChatEventDto::Text { text, source: Some(source) } => Some((source.as_str(), text.as_str())),
            _ => None,
        })
        .collect();
    words.sort();
    assert_eq!(words, [("Repo", "the engine's own words"), ("Site", "the engine's own words")], "{events:?}");
    assert!(!events.iter().any(|e| matches!(e, ChatEventDto::Text { source: None, .. })), "and none of them is unmarked: {events:?}");
}

/// Asks for a plan, then for accept-all, then says what each answered.
struct ModeAsker;

#[async_trait]
impl ModelProvider for ModeAsker {
    async fn chat_stream(&self, messages: Vec<Message>, _: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
        let results: Vec<&Message> = messages.iter().filter(|m| m.role == Role::Tool).collect();
        let call = |mode: &str| {
            let call = ToolCall { id: "call-1".into(), name: "code_task".into(), arguments: json!({ "project": "repo", "task": "look around", "mode": mode }), thought_signature: None };
            Ok(response_stream(Response { content: String::new(), tool_calls: vec![call], usage: None }))
        };
        match results.len() {
            0 => call("plan"),
            1 => call("acceptAll"),
            _ => Ok(response_stream(Response { content: results.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join(" || "), tool_calls: Vec::new(), usage: None })),
        }
    }
}

/// Writes down the mode each task arrived in.
#[derive(Default)]
struct ModeProbe {
    modes: Mutex<Vec<CodeMode>>,
}

#[async_trait]
impl CodeEngine for ModeProbe {
    async fn run_turn(&self, request: TurnRequest, _: Option<Arc<dyn Approver>>, _: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
        self.modes.lock().unwrap().push(*request.mode.borrow());
        Ok(TurnOutcome { session_id: "ses_p".into(), text: "a plan".into(), tools_used: Vec::new() })
    }

    async fn abort(&self, _: &str, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_manager_can_ask_for_a_plan_but_never_for_accept_all() {
    std::env::set_var(warden_core::memory::embed::OFF_SWITCH, "1");
    let dir = std::env::temp_dir().join(format!("warden-server-code-mode-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut orchestrator = Orchestrator::new(Arc::new(ModeAsker), Arc::new(Vault::new(dir.join("vault"))));
    let slot: CodeEngineSlot = Arc::new(OnceLock::new());
    register_code_task(&mut orchestrator, &slot, None);
    let probe = Arc::new(ModeProbe::default());
    let engine: Arc<dyn CodeEngine> = probe.clone();
    slot.set(engine).ok();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), KEY, "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json")).await.unwrap().with_users_dir(dir.join("users"));
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    let mut me = ServerConnection::connect(&format!("ws://{addr}"), "laptop", "Laptop", KEY).await.unwrap();
    save_code_project(&mut me).await;

    ask_for_the_task(&mut me).await;
    match next(&mut me).await {
        ServerMessage::ChatResponse { content, .. } => {
            assert!(content.contains("a plan"), "the plan came back: {content}");
            assert!(content.contains("more than this agent may ask for") && content.contains("manual, acceptEdits, plan"), "accept-all was refused with the allowed list: {content}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(*probe.modes.lock().unwrap(), [CodeMode::Plan], "only the plan reached the engine");
}

#[tokio::test]
async fn a_no_from_the_person_reaches_the_engine() {
    let engine = Arc::new(Scripted::default());
    let hub = spin_up(Some(engine.clone())).await;
    let mut me = owner(&hub).await;
    save_code_project(&mut me).await;

    ask_for_the_task(&mut me).await;
    let approval_id = match next(&mut me).await {
        ServerMessage::ApprovalRequest { approval_id, .. } => approval_id,
        other => panic!("{other:?}"),
    };
    me.send(&ClientMessage::ResolveApproval { approval_id, approved: false, always: false }).await.unwrap();
    match next(&mut me).await {
        ServerMessage::ChatResponse { content, .. } => assert!(content.contains("the person refused"), "{content}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(*engine.approvals.lock().unwrap(), [false]);
}

#[tokio::test]
async fn a_hub_whose_engine_never_started_tells_the_model_instead_of_hanging() {
    let hub = spin_up(None).await;
    let mut me = owner(&hub).await;
    save_code_project(&mut me).await;

    ask_for_the_task(&mut me).await;
    match next(&mut me).await {
        ServerMessage::ChatResponse { content, .. } => assert!(content.contains("isn't available"), "{content}"),
        other => panic!("{other:?}"),
    }
}

//! Checks, with a real model, what only passed with a made-up one so far (P156 item 1): that the model really calls
//! `use_skill` and `manage_skill` (P73), `manage_tasks` (P95) and `delegate_task` two levels deep (P60), and that the
//! hub's own tools do what it asked. Each test builds the orchestrator the way `bootstrap` does (same tools, same
//! types) around the real model, in throwaway folders, and looks at the effect (a saved skill, a saved task, a token
//! only the innermost sub-agent can read), never at how the model words things.
//!
//! `#[ignore]`d: it calls a paid API. Nothing of yours is read or written, and the key comes only from the environment
//! of the one command:
//!
//! ```text
//! WARDEN_EVAL_PROVIDER=openai_compatible WARDEN_EVAL_BASE_URL=https://openrouter.ai/api/v1 \
//!   WARDEN_EVAL_MODEL=<model id> WARDEN_EVAL_KEY=<key> \
//!   cargo test -p warden-bootstrap --test real_model -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `WARDEN_EVAL_PROVIDER` is `anthropic`, `openai`, `gemini` or `openai_compatible` (which also needs
//! `WARDEN_EVAL_BASE_URL`; OpenRouter is `https://openrouter.ai/api/v1`). A model is not deterministic: a red test says
//! what the model did instead, so read the printed events before blaming the code.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use warden_bootstrap::{load_config_from_path, ManageTasksTool};
use warden_core::memory::Vault;
use warden_core::model::{ModelProvider, StreamEvent};
use warden_core::orchestrator::{MessageOutcome, Orchestrator};
use warden_core::skill::{Skill, SkillStore};
use warden_core::tool::delegate::DelegateTool;
use warden_core::tool::skill_tools::{ManageSkillTool, ReadSkillFileTool, UseSkillTool};
use warden_core::tool::{ApprovalRequest, Approver, Tool, ToolSpec};

fn real_model() -> Arc<dyn ModelProvider> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let provider = var("WARDEN_EVAL_PROVIDER").expect("set WARDEN_EVAL_PROVIDER (anthropic, openai, gemini or openai_compatible)");
    let model = var("WARDEN_EVAL_MODEL").expect("set WARDEN_EVAL_MODEL to the model id");
    let key = var("WARDEN_EVAL_KEY").unwrap_or_default();
    match provider.as_str() {
        "anthropic" => Arc::new(warden_core::model::anthropic::AnthropicProvider::new(key, model)),
        "openai" => Arc::new(warden_core::model::openai::OpenAiProvider::new(key, model)),
        "gemini" => Arc::new(warden_core::model::gemini::GeminiProvider::new(key, model)),
        "openai_compatible" => {
            let base = var("WARDEN_EVAL_BASE_URL").expect("openai_compatible needs WARDEN_EVAL_BASE_URL");
            Arc::new(warden_core::model::openai::OpenAiProvider::with_base_url(key, model, base))
        }
        other => panic!("unknown WARDEN_EVAL_PROVIDER '{other}'"),
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("warden-real-model-{name}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What one turn did: the tools the model asked for, in order, and how it came out.
struct Turn {
    tools: Vec<String>,
    outcome: MessageOutcome,
}

impl Turn {
    fn report(&self, name: &str) {
        let tokens = self.outcome.usage.as_ref().map(|u| (u.prompt_tokens, u.completion_tokens));
        println!("[{name}] tools asked for: {:?}; tokens (prompt, completion): {tokens:?}\n[{name}] answer: {}", self.tools, self.outcome.content);
    }
}

async fn ask(orchestrator: &Orchestrator, prompt: &str) -> Turn {
    let mut tools = Vec::new();
    let outcome = orchestrator
        .handle_message_streaming(&[], prompt, |event| {
            if let StreamEvent::ToolCallDelta { name: Some(name), .. } = event {
                tools.push(name.clone());
            }
        })
        .await
        .expect("the turn failed (bad key, model id or base url?)");
    Turn { tools, outcome }
}

fn skill_tools(vault: &Arc<Vault>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(UseSkillTool::new(SkillStore::new(vault.clone()))),
        Arc::new(ReadSkillFileTool::new(SkillStore::new(vault.clone()))),
        Arc::new(ManageSkillTool::new(SkillStore::new(vault.clone()))),
    ]
}

fn skill(name: &str, description: &str, body: &str) -> Skill {
    Skill { name: name.into(), description: description.into(), body: body.into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None }
}

#[tokio::test]
#[ignore = "calls a paid model API: set WARDEN_EVAL_PROVIDER, WARDEN_EVAL_MODEL and WARDEN_EVAL_KEY"]
async fn the_model_loads_a_skill_the_catalog_lists_and_follows_it() {
    let vault = Arc::new(Vault::new(temp_dir("skill-use")));
    SkillStore::new(vault.clone())
        .save(&skill(
            "build-status-report",
            "Use when someone asks for a status report on a build or a release.",
            "Write the report as exactly three bullet points. After the bullets, add one last line that is exactly: CODE-ZEBRA-7731",
        ))
        .unwrap();
    let mut orchestrator = Orchestrator::new(real_model(), vault.clone());
    for tool in skill_tools(&vault) {
        orchestrator.register_tool(tool);
    }

    let turn = ask(&orchestrator, "Give me a short status report on yesterday's build. It went fine.").await;
    turn.report("skill-use");

    assert!(turn.tools.iter().any(|t| t == "use_skill"), "the model never called use_skill; it asked for {:?}", turn.tools);
    assert!(turn.outcome.content.contains("CODE-ZEBRA-7731"), "the model loaded the skill but did not follow its instructions");
}

#[tokio::test]
#[ignore = "calls a paid model API: set WARDEN_EVAL_PROVIDER, WARDEN_EVAL_MODEL and WARDEN_EVAL_KEY"]
async fn the_model_saves_a_skill_when_asked_to() {
    let vault = Arc::new(Vault::new(temp_dir("skill-manage")));
    let store = SkillStore::new(vault.clone());
    let mut orchestrator = Orchestrator::new(real_model(), vault.clone());
    for tool in skill_tools(&vault) {
        orchestrator.register_tool(tool);
    }

    let turn = ask(
        &orchestrator,
        "Save a skill named 'haiku-reply' for me. It applies when I ask for a poem about the sea, and its instructions are: answer with a haiku and nothing else.",
    )
    .await;
    turn.report("skill-manage");

    assert!(turn.tools.iter().any(|t| t == "manage_skill"), "the model never called manage_skill; it asked for {:?}", turn.tools);
    let saved = store.list();
    let found = saved.iter().find(|s| s.name.contains("haiku"));
    assert!(found.is_some(), "no skill with 'haiku' in its name was saved; the vault has {:?}", saved.iter().map(|s| &s.name).collect::<Vec<_>>());
    assert!(found.unwrap().body.to_lowercase().contains("haiku"), "the saved skill lost its instructions");
}

struct Yes(Mutex<Vec<ApprovalRequest>>);

#[async_trait]
impl Approver for Yes {
    async fn approve(&self, request: ApprovalRequest) -> bool {
        self.0.lock().unwrap().push(request);
        true
    }
}

#[tokio::test]
#[ignore = "calls a paid model API: set WARDEN_EVAL_PROVIDER, WARDEN_EVAL_MODEL and WARDEN_EVAL_KEY"]
async fn the_model_schedules_a_task_with_a_cron_and_a_zone_after_the_person_approves() {
    let dir = temp_dir("tasks");
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    let vault = Arc::new(Vault::new(dir.join("vault")));
    let approver = Arc::new(Yes(Mutex::new(Vec::new())));
    let mut orchestrator = Orchestrator::new(real_model(), vault);
    orchestrator.register_tool(Arc::new(ManageTasksTool::new(&config_path)));
    let orchestrator = orchestrator.with_approver(approver.clone());

    let turn = ask(
        &orchestrator,
        "Set up a recurring task: every Monday at 9:00 in the America/Sao_Paulo time zone, remind me to water the plants. Use your task tool to create it.",
    )
    .await;
    turn.report("tasks");

    assert!(turn.tools.iter().any(|t| t == "manage_tasks"), "the model never called manage_tasks; it asked for {:?}", turn.tools);
    let asked = approver.0.lock().unwrap();
    assert!(!asked.is_empty(), "the tool never asked the person to approve the task");
    let config = load_config_from_path(&config_path, true).unwrap();
    assert_eq!(config.tasks.len(), 1, "expected one saved task, found {:?}", config.tasks);
    let task = &config.tasks[0];
    let cron = task.cron.as_deref().unwrap_or_default();
    println!("[tasks] saved: cron={cron:?} timezone={:?} prompt={:?}", task.timezone, task.prompt);
    // Monday is 1 in cron (or the name); 9:00 is minute 0 hour 9.
    let fields: Vec<&str> = cron.split_whitespace().collect();
    assert!(fields.len() == 5 && fields[0] == "0" && fields[1] == "9", "the cron '{cron}' is not 9:00");
    assert!(fields[4] == "1" || fields[4].eq_ignore_ascii_case("mon"), "the cron '{cron}' is not Mondays");
    assert_eq!(task.timezone.as_deref(), Some("America/Sao_Paulo"));
    assert!(task.prompt.to_lowercase().contains("plant"), "the task prompt lost what to do: {:?}", task.prompt);
}

/// Hands out a code name only the orchestrator that has this tool can read.
struct Codename;

#[async_trait]
impl Tool for Codename {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "lookup_codename".to_string(),
            description: "Returns the secret code name of the project. Only this tool knows it.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Value) -> anyhow::Result<Value> {
        Ok(json!({ "codename": "AURORA-4096-QX" }))
    }
}

#[tokio::test]
#[ignore = "calls a paid model API: set WARDEN_EVAL_PROVIDER, WARDEN_EVAL_MODEL and WARDEN_EVAL_KEY"]
async fn the_model_delegates_two_levels_deep_to_reach_a_tool_only_the_innermost_agent_has() {
    let vault = Arc::new(Vault::new(temp_dir("delegate")));
    let model = real_model();

    // Same shape `bootstrap` builds: each level can delegate to the next one, and only the last has the tool.
    let mut leaf = Orchestrator::new(model.clone(), vault.clone());
    leaf.register_tool(Arc::new(Codename));
    let mut middle = Orchestrator::new(model.clone(), vault.clone());
    middle.register_tool(Arc::new(DelegateTool::new(leaf)));
    let mut top = Orchestrator::new(model, vault);
    top.register_tool(Arc::new(DelegateTool::new(middle)));

    let turn = ask(&top, "What is the secret code name of the project? You cannot look it up yourself: delegate the task, and tell the sub-agent to delegate it further if it cannot look it up either.").await;
    turn.report("delegate");

    assert!(turn.tools.iter().any(|t| t == "delegate_task"), "the model never called delegate_task; it asked for {:?}", turn.tools);
    assert!(turn.outcome.content.contains("AURORA-4096-QX"), "the code name did not make it back up through two levels of sub-agents");
}

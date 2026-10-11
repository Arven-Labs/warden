//! The desktop's own chat running a code project's conversation (P103 b): the same task for the opencode that the hub
//! runs for the web (`warden_bootstrap::code_turn::CodeTurn`), but in this process, so it doesn't need the embedded hub
//! turned on. What the engine does reaches the window as `chat-event`s, its asks go to the same modal as every other
//! approval, and `cancel_turn` stops it.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::OnceCell;
use warden_bootstrap::code_turn::CodeTurn;
use warden_core::code_engine::{CodeEngine, CodeEvent, CodeMode, CodeModes, TurnOutcome, TurnRequest};
use warden_core::model::Attachment;
use warden_core::orchestrator::Orchestrator;
use warden_core::project::Project;
use warden_core::tool::code_task::CodeEngineSlot;
use warden_core::tool::Approver;
use warden_server::code_turns::{opencode_engine, CodeTurns};
use warden_server::engine_models::EngineModels;
use warden_server::openai_api::EngineModelSlot;
use warden_server::SharedOrchestrator;
use warden_server_protocol::protocol::ChatEventDto;

use crate::{approval, AppState};

/// What a turn of code needs, started with the first one: a hub with no code project never runs the opencode.
struct Runtime {
    /// The model the opencode talks to goes through this one, which follows the desktop's settings (see `run_turn`).
    shared: SharedOrchestrator,
    engine: Arc<dyn CodeEngine>,
}

#[derive(Default)]
pub struct CodeState {
    runtime: Arc<OnceCell<Runtime>>,
    /// The tasks running now, to find the session `cancel_turn` stops.
    turns: CodeTurns,
    /// How much each conversation asks, changeable while its task runs (`set_code_mode`).
    modes: CodeModes,
    /// Where the engine is for the `code_task` of every orchestrator the desktop builds (`bootstrap_desktop`): filled once by `install_engine`.
    slot: CodeEngineSlot,
    /// The hub's models a task may name, refilled by every build so the engine's route follows the settings.
    models: EngineModelSlot,
}

/// The engine as the agents' `code_task` sees it: it asks for the real one, and for the model route it needs, only when a task comes.
/// The desktop builds its orchestrator before there is any engine (the route needs the orchestrator), so the tool is given this.
struct LazyEngine {
    /// The desktop's current orchestrator, which the model route follows.
    orchestrator: Arc<Mutex<Result<Orchestrator, String>>>,
    runtime: Arc<OnceCell<Runtime>>,
    models: EngineModelSlot,
}

impl LazyEngine {
    async fn runtime(&self) -> anyhow::Result<&Runtime> {
        let base = self.orchestrator.lock().unwrap().clone().map_err(|e| anyhow::anyhow!("the desktop's settings don't start: {e}"))?;
        runtime_of(&self.runtime, &base, &self.models).await
    }
}

#[async_trait]
impl CodeEngine for LazyEngine {
    async fn run_turn(&self, request: TurnRequest, approver: Option<Arc<dyn Approver>>, on_event: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
        self.runtime().await?.engine.run_turn(request, approver, on_event).await
    }

    async fn abort(&self, workdir: &str, session: &str) -> anyhow::Result<()> {
        // Nothing is running if the engine was never started.
        match self.runtime.get() {
            Some(runtime) => runtime.engine.abort(workdir, session).await,
            None => Ok(()),
        }
    }
}

/// The runtime of the first code task, started then: the loopback door to the model (which follows `base` from here on) and the engine.
async fn runtime_of<'a>(cell: &'a OnceCell<Runtime>, base: &Orchestrator, models: &EngineModelSlot) -> anyhow::Result<&'a Runtime> {
    let runtime = cell
        .get_or_try_init(|| async {
            let shared = SharedOrchestrator::new(base.clone());
            let door = EngineModels::start(shared.clone(), models.clone()).await?;
            Ok::<_, anyhow::Error>(Runtime { shared, engine: opencode_engine(&door) })
        })
        .await?;
    // A provider changed in Settings since the last task reaches the opencode's next request.
    runtime.shared.replace(base.clone());
    Ok(runtime)
}

/// What the desktop's orchestrator is built from: `bootstrap`, plus the `code_task` an agent can be given (the hub does the same).
pub async fn bootstrap_desktop(engine: &CodeEngineSlot, engine_models: &EngineModelSlot) -> anyhow::Result<Orchestrator> {
    let mut orchestrator = warden_bootstrap::bootstrap(None, warden_bootstrap::Overrides::default(), crate::desktop_default_vault_path()).await?;
    let models = warden_bootstrap::load_config(None).ok().and_then(|config| warden_bootstrap::model_choices(&config));
    *engine_models.write().unwrap_or_else(|e| e.into_inner()) = models.clone();
    warden_server::code_turns::register_code_task(&mut orchestrator, engine, models);
    Ok(orchestrator)
}

impl CodeState {
    /// Puts the engine where `code_task` looks for it, once the orchestrator it follows exists. Nothing is started until a task.
    pub fn install_engine(&self, orchestrator: Arc<Mutex<Result<Orchestrator, String>>>) {
        let engine: Arc<dyn CodeEngine> = Arc::new(LazyEngine { orchestrator, runtime: self.runtime.clone(), models: self.models.clone() });
        self.slot.set(engine).ok();
    }

    /// The one engine of this process, for the embedded hub's code conversations: the tela Code, `code_task` and the hub share its
    /// opencode processes, so a folder never has two. `None` until `install_engine`.
    pub fn engine(&self) -> Option<Arc<dyn CodeEngine>> {
        self.slot.get().cloned()
    }

    /// The same slots, for a settings host that builds orchestrators of its own (the embedded hub's).
    pub fn slots(&self) -> (CodeEngineSlot, EngineModelSlot) {
        (self.slot.clone(), self.models.clone())
    }

    /// A task of this conversation is running here now.
    pub fn is_running(&self, conversation_id: &str) -> bool {
        self.turns.is_running(conversation_id)
    }

    /// `mode` is a name a window sent (`CodeMode::parse`).
    pub fn set_mode(&self, conversation_id: &str, mode: &str) {
        self.modes.set(conversation_id, CodeMode::parse(mode));
    }
}

/// Runs `content` as a task in `project`'s folder and returns what the engine answered. The exchange is already saved
/// in the conversation when this returns.
pub async fn run_turn(
    app: AppHandle,
    state: &AppState,
    base: &Orchestrator,
    project: Project,
    conversation_id: &str,
    content: &str,
    attachments: Vec<Attachment>,
) -> Result<String, String> {
    let runtime = runtime_of(&state.code.runtime, base, &state.code.models).await.map_err(|e| format!("{e:#}"))?;

    // Where the conversation is: the hub's folder if it was shared (P124), so the phone and this window go on in the same file.
    let conversations_dir = crate::share_cmds::conversation_dir(conversation_id)?;
    let turns = state.code.turns.clone();
    turns.begin(conversation_id, project.workdir.as_deref().unwrap_or_default());
    let (event_app, event_turns, event_id) = (app.clone(), turns.clone(), conversation_id.to_string());
    let mut on_event = move |event: CodeEvent| match event {
        CodeEvent::Session(session) => event_turns.set_session(&event_id, session),
        other => {
            if let Some(event) = ChatEventDto::from_code(other) {
                let _ = event_app.emit("chat-event", json!({ "conversationId": event_id, "event": event }));
            }
        }
    };
    let seed = warden_server::chat_input::title_seed(content, &attachments);
    let turn = CodeTurn {
        engine: runtime.engine.as_ref(),
        project: &project,
        conversations_dir: &conversations_dir,
        conversation_id,
        title_seed: &seed,
        user_input: content,
        attachments,
        mode: state.code.modes.subscribe(conversation_id),
    };
    let approver = Arc::new(approval::TauriApprover { app, broker: state.approvals.clone() });
    let result = turn.run(Some(approver), &mut on_event).await;
    turns.end(conversation_id);
    result.map(|outcome| outcome.content).map_err(|e| format!("{e:#}"))
}

/// How much this conversation asks before the engine acts: `manual`, `acceptEdits`, `acceptAll` or `plan`. Takes effect
/// at once, a task that is running included.
#[tauri::command]
pub fn set_code_mode(state: State<'_, AppState>, conversation_id: String, mode: String) {
    state.code.set_mode(&conversation_id, &mode);
}

/// The Stop button: asks the engine to stop the task this conversation is running. Harmless when it isn't running one.
#[tauri::command]
pub async fn cancel_turn(state: State<'_, AppState>, conversation_id: String) -> Result<(), String> {
    let (Some((workdir, session)), Some(runtime)) = (state.code.turns.session_of(&conversation_id), state.code.runtime.get()) else {
        return Ok(());
    };
    runtime.engine.abort(&workdir, &session).await.map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use warden_core::model::{ChatStream, Message, ModelProvider};
    use warden_core::tool::ToolSpec;

    use super::*;

    /// A model nobody calls: the tests only look at what the engine is wired to.
    struct NoModel;
    #[async_trait]
    impl ModelProvider for NoModel {
        async fn chat_stream(&self, _: Vec<Message>, _: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
            anyhow::bail!("not called")
        }
    }

    fn orchestrator() -> Orchestrator {
        Orchestrator::new(Arc::new(NoModel), Arc::new(warden_core::memory::Vault::new(std::env::temp_dir().join(format!("desktop-code-cmds-{}", std::process::id())))))
    }

    fn request() -> TurnRequest {
        TurnRequest {
            workdir: "/nowhere".into(),
            session_id: None,
            prompt: "x".into(),
            title: "x".into(),
            target: "x".into(),
            system: None,
            mode: tokio::sync::watch::channel(CodeMode::Manual).1,
            model: None,
        }
    }

    #[test]
    fn the_engine_is_put_in_the_slot_code_task_looks_in_and_only_once_started_when_a_task_asks() {
        let code = CodeState::default();
        let (slot, _) = code.slots();
        assert!(slot.get().is_none());
        code.install_engine(Arc::new(Mutex::new(Ok(orchestrator()))));
        assert!(slot.get().is_some(), "code_task can find the engine");
        assert!(code.runtime.get().is_none(), "but nothing was started: no door to the model, no opencode");
    }

    #[test]
    fn installing_again_keeps_the_one_engine_the_hub_and_the_desktop_share() {
        let code = CodeState::default();
        assert!(code.engine().is_none());
        code.install_engine(Arc::new(Mutex::new(Ok(orchestrator()))));
        let first = code.engine().unwrap();
        // The embedded hub installs too (it may start before or after); it must not make a second engine.
        code.install_engine(Arc::new(Mutex::new(Ok(orchestrator()))));
        assert!(Arc::ptr_eq(&first, &code.engine().unwrap()));
    }

    #[tokio::test]
    async fn a_task_on_a_desktop_whose_settings_dont_start_is_refused_with_why_and_nothing_is_started() {
        let code = CodeState::default();
        code.install_engine(Arc::new(Mutex::new(Err("no provider".to_string()))));
        let engine = code.slots().0.get().cloned().unwrap();
        let err = engine.run_turn(request(), None, &mut |_| {}).await.err().expect("refused");
        assert!(err.to_string().contains("settings don't start") && err.to_string().contains("no provider"), "{err}");
        assert!(code.runtime.get().is_none());
        // Stopping what never started is harmless.
        engine.abort("/nowhere", "ses_none").await.unwrap();
    }

    #[tokio::test]
    async fn the_first_task_starts_the_runtime_with_the_desktops_orchestrator_and_a_later_one_follows_a_changed_one() {
        let code = CodeState::default();
        let current = Arc::new(Mutex::new(Ok(orchestrator())));
        let lazy = LazyEngine { orchestrator: current.clone(), runtime: code.runtime.clone(), models: code.slots().1 };
        let first = lazy.runtime().await.unwrap().shared.current();
        assert!(code.runtime.get().is_some());
        // A change in Settings reaches the model route with the next task.
        let changed = orchestrator().with_spend_context(warden_core::spend::SpendContext::new("changed"));
        *current.lock().unwrap() = Ok(changed);
        let second = lazy.runtime().await.unwrap().shared.current();
        assert!(!Arc::ptr_eq(&first, &second));
    }
}

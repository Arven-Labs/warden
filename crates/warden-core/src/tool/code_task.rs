//! `code_task` (P89, P124, tension 3 of `VISAO_AGENTES.md`): an agent hands a task to the code engine (the opencode) working
//! in a code project's folder, the way a Programming Manager would. The engine is a tool of the agent, not a second agent
//! of the Warden: the agent says what it wants done, the engine does it in the folder and the agent reads the answer.
//!
//! It is as dangerous as `shell` — the engine edits files and runs commands — so it is held the same way: every ask of
//! the engine goes to the person through `with_approver`'s approver, in `Manual` mode that nobody can loosen from here,
//! and without an approver (a channel that can't ask) the tool refuses.
//!
//! With the turn's `JobBoard` (`with_jobs`) a task can also run in the background (`background: true`), several at a time, and is
//! recorded like any delegation (P123): it shows on the tasks screen with its state and can be stopped from there (not paused: the
//! engine has no gate to look at). Two tasks on the same folder run one after the other, since they would edit the same files
//! through the same engine process; tasks on different folders run side by side, within the board's limit.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::autonomy::{Autonomy, Category, Classifier};
use crate::code_engine::{CodeEngine, CodeEvent, CodeMode, TurnOutcome, TurnRequest};
use crate::jobs::{task_controls, JobBoard, TaskDraft};
use crate::memory::Vault;
use crate::project::ProjectStore;
use crate::worktree::{self, Worktree};
use crate::tool::delegate::{model_property, ModelChoices};
use crate::tool::job_tools::{background_property, job_label, start_task_with, wants_background};
use crate::tool::{Approver, ProgressSink, Tool, ToolSpec};

/// The lock of a folder: whoever works in it holds it. Process-wide rather than per tool, because the hub builds its tools again
/// whenever its settings are saved and a task already running must still keep the next one out.
fn folder_lock(workdir: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).entry(workdir.to_string()).or_default().clone()
}

/// Whether a task is working in `workdir` right now: what is in that folder can't be merged or thrown away yet.
pub(crate) fn folder_busy(workdir: &str) -> bool {
    folder_lock(workdir).try_lock_owned().is_err()
}

/// A task in `workdir` that never ends until the guard is dropped, for the tests of what must not happen while one runs.
#[cfg(test)]
pub(crate) fn hold_folder_for_test(workdir: &str) -> tokio::sync::OwnedMutexGuard<()> {
    folder_lock(workdir).try_lock_owned().expect("nobody else is working there")
}

/// Stops the engine's session when the work that started it is dropped before it finished (a person stopped the task, or the turn
/// that started it ended): the engine is another process, and without this it would go on editing after the task is gone.
struct StopOnDrop {
    engine: Arc<dyn CodeEngine>,
    workdir: String,
    session: Arc<Mutex<Option<String>>>,
    armed: bool,
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let Some(session) = self.session.lock().unwrap_or_else(|e| e.into_inner()).clone().filter(|_| self.armed) else { return };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
        let (engine, workdir) = (self.engine.clone(), self.workdir.clone());
        runtime.spawn(async move {
            let _ = engine.abort(&workdir, &session).await;
        });
    }
}

/// Where a task says what it is doing (P124) and under which project, so lines of tasks running side by side can be told apart.
#[derive(Clone)]
struct Reporter {
    sink: ProgressSink,
    project_id: String,
    project_name: String,
}

impl Reporter {
    /// The tool rows and notices of the engine, marked with the project. Not its text: two tasks in parallel would mix it in one bubble,
    /// and it arrives in the answer anyway. Not the session either: that is not for showing.
    fn report(&self, event: CodeEvent) {
        match event {
            CodeEvent::Tool(mut tool) => {
                tool.call_id = format!("{}:{}", self.project_id, tool.call_id);
                tool.title = format!("{}: {}", self.project_name, tool.title);
                self.sink.send("code_task", CodeEvent::Tool(tool));
            }
            CodeEvent::Notice(text) => self.sink.send("code_task", CodeEvent::Notice(format!("{}: {text}", self.project_name))),
            CodeEvent::Text(_) | CodeEvent::Session(_) => {}
        }
    }
}

/// Runs `request` in the engine once nobody else is working in its folder, and stops the session if the run is dropped.
async fn run_in_engine(engine: Arc<dyn CodeEngine>, approver: Arc<dyn Approver>, request: TurnRequest, reporter: Option<Reporter>) -> anyhow::Result<TurnOutcome> {
    let workdir = request.workdir.clone();
    let _folder = folder_lock(&workdir).lock_owned().await;
    let session = Arc::new(Mutex::new(request.session_id.clone()));
    let mut stop = StopOnDrop { engine: engine.clone(), workdir, session: session.clone(), armed: true };
    let outcome = engine
        .run_turn(request, Some(approver), &mut move |event| {
            if let CodeEvent::Session(id) = event {
                *session.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
            } else if let Some(reporter) = &reporter {
                reporter.report(event);
            }
        })
        .await;
    stop.armed = false;
    outcome
}

/// Where an isolated task's work was left (P124): the worktree it ran in, or the note that there was nothing to keep.
struct Isolation {
    id: String,
    branch: String,
    path: String,
    changed: bool,
    summary: String,
}

impl Isolation {
    /// What a result tells besides the answer. A kept worktree is named, to be worked in again or looked at; an empty one is gone.
    fn fields(&self) -> Value {
        if self.changed {
            json!({ "worktree": self.id, "branch": self.branch, "path": self.path, "changes": self.summary })
        } else {
            json!({ "worktree": null, "note": "the task changed nothing, so its worktree and branch were removed" })
        }
    }

    fn describe(&self) -> String {
        if self.changed {
            format!("\nworktree: {} (branch {}, folder {})\n{}", self.id, self.branch, self.path, self.summary)
        } else {
            "\nworktree: none kept (the task changed nothing, so its worktree and branch were removed)".to_string()
        }
    }
}

/// Runs the task and, if it had a worktree of its own, looks at what it left there. The worktree is looked at even when the task failed:
/// a task that fell over half way may have edited files, and an empty one should not be left behind.
async fn run_task(engine: Arc<dyn CodeEngine>, approver: Arc<dyn Approver>, request: TurnRequest, reporter: Option<Reporter>, isolated: Option<Worktree>) -> anyhow::Result<(TurnOutcome, Option<Isolation>)> {
    let outcome = run_in_engine(engine, approver, request, reporter).await;
    let isolation = match isolated {
        None => None,
        Some(worktree) => {
            let left = worktree::finish(&worktree).await;
            match (&outcome, left) {
                (Ok(_), Err(err)) => return Err(err.context(format!("looking at the worktree {}", worktree.id))),
                (_, Ok(left)) => Some(Isolation { id: worktree.id.clone(), branch: worktree.branch.clone(), path: worktree.path.to_string_lossy().into_owned(), changed: left.changed, summary: left.summary }),
                (Err(_), Err(_)) => None,
            }
        }
    };
    Ok((outcome?, isolation))
}

/// What a background task's result says: the answer, the worktree if it had one, and the session to continue and the tools it used (a `jobs` result is text).
fn describe(outcome: &TurnOutcome, isolation: Option<&Isolation>) -> String {
    let tools = if outcome.tools_used.is_empty() { String::new() } else { format!("\ntools_used: {}", outcome.tools_used.join(", ")) };
    let worktree = isolation.map(Isolation::describe).unwrap_or_default();
    format!("{}\n\nsession_id: {}{tools}{worktree}", outcome.text, outcome.session_id)
}

/// Where the engine will be, once there is one: the hub builds its tools before the engine (which needs the hub's model
/// route), and builds them again whenever its settings are saved, so the tool holds a slot the hub fills once.
pub type CodeEngineSlot = Arc<OnceLock<Arc<dyn CodeEngine>>>;

#[derive(Clone)]
pub struct CodeTaskTool {
    projects: ProjectStore,
    engine: CodeEngineSlot,
    approver: Option<Arc<dyn Approver>>,
    /// The turn's background jobs, once bound (`with_jobs`): only then does the tool accept `background` and record its tasks.
    jobs: Option<Arc<JobBoard>>,
    /// The autonomy of the agent calling (`with_autonomy`, P122): it only goes down, and sets how loose a `mode` the agent may ask for.
    autonomy: Autonomy,
    /// A person asked to be asked about critical-infrastructure actions for this agent (`with_approval_rules`), and `code_task` is
    /// one: the engine's own asks are what they meant to keep, so no mode that answers them for the engine is allowed.
    asks_for_critical: bool,
    /// Where the tool says what its tasks are doing, while the turn that started them is being watched (`with_progress`).
    progress: Option<ProgressSink>,
    /// The hub's models a task may be given (P123, `with_models`), the way a delegation picks one; none offered means the engine uses the hub's default.
    models: Option<ModelChoices>,
}

/// The name a `mode` argument uses (the same ones the clients send).
fn mode_name(mode: CodeMode) -> &'static str {
    match mode {
        CodeMode::Manual => "manual",
        CodeMode::AcceptEdits => "acceptEdits",
        CodeMode::AcceptAll => "acceptAll",
        CodeMode::Plan => "plan",
    }
}

/// Strict, unlike `CodeMode::parse` (which turns an unknown name into `Manual` for a client that sent garbage): an agent that asks for
/// a mode that doesn't exist is told so.
fn mode_named(name: &str) -> Option<CodeMode> {
    [CodeMode::Manual, CodeMode::AcceptEdits, CodeMode::AcceptAll, CodeMode::Plan].into_iter().find(|mode| mode_name(*mode) == name)
}

impl CodeTaskTool {
    pub fn new(vault: Arc<Vault>, engine: CodeEngineSlot) -> Self {
        Self { projects: ProjectStore::new(vault), engine, approver: None, jobs: None, autonomy: Autonomy::Autonomous, asks_for_critical: false, progress: None, models: None }
    }

    /// Lets the caller pick the model of each task, from `models`.
    pub fn with_models(mut self, models: ModelChoices) -> Self {
        self.models = Some(models);
        self
    }

    /// The model a call asked for (or the caller's default), refused when it isn't one of the choices. `None` is the hub's default.
    fn model_of(&self, args: &Value) -> anyhow::Result<Option<String>> {
        let asked = args.get("model").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty());
        let Some(choices) = &self.models else {
            return match asked {
                Some(_) => anyhow::bail!("this agent can't choose a model for a task — leave out 'model'"),
                None => Ok(None),
            };
        };
        let Some(wanted) = asked.or(choices.default.as_deref()) else { return Ok(None) };
        if !choices.ids.iter().any(|id| id == wanted) {
            anyhow::bail!("unknown model '{wanted}' — choose one of: {}", choices.ids.join(", "));
        }
        Ok(Some(wanted.to_string()))
    }

    /// The modes an agent may ask for (P124): never `acceptAll` (the shell is no sandbox and that mode asks nothing, so only a person
    /// turns it on, from the Code screen); `acceptEdits` only for an agent that acts on its own (4 and 5) and whose person did not ask
    /// to be asked about critical infrastructure; `plan` and `manual` otherwise. Below level 3 the tool is refused before it runs, so
    /// `plan`, which changes nothing, is all that is left to offer.
    fn allowed_modes(&self) -> Vec<CodeMode> {
        match self.autonomy {
            Autonomy::AnswerOnly | Autonomy::Suggest => vec![CodeMode::Plan],
            Autonomy::AskFirst => vec![CodeMode::Manual, CodeMode::Plan],
            Autonomy::Autonomous | Autonomy::Manager if self.asks_for_critical => vec![CodeMode::Manual, CodeMode::Plan],
            Autonomy::Autonomous | Autonomy::Manager => vec![CodeMode::Manual, CodeMode::AcceptEdits, CodeMode::Plan],
        }
    }

    /// The projects a task can go to: the ones that run on the engine, with their folder.
    fn code_projects(&self) -> Vec<crate::project::Project> {
        self.projects.list().into_iter().filter(|p| p.code && p.workdir.is_some()).collect()
    }
}

#[async_trait]
impl Tool for CodeTaskTool {
    fn spec(&self) -> ToolSpec {
        let projects = self.code_projects();
        let listing = if projects.is_empty() {
            "(none yet: a person has to make a project with a working folder and code mode)".to_string()
        } else {
            projects.iter().map(|p| format!("- {}: {}", p.id, if p.description.is_empty() { &p.name } else { &p.description })).collect::<Vec<_>>().join("\n")
        };
        let mut project = json!({ "type": "string", "description": "Which code project to work in." });
        // An empty `enum` is not valid JSON Schema and some providers refuse the whole request over it.
        if !projects.is_empty() {
            project["enum"] = json!(projects.iter().map(|p| p.id.clone()).collect::<Vec<_>>());
        }
        let mut properties = json!({
            "project": project,
            "task": {
                "type": "string",
                "description": "A complete, self-contained description of the task."
            },
            "session_id": {
                "type": "string",
                "description": "The session_id a previous code_task returned, to continue that session instead of starting a new one."
            }
        });
        let allowed = self.allowed_modes();
        properties["mode"] = json!({
            "type": "string",
            "enum": allowed.iter().map(|m| mode_name(*m)).collect::<Vec<_>>(),
            "description": "How much the engine asks the person before acting: 'manual' asks before every edit and command (the default); \
                'acceptEdits' lets it change files without asking and still asks before commands; 'plan' lets it change nothing and \
                answers with a plan. Only the modes your autonomy allows are listed."
        });
        if let Some(models) = &self.models {
            properties["model"] = model_property(models);
        }
        properties["isolate"] = json!({
            "type": "boolean",
            "description": "Run the task in a git worktree of its own, on its own branch, so it can work at the same time as other tasks on this project \
                (without it, two tasks on one project run one after the other). The worktree starts from the last commit: what is changed and \
                not committed in the project's folder is not in it. When the task ends, a worktree where nothing changed is removed; one with \
                work is kept and the result names its 'worktree', 'branch' and folder. Nothing is merged for you. Needs a git repository."
        });
        properties["worktree"] = json!({
            "type": "string",
            "description": "The 'worktree' an earlier isolated task returned, to go on working in it (with that task's 'session_id' to continue its session). Leave out 'isolate' then."
        });
        let mut background_note = "";
        if self.jobs.is_some() {
            properties["background"] = background_property();
            background_note = " Tasks on different projects can run side by side in the background; two on the same project run one after the other, unless they are \
                 'isolate'd, which gives each its own worktree. A background task's result (read with 'jobs') ends with the 'session_id' to continue it.";
        }
        ToolSpec {
            name: "code_task".to_string(),
            description: format!(
                "Hand a programming task to the code engine (the opencode) working in a code project's folder: it reads and edits \
                 the files and runs commands there, and answers with what it did. By default ('manual') the person is asked to \
                 approve each of its actions, so a refused action did not happen; 'mode' can loosen that as far as your autonomy \
                 allows. The engine does NOT see this conversation — the 'task' must be \
                 complete and self-contained. To continue the work of an earlier task (same files read, same context), pass the \
                 'session_id' that task returned.{background_note} Code projects:\n{listing}"
            ),
            parameters: json!({
                "type": "object",
                "properties": properties,
                "required": ["project", "task"]
            }),
        }
    }

    fn with_model_choices(&self, choices: Option<&ModelChoices>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { models: choices.cloned(), ..self.clone() }))
    }

    fn with_vault(&self, vault: &Arc<Vault>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { projects: ProjectStore::new(vault.clone()), ..self.clone() }))
    }

    fn with_approver(&self, approver: Arc<dyn Approver>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { approver: Some(approver), ..self.clone() }))
    }

    fn with_jobs(&self, board: &Arc<JobBoard>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { jobs: Some(board.clone()), ..self.clone() }))
    }

    fn with_progress(&self, sink: &ProgressSink) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { progress: Some(sink.clone()), ..self.clone() }))
    }

    fn with_autonomy(&self, level: Autonomy, _read_only: &[String]) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { autonomy: self.autonomy.min(level), ..self.clone() }))
    }

    fn with_approval_rules(&self, required: &[Category], _classifier: Option<&Classifier>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { asks_for_critical: self.asks_for_critical || required.contains(&Category::CriticalInfra), ..self.clone() }))
    }

    async fn call(&self, args: Value) -> anyhow::Result<Value> {
        let id = args.get("project").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing required 'project' argument"))?;
        let task = args.get("task").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| anyhow::anyhow!("missing required 'task' argument"))?;
        let session_id = args.get("session_id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

        let projects = self.code_projects();
        let project = projects.iter().find(|p| p.id == id).ok_or_else(|| {
            let available = projects.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ");
            anyhow::anyhow!("'{id}' is not a code project — must be one of: {}", if available.is_empty() { "(none)" } else { &available })
        })?;
        let requested = match args.get("mode").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty()) {
            None => CodeMode::Manual,
            Some(name) => {
                let allowed = self.allowed_modes();
                let names = allowed.iter().map(|m| mode_name(*m)).collect::<Vec<_>>().join(", ");
                let mode = mode_named(name).ok_or_else(|| anyhow::anyhow!("'{name}' is not a mode — must be one of: {names}"))?;
                if !allowed.contains(&mode) {
                    anyhow::bail!("'{name}' is more than this agent may ask for — must be one of: {names}");
                }
                mode
            }
        };
        let model = self.model_of(&args)?;
        let Some(approver) = self.approver.clone() else {
            anyhow::bail!("the code engine asks the person before each of its actions, and this channel can't ask (use the desktop app or the web)");
        };
        let Some(engine) = self.engine.get().cloned() else {
            anyhow::bail!("the code engine isn't available on this machine");
        };
        let mut workdir = project.workdir.clone().ok_or_else(|| anyhow::anyhow!("the project '{}' has no working folder", project.name))?;
        // A worktree of its own (P124): made now, so that a refusal above leaves nothing behind. The task then works in that folder, which
        // is also the folder it queues on, so isolated tasks run side by side.
        let reopen = args.get("worktree").and_then(Value::as_str).map(str::trim).filter(|w| !w.is_empty());
        let isolate = args.get("isolate").and_then(Value::as_bool).unwrap_or(false);
        if reopen.is_some() && isolate {
            anyhow::bail!("pass 'isolate' for a new worktree or 'worktree' to go back to one, not both");
        }
        let isolated = match reopen {
            Some(existing) => Some(worktree::open(std::path::Path::new(&workdir), existing).await?),
            None if isolate => Some(worktree::create(std::path::Path::new(&workdir)).await?),
            None => None,
        };
        let (shown_id, shown_name) = match &isolated {
            Some(made) => {
                workdir = made.path.to_string_lossy().into_owned();
                (format!("{}:{}", project.id, made.id), format!("{} [wt {}]", project.name, made.id))
            }
            None => (project.id.clone(), project.name.clone()),
        };

        // The mode is fixed for the task: nobody holds the sender, so nothing can loosen it while the task runs. It is the one asked for,
        // within what this agent's autonomy allows (`Manual` when it asks for none).
        let (_, mode) = tokio::sync::watch::channel(requested);
        let request = TurnRequest {
            workdir,
            session_id,
            prompt: task.to_string(),
            title: project.name.clone(),
            target: project.name.clone(),
            system: Some(project.instructions.trim().to_string()).filter(|s| !s.is_empty()),
            mode,
            model: model.clone(),
        };
        let reporter = self.progress.clone().map(|sink| Reporter { sink, project_id: shown_id, project_name: shown_name.clone() });
        let draft = TaskDraft { assignee: format!("opencode · {shown_name}"), objective: task.to_string(), model };
        let result_of = |outcome: &TurnOutcome, isolation: Option<&Isolation>| {
            let mut result = json!({ "answer": outcome.text, "session_id": outcome.session_id, "tools_used": outcome.tools_used });
            if let (Some(isolation), Some(fields)) = (isolation, result.as_object_mut()) {
                fields.extend(isolation.fields().as_object().cloned().unwrap_or_default());
            }
            result
        };

        if let Some(board) = &self.jobs {
            // In the background: this returns a job_id at once and the engine works while the agent carries on. The task is on the
            // tasks screen; it can be stopped there, but there is no gate in the engine to pause.
            if wants_background(&args) {
                let label = job_label(&draft.assignee, task);
                let named = isolated.as_ref().map(|made| made.id.clone());
                let mut started = start_task_with(board, label, draft, move |_| async move {
                    let (outcome, isolation) = run_task(engine, approver, request, reporter, isolated).await?;
                    Ok((describe(&outcome, isolation.as_ref()), None))
                });
                // The worktree is known now, before the task ends, for a task that will be asked to go back to it.
                if let (Some(named), Some(fields)) = (named, started.as_object_mut()) {
                    fields.insert("worktree".to_string(), json!(named));
                }
                if let Some(recorded) = board.list().last().and_then(|job| job.task.as_ref()) {
                    task_controls().forbid_pause(&recorded.task_id);
                }
                return Ok(started);
            }
            // Waited for, but still a task in the log, like the delegations (P123).
            let text = board
                .run_recorded_as(draft, false, move |_| async move {
                    let (outcome, isolation) = run_task(engine, approver, request, reporter, isolated).await?;
                    Ok((result_of(&outcome, isolation.as_ref()).to_string(), None))
                })
                .await?;
            return Ok(serde_json::from_str(&text)?);
        }

        let (outcome, isolation) = run_task(engine, approver, request, reporter, isolated).await?;
        Ok(result_of(&outcome, isolation.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::code_engine::{ToolEvent, ToolStatus, TurnOutcome};
    use crate::project::Project;
    use crate::tool::ApprovalRequest;

    fn vault(name: &str) -> Arc<Vault> {
        Arc::new(Vault::new(std::env::temp_dir().join(format!("warden-code-task-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))))
    }

    fn with_projects(name: &str) -> Arc<Vault> {
        let vault = vault(name);
        let store = ProjectStore::new(vault.clone());
        let code = Project { id: "repo".into(), name: "Repo".into(), description: "The main repo".into(), instructions: "Use tabs.".into(), workdir: Some("/home/me/repo".into()), code: true };
        store.save(&code).unwrap();
        store.save(&Project { id: "shell-only".into(), code: false, ..code.clone() }).unwrap();
        store.save(&Project { id: "notes".into(), workdir: None, code: false, ..code }).unwrap();
        vault
    }

    /// An engine that answers from a script, remembers what it was asked and what mode it was told to run in.
    #[derive(Default)]
    struct Scripted {
        asked: Mutex<Vec<(TurnRequest, CodeMode, bool)>>,
    }
    #[async_trait]
    impl CodeEngine for Scripted {
        async fn run_turn(&self, request: TurnRequest, approver: Option<Arc<dyn Approver>>, on_event: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
            on_event(CodeEvent::Tool(ToolEvent { call_id: "c1".into(), tool: "edit".into(), title: "src/lib.rs".into(), status: ToolStatus::Completed }));
            let mode = *request.mode.borrow();
            let session = request.session_id.clone().unwrap_or_else(|| "ses_new".to_string());
            self.asked.lock().unwrap().push((request, mode, approver.is_some()));
            Ok(TurnOutcome { session_id: session, text: "Done.".into(), tools_used: vec!["edit".into()] })
        }
        async fn abort(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct Yes;
    #[async_trait]
    impl Approver for Yes {
        async fn approve(&self, _: ApprovalRequest) -> bool {
            true
        }
    }

    fn tool(name: &str, engine: Option<Arc<Scripted>>, asks: bool) -> CodeTaskTool {
        let slot: CodeEngineSlot = Arc::new(OnceLock::new());
        if let Some(engine) = engine {
            let engine: Arc<dyn CodeEngine> = engine;
            slot.set(engine).ok();
        }
        let mut tool = CodeTaskTool::new(with_projects(name), slot);
        if asks {
            tool.approver = Some(Arc::new(Yes));
        }
        tool
    }

    #[test]
    fn only_code_projects_with_a_folder_are_offered() {
        let spec = tool("spec", None, false).spec();
        assert_eq!(spec.parameters["properties"]["project"]["enum"], json!(["repo"]));
        assert!(spec.description.contains("repo: The main repo"));
    }

    #[test]
    fn with_no_code_project_the_schema_has_no_empty_enum() {
        let empty = CodeTaskTool::new(vault("empty"), Arc::new(OnceLock::new()));
        assert!(empty.spec().parameters["properties"]["project"].get("enum").is_none());
    }

    #[tokio::test]
    async fn without_an_approver_it_refuses_and_the_engine_never_runs() {
        let engine = Arc::new(Scripted::default());
        let err = tool("noapprover", Some(engine.clone()), false).call(json!({ "project": "repo", "task": "fix it" })).await.unwrap_err();
        assert!(err.to_string().contains("can't ask"), "{err}");
        assert!(engine.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_task_runs_in_the_projects_folder_in_manual_mode_with_the_approver() {
        let engine = Arc::new(Scripted::default());
        let result = tool("runs", Some(engine.clone()), true).call(json!({ "project": "repo", "task": "  fix it  " })).await.unwrap();
        assert_eq!(result, json!({ "answer": "Done.", "session_id": "ses_new", "tools_used": ["edit"] }));
        let asked = engine.asked.lock().unwrap();
        let (request, mode, had_approver) = &asked[0];
        assert_eq!((request.workdir.as_str(), request.prompt.as_str(), request.system.as_deref(), request.target.as_str()), ("/home/me/repo", "fix it", Some("Use tabs."), "Repo"));
        assert_eq!(request.session_id, None);
        assert_eq!(*mode, CodeMode::Manual);
        assert!(*had_approver, "the engine's asks must reach the person");
    }

    #[tokio::test]
    async fn a_session_id_continues_the_session() {
        let engine = Arc::new(Scripted::default());
        let result = tool("continues", Some(engine.clone()), true).call(json!({ "project": "repo", "task": "go on", "session_id": "ses_7" })).await.unwrap();
        assert_eq!(result["session_id"], "ses_7");
        assert_eq!(engine.asked.lock().unwrap()[0].0.session_id.as_deref(), Some("ses_7"));
    }

    // --- Progress (P124) ---

    /// An engine that narrates a task: its session, some text, a command running and then done, and a notice.
    struct Chatty;
    #[async_trait]
    impl CodeEngine for Chatty {
        async fn run_turn(&self, _: TurnRequest, _: Option<Arc<dyn Approver>>, on_event: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
            let tool = |status| CodeEvent::Tool(ToolEvent { call_id: "c1".into(), tool: "bash".into(), title: "cargo test".into(), status });
            on_event(CodeEvent::Session("ses_chat".into()));
            on_event(CodeEvent::Text("I will run the tests".into()));
            on_event(tool(ToolStatus::Running));
            on_event(CodeEvent::Notice("retrying".into()));
            on_event(tool(ToolStatus::Completed));
            Ok(TurnOutcome { session_id: "ses_chat".into(), text: "ok".into(), tools_used: vec!["bash".into()] })
        }
        async fn abort(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn chatty_tool(name: &str, with_sink: bool) -> (Arc<dyn Tool>, tokio::sync::mpsc::UnboundedReceiver<crate::tool::ToolProgress>) {
        let slot: CodeEngineSlot = Arc::new(OnceLock::new());
        let engine: Arc<dyn CodeEngine> = Arc::new(Chatty);
        slot.set(engine).ok();
        let mut tool = CodeTaskTool::new(with_many_projects(name), slot);
        tool.approver = Some(Arc::new(Yes));
        let (sink, receiver) = crate::tool::progress_channel();
        let tool: Arc<dyn Tool> = if with_sink { tool.with_progress(&sink).unwrap() } else { Arc::new(tool) };
        (tool, receiver)
    }

    fn said(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<crate::tool::ToolProgress>) -> Vec<crate::tool::ToolProgress> {
        std::iter::from_fn(|| receiver.try_recv().ok()).collect()
    }

    #[tokio::test]
    async fn a_task_says_its_tools_and_notices_under_its_project_and_not_its_text_or_session() {
        let (tool, mut receiver) = chatty_tool("says", true);
        tool.call(json!({ "project": "repo", "task": "test it" })).await.unwrap();

        let said = said(&mut receiver);
        let tool_row = |status| crate::tool::ToolProgress { tool: "code_task".into(), event: CodeEvent::Tool(ToolEvent { call_id: "repo:c1".into(), tool: "bash".into(), title: "REPO: cargo test".into(), status }) };
        assert_eq!(said, [tool_row(ToolStatus::Running), crate::tool::ToolProgress { tool: "code_task".into(), event: CodeEvent::Notice("REPO: retrying".into()) }, tool_row(ToolStatus::Completed)]);
    }

    #[tokio::test]
    async fn two_tasks_in_different_projects_do_not_share_a_line() {
        let (tool, mut receiver) = chatty_tool("two-lines", true);
        tool.call(json!({ "project": "repo", "task": "a" })).await.unwrap();
        tool.call(json!({ "project": "site", "task": "b" })).await.unwrap();
        let ids: std::collections::BTreeSet<String> = said(&mut receiver)
            .into_iter()
            .filter_map(|p| match p.event {
                CodeEvent::Tool(t) => Some(t.call_id),
                _ => None,
            })
            .collect();
        assert_eq!(ids.into_iter().collect::<Vec<_>>(), ["repo:c1", "site:c1"], "the engine's own call id is the same in both");
    }

    #[tokio::test]
    async fn without_a_sink_nothing_is_said_and_the_answer_is_the_same() {
        let (tool, mut receiver) = chatty_tool("quiet", false);
        let result = tool.call(json!({ "project": "repo", "task": "test it" })).await.unwrap();
        assert_eq!(result["answer"], "ok");
        assert!(said(&mut receiver).is_empty());
    }

    #[tokio::test]
    async fn a_background_task_says_it_too() {
        let (tool, mut receiver) = chatty_tool("bg-says", true);
        let board = crate::jobs::JobBoard::new(2);
        let tool = tool.with_jobs(&board).unwrap();
        let started = tool.call(json!({ "project": "site", "task": "test it", "background": true })).await.unwrap();
        board.wait(started["job_id"].as_str().unwrap()).await;
        let rows = said(&mut receiver);
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(rows.iter().all(|p| p.tool == "code_task"));
    }

    // --- Mode and autonomy (P124) ---

    fn at(name: &str, level: Autonomy) -> (Arc<Scripted>, Arc<dyn Tool>) {
        let engine = Arc::new(Scripted::default());
        let tool = tool(name, Some(engine.clone()), true).with_autonomy(level, &[]).unwrap();
        (engine, tool)
    }

    fn modes_in(tool: &Arc<dyn Tool>) -> Vec<String> {
        tool.spec().parameters["properties"]["mode"]["enum"].as_array().unwrap().iter().map(|m| m.as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn each_autonomy_offers_the_modes_it_may_ask_for_and_never_accept_all() {
        let offered = |level| modes_in(&at("offers", level).1);
        assert_eq!(offered(Autonomy::Suggest), ["plan"]);
        assert_eq!(offered(Autonomy::AskFirst), ["manual", "plan"]);
        assert_eq!(offered(Autonomy::Autonomous), ["manual", "acceptEdits", "plan"]);
        assert_eq!(offered(Autonomy::Manager), ["manual", "acceptEdits", "plan"]);
        // A tool no orchestrator scoped yet is an agent acting on its own: same as 4.
        assert_eq!(modes_in(&(Arc::new(tool("unscoped", None, true)) as Arc<dyn Tool>)), ["manual", "acceptEdits", "plan"]);
    }

    #[tokio::test]
    async fn the_mode_asked_for_reaches_the_engine_and_none_asked_is_manual() {
        let (engine, tool) = at("mode-reaches", Autonomy::Autonomous);
        for (args, expected) in [
            (json!({ "project": "repo", "task": "a" }), CodeMode::Manual),
            (json!({ "project": "repo", "task": "b", "mode": "acceptEdits" }), CodeMode::AcceptEdits),
            (json!({ "project": "repo", "task": "c", "mode": "plan" }), CodeMode::Plan),
            (json!({ "project": "repo", "task": "d", "mode": "  manual " }), CodeMode::Manual),
        ] {
            tool.call(args).await.unwrap();
            assert_eq!(engine.asked.lock().unwrap().last().unwrap().1, expected);
        }
    }

    fn choices(default: Option<&str>) -> ModelChoices {
        ModelChoices {
            ids: vec!["fast".into(), "strong".into()],
            hints: vec![("strong".into(), "hard reasoning".into())],
            default: default.map(str::to_string),
            resolve: Arc::new(|_| anyhow::bail!("not built in this test")),
        }
    }

    #[tokio::test]
    async fn the_model_asked_for_reaches_the_engine_and_only_one_of_the_choices_is_taken() {
        let (engine, tool) = at("model-reaches", Autonomy::Autonomous);
        // Nothing to choose from: no `model` in the schema, and asking for one is refused.
        assert!(tool.spec().parameters["properties"].get("model").is_none());
        let err = tool.call(json!({ "project": "repo", "task": "a", "model": "fast" })).await.unwrap_err();
        assert!(err.to_string().contains("can't choose a model"), "{err}");

        let tool = tool.with_model_choices(Some(&choices(None))).unwrap();
        let spec = tool.spec();
        assert_eq!(spec.parameters["properties"]["model"]["enum"], json!(["fast", "strong"]));
        assert!(spec.parameters["properties"]["model"]["description"].as_str().unwrap().contains("strong: hard reasoning"));
        for (args, expected) in [
            (json!({ "project": "repo", "task": "b" }), None),
            (json!({ "project": "repo", "task": "c", "model": " strong " }), Some("strong")),
        ] {
            tool.call(args).await.unwrap();
            assert_eq!(engine.asked.lock().unwrap().last().unwrap().0.model.as_deref(), expected);
        }
        let before = engine.asked.lock().unwrap().len();
        let err = tool.call(json!({ "project": "repo", "task": "d", "model": "ghost" })).await.unwrap_err();
        assert!(err.to_string().contains("unknown model 'ghost'") && err.to_string().contains("fast, strong"), "{err}");
        assert_eq!(engine.asked.lock().unwrap().len(), before, "a refused model never reaches the engine");

        // A person limited the agent to one model: it does every task, whatever is (not) asked.
        let limited = tool.with_model_choices(Some(&choices(Some("fast")))).unwrap();
        limited.call(json!({ "project": "repo", "task": "e" })).await.unwrap();
        assert_eq!(engine.asked.lock().unwrap().last().unwrap().0.model.as_deref(), Some("fast"));
    }

    #[tokio::test]
    async fn a_mode_above_the_agents_ceiling_or_not_a_mode_is_refused_with_what_is_allowed() {
        let (engine, tool) = at("mode-refused", Autonomy::AskFirst);
        for (mode, why) in [("acceptEdits", "more than this agent may ask for"), ("acceptAll", "more than this agent may ask for"), ("yolo", "not a mode")] {
            let err = tool.call(json!({ "project": "repo", "task": "x", "mode": mode })).await.unwrap_err().to_string();
            assert!(err.contains(why) && err.contains("manual, plan"), "{mode}: {err}");
        }
        assert!(engine.asked.lock().unwrap().is_empty(), "nothing reached the engine");

        // Not even at 5: accept-all is a person's to turn on.
        let (engine, tool) = at("mode-never-all", Autonomy::Manager);
        let err = tool.call(json!({ "project": "repo", "task": "x", "mode": "acceptAll" })).await.unwrap_err().to_string();
        assert!(err.contains("more than this agent may ask for"), "{err}");
        assert!(engine.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_mode_reaches_the_engine_in_the_background_too() {
        let engine = Arc::new(Held::default());
        let board = crate::jobs::JobBoard::new(2);
        let tool = held_tool("bg-mode", &engine, &board);
        engine.gate.add_permits(1);
        let started = tool.call(json!({ "project": "repo", "task": "look", "mode": "plan", "background": true })).await.unwrap();
        board.wait(started["job_id"].as_str().unwrap()).await;
        assert_eq!(*engine.modes.lock().unwrap(), [CodeMode::Plan]);
    }

    #[test]
    fn the_autonomy_only_goes_down_and_a_critical_infrastructure_rule_takes_the_edits_away() {
        let (_, tool) = at("only-down", Autonomy::AskFirst);
        let raised = tool.with_autonomy(Autonomy::Autonomous, &[]).unwrap();
        assert_eq!(modes_in(&raised), ["manual", "plan"], "re-scoped to a higher level, it keeps the lower ceiling");

        let (_, free) = at("critical", Autonomy::Autonomous);
        assert!(modes_in(&free).contains(&"acceptEdits".to_string()));
        let asked = free.with_approval_rules(&[Category::DeleteData], None).unwrap();
        assert!(modes_in(&asked).contains(&"acceptEdits".to_string()), "another kind of rule changes nothing here");
        let asked = free.with_approval_rules(&[Category::CriticalInfra], None).unwrap();
        assert_eq!(modes_in(&asked), ["manual", "plan"], "the person asked to be asked: the engine's own asks stay");
        assert_eq!(modes_in(&asked.with_approval_rules(&[], None).unwrap()), ["manual", "plan"], "and it only ever grows");
    }

    #[test]
    fn an_orchestrator_held_to_ask_first_hands_its_code_task_the_lower_ceiling() {
        let engine: Arc<dyn CodeEngine> = Arc::new(Scripted::default());
        let slot: CodeEngineSlot = Arc::new(OnceLock::new());
        slot.set(engine).ok();
        let mut orchestrator = crate::orchestrator::Orchestrator::new(Arc::new(NoModel), vault("orchestrated"));
        orchestrator.register_tool(Arc::new(CodeTaskTool::new(with_projects("orchestrated"), slot)));
        let spec_of = |o: &crate::orchestrator::Orchestrator| o.tools().iter().find(|t| t.spec().name == "code_task").unwrap().spec();
        let offered = |o: &crate::orchestrator::Orchestrator| spec_of(o).parameters["properties"]["mode"]["enum"].clone();

        assert_eq!(offered(&orchestrator), json!(["manual", "acceptEdits", "plan"]));
        let careful = orchestrator.with_autonomy(Autonomy::AskFirst, &[]);
        assert_eq!(offered(&careful), json!(["manual", "plan"]));
        assert_eq!(offered(&careful.with_autonomy(Autonomy::Autonomous, &[])), json!(["manual", "plan"]), "and nothing raises it again");
    }

    // --- Worktrees (P124) ---

    /// A project whose folder is a real git repository with one commit (the worktrees are git's), inside a folder of its own.
    fn git_project(name: &str) -> (std::path::PathBuf, Arc<Vault>) {
        let outer = std::env::temp_dir().join(format!("warden-code-task-wt-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let repo = outer.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).status().unwrap().success());
        git(&["init", "-q"]);
        std::fs::write(repo.join("a.txt"), "one").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "first"]);
        let vault = vault(name);
        let project = Project { id: "repo".into(), name: "Repo".into(), description: String::new(), instructions: String::new(), workdir: Some(repo.to_string_lossy().into_owned()), code: true };
        ProjectStore::new(vault.clone()).save(&project).unwrap();
        (outer, vault)
    }

    /// An engine that edits a file in the folder it is given when the task says "edit", and writes down the folders it worked in.
    #[derive(Default)]
    struct Editor {
        folders: Mutex<Vec<String>>,
    }
    #[async_trait]
    impl CodeEngine for Editor {
        async fn run_turn(&self, request: TurnRequest, _: Option<Arc<dyn Approver>>, _: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
            self.folders.lock().unwrap().push(request.workdir.clone());
            if request.prompt.contains("edit") {
                std::fs::write(std::path::Path::new(&request.workdir).join("made.txt"), "by the engine")?;
            }
            Ok(TurnOutcome { session_id: "ses_wt".into(), text: "Done.".into(), tools_used: Vec::new() })
        }
        async fn abort(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn editing_tool(vault: Arc<Vault>) -> (Arc<Editor>, CodeTaskTool) {
        let editor = Arc::new(Editor::default());
        let slot: CodeEngineSlot = Arc::new(OnceLock::new());
        let shared: Arc<dyn CodeEngine> = editor.clone();
        slot.set(shared).ok();
        let mut tool = CodeTaskTool::new(vault, slot);
        tool.approver = Some(Arc::new(Yes));
        (editor, tool)
    }

    #[tokio::test]
    async fn an_isolated_task_works_in_a_worktree_of_its_own_and_keeps_it_only_if_it_changed_something() {
        let (outer, vault) = git_project("isolate");
        let (editor, tool) = editing_tool(vault);
        let project = tool.code_projects()[0].workdir.clone().unwrap();

        // Nothing changed: the worktree is gone, and the answer says so.
        let idle = tool.call(json!({ "project": "repo", "task": "look around", "isolate": true })).await.unwrap();
        let folder = editor.folders.lock().unwrap()[0].clone();
        assert_ne!(folder, project, "the engine worked elsewhere");
        assert!(folder.contains(".warden-worktrees"), "{folder}");
        assert_eq!((idle["worktree"].clone(), idle["answer"].clone()), (json!(null), json!("Done.")));
        assert!(!std::path::Path::new(&folder).exists());

        // Something changed: the worktree and its branch are kept and named.
        let busy = tool.call(json!({ "project": "repo", "task": "edit a file", "isolate": true })).await.unwrap();
        let kept = busy["path"].as_str().unwrap();
        assert!(std::path::Path::new(kept).join("made.txt").is_file());
        assert_eq!(busy["branch"], format!("warden/{}", busy["worktree"].as_str().unwrap()));
        assert!(busy["changes"].as_str().unwrap().contains("made.txt"), "{busy}");
        assert!(!std::path::Path::new(&project).join("made.txt").exists(), "the project's own folder was not touched");

        // A task without `isolate` is as it always was.
        let plain = tool.call(json!({ "project": "repo", "task": "look around" })).await.unwrap();
        assert!(plain.get("worktree").is_none(), "{plain}");
        assert_eq!(editor.folders.lock().unwrap().last().unwrap(), &project);
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_kept_worktree_can_be_gone_back_to_and_a_bad_ask_is_refused_before_the_engine_runs() {
        let (outer, vault) = git_project("goback");
        let (editor, tool) = editing_tool(vault);
        let first = tool.call(json!({ "project": "repo", "task": "edit a file", "isolate": true })).await.unwrap();
        let id = first["worktree"].as_str().unwrap().to_string();

        let again = tool.call(json!({ "project": "repo", "task": "go on", "worktree": id, "session_id": "ses_wt" })).await.unwrap();
        assert_eq!(editor.folders.lock().unwrap().last().unwrap(), first["path"].as_str().unwrap(), "the same folder");
        assert_eq!(again["worktree"], first["worktree"], "and still kept: it still has the work");

        let before = editor.folders.lock().unwrap().len();
        for (args, why) in [
            (json!({ "project": "repo", "task": "x", "isolate": true, "worktree": id }), "not both"),
            (json!({ "project": "repo", "task": "x", "worktree": "../repo" }), "isn't a worktree id"),
            (json!({ "project": "repo", "task": "x", "worktree": "abcdef" }), "no worktree 'abcdef'"),
        ] {
            let err = tool.call(args).await.unwrap_err();
            assert!(err.to_string().contains(why), "{why}: {err}");
        }
        assert_eq!(editor.folders.lock().unwrap().len(), before, "the engine never ran");
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_project_that_is_no_git_repository_cannot_be_isolated() {
        let (editor, tool) = editing_tool(with_projects("no-git"));
        let err = tool.call(json!({ "project": "repo", "task": "x", "isolate": true })).await.unwrap_err();
        assert!(err.to_string().contains("isn't in a git repository"), "{err}");
        assert!(editor.folders.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn isolated_tasks_on_one_project_run_side_by_side_where_plain_ones_wait_in_line() {
        let (outer, vault) = git_project("side-by-side");
        let engine = Arc::new(Held::default());
        let slot: CodeEngineSlot = Arc::new(OnceLock::new());
        let shared: Arc<dyn CodeEngine> = engine.clone();
        slot.set(shared).ok();
        let mut tool = CodeTaskTool::new(vault, slot);
        tool.approver = Some(Arc::new(Yes));
        let board = crate::jobs::JobBoard::new(4);
        let tool = tool.with_jobs(&board).unwrap();

        let first = tool.call(json!({ "project": "repo", "task": "one", "background": true, "isolate": true })).await.unwrap();
        let second = tool.call(json!({ "project": "repo", "task": "two", "background": true, "isolate": true })).await.unwrap();
        assert_ne!(first["worktree"], second["worktree"], "each has its own, and the call already says which");
        eventually("both are working at once", || engine.peak.load(std::sync::atomic::Ordering::SeqCst) == 2).await;

        engine.gate.add_permits(2);
        eventually("both ended", || board.list().iter().all(|job| job.state.is_finished())).await;
        // The engine of this test changed nothing, so neither worktree is left.
        let left = std::fs::read_dir(outer.join(".warden-worktrees").join("repo")).map(|dir| dir.count()).unwrap_or(0);
        assert_eq!(left, 0, "nothing changed, nothing kept");
        std::fs::remove_dir_all(&outer).ok();
    }

    // --- Background (P124) ---

    /// Three code projects: `repo` and `twin` share a folder, `site` has its own.
    fn with_many_projects(name: &str) -> Arc<Vault> {
        let vault = vault(name);
        let store = ProjectStore::new(vault.clone());
        for (id, workdir) in [("repo", "/home/me/repo"), ("twin", "/home/me/repo"), ("site", "/home/me/site")] {
            store.save(&Project { id: id.into(), name: id.to_uppercase(), description: String::new(), instructions: String::new(), workdir: Some(workdir.into()), code: true }).unwrap();
        }
        vault
    }

    /// An engine that holds every task until the test lets it go, and writes down how many ran at once and what it was told to stop.
    struct Held {
        gate: tokio::sync::Semaphore,
        running: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
        started: Mutex<Vec<String>>,
        modes: Mutex<Vec<CodeMode>>,
        aborted: Mutex<Vec<(String, String)>>,
    }
    impl Default for Held {
        /// Closed: no task finishes until the test adds a permit.
        fn default() -> Self {
            Self { gate: tokio::sync::Semaphore::new(0), running: Default::default(), peak: Default::default(), started: Default::default(), modes: Default::default(), aborted: Default::default() }
        }
    }

    /// A model nobody calls: the orchestrator test only looks at the tools it hands out.
    struct NoModel;
    #[async_trait]
    impl crate::model::ModelProvider for NoModel {
        async fn chat_stream(&self, _: Vec<crate::model::Message>, _: Vec<ToolSpec>) -> anyhow::Result<crate::model::ChatStream> {
            anyhow::bail!("not used by this test")
        }
    }
    #[async_trait]
    impl CodeEngine for Held {
        async fn run_turn(&self, request: TurnRequest, _: Option<Arc<dyn Approver>>, on_event: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
            use std::sync::atomic::Ordering::SeqCst;
            let session = format!("ses_{}", request.workdir);
            on_event(CodeEvent::Session(session.clone()));
            self.started.lock().unwrap().push(request.prompt.clone());
            self.modes.lock().unwrap().push(*request.mode.borrow());
            let now = self.running.fetch_add(1, SeqCst) + 1;
            self.peak.fetch_max(now, SeqCst);
            let permit = self.gate.acquire().await.unwrap();
            permit.forget();
            self.running.fetch_sub(1, SeqCst);
            Ok(TurnOutcome { session_id: session, text: format!("did {}", request.prompt), tools_used: vec!["edit".into()] })
        }
        async fn abort(&self, workdir: &str, session_id: &str) -> anyhow::Result<()> {
            self.aborted.lock().unwrap().push((workdir.to_string(), session_id.to_string()));
            Ok(())
        }
    }

    fn held_tool(name: &str, engine: &Arc<Held>, board: &Arc<crate::jobs::JobBoard>) -> Arc<dyn Tool> {
        let slot: CodeEngineSlot = Arc::new(OnceLock::new());
        let shared: Arc<dyn CodeEngine> = engine.clone();
        slot.set(shared).ok();
        let mut tool = CodeTaskTool::new(with_many_projects(name), slot);
        tool.approver = Some(Arc::new(Yes));
        tool.with_jobs(board).unwrap()
    }

    /// Waits (briefly) until `check` holds, so a test does not depend on how fast a task starts.
    async fn eventually(what: &str, check: impl Fn() -> bool) {
        for _ in 0..200 {
            if check() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("never happened: {what}");
    }

    #[test]
    fn background_is_offered_only_with_the_turns_jobs() {
        let plain = tool("nojobs", None, true).spec();
        assert!(plain.parameters["properties"].get("background").is_none());
        assert!(!plain.description.contains("side by side"));
        let bound = tool("jobs", None, true).with_jobs(&crate::jobs::JobBoard::new(2)).unwrap().spec();
        assert_eq!(bound.parameters["properties"]["background"]["type"], "boolean");
        assert!(bound.description.contains("session_id"));
    }

    #[tokio::test]
    async fn a_background_task_returns_a_job_at_once_and_its_result_carries_the_session_to_continue() {
        let engine = Arc::new(Held::default());
        let board = crate::jobs::JobBoard::new(2);
        let tool = held_tool("bg-result", &engine, &board);

        let started = tool.call(json!({ "project": "repo", "task": "fix it", "background": true })).await.unwrap();
        assert_eq!(started["status"], "started");
        let job = started["job_id"].as_str().unwrap().to_string();
        eventually("the engine got the task", || engine.started.lock().unwrap().len() == 1).await;
        assert_eq!(board.state(&job), Some(crate::jobs::JobState::Running), "the call did not wait for the engine");

        engine.gate.add_permits(1);
        match board.wait(&job).await {
            Some(crate::jobs::JobState::Done(text)) => assert_eq!(text, "did fix it\n\nsession_id: ses_/home/me/repo\ntools_used: edit"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn tasks_on_different_folders_run_side_by_side_and_on_the_same_folder_one_after_the_other() {
        use std::sync::atomic::Ordering::SeqCst;
        let engine = Arc::new(Held::default());
        let board = crate::jobs::JobBoard::new(4);
        let tool = held_tool("bg-folders", &engine, &board);

        let mut jobs = Vec::new();
        for (project, task) in [("repo", "a"), ("site", "b"), ("twin", "c")] {
            let started = tool.call(json!({ "project": project, "task": task, "background": true })).await.unwrap();
            jobs.push(started["job_id"].as_str().unwrap().to_string());
        }
        eventually("repo and site run together", || engine.running.load(SeqCst) == 2).await;
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        assert_eq!(engine.running.load(SeqCst), 2, "twin shares the folder of repo, so it waits");
        assert_eq!(engine.started.lock().unwrap().len(), 2);

        engine.gate.add_permits(3);
        for job in &jobs {
            assert!(matches!(board.wait(job).await, Some(crate::jobs::JobState::Done(_))));
        }
        assert_eq!(engine.started.lock().unwrap().len(), 3);
        assert_eq!(engine.peak.load(SeqCst), 2, "never more than the two folders at once");
    }

    #[tokio::test]
    async fn dropping_a_background_task_stops_the_engines_session() {
        let engine = Arc::new(Held::default());
        let board = crate::jobs::JobBoard::new(2);
        let tool = held_tool("bg-abort", &engine, &board);
        tool.call(json!({ "project": "site", "task": "long one", "background": true })).await.unwrap();
        eventually("the engine started", || engine.started.lock().unwrap().len() == 1).await;

        board.abort_unfinished();
        eventually("the session was stopped", || !engine.aborted.lock().unwrap().is_empty()).await;
        assert_eq!(engine.aborted.lock().unwrap()[0], ("/home/me/site".to_string(), "ses_/home/me/site".to_string()));

        // And one that finished is not stopped afterwards.
        let engine = Arc::new(Held::default());
        let board = crate::jobs::JobBoard::new(2);
        let tool = held_tool("bg-abort-done", &engine, &board);
        engine.gate.add_permits(1);
        let started = tool.call(json!({ "project": "site", "task": "quick", "background": true })).await.unwrap();
        board.wait(started["job_id"].as_str().unwrap()).await;
        drop(board);
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert!(engine.aborted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_background_task_without_an_approver_is_refused_before_anything_starts() {
        let engine = Arc::new(Held::default());
        let slot: CodeEngineSlot = Arc::new(OnceLock::new());
        let shared: Arc<dyn CodeEngine> = engine.clone();
        slot.set(shared).ok();
        let tool = CodeTaskTool::new(with_many_projects("bg-noask"), slot).with_jobs(&crate::jobs::JobBoard::new(2)).unwrap();
        let err = tool.call(json!({ "project": "repo", "task": "x", "background": true })).await.unwrap_err();
        assert!(err.to_string().contains("can't ask"), "{err}");
        assert!(engine.started.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn with_the_jobs_but_without_background_the_answer_is_the_same_json_as_ever() {
        let engine = Arc::new(Scripted::default());
        let slot: CodeEngineSlot = Arc::new(OnceLock::new());
        let shared: Arc<dyn CodeEngine> = engine.clone();
        slot.set(shared).ok();
        let mut tool = CodeTaskTool::new(with_projects("waits"), slot);
        tool.approver = Some(Arc::new(Yes));
        let tool = tool.with_jobs(&crate::jobs::JobBoard::new(2)).unwrap();
        let result = tool.call(json!({ "project": "repo", "task": "fix it" })).await.unwrap();
        assert_eq!(result, json!({ "answer": "Done.", "session_id": "ses_new", "tools_used": ["edit"] }));
    }

    /// Writes down the tasks it is told about, under ids no other test uses (the registry of controls is the process's).
    struct Recorded {
        prefix: &'static str,
        seen: Mutex<Vec<String>>,
    }
    impl crate::jobs::TaskRecorder for Recorded {
        fn created(&self, task: &crate::jobs::TaskSpec) -> String {
            let mut seen = self.seen.lock().unwrap();
            let number = seen.iter().filter(|line| line.starts_with("created")).count() + 1;
            seen.push(format!("created {}", task.assignee));
            format!("{}-{number}", self.prefix)
        }
        fn running(&self, id: &str) {
            self.seen.lock().unwrap().push(format!("running {id}"));
        }
        fn finished(&self, id: &str, outcome: crate::jobs::TaskOutcome) {
            let how = match outcome {
                crate::jobs::TaskOutcome::Done { .. } => "done",
                crate::jobs::TaskOutcome::Failed { .. } => "failed",
                crate::jobs::TaskOutcome::Cancelled => "cancelled",
                crate::jobs::TaskOutcome::Stopped => "stopped",
            };
            self.seen.lock().unwrap().push(format!("{how} {id}"));
        }
    }

    #[tokio::test]
    async fn a_recorded_code_task_is_on_the_tasks_screen_stoppable_and_not_pausable() {
        let engine = Arc::new(Held::default());
        let recorder = Arc::new(Recorded { prefix: "codetask", seen: Mutex::new(Vec::new()) });
        let context = crate::jobs::TaskContext { group: "g".into(), owner: Some("manager".into()), channel: "hub".into(), parent: None, depth: 0 };
        let board = crate::jobs::JobBoard::recording(2, recorder.clone(), context);
        let tool = held_tool("recorded", &engine, &board);

        tool.call(json!({ "project": "repo", "task": "go", "background": true })).await.unwrap();
        eventually("the engine started", || engine.started.lock().unwrap().len() == 1).await;
        assert_eq!(recorder.seen.lock().unwrap()[0], "created opencode · REPO");
        assert!(task_controls().is_controllable("codetask-1"));
        assert!(!task_controls().is_pausable("codetask-1"), "the engine has no gate, so there is nothing to pause");
        assert!(!task_controls().pause("codetask-1"));

        assert!(task_controls().cancel("codetask-1"), "a person can stop it");
        eventually("the session was stopped", || !engine.aborted.lock().unwrap().is_empty()).await;
        eventually("it is recorded as stopped", || recorder.seen.lock().unwrap().contains(&"stopped codetask-1".to_string())).await;

        // The same task when the agent waits for it: recorded too, and not pausable either.
        engine.gate.add_permits(1);
        tool.call(json!({ "project": "site", "task": "wait for me" })).await.unwrap();
        let seen = recorder.seen.lock().unwrap().clone();
        assert!(seen.contains(&"created opencode · SITE".to_string()) && seen.contains(&"done codetask-2".to_string()), "{seen:?}");
    }

    #[tokio::test]
    async fn a_project_that_is_not_a_code_one_and_a_missing_engine_are_refused() {
        let engine = Arc::new(Scripted::default());
        for project in ["shell-only", "notes", "gone"] {
            let err = tool("notcode", Some(engine.clone()), true).call(json!({ "project": project, "task": "x" })).await.unwrap_err();
            assert!(err.to_string().contains("not a code project"), "{project}: {err}");
        }
        assert!(engine.asked.lock().unwrap().is_empty());
        let err = tool("noengine", None, true).call(json!({ "project": "repo", "task": "x" })).await.unwrap_err();
        assert!(err.to_string().contains("isn't available"), "{err}");
        let err = tool("notask", Some(engine), true).call(json!({ "project": "repo", "task": "   " })).await.unwrap_err();
        assert!(err.to_string().contains("'task'"), "{err}");
    }
}

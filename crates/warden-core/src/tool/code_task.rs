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

use crate::code_engine::{CodeEngine, CodeEvent, CodeMode, TurnOutcome, TurnRequest};
use crate::jobs::{task_controls, JobBoard, TaskDraft};
use crate::memory::Vault;
use crate::project::ProjectStore;
use crate::tool::job_tools::{background_property, job_label, start_task_with, wants_background};
use crate::tool::{Approver, Tool, ToolSpec};

/// The lock of a folder: whoever works in it holds it. Process-wide rather than per tool, because the hub builds its tools again
/// whenever its settings are saved and a task already running must still keep the next one out.
fn folder_lock(workdir: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).entry(workdir.to_string()).or_default().clone()
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

/// Runs `request` in the engine once nobody else is working in its folder, and stops the session if the run is dropped.
async fn run_in_engine(engine: Arc<dyn CodeEngine>, approver: Arc<dyn Approver>, request: TurnRequest) -> anyhow::Result<TurnOutcome> {
    let workdir = request.workdir.clone();
    let _folder = folder_lock(&workdir).lock_owned().await;
    let session = Arc::new(Mutex::new(request.session_id.clone()));
    let mut stop = StopOnDrop { engine: engine.clone(), workdir, session: session.clone(), armed: true };
    let outcome = engine
        .run_turn(request, Some(approver), &mut move |event| {
            if let CodeEvent::Session(id) = event {
                *session.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
            }
        })
        .await;
    stop.armed = false;
    outcome
}

/// What a background task's result says: the answer, and the session to continue and the tools it used (a `jobs` result is text).
fn describe(outcome: &TurnOutcome) -> String {
    let tools = if outcome.tools_used.is_empty() { String::new() } else { format!("\ntools_used: {}", outcome.tools_used.join(", ")) };
    format!("{}\n\nsession_id: {}{tools}", outcome.text, outcome.session_id)
}

/// Where the engine will be, once there is one: the hub builds its tools before the engine (which needs the hub's model
/// route), and builds them again whenever its settings are saved, so the tool holds a slot the hub fills once.
pub type CodeEngineSlot = Arc<OnceLock<Arc<dyn CodeEngine>>>;

pub struct CodeTaskTool {
    projects: ProjectStore,
    engine: CodeEngineSlot,
    approver: Option<Arc<dyn Approver>>,
    /// The turn's background jobs, once bound (`with_jobs`): only then does the tool accept `background` and record its tasks.
    jobs: Option<Arc<JobBoard>>,
}

impl CodeTaskTool {
    pub fn new(vault: Arc<Vault>, engine: CodeEngineSlot) -> Self {
        Self { projects: ProjectStore::new(vault), engine, approver: None, jobs: None }
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
        let mut background_note = "";
        if self.jobs.is_some() {
            properties["background"] = background_property();
            background_note = " Tasks on different projects can run side by side in the background; two on the same project run one after the other. \
                 A background task's result (read with 'jobs') ends with the 'session_id' to continue it.";
        }
        ToolSpec {
            name: "code_task".to_string(),
            description: format!(
                "Hand a programming task to the code engine (the opencode) working in a code project's folder: it reads and edits \
                 the files and runs commands there, and answers with what it did. The person is asked to approve each of its \
                 actions, so a refused action did not happen. The engine does NOT see this conversation — the 'task' must be \
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

    fn with_vault(&self, vault: &Arc<Vault>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { projects: ProjectStore::new(vault.clone()), engine: self.engine.clone(), approver: self.approver.clone(), jobs: self.jobs.clone() }))
    }

    fn with_approver(&self, approver: Arc<dyn Approver>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { projects: self.projects.clone(), engine: self.engine.clone(), approver: Some(approver), jobs: self.jobs.clone() }))
    }

    fn with_jobs(&self, board: &Arc<JobBoard>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { projects: self.projects.clone(), engine: self.engine.clone(), approver: self.approver.clone(), jobs: Some(board.clone()) }))
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
        let Some(approver) = self.approver.clone() else {
            anyhow::bail!("the code engine asks the person before each of its actions, and this channel can't ask (use the desktop app or the web)");
        };
        let Some(engine) = self.engine.get().cloned() else {
            anyhow::bail!("the code engine isn't available on this machine");
        };
        let workdir = project.workdir.clone().ok_or_else(|| anyhow::anyhow!("the project '{}' has no working folder", project.name))?;

        // `Manual` for good: nobody holds the sender, so nothing can switch it to "accept all" while the task runs.
        let (_, mode) = tokio::sync::watch::channel(CodeMode::Manual);
        let request = TurnRequest {
            workdir,
            session_id,
            prompt: task.to_string(),
            title: project.name.clone(),
            target: project.name.clone(),
            system: Some(project.instructions.trim().to_string()).filter(|s| !s.is_empty()),
            mode,
        };
        let draft = TaskDraft { assignee: format!("opencode · {}", project.name), objective: task.to_string(), model: None };
        let result_of = |outcome: &TurnOutcome| json!({ "answer": outcome.text, "session_id": outcome.session_id, "tools_used": outcome.tools_used });

        if let Some(board) = &self.jobs {
            // In the background: this returns a job_id at once and the engine works while the agent carries on. The task is on the
            // tasks screen; it can be stopped there, but there is no gate in the engine to pause.
            if wants_background(&args) {
                let label = job_label(&draft.assignee, task);
                let started = start_task_with(board, label, draft, move |_| async move {
                    let outcome = run_in_engine(engine, approver, request).await?;
                    Ok((describe(&outcome), None))
                });
                if let Some(recorded) = board.list().last().and_then(|job| job.task.as_ref()) {
                    task_controls().forbid_pause(&recorded.task_id);
                }
                return Ok(started);
            }
            // Waited for, but still a task in the log, like the delegations (P123).
            let text = board
                .run_recorded_as(draft, false, move |_| async move {
                    let outcome = run_in_engine(engine, approver, request).await?;
                    Ok((result_of(&outcome).to_string(), None))
                })
                .await?;
            return Ok(serde_json::from_str(&text)?);
        }

        let outcome = run_in_engine(engine, approver, request).await?;
        Ok(result_of(&outcome))
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
        aborted: Mutex<Vec<(String, String)>>,
    }
    impl Default for Held {
        /// Closed: no task finishes until the test adds a permit.
        fn default() -> Self {
            Self { gate: tokio::sync::Semaphore::new(0), running: Default::default(), peak: Default::default(), started: Default::default(), aborted: Default::default() }
        }
    }
    #[async_trait]
    impl CodeEngine for Held {
        async fn run_turn(&self, request: TurnRequest, _: Option<Arc<dyn Approver>>, on_event: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
            use std::sync::atomic::Ordering::SeqCst;
            let session = format!("ses_{}", request.workdir);
            on_event(CodeEvent::Session(session.clone()));
            self.started.lock().unwrap().push(request.prompt.clone());
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

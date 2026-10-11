//! `code_worktree` (P124): what an agent does with the worktrees its `code_task`s left (`isolate`) — look at what a task made, and
//! bring it into the project or throw it away. The review and the integration of a Programming Manager (`VISAO_AGENTES.md` §13).
//!
//! Looking (`list`, `diff`) changes nothing. Merging and discarding change the person's folder or lose work, so each of them asks the
//! person, always, whatever the agent's autonomy: the tool asks through `with_approver`'s approver itself, and without one (a channel
//! that can't ask) it refuses. Asking for changes is a `code_task` with the same `worktree` id.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::memory::Vault;
use crate::project::ProjectStore;
use crate::tool::code_task::folder_busy;
use crate::tool::{ApprovalRequest, Approver, Tool, ToolSpec};
use crate::worktree::{self, Merged, Worktree};

/// How much of a patch an agent is shown at once; the file list is always whole.
const DIFF_CHARS: usize = 20_000;

/// How long a worktree with nothing in it is left alone before it is tidied away: a task may have made one and not started in it yet.
const EMPTY_GRACE: Duration = Duration::from_secs(60 * 60);

/// How long work must have sat untouched to be called forgotten (`list` marks it, `prune` offers it), unless the call says otherwise.
const DEFAULT_STALE_DAYS: u64 = 14;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Tidies the empty worktrees and the leftovers of `workdir`'s repository (`worktree::sweep_empty`), where no work can be lost. Best
/// effort: a listing or a new task doesn't fail because the tidying did.
pub(crate) async fn tidy(workdir: &Path) {
    if let Err(err) = worktree::sweep_empty(workdir, EMPTY_GRACE, SystemTime::now(), |path| folder_busy(&path.to_string_lossy())).await {
        eprintln!("warden: couldn't tidy the worktrees of {}: {err:#}", workdir.display());
    }
}

/// Tidies the empty worktrees of every code project's repository (`sweep_empty`), once per repository even when several projects share
/// it, and says what it removed (`worktree id`s). For the hub's timer: nobody listed or isolated anything, so nobody else would tidy.
/// Work is never touched, and a repository that can't be tidied (not git any more, a folder gone) is reported and skipped.
pub async fn tidy_projects(projects: &ProjectStore, now: SystemTime, grace: Duration, busy: &(dyn Fn(&Path) -> bool + Sync)) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut removed = Vec::new();
    for project in projects.list().into_iter().filter(|p| p.code) {
        let Some(workdir) = project.workdir.map(std::path::PathBuf::from) else { continue };
        // Not a repository (or not there): nothing of ours can be in it.
        let Ok(repo) = worktree::repository_of(&workdir).await else { continue };
        if !seen.insert(repo) {
            continue;
        }
        match worktree::sweep_empty(&workdir, grace, now, busy).await {
            Ok(mut ids) => removed.append(&mut ids),
            Err(err) => eprintln!("warden: couldn't tidy the worktrees of {}: {err:#}", workdir.display()),
        }
    }
    removed
}

/// `tidy_projects` as the hub's timer runs it: now, with the usual grace, and a folder a task is working in left alone.
pub async fn tidy_all(projects: &ProjectStore) -> Vec<String> {
    tidy_projects(projects, SystemTime::now(), EMPTY_GRACE, &|path| folder_busy(&path.to_string_lossy())).await
}

fn days(idle: Duration) -> u64 {
    idle.as_secs() / DAY.as_secs()
}

#[derive(Clone)]
pub struct CodeWorktreeTool {
    projects: ProjectStore,
    approver: Option<Arc<dyn Approver>>,
}

impl CodeWorktreeTool {
    pub fn new(vault: Arc<Vault>) -> Self {
        Self { projects: ProjectStore::new(vault), approver: None }
    }

    /// The projects that run on the engine, with their folder (the ones `code_task` takes).
    fn code_projects(&self) -> Vec<crate::project::Project> {
        self.projects.list().into_iter().filter(|p| p.code && p.workdir.is_some()).collect()
    }

    /// Asks the person; a channel that can't ask, or a no, stops the action.
    async fn confirmed(&self, project: &str, action: &str, detail: String) -> anyhow::Result<()> {
        let Some(approver) = &self.approver else {
            anyhow::bail!("{action} changes the person's folder or loses work, so it needs a yes from the person, and this channel can't ask (use the desktop app or the web)");
        };
        if !approver.approve(ApprovalRequest::new(project, action, detail)).await {
            anyhow::bail!("the person said no to: {action}. Nothing was changed.");
        }
        Ok(())
    }

    /// Throws away the work that has sat untouched for `older_than_days` (14 by default), after one yes from the person that lists it all.
    /// A worktree a task is working in is left, and so is anything touched more recently than that.
    async fn prune(&self, project: &str, workdir: &Path, args: &Value) -> anyhow::Result<Value> {
        let older_than = match args.get("older_than_days") {
            None | Some(Value::Null) => DEFAULT_STALE_DAYS,
            Some(given) => given.as_u64().filter(|days| *days >= 1).ok_or_else(|| anyhow::anyhow!("'older_than_days' must be a whole number of days, at least 1"))?,
        };
        tidy(workdir).await;
        let entries = worktree::list(workdir, SystemTime::now()).await?;
        let (busy, forgotten): (Vec<_>, Vec<_>) = worktree::stale(&entries, Duration::from_secs(older_than * DAY.as_secs())).into_iter().partition(|entry| folder_busy(&entry.path.to_string_lossy()));
        let skipped: Vec<&str> = busy.iter().map(|entry| entry.id.as_str()).collect();
        if forgotten.is_empty() {
            return Ok(json!({ "pruned": [], "skipped_busy": skipped, "note": format!("no worktree with work has sat untouched for {older_than} days or more") }));
        }
        let listing = forgotten
            .iter()
            .map(|entry| format!("- {} (branch {}): untouched for {} days, {} changed files, {} commits", entry.id, entry.branch, days(entry.idle), entry.changed_files, entry.commits))
            .collect::<Vec<_>>()
            .join("\n");
        self.confirmed(project, "discard forgotten worktrees", format!("Throw away {} worktrees, with all their work, that nothing has touched for {older_than} days or more:\n{listing}", forgotten.len())).await?;
        let mut pruned = Vec::new();
        for entry in forgotten {
            worktree::discard(&worktree::open(workdir, &entry.id).await?).await?;
            pruned.push(entry.id.clone());
        }
        Ok(json!({ "pruned": pruned, "skipped_busy": skipped }))
    }
}

#[async_trait]
impl Tool for CodeWorktreeTool {
    fn spec(&self) -> ToolSpec {
        let projects = self.code_projects();
        let mut project = json!({ "type": "string", "description": "Which code project." });
        // An empty `enum` is not valid JSON Schema and some providers refuse the whole request over it.
        if !projects.is_empty() {
            project["enum"] = json!(projects.iter().map(|p| p.id.clone()).collect::<Vec<_>>());
        }
        ToolSpec {
            name: "code_worktree".to_string(),
            description: "Review and integrate the worktrees that isolated code_task runs ('isolate') left, each on its own branch. \
                'list' shows the project's worktrees and how much work each has; 'diff' shows what a worktree's task changed; 'merge' brings it into \
                the branch the project's folder has checked out, as a merge commit, and removes the worktree; 'discard' throws it away; 'prune' throws away every \
                worktree whose work has sat untouched for 14 days or more ('older_than_days'), after one yes from the person for the whole list. 'list' \
                also tidies away, by itself, the worktrees where nothing was changed, and marks the forgotten ones ('stale'). 'merge', 'discard' and \
                'prune' always ask the person first. A merge that conflicts changes nothing and keeps the worktree: ask for the fix with code_task \
                (same 'worktree'), then merge again. To ask for changes to a task's work, run code_task again with its 'worktree'."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "diff", "merge", "discard", "prune"] },
                    "project": project,
                    "worktree": { "type": "string", "description": "The 'worktree' id a code_task returned (for diff, merge and discard)." },
                    "title": { "type": "string", "description": "For merge: what the work is, for the commit messages." },
                    "older_than_days": { "type": "integer", "minimum": 1, "description": "For prune: how many days a worktree's work must have sat untouched to be thrown away (14 by default)." }
                },
                "required": ["action", "project"]
            }),
        }
    }

    fn with_vault(&self, vault: &Arc<Vault>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { projects: ProjectStore::new(vault.clone()), ..self.clone() }))
    }

    fn with_approver(&self, approver: Arc<dyn Approver>) -> Option<Arc<dyn Tool>> {
        Some(Arc::new(Self { approver: Some(approver), ..self.clone() }))
    }

    async fn call(&self, args: Value) -> anyhow::Result<Value> {
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing required 'action' argument"))?;
        let id = args.get("project").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing required 'project' argument"))?;
        let projects = self.code_projects();
        let project = projects.iter().find(|p| p.id == id).ok_or_else(|| {
            let available = projects.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ");
            anyhow::anyhow!("'{id}' is not a code project — must be one of: {}", if available.is_empty() { "(none)" } else { &available })
        })?;
        let workdir = project.workdir.clone().ok_or_else(|| anyhow::anyhow!("the project '{}' has no working folder", project.name))?;
        let workdir = Path::new(&workdir);

        if action == "list" {
            tidy(workdir).await;
            let entries = worktree::list(workdir, SystemTime::now()).await?;
            let forgotten = Duration::from_secs(DEFAULT_STALE_DAYS * DAY.as_secs());
            let listed: Vec<Value> = entries
                .iter()
                .map(|e| json!({ "worktree": e.id, "branch": e.branch, "changed_files": e.changed_files, "commits": e.commits, "idle_days": days(e.idle), "stale": e.has_work() && e.idle >= forgotten }))
                .collect();
            let orphans = worktree::orphan_branches(workdir).await?;
            return Ok(json!({ "worktrees": listed, "orphan_branches": orphans }));
        }
        if action == "prune" {
            return self.prune(&project.name, workdir, &args).await;
        }
        if !matches!(action, "diff" | "merge" | "discard") {
            anyhow::bail!("'{action}' is not an action — must be one of: list, diff, merge, discard, prune");
        }
        let wanted = args.get("worktree").and_then(Value::as_str).map(str::trim).filter(|w| !w.is_empty()).ok_or_else(|| anyhow::anyhow!("'{action}' needs the 'worktree' id"))?;
        let found: Worktree = worktree::open(workdir, wanted).await?;
        if action == "diff" {
            return Ok(json!({ "worktree": found.id, "branch": found.branch, "diff": worktree::diff(&found, DIFF_CHARS).await? }));
        }

        if folder_busy(&found.path.to_string_lossy()) {
            anyhow::bail!("a task is still working in worktree {wanted}; wait for it to end (or stop it) before this");
        }
        let seen = worktree::diff(&found, 0).await?;
        let seen = seen.split("\n\n").next().unwrap_or_default();
        if action == "discard" {
            self.confirmed(&project.name, "discard worktree", format!("Throw away worktree {} (branch {}) and all its work:\n{seen}", found.id, found.branch)).await?;
            worktree::discard(&found).await?;
            return Ok(json!({ "discarded": true, "worktree": found.id }));
        }
        let title = args.get("title").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map_or_else(|| format!("work of worktree {}", found.id), str::to_string);
        self.confirmed(&project.name, "merge worktree", format!("Merge worktree {} (branch {}) into the project's current branch as \"{title}\":\n{seen}", found.id, found.branch)).await?;
        Ok(match worktree::merge(&found, &title).await? {
            Merged::Done { commit, stat } => json!({ "merged": true, "commit": commit, "changes": stat, "worktree": null }),
            Merged::Conflict { files } => json!({
                "merged": false,
                "conflicts": files,
                "worktree": found.id,
                "next": "Nothing was changed in the project's folder and the worktree is kept. Ask for the fix with code_task (same 'worktree'), e.g. to bring the project's current branch into it and resolve the conflicts, then merge again."
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;
    use crate::project::Project;

    /// A project in a real git repository with a worktree that has work (a new file) — everything the tool does is git's.
    struct Setup {
        outer: PathBuf,
        repo: PathBuf,
        id: String,
        tool: CodeWorktreeTool,
    }

    async fn setup(name: &str) -> Setup {
        let outer = std::env::temp_dir().join(format!("warden-code-worktree-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let repo = outer.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |dir: &Path, args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success(), "git {args:?}");
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@t"]);
        std::fs::write(repo.join("a.txt"), "one").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "first"]);
        let made = worktree::create(&repo).await.unwrap();
        std::fs::write(made.path.join("made.txt"), "by a task").unwrap();
        let vault = Arc::new(Vault::new(outer.join("vault")));
        ProjectStore::new(vault.clone()).save(&Project { id: "repo".into(), name: "Repo".into(), description: String::new(), instructions: String::new(), workdir: Some(repo.to_string_lossy().into_owned()), code: true }).unwrap();
        ProjectStore::new(vault.clone()).save(&Project { id: "notes".into(), name: "Notes".into(), description: String::new(), instructions: String::new(), workdir: None, code: false }).unwrap();
        Setup { outer, repo, id: made.id, tool: CodeWorktreeTool::new(vault) }
    }

    /// Answers every ask the same way and remembers what it was asked.
    struct Says {
        yes: bool,
        asked: Mutex<Vec<ApprovalRequest>>,
    }
    #[async_trait]
    impl Approver for Says {
        async fn approve(&self, request: ApprovalRequest) -> bool {
            self.asked.lock().unwrap().push(request);
            self.yes
        }
    }

    fn asked_by(tool: &CodeWorktreeTool, yes: bool) -> (Arc<dyn Tool>, Arc<Says>) {
        let says = Arc::new(Says { yes, asked: Mutex::default() });
        (tool.with_approver(says.clone()).unwrap(), says)
    }

    #[tokio::test]
    async fn looking_asks_nobody_and_changes_nothing() {
        let setup = setup("look").await;
        let listed = setup.tool.call(json!({ "action": "list", "project": "repo" })).await.unwrap();
        assert_eq!(listed["worktrees"], json!([{ "worktree": setup.id, "branch": format!("warden/{}", setup.id), "changed_files": 1, "commits": 0, "idle_days": 0, "stale": false }]));
        assert_eq!(listed["orphan_branches"], json!([]));
        let seen = setup.tool.call(json!({ "action": "diff", "project": "repo", "worktree": setup.id })).await.unwrap();
        assert!(seen["diff"].as_str().unwrap().contains("made.txt") && seen["diff"].as_str().unwrap().contains("+by a task"), "{seen}");
        assert!(!setup.repo.join("made.txt").exists());
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    #[tokio::test]
    async fn a_merge_asks_the_person_what_it_will_bring_and_a_no_changes_nothing() {
        let setup = setup("merge").await;
        let (no, says_no) = asked_by(&setup.tool, false);
        let err = no.call(json!({ "action": "merge", "project": "repo", "worktree": setup.id, "title": "add made" })).await.unwrap_err();
        assert!(err.to_string().contains("said no") && err.to_string().contains("Nothing was changed"), "{err}");
        {
            let asked = says_no.asked.lock().unwrap();
            assert_eq!((asked.len(), asked[0].action.as_str(), asked[0].target.as_str()), (1, "merge worktree", "Repo"));
            assert!(asked[0].detail.contains("made.txt") && asked[0].detail.contains("add made"), "the person is told what comes in: {}", asked[0].detail);
        }
        assert!(!setup.repo.join("made.txt").exists(), "and nothing came in");

        let (yes, says_yes) = asked_by(&setup.tool, true);
        let merged = yes.call(json!({ "action": "merge", "project": "repo", "worktree": setup.id, "title": "add made" })).await.unwrap();
        assert_eq!(merged["merged"], true);
        assert!(merged["changes"].as_str().unwrap().contains("made.txt") && merged["commit"].as_str().is_some(), "{merged}");
        assert_eq!(says_yes.asked.lock().unwrap().len(), 1);
        assert!(setup.repo.join("made.txt").is_file());
        assert_eq!(setup.tool.call(json!({ "action": "list", "project": "repo" })).await.unwrap()["worktrees"], json!([]), "the worktree is gone");
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    #[tokio::test]
    async fn discarding_asks_too_and_without_anyone_to_ask_neither_merge_nor_discard_runs() {
        let setup = setup("discard").await;
        for action in ["merge", "discard"] {
            let err = setup.tool.call(json!({ "action": action, "project": "repo", "worktree": setup.id })).await.unwrap_err();
            assert!(err.to_string().contains("can't ask"), "{action}: {err}");
        }
        let (no, _) = asked_by(&setup.tool, false);
        assert!(no.call(json!({ "action": "discard", "project": "repo", "worktree": setup.id })).await.is_err());
        assert_eq!(setup.tool.call(json!({ "action": "list", "project": "repo" })).await.unwrap()["worktrees"].as_array().unwrap().len(), 1, "still there");

        let (yes, says) = asked_by(&setup.tool, true);
        let done = yes.call(json!({ "action": "discard", "project": "repo", "worktree": setup.id })).await.unwrap();
        assert_eq!(done, json!({ "discarded": true, "worktree": setup.id }));
        assert_eq!(says.asked.lock().unwrap()[0].action, "discard worktree");
        assert_eq!(setup.tool.call(json!({ "action": "list", "project": "repo" })).await.unwrap()["worktrees"], json!([]));
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    #[tokio::test]
    async fn a_conflicting_merge_says_which_files_and_keeps_the_worktree() {
        let setup = setup("conflict").await;
        let made = worktree::open(&setup.repo, &setup.id).await.unwrap();
        std::fs::write(made.path.join("a.txt"), "the task's").unwrap();
        std::fs::write(setup.repo.join("a.txt"), "the person's").unwrap();
        assert!(std::process::Command::new("git").arg("-C").arg(&setup.repo).args(["commit", "-q", "-am", "mine"]).status().unwrap().success());
        let (yes, _) = asked_by(&setup.tool, true);
        let merged = yes.call(json!({ "action": "merge", "project": "repo", "worktree": setup.id })).await.unwrap();
        assert_eq!((merged["merged"].clone(), merged["conflicts"].clone(), merged["worktree"].clone()), (json!(false), json!(["a.txt"]), json!(setup.id)));
        assert!(merged["next"].as_str().unwrap().contains("code_task"));
        assert_eq!(std::fs::read_to_string(setup.repo.join("a.txt")).unwrap(), "the person's");
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    #[tokio::test]
    async fn a_worktree_a_task_is_still_working_in_cannot_be_merged_or_discarded() {
        let setup = setup("busy").await;
        let made = worktree::open(&setup.repo, &setup.id).await.unwrap();
        let held = crate::tool::code_task::hold_folder_for_test(&made.path.to_string_lossy());
        let (yes, says) = asked_by(&setup.tool, true);
        for action in ["merge", "discard"] {
            let err = yes.call(json!({ "action": action, "project": "repo", "worktree": setup.id })).await.unwrap_err();
            assert!(err.to_string().contains("still working"), "{action}: {err}");
        }
        assert!(says.asked.lock().unwrap().is_empty(), "the person isn't asked about something that can't happen now");
        drop(held);
        assert!(yes.call(json!({ "action": "discard", "project": "repo", "worktree": setup.id })).await.is_ok());
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    /// Makes everything the task touched in the worktree look `days` days old: the files and the folder.
    fn age(made: &Worktree, files: &[&str], days: u64) {
        let old = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
        for file in files {
            std::fs::File::open(made.path.join(file)).unwrap().set_modified(old).unwrap();
        }
        std::fs::File::open(&made.path).unwrap().set_modified(old).unwrap();
    }

    /// A worktree with a file the task made, aged by `days`.
    async fn worktree_aged(setup: &Setup, file: &str, days: u64) -> Worktree {
        let made = worktree::create(&setup.repo).await.unwrap();
        std::fs::write(made.path.join(file), "work").unwrap();
        age(&made, &[file], days);
        made
    }

    #[tokio::test]
    async fn a_listing_marks_the_forgotten_and_tidies_the_empty_ones_away_by_itself() {
        let setup = setup("tidy").await;
        let forgotten = worktree_aged(&setup, "old.txt", 20).await;
        let empty = worktree::create(&setup.repo).await.unwrap();
        age(&empty, &[], 3);
        let fresh_empty = worktree::create(&setup.repo).await.unwrap();

        let listed = setup.tool.call(json!({ "action": "list", "project": "repo" })).await.unwrap();
        let by_id = |id: &str| listed["worktrees"].as_array().unwrap().iter().find(|w| w["worktree"] == id).cloned();
        assert_eq!((by_id(&forgotten.id).unwrap()["stale"].clone(), by_id(&forgotten.id).unwrap()["idle_days"].clone()), (json!(true), json!(20)));
        assert_eq!(by_id(&setup.id).unwrap()["stale"], false, "work from a moment ago is not forgotten");
        assert!(by_id(&empty.id).is_none() && !empty.path.exists(), "an empty one, long left, is tidied away: nothing was lost");
        assert!(by_id(&fresh_empty.id).is_some(), "an empty one just made might be about to be used");
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    #[tokio::test]
    async fn pruning_asks_once_with_the_list_and_throws_away_only_what_has_sat_untouched_long_enough() {
        let setup = setup("prune").await;
        let old = worktree_aged(&setup, "old.txt", 20).await;
        let older = worktree_aged(&setup, "older.txt", 40).await;
        let middle = worktree_aged(&setup, "middle.txt", 8).await;

        // A no: nothing is touched, and the person saw what would go.
        let (no, says_no) = asked_by(&setup.tool, false);
        let err = no.call(json!({ "action": "prune", "project": "repo" })).await.unwrap_err();
        assert!(err.to_string().contains("said no"), "{err}");
        {
            let asked = says_no.asked.lock().unwrap();
            assert_eq!((asked.len(), asked[0].action.as_str()), (1, "discard forgotten worktrees"));
            let detail = &asked[0].detail;
            assert!(detail.contains(&old.id) && detail.contains(&older.id) && detail.contains("20 days") && detail.contains("40 days"), "{detail}");
            assert!(!detail.contains(&middle.id) && !detail.contains(&setup.id), "what is not forgotten isn't offered: {detail}");
        }
        assert!(old.path.is_dir() && older.path.is_dir());

        // A yes: the two forgotten go, in one go, and the rest stays.
        let (yes, says_yes) = asked_by(&setup.tool, true);
        let done = yes.call(json!({ "action": "prune", "project": "repo" })).await.unwrap();
        assert_eq!(says_yes.asked.lock().unwrap().len(), 1, "one question for the whole list");
        let mut pruned: Vec<String> = done["pruned"].as_array().unwrap().iter().map(|id| id.as_str().unwrap().to_string()).collect();
        pruned.sort();
        let mut expected = vec![old.id.clone(), older.id.clone()];
        expected.sort();
        assert_eq!(pruned, expected);
        assert!(!old.path.exists() && !older.path.exists() && middle.path.is_dir() && setup.repo.is_dir());

        // A shorter limit reaches the middle one; one that finds nothing doesn't bother the person.
        let again = yes.call(json!({ "action": "prune", "project": "repo", "older_than_days": 5 })).await.unwrap();
        assert_eq!(again["pruned"], json!([middle.id]));
        let (asked_none, says_none) = asked_by(&setup.tool, true);
        let nothing = asked_none.call(json!({ "action": "prune", "project": "repo", "older_than_days": 90 })).await.unwrap();
        assert_eq!(nothing["pruned"], json!([]));
        assert!(says_none.asked.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    #[tokio::test]
    async fn pruning_needs_someone_to_ask_leaves_a_worktree_in_use_and_refuses_a_nonsense_limit() {
        let setup = setup("prune-guards").await;
        let busy = worktree_aged(&setup, "old.txt", 20).await;
        let err = setup.tool.call(json!({ "action": "prune", "project": "repo" })).await.unwrap_err();
        assert!(err.to_string().contains("can't ask"), "{err}");
        assert!(busy.path.is_dir());

        let (yes, says) = asked_by(&setup.tool, true);
        for bad in [json!(0), json!(-3), json!("soon"), json!(1.5)] {
            let err = yes.call(json!({ "action": "prune", "project": "repo", "older_than_days": bad })).await.unwrap_err();
            assert!(err.to_string().contains("whole number of days"), "{bad}: {err}");
        }
        assert!(says.asked.lock().unwrap().is_empty());

        let _working = crate::tool::code_task::hold_folder_for_test(&busy.path.to_string_lossy());
        let done = yes.call(json!({ "action": "prune", "project": "repo" })).await.unwrap();
        assert_eq!((done["pruned"].clone(), done["skipped_busy"].clone()), (json!([]), json!([busy.id])), "a task is working in it");
        assert!(busy.path.is_dir());
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    #[tokio::test]
    async fn bad_asks_are_refused_with_what_is_valid() {
        let setup = setup("bad").await;
        let (yes, _) = asked_by(&setup.tool, true);
        for (args, why) in [
            (json!({ "action": "list", "project": "notes" }), "is not a code project"),
            (json!({ "action": "list", "project": "ghost" }), "must be one of: repo"),
            (json!({ "action": "burn", "project": "repo" }), "must be one of: list, diff, merge, discard"),
            (json!({ "action": "merge", "project": "repo" }), "needs the 'worktree' id"),
            (json!({ "action": "diff", "project": "repo", "worktree": "../repo" }), "isn't a worktree id"),
            (json!({ "action": "diff", "project": "repo", "worktree": "abcdef" }), "no worktree 'abcdef'"),
        ] {
            let err = yes.call(args).await.unwrap_err();
            assert!(err.to_string().contains(why), "{why}: {err}");
        }
        let spec = setup.tool.spec();
        assert_eq!(spec.parameters["properties"]["project"]["enum"], json!(["repo"]));
        std::fs::remove_dir_all(&setup.outer).ok();
    }

    #[tokio::test]
    async fn the_timers_sweep_removes_only_the_empty_forgotten_ones_and_looks_at_each_repository_once() {
        let setup = setup("sweep").await;
        let empty = worktree::create(&setup.repo).await.unwrap();
        // A second project over the same repository, and one that isn't code: neither adds a sweep or a failure.
        let workdir = Some(setup.repo.to_string_lossy().into_owned());
        let again = crate::project::Project { id: "again".into(), name: "Again".into(), description: String::new(), instructions: String::new(), workdir, code: true };
        setup.tool.projects.save(&again).unwrap();
        let later = SystemTime::now() + 30 * DAY;

        // A task is in the empty one: it stays, and it was asked about once, not once per project.
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let kept = tidy_projects(&setup.tool.projects, later, EMPTY_GRACE, &|_| {
            asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            true
        })
        .await;
        assert_eq!((kept, asked.load(std::sync::atomic::Ordering::SeqCst)), (Vec::<String>::new(), 1));
        assert!(empty.path.is_dir());

        // Too recent: the grace protects a worktree a task has just made.
        assert!(tidy_projects(&setup.tool.projects, SystemTime::now(), EMPTY_GRACE, &|_| false).await.is_empty());
        assert!(empty.path.is_dir());

        let removed = tidy_projects(&setup.tool.projects, later, EMPTY_GRACE, &|_| false).await;
        assert_eq!(removed, vec![empty.id.clone()]);
        assert!(!empty.path.is_dir());
        assert!(worktree::open(&setup.repo, &setup.id).await.is_ok(), "the one with work is untouched, however old");
        std::fs::remove_dir_all(&setup.outer).ok();
    }
}

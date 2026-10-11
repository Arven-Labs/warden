//! The hub's timer over the worktrees tasks left (P124), on a real hub and a real git repository: with a code engine an empty worktree
//! nobody came back to is tidied away by itself; without one nothing runs; work is never touched.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use support::MockProvider;
use warden_core::code_engine::{CodeEngine, CodeEvent, TurnOutcome, TurnRequest};
use warden_core::memory::Vault;
use warden_core::orchestrator::Orchestrator;
use warden_core::project::{Project, ProjectStore};
use warden_core::tool::Approver;
use warden_core::worktree;
use warden_server::Server;

/// An engine that is only there: the timer cares that the hub has one, not what it does.
struct Idle;

#[async_trait]
impl CodeEngine for Idle {
    async fn run_turn(&self, _: TurnRequest, _: Option<Arc<dyn Approver>>, _: &mut (dyn FnMut(CodeEvent) + Send)) -> anyhow::Result<TurnOutcome> {
        anyhow::bail!("not used")
    }

    async fn abort(&self, _: &str, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

fn git(dir: &Path, args: &[&str]) {
    assert!(std::process::Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success(), "git {args:?}");
}

/// A hub (with or without a code engine) over a code project in a real repository that has two worktrees: one empty and old, one with
/// work. Returns the paths of both.
async fn hub(name: &str, with_engine: bool) -> (PathBuf, PathBuf, PathBuf) {
    std::env::set_var(warden_core::memory::embed::OFF_SWITCH, "1");
    let outer = std::env::temp_dir().join(format!("warden-server-sweep-{name}-{}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()));
    let repo = outer.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "t"]);
    git(&repo, &["config", "user.email", "t@t"]);
    std::fs::write(repo.join("a.txt"), "one").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);

    let empty = worktree::create(&repo).await.unwrap();
    let with_work = worktree::create(&repo).await.unwrap();
    std::fs::write(with_work.path.join("made.txt"), "by a task").unwrap();
    // Left alone for longer than the grace: the folder says when it was last touched.
    std::fs::File::open(&empty.path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(3 * 60 * 60)).unwrap();
    std::fs::File::open(&with_work.path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(3 * 60 * 60)).unwrap();

    let vault = Arc::new(Vault::new(outer.join("vault")));
    ProjectStore::new(vault.clone())
        .save(&Project { id: "repo".into(), name: "Repo".into(), description: String::new(), instructions: String::new(), workdir: Some(repo.to_string_lossy().into_owned()), code: true })
        .unwrap();
    let orchestrator = Orchestrator::new(Arc::new(MockProvider::replying("hi")), vault);
    let mut server = Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), outer.join("conversations"), outer.join("devices.json"))
        .await
        .unwrap()
        .with_worktree_sweep(Duration::from_millis(100));
    if with_engine {
        server = server.with_code_engine(Arc::new(Idle));
    }
    tokio::spawn(server.serve());
    (outer, empty.path, with_work.path)
}

#[tokio::test]
async fn a_hub_with_a_code_engine_tidies_the_empty_forgotten_worktree_by_itself_and_never_the_one_with_work() {
    let (outer, empty, with_work) = hub("on", true).await;
    for _ in 0..100 {
        if !empty.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!empty.exists(), "the empty one was tidied away without anyone listing");
    assert!(with_work.join("made.txt").is_file(), "work is never touched");
    std::fs::remove_dir_all(&outer).ok();
}

#[tokio::test]
async fn a_hub_without_a_code_engine_leaves_the_worktrees_alone() {
    let (outer, empty, with_work) = hub("off", false).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(empty.exists() && with_work.exists(), "no engine, no task that could be in them: the timer doesn't run");
    std::fs::remove_dir_all(&outer).ok();
}

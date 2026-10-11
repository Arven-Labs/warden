//! A git worktree for one code task (P124): its own copy of the repository's files, on its own branch, so two tasks on the same
//! project can edit side by side instead of waiting for each other (`code_task`'s `isolate`).
//!
//! The worktrees live beside the repository, never in it (`<parent>/.warden-worktrees/<repository>/<id>`), so the person's own
//! tree isn't touched. A worktree starts from the last commit: what the main folder has changed and not committed isn't in it.
//! When the task ends one with no change is removed; one with work is kept, on the branch `warden/<id>`, for someone to look at
//! and merge — nothing here merges.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use rand::RngCore;
use tokio::process::Command;

/// How many changed files and commits a summary lists before it says "and more".
const SUMMARY_LINES: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Six hex digits; what a later task names to come back to this worktree.
    pub id: String,
    /// The repository the worktree belongs to (its top folder).
    repo: PathBuf,
    /// The top folder of the worktree.
    root: PathBuf,
    /// Where the engine works: the same subfolder of the worktree that the project has in the repository.
    pub path: PathBuf,
    pub branch: String,
}

/// What a task left in its worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// `false`: nothing was changed, and the worktree and its branch are gone.
    pub changed: bool,
    /// The files changed and the commits made, short; empty when nothing changed.
    pub summary: String,
}

fn branch_of(id: &str) -> String {
    format!("warden/{id}")
}

/// A name that can only be one of ours: nothing that reaches outside the worktrees folder.
fn is_id(text: &str) -> bool {
    text.len() == 6 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

async fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .map_err(|err| if err.kind() == std::io::ErrorKind::NotFound { anyhow!("git isn't installed on the machine running the task") } else { anyhow!("couldn't run git: {err}") })?;
    if !output.status.success() {
        bail!("`git {}` failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
}

/// The repository `workdir` is in (its top folder) and the path of `workdir` inside it.
async fn locate(workdir: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    let top = git(workdir, &["rev-parse", "--show-toplevel"]).await.map_err(|_| anyhow!("{} isn't in a git repository, so a task can't be given a worktree of its own", workdir.display()))?;
    let prefix = git(workdir, &["rev-parse", "--show-prefix"]).await?;
    Ok((PathBuf::from(top), PathBuf::from(prefix)))
}

fn container_of(repo: &Path) -> anyhow::Result<PathBuf> {
    let name = repo.file_name().ok_or_else(|| anyhow!("the repository {} has no folder name", repo.display()))?;
    let parent = repo.parent().ok_or_else(|| anyhow!("the repository {} has no parent folder", repo.display()))?;
    Ok(parent.join(".warden-worktrees").join(name))
}

/// A new worktree for a task in `workdir`, from the repository's last commit.
pub async fn create(workdir: &Path) -> anyhow::Result<Worktree> {
    let (repo, prefix) = locate(workdir).await?;
    git(&repo, &["rev-parse", "--verify", "HEAD"]).await.map_err(|_| anyhow!("the repository has no commit yet, so there is nothing for a worktree to start from"))?;
    let container = container_of(&repo)?;
    tokio::fs::create_dir_all(&container).await.with_context(|| format!("making {}", container.display()))?;
    let mut bytes = [0u8; 3];
    rand::thread_rng().fill_bytes(&mut bytes);
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let root = container.join(&id);
    git(&repo, &["worktree", "add", "-b", &branch_of(&id), &root.to_string_lossy(), "HEAD"]).await?;
    Ok(Worktree { path: root.join(prefix), id: id.clone(), repo, root, branch: branch_of(&id) })
}

/// A worktree an earlier task made, by its id.
pub async fn open(workdir: &Path, id: &str) -> anyhow::Result<Worktree> {
    if !is_id(id) {
        bail!("'{id}' isn't a worktree id (six hex digits, as an earlier code_task returned)");
    }
    let (repo, prefix) = locate(workdir).await?;
    let root = container_of(&repo)?.join(id);
    if !root.is_dir() {
        bail!("there is no worktree '{id}' for this project (it was removed, or it was made for another one)");
    }
    Ok(Worktree { path: root.join(prefix), id: id.to_string(), repo, root, branch: branch_of(id) })
}

/// Looks at what the task left. Nothing changed: the worktree and its branch are removed. Otherwise they stay, and the summary says what is in them.
pub async fn finish(worktree: &Worktree) -> anyhow::Result<Finished> {
    let status = git(&worktree.root, &["status", "--porcelain"]).await?;
    let range = format!("HEAD..{}", worktree.branch);
    let commits = git(&worktree.repo, &["log", "--oneline", &range]).await?;
    if status.is_empty() && commits.is_empty() {
        git(&worktree.repo, &["worktree", "remove", "--force", &worktree.root.to_string_lossy()]).await?;
        git(&worktree.repo, &["branch", "-D", &worktree.branch]).await?;
        return Ok(Finished { changed: false, summary: String::new() });
    }
    let mut lines: Vec<String> = Vec::new();
    if !commits.is_empty() {
        lines.push("commits:".to_string());
        lines.extend(commits.lines().map(|line| format!("  {line}")));
    }
    if !status.is_empty() {
        lines.push("not committed:".to_string());
        lines.extend(status.lines().map(|line| format!("  {line}")));
    }
    if lines.len() > SUMMARY_LINES {
        let more = lines.len() - SUMMARY_LINES;
        lines.truncate(SUMMARY_LINES);
        lines.push(format!("  … and {more} more lines"));
    }
    Ok(Finished { changed: true, summary: lines.join("\n") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("warden-worktree-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).status().unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// A repository with one commit and a subfolder, inside a folder of its own so the worktrees beside it are cleaned up too.
    fn repository(name: &str) -> (PathBuf, PathBuf) {
        let outer = scratch(name);
        let repo = outer.join("repo");
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        run(&repo, &["init", "-q"]);
        std::fs::write(repo.join("a.txt"), "one").unwrap();
        std::fs::write(repo.join("sub").join("b.txt"), "two").unwrap();
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-q", "-m", "first"]);
        (outer, repo)
    }

    #[tokio::test]
    async fn a_worktree_starts_from_the_last_commit_beside_the_repository_and_not_in_it() {
        let (outer, repo) = repository("create");
        std::fs::write(repo.join("a.txt"), "changed and not committed").unwrap();
        let worktree = create(&repo).await.unwrap();
        assert!(is_id(&worktree.id) && worktree.branch == format!("warden/{}", worktree.id));
        assert!(worktree.path.starts_with(outer.join(".warden-worktrees").join("repo")), "{:?}", worktree.path);
        assert!(!worktree.path.starts_with(&repo), "the person's tree isn't touched");
        assert_eq!(std::fs::read_to_string(worktree.path.join("a.txt")).unwrap(), "one", "what the main folder changed and didn't commit isn't there");
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_project_in_a_subfolder_works_in_the_same_subfolder_of_the_worktree() {
        let (outer, repo) = repository("subfolder");
        let worktree = create(&repo.join("sub")).await.unwrap();
        assert!(worktree.path.ends_with("sub") && worktree.path.join("b.txt").is_file(), "{:?}", worktree.path);
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_folder_with_no_repository_or_no_commit_is_refused_with_why() {
        let plain = scratch("plain");
        let err = create(&plain).await.unwrap_err();
        assert!(err.to_string().contains("isn't in a git repository"), "{err}");

        let empty = scratch("empty");
        run(&empty, &["init", "-q"]);
        let err = create(&empty).await.unwrap_err();
        assert!(err.to_string().contains("no commit yet"), "{err}");
        std::fs::remove_dir_all(&plain).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    #[tokio::test]
    async fn only_an_id_one_of_ours_made_can_be_opened() {
        let (outer, repo) = repository("open");
        let made = create(&repo).await.unwrap();
        assert_eq!(open(&repo, &made.id).await.unwrap(), made);
        for bad in ["../repo", "abc", "ABCDEF", "zzzzzz", ""] {
            let err = open(&repo, bad).await.unwrap_err();
            assert!(err.to_string().contains("isn't a worktree id"), "{bad}: {err}");
        }
        let err = open(&repo, "abcdef").await.unwrap_err();
        assert!(err.to_string().contains("no worktree 'abcdef'"), "{err}");
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_worktree_with_nothing_changed_goes_away_with_its_branch_and_one_with_work_stays() {
        let (outer, repo) = repository("finish");
        let idle = create(&repo).await.unwrap();
        assert_eq!(finish(&idle).await.unwrap(), Finished { changed: false, summary: String::new() });
        assert!(!idle.path.exists(), "the folder is gone");
        let branches = git(&repo, &["branch", "--list", &idle.branch]).await.unwrap();
        assert!(branches.is_empty(), "and so is the branch: {branches}");

        let busy = create(&repo).await.unwrap();
        std::fs::write(busy.path.join("a.txt"), "edited").unwrap();
        std::fs::write(busy.path.join("new.txt"), "new").unwrap();
        let kept = finish(&busy).await.unwrap();
        assert!(kept.changed && kept.summary.contains("not committed:") && kept.summary.contains("a.txt") && kept.summary.contains("new.txt"), "{}", kept.summary);
        assert!(busy.path.join("a.txt").is_file(), "the work is still there");

        // A commit alone counts as work, and a later look still finds the worktree by its id.
        let committed = create(&repo).await.unwrap();
        std::fs::write(committed.path.join("c.txt"), "c").unwrap();
        run(&committed.path, &["add", "."]);
        run(&committed.path, &["commit", "-q", "-m", "add c"]);
        let again = open(&repo, &committed.id).await.unwrap();
        let summary = finish(&again).await.unwrap();
        assert!(summary.changed && summary.summary.contains("commits:") && summary.summary.contains("add c") && !summary.summary.contains("not committed"), "{}", summary.summary);
        std::fs::remove_dir_all(&outer).ok();
    }
}

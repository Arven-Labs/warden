//! A git worktree for one code task (P124): its own copy of the repository's files, on its own branch, so two tasks on the same
//! project can edit side by side instead of waiting for each other (`code_task`'s `isolate`).
//!
//! The worktrees live beside the repository, never in it (`<parent>/.warden-worktrees/<repository>/<id>`), so the person's own
//! tree isn't touched. A worktree starts from the last commit: what the main folder has changed and not committed isn't in it.
//! When the task ends one with no change is removed; one with work is kept, on the branch `warden/<id>`, for someone to look at
//! and merge — nothing here merges.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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

/// One worktree of a project, as a listing shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub branch: String,
    /// Files changed and not committed.
    pub changed_files: usize,
    /// Commits on the branch the project's own branch doesn't have.
    pub commits: usize,
    /// Where a task would work in it (what `code_task` queues on).
    pub path: PathBuf,
    /// How long nothing has touched it: since the newest of its last commit, the files changed and the folder itself.
    pub idle: Duration,
}

impl Entry {
    /// Whether the task left anything in it: a changed file or a commit.
    pub fn has_work(&self) -> bool {
        self.changed_files > 0 || self.commits > 0
    }
}

/// The newest time anything touched the worktree: its folder, the files it has changed, and its last commit if it made some.
async fn last_touched(repo: &Path, root: &Path, branch: &str, status: &str, has_commits: bool) -> SystemTime {
    let modified = |path: PathBuf| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut newest = modified(root.to_path_buf()).unwrap_or(SystemTime::UNIX_EPOCH);
    for line in status.lines() {
        // `XY path`, or `XY old -> new` for a rename.
        let path = line.get(3..).unwrap_or_default().rsplit(" -> ").next().unwrap_or_default().trim_matches('"');
        if let Some(time) = modified(root.join(path)) {
            newest = newest.max(time);
        }
    }
    if has_commits {
        if let Some(seconds) = git(repo, &["log", "-1", "--format=%ct", branch]).await.ok().and_then(|text| text.trim().parse::<u64>().ok()) {
            newest = newest.max(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds));
        }
    }
    newest
}

/// The worktrees a project's repository has from tasks (kept because they have work, or because nobody looked yet), with how long each
/// has been left alone as of `now`.
pub async fn list(workdir: &Path, now: SystemTime) -> anyhow::Result<Vec<Entry>> {
    let (repo, prefix) = locate(workdir).await?;
    let container = container_of(&repo)?;
    let mut ids: Vec<String> = Vec::new();
    if let Ok(mut dir) = tokio::fs::read_dir(&container).await {
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_id(&name) && entry.path().is_dir() {
                ids.push(name);
            }
        }
    }
    ids.sort();
    let mut entries = Vec::new();
    for id in ids {
        let branch = branch_of(&id);
        let root = container.join(&id);
        let status = git(&root, &["status", "--porcelain"]).await.unwrap_or_default();
        let commits = git(&repo, &["log", "--oneline", &format!("HEAD..{branch}")]).await.unwrap_or_default();
        let touched = last_touched(&repo, &root, &branch, &status, !commits.is_empty()).await;
        let idle = now.duration_since(touched).unwrap_or_default();
        entries.push(Entry { path: root.join(&prefix), id, branch, changed_files: status.lines().count(), commits: commits.lines().count(), idle });
    }
    Ok(entries)
}

/// Branches of ours (`warden/<id>`) whose worktree folder is gone: whatever was committed on them is still there, but nothing shows it.
pub async fn orphan_branches(workdir: &Path) -> anyhow::Result<Vec<String>> {
    let (repo, _) = locate(workdir).await?;
    let container = container_of(&repo)?;
    let branches = git(&repo, &["for-each-ref", "--format=%(refname:short)", "refs/heads/warden/"]).await?;
    Ok(branches.lines().filter(|branch| branch.strip_prefix("warden/").is_some_and(|id| is_id(id) && !container.join(id).is_dir())).map(str::to_string).collect())
}

/// Tidies what tasks left that holds no work, and says which worktrees it removed. Never touches work:
/// - git's own record of worktrees whose folder was deleted by hand is dropped (`git worktree prune`);
/// - a worktree with no changed file and no commit, that nothing has touched for `grace` and no task is working in (`busy`), is removed
///   with its branch. The grace is for a task that has made its worktree and not yet started in it;
/// - a branch of ours with no folder is deleted only if it is already merged (`git branch -d` refuses anything else); one with commits
///   that aren't merged stays, and `orphan_branches` names it.
pub async fn sweep_empty(workdir: &Path, grace: Duration, now: SystemTime, busy: impl Fn(&Path) -> bool) -> anyhow::Result<Vec<String>> {
    let (repo, _) = locate(workdir).await?;
    git(&repo, &["worktree", "prune"]).await?;
    let mut removed = Vec::new();
    for entry in list(workdir, now).await? {
        if entry.has_work() || entry.idle < grace || busy(&entry.path) {
            continue;
        }
        let root = container_of(&repo)?.join(&entry.id);
        git(&repo, &["worktree", "remove", "--force", &root.to_string_lossy()]).await?;
        git(&repo, &["branch", "-D", &entry.branch]).await?;
        removed.push(entry.id);
    }
    for branch in orphan_branches(workdir).await? {
        let _ = git(&repo, &["branch", "-d", &branch]).await;
    }
    Ok(removed)
}

/// The worktrees with work that nothing has touched for `older_than`: the ones that look forgotten.
pub fn stale(entries: &[Entry], older_than: Duration) -> Vec<&Entry> {
    entries.iter().filter(|entry| entry.has_work() && entry.idle >= older_than).collect()
}

/// Everything the task changed in `worktree` (its commits and what is not committed yet) against where its branch left the project's
/// own: the files and the patch, the patch cut at `max_chars`.
pub async fn diff(worktree: &Worktree, max_chars: usize) -> anyhow::Result<String> {
    // Staging inside the worktree makes the files the task created show in the diff; it changes nothing the person has.
    git(&worktree.root, &["add", "-A"]).await?;
    let base = git(&worktree.repo, &["merge-base", "HEAD", &worktree.branch]).await?;
    let stat = git(&worktree.root, &["diff", "--cached", "--stat", &base]).await?;
    let patch = git(&worktree.root, &["diff", "--cached", &base]).await?;
    if stat.is_empty() && patch.is_empty() {
        return Ok("(no changes)".to_string());
    }
    let shown: String = patch.chars().take(max_chars).collect();
    let cut = if shown.len() < patch.len() { format!("\n… patch cut at {max_chars} characters; the files above are all of them") } else { String::new() };
    Ok(format!("{stat}\n\n{shown}{cut}"))
}

/// How a merge ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// In the project's branch, as `commit`; the worktree and its branch are gone.
    Done { commit: String, stat: String },
    /// The two sides changed the same lines. Nothing was changed in the project's folder, and the worktree is kept to be fixed.
    Conflict { files: Vec<String> },
}

/// Brings the task's work into the branch the project's folder has checked out, as a merge commit that keeps the task's own commits.
/// What the task left uncommitted is committed on its branch first. Refused, before anything is touched, when the project's folder has
/// changes of its own that aren't committed (a merge would mix them in) or no branch checked out.
pub async fn merge(worktree: &Worktree, title: &str) -> anyhow::Result<Merged> {
    git(&worktree.repo, &["symbolic-ref", "-q", "HEAD"]).await.map_err(|_| anyhow!("the project's folder has no branch checked out (a detached HEAD), so there is nothing to merge into"))?;
    let dirty = git(&worktree.repo, &["status", "--porcelain"]).await?;
    if !dirty.is_empty() {
        bail!("the project's folder has changes that aren't committed; commit or stash them first, so the merge doesn't mix them in:\n{dirty}");
    }
    git(&worktree.root, &["add", "-A"]).await?;
    if !git(&worktree.root, &["diff", "--cached", "--name-only"]).await?.is_empty() {
        git(&worktree.root, &["commit", "-q", "-m", &format!("warden: {title}")]).await.context("committing what the task left (are user.name and user.email set in git?)")?;
    }
    let message = format!("Merge {}: {title}", worktree.branch);
    match git(&worktree.repo, &["merge", "--no-ff", "-m", &message, &worktree.branch]).await {
        Ok(_) => {
            let commit = git(&worktree.repo, &["rev-parse", "--short", "HEAD"]).await?;
            let stat = git(&worktree.repo, &["show", "--stat", "--format=", "HEAD"]).await?;
            git(&worktree.repo, &["worktree", "remove", "--force", &worktree.root.to_string_lossy()]).await?;
            git(&worktree.repo, &["branch", "-d", &worktree.branch]).await?;
            Ok(Merged::Done { commit, stat })
        }
        Err(err) => {
            let files: Vec<String> = git(&worktree.repo, &["diff", "--name-only", "--diff-filter=U"]).await.unwrap_or_default().lines().map(str::to_string).collect();
            if files.is_empty() {
                return Err(err);
            }
            // Back to what the project's folder was before, whatever the merge left half done.
            let _ = git(&worktree.repo, &["merge", "--abort"]).await;
            Ok(Merged::Conflict { files })
        }
    }
}

/// Throws the task's work away: the worktree and its branch, with whatever is in them.
pub async fn discard(worktree: &Worktree) -> anyhow::Result<()> {
    git(&worktree.repo, &["worktree", "remove", "--force", &worktree.root.to_string_lossy()]).await?;
    git(&worktree.repo, &["branch", "-D", &worktree.branch]).await?;
    Ok(())
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
        // Its own identity, so what the module commits doesn't depend on the machine the tests run on.
        run(&repo, &["config", "user.name", "t"]);
        run(&repo, &["config", "user.email", "t@t"]);
        std::fs::write(repo.join("a.txt"), "one").unwrap();
        std::fs::write(repo.join("sub").join("b.txt"), "two").unwrap();
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-q", "-m", "first"]);
        (outer, repo)
    }

    fn out(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Makes `path` look as if it was last touched `ago` before `now`.
    fn touched(path: &Path, now: SystemTime, ago: Duration) {
        std::fs::File::open(path).unwrap().set_modified(now - ago).unwrap();
    }

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    #[tokio::test]
    async fn how_long_a_worktree_has_been_left_alone_follows_the_newest_thing_in_it() {
        let (outer, repo) = repository("idle");
        let now = SystemTime::now() + 30 * DAY;
        let empty = create(&repo).await.unwrap();
        let edited = create(&repo).await.unwrap();
        std::fs::write(edited.path.join("a.txt"), "edited").unwrap();
        touched(&edited.path.join("a.txt"), now, 2 * DAY);
        let committed = create(&repo).await.unwrap();
        std::fs::write(committed.path.join("c.txt"), "c").unwrap();
        run(&committed.path, &["add", "."]);
        run(&committed.path, &["commit", "-q", "-m", "add c"]);

        let listed = list(&repo, now).await.unwrap();
        let idle = |id: &str| listed.iter().find(|e| e.id == id).unwrap().idle;
        assert!(idle(&empty.id) >= 29 * DAY, "nothing has touched it since it was made: {:?}", idle(&empty.id));
        assert_eq!(idle(&edited.id).as_secs() / DAY.as_secs(), 2, "the file edited two days ago is the newest thing in it");
        assert!(idle(&committed.id) >= 29 * DAY && listed.iter().find(|e| e.id == committed.id).unwrap().has_work(), "a commit is work, and its time is when it was made");
        assert!(!listed.iter().find(|e| e.id == empty.id).unwrap().has_work());

        let forgotten = stale(&listed, 10 * DAY);
        assert_eq!(forgotten.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().len(), 1, "only work counts as forgotten, and only past the limit");
        assert_eq!(forgotten[0].id, committed.id);
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn tidying_removes_only_what_is_empty_old_enough_and_not_in_use() {
        let (outer, repo) = repository("sweep");
        let empty = create(&repo).await.unwrap();
        let busy = create(&repo).await.unwrap();
        let edited = create(&repo).await.unwrap();
        std::fs::write(edited.path.join("new.txt"), "work").unwrap();
        let committed = create(&repo).await.unwrap();
        std::fs::write(committed.path.join("c.txt"), "c").unwrap();
        run(&committed.path, &["add", "."]);
        run(&committed.path, &["commit", "-q", "-m", "add c"]);
        let hour = Duration::from_secs(60 * 60);

        // Just made: a task may be about to start in it, so nothing goes yet.
        let soon = SystemTime::now() + Duration::from_secs(10 * 60);
        assert!(sweep_empty(&repo, hour, soon, |_| false).await.unwrap().is_empty());
        assert!(empty.path.is_dir());

        // Long after: the empty one that nobody works in goes, with its branch. The busy, the edited and the committed stay.
        let later = SystemTime::now() + 30 * DAY;
        let busy_path = busy.path.clone();
        let removed = sweep_empty(&repo, hour, later, move |path| path == busy_path).await.unwrap();
        assert_eq!(removed, std::slice::from_ref(&empty.id));
        assert!(!empty.path.exists() && out(&repo, &["branch", "--list", &empty.branch]).is_empty());
        for kept in [&busy, &edited, &committed] {
            assert!(kept.path.is_dir() && !out(&repo, &["branch", "--list", &kept.branch]).is_empty(), "{} stays", kept.id);
        }
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_folder_deleted_by_hand_is_dropped_from_git_and_its_branch_goes_only_if_nothing_would_be_lost() {
        let (outer, repo) = repository("orphans");
        let no_work = create(&repo).await.unwrap();
        let with_work = create(&repo).await.unwrap();
        std::fs::write(with_work.path.join("c.txt"), "c").unwrap();
        run(&with_work.path, &["add", "."]);
        run(&with_work.path, &["commit", "-q", "-m", "add c"]);
        std::fs::remove_dir_all(&no_work.path).unwrap();
        std::fs::remove_dir_all(&with_work.path).unwrap();
        assert!(out(&repo, &["worktree", "list"]).contains(&no_work.id), "git still has them on its books");
        assert_eq!(orphan_branches(&repo).await.unwrap().len(), 2);

        sweep_empty(&repo, Duration::ZERO, SystemTime::now(), |_| false).await.unwrap();
        let books = out(&repo, &["worktree", "list"]);
        assert!(!books.contains(&no_work.id) && !books.contains(&with_work.id), "the records of folders that are gone are dropped:\n{books}");
        assert!(out(&repo, &["branch", "--list", &no_work.branch]).is_empty(), "nothing on it, so nothing is lost");
        assert!(!out(&repo, &["branch", "--list", &with_work.branch]).is_empty(), "its commit is work and stays");
        assert_eq!(orphan_branches(&repo).await.unwrap(), std::slice::from_ref(&with_work.branch), "and it is said, since nothing else shows it");
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn the_diff_shows_what_a_task_made_edited_and_committed_and_a_listing_says_which_worktrees_have_work() {
        let (outer, repo) = repository("diff");
        assert!(list(&repo, SystemTime::now()).await.unwrap().is_empty());
        let worktree = create(&repo).await.unwrap();
        assert_eq!(diff(&worktree, 10_000).await.unwrap(), "(no changes)");

        std::fs::write(worktree.path.join("a.txt"), "edited").unwrap();
        std::fs::write(worktree.path.join("new.txt"), "brand new").unwrap();
        std::fs::write(worktree.path.join("c.txt"), "committed").unwrap();
        run(&worktree.path, &["add", "c.txt"]);
        run(&worktree.path, &["commit", "-q", "-m", "add c"]);
        std::fs::write(worktree.path.join("late.txt"), "made after the commit").unwrap();
        let shown = diff(&worktree, 10_000).await.unwrap();
        for wanted in ["a.txt", "new.txt", "c.txt", "late.txt", "+brand new", "+edited"] {
            assert!(shown.contains(wanted), "{wanted} in\n{shown}");
        }
        let cut = diff(&worktree, 20).await.unwrap();
        assert!(cut.contains("patch cut at 20 characters") && cut.contains("new.txt"), "the files still all show:\n{cut}");

        let listed = list(&repo, SystemTime::now()).await.unwrap();
        assert_eq!(listed.iter().map(|e| (e.id.as_str(), e.branch.as_str(), e.changed_files, e.commits)).collect::<Vec<_>>(), [(worktree.id.as_str(), worktree.branch.as_str(), 3, 1)]);
        assert_eq!(listed[0].path, worktree.path);
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_merge_keeps_the_tasks_commits_commits_what_it_left_and_clears_the_worktree() {
        let (outer, repo) = repository("merge");
        let worktree = create(&repo).await.unwrap();
        std::fs::write(worktree.path.join("c.txt"), "committed").unwrap();
        run(&worktree.path, &["add", "."]);
        run(&worktree.path, &["commit", "-q", "-m", "add c"]);
        std::fs::write(worktree.path.join("late.txt"), "not committed").unwrap();

        let Merged::Done { commit, stat } = merge(&worktree, "add the files").await.unwrap() else { panic!("a merge with no conflict") };
        assert!(stat.contains("c.txt") && stat.contains("late.txt"), "{stat}");
        assert_eq!(out(&repo, &["rev-parse", "--short", "HEAD"]), commit);
        assert_eq!(out(&repo, &["rev-list", "--parents", "-n", "1", "HEAD"]).split_whitespace().count(), 3, "a merge commit: it and its two parents");
        assert!(out(&repo, &["log", "-1", "--format=%s"]).starts_with("Merge warden/"), "{}", out(&repo, &["log", "-1", "--format=%s"]));
        assert!(out(&repo, &["log", "--format=%s"]).contains("add c"), "the task's own commit is in the history");
        assert!(repo.join("c.txt").is_file() && repo.join("late.txt").is_file(), "and the files are in the project's folder");
        assert!(!worktree.path.exists() && out(&repo, &["branch", "--list", &worktree.branch]).is_empty(), "the worktree and its branch are gone");
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_conflict_leaves_the_projects_folder_as_it_was_and_keeps_the_worktree_to_be_fixed() {
        let (outer, repo) = repository("conflict");
        let worktree = create(&repo).await.unwrap();
        std::fs::write(worktree.path.join("a.txt"), "the task's version").unwrap();
        std::fs::write(repo.join("a.txt"), "the person's version").unwrap();
        run(&repo, &["commit", "-q", "-am", "the person changed it too"]);
        let head = out(&repo, &["rev-parse", "HEAD"]);

        let merged = merge(&worktree, "change a").await.unwrap();
        assert_eq!(merged, Merged::Conflict { files: vec!["a.txt".to_string()] });
        assert_eq!(out(&repo, &["rev-parse", "HEAD"]), head, "no merge commit");
        assert!(out(&repo, &["status", "--porcelain"]).is_empty(), "no half-done merge in the folder");
        assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "the person's version");
        assert!(worktree.path.join("a.txt").is_file(), "the worktree is kept");
        assert!(!out(&repo, &["branch", "--list", &worktree.branch]).is_empty());
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn a_merge_is_refused_before_anything_is_touched_when_the_folder_is_dirty_has_no_branch_or_git_has_no_name() {
        let (outer, repo) = repository("refused");
        let worktree = create(&repo).await.unwrap();
        std::fs::write(worktree.path.join("c.txt"), "work").unwrap();

        std::fs::write(repo.join("a.txt"), "the person is in the middle of this").unwrap();
        let err = merge(&worktree, "t").await.unwrap_err();
        assert!(err.to_string().contains("aren't committed") && err.to_string().contains("a.txt"), "{err}");
        run(&repo, &["checkout", "-q", "--", "a.txt"]);

        // No name for the commit of what the task left: said, and the worktree is as it was.
        run(&repo, &["config", "user.name", ""]);
        let err = merge(&worktree, "t").await.unwrap_err();
        assert!(format!("{err:#}").contains("user.name"), "{err:#}");
        run(&repo, &["config", "user.name", "t"]);

        let head = out(&repo, &["rev-parse", "HEAD"]);
        run(&repo, &["checkout", "-q", "--detach"]);
        let err = merge(&worktree, "t").await.unwrap_err();
        assert!(err.to_string().contains("no branch checked out"), "{err}");
        assert_eq!(out(&repo, &["rev-parse", "HEAD"]), head);
        assert!(worktree.path.join("c.txt").is_file());
        std::fs::remove_dir_all(&outer).ok();
    }

    #[tokio::test]
    async fn discarding_removes_the_worktree_the_branch_and_the_work() {
        let (outer, repo) = repository("discard");
        let worktree = create(&repo).await.unwrap();
        std::fs::write(worktree.path.join("c.txt"), "work").unwrap();
        discard(&worktree).await.unwrap();
        assert!(!worktree.path.exists() && out(&repo, &["branch", "--list", &worktree.branch]).is_empty());
        assert!(list(&repo, SystemTime::now()).await.unwrap().is_empty());
        std::fs::remove_dir_all(&outer).ok();
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

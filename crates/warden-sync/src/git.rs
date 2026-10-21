//! Sync via a self-hosted/remote git repo (P63) — a sibling engine to the Arweave/TruthID one in
//! `lib.rs`'s `SyncEngine`, for whoever doesn't want to depend on TruthID/Arweave to sync their
//! vault. Reuses `bundle`/`diff`/`manifest`/`pairing` completely unchanged — only the transport
//! differs: instead of publishing an encrypted blob via TruthID's `pin()`, each push becomes one
//! git commit carrying the same encrypted `SyncBundle` as an opaque file (`bundle.enc`), pushed to
//! a plain HTTPS remote. `SyncManifest` needed no new fields for this: `last_tx_id` is already a
//! generic "last applied version" string (Arweave tx id today, a commit sha here) and
//! `fold_into_manifest` never inspects its shape.
//!
//! **Why shell out to `git`** rather than `git2`/`gix`: `git2` (libgit2) drags in OpenSSL/libssh2
//! native deps, at odds with the workspace's rustls-only posture and the exact cross-compile pain
//! already documented for `openssl-sys` on Android (P53/56); `gix` is pure Rust but its push
//! support was historically less mature — reconsider once mobile needs this (shelling out doesn't
//! work there). Shelling out also inherits whatever credential/SSH setup the user already has in
//! `~/.gitconfig` for free. Trade-off accepted: v1 is desktop/CLI only, wherever an installable
//! `git` exists.
//!
//! **Auth**: HTTPS + a personal access token, injected as `x-access-token:<token>@host` directly
//! into the URL passed as a *positional argument* to `ls-remote`/`fetch`/`push` — never via
//! `git remote add`, so the token never lands in this repo's `.git/config`. Any git stderr that
//! might echo the authenticated URL back (git does this on some failures) has the token redacted
//! before it becomes part of an `anyhow::Error`.
//!
//! **Branch**: always `main`, regardless of the remote's configured default branch — this engine
//! never touches any other ref.

use std::path::{Path, PathBuf};
use std::process::Command;

use warden_core::memory::Vault;

use crate::bundle::{self, ConfigChange};
use crate::diff;
use crate::manifest::{self, SyncDirection, SyncSecrets};
use crate::oauth;
use crate::syncignore::SyncIgnore;

const BUNDLE_FILE_NAME: &str = "bundle.enc";
const BRANCH: &str = "main";

#[derive(Debug)]
pub struct GitPushOutcome {
    pub commit_sha: String,
    pub files_changed: usize,
    pub config_changed: bool,
    /// MCP OAuth logins sent, new, changed or deleted (P54).
    pub oauth_files: usize,
}

#[derive(Debug)]
pub struct GitPullOutcome {
    pub commits_applied: usize,
    pub files_written: usize,
    pub files_deleted: usize,
    /// Entries some replayed commit carried but this device's own `.syncignore` (P75) excluded.
    pub files_ignored: usize,
    pub config_updated: bool,
    /// MCP OAuth logins written or removed (P54).
    pub oauth_files: usize,
    /// File names of those logins (`<server>.json`), once each, so a host can reconnect the MCP servers they belong to.
    pub oauth_logins: Vec<String>,
    pub warnings: Vec<String>,
}

impl GitPullOutcome {
    /// Whether whoever built an orchestrator before this pull should rebuild it: a new `config.toml`, or an MCP OAuth login
    /// that arrived (P54). Same rule as `SyncReport::needs_reload` in `warden-bootstrap`, for the pulls a person starts by hand.
    pub fn needs_reload(&self) -> bool {
        self.config_updated || !self.oauth_logins.is_empty()
    }

    fn up_to_date(warning: &str) -> Self {
        Self { commits_applied: 0, files_written: 0, files_deleted: 0, files_ignored: 0, config_updated: false, oauth_files: 0, oauth_logins: Vec::new(), warnings: vec![warning.to_string()] }
    }
}

pub struct GitSyncEngine {
    vault: Vault,
    config_path: PathBuf,
    secrets_path: PathBuf,
    manifest_path: PathBuf,
    local_repo_path: PathBuf,
    /// The bare remote URL, no credentials — e.g. `"https://gitea.example.com/user/vault.git"`.
    /// A non-`https://` value (a plain filesystem path, or `file://...`) is passed through
    /// untouched, no credential injected — what this crate's own tests use against a real local
    /// bare repo, same "hermetic against the real protocol" spirit as `fake_arweave_gateway.rs`.
    remote_url: String,
    token: String,
    /// Where the MCP OAuth logins live (P54); `None` leaves them out of both directions.
    oauth_dir: Option<PathBuf>,
}

impl GitSyncEngine {
    pub fn new(
        vault_root: PathBuf,
        config_path: PathBuf,
        secrets_path: PathBuf,
        manifest_path: PathBuf,
        local_repo_path: PathBuf,
        remote_url: String,
        token: String,
    ) -> Self {
        Self { vault: Vault::new(vault_root), config_path, secrets_path, manifest_path, local_repo_path, remote_url, token, oauth_dir: None }
    }

    /// Syncs the MCP OAuth logins in `dir` (one `<server>.json` each) along with the vault and `config.toml`. Off by default, so
    /// tests never touch the real config folder.
    pub fn with_oauth_dir(mut self, dir: PathBuf) -> Self {
        self.oauth_dir = Some(dir);
        self
    }

    fn load_secrets(&self) -> anyhow::Result<SyncSecrets> {
        manifest::load_secrets(&self.secrets_path)?.ok_or_else(|| {
            anyhow::anyhow!("sync has not been initialized on this device yet — run init_fresh or pair with an existing device first")
        })
    }

    /// Computes the diff, builds+encrypts the bundle, and pushes it as one commit. Returns `None`
    /// if neither the vault nor `config.toml` changed since the last successful sync — same
    /// early-out as `push::begin_push`. On a non-fast-forward rejection, returns a clear error
    /// pointing at `pull()` instead of attempting any merge.
    pub async fn push(&self) -> anyhow::Result<Option<GitPushOutcome>> {
        let _one_sync_at_a_time = crate::lock::SyncLock::acquire(&crate::lock::lock_path_for(&self.manifest_path)).await?;
        let secrets = self.load_secrets()?;
        let manifest = manifest::load_manifest(&self.manifest_path)?;

        let diff = diff::diff_vault(&self.vault, &manifest)?;
        let config_bytes = std::fs::read(&self.config_path).ok();
        let config_changed = diff::config_changed(config_bytes.as_deref(), &manifest);
        let oauth_diff = oauth::diff_oauth(self.oauth_dir.as_deref(), &manifest)?;
        if diff.is_empty() && !config_changed && oauth_diff.is_empty() {
            return Ok(None);
        }

        let config_change = if !config_changed {
            None
        } else {
            match config_bytes {
                Some(bytes) => Some(ConfigChange::Updated(bytes)),
                None => Some(ConfigChange::Deleted),
            }
        };

        let next_counter = manifest.manifest_counter + 1;
        let mut bundle = bundle::build_bundle(&self.vault, config_change, &diff, next_counter, &secrets.device_id)?;
        oauth::attach(&mut bundle, self.oauth_dir.as_deref(), &oauth_diff)?;
        let encrypted = bundle::encrypt_bundle(&bundle, &secrets.vault_key)?;

        ensure_local_repo(&self.local_repo_path, &self.remote_url)?;
        // Deliberately does NOT fetch/fast-forward onto the remote's current head first: local
        // `main` is reset to exactly what *this device* last applied (`manifest.last_tx_id`, `None`
        // meaning "nothing yet" — a fresh orphan history), so `git push` only succeeds when that
        // really is still the remote's tip. If someone else pushed meanwhile, the push below is
        // rejected by git itself (non-fast-forward) — see module docs for why no auto-merge.
        checkout_local_main_at_known_position(&self.local_repo_path, manifest.last_tx_id.as_deref())?;

        std::fs::write(self.local_repo_path.join(BUNDLE_FILE_NAME), &encrypted)?;
        run_git(&self.local_repo_path, &["add", BUNDLE_FILE_NAME])?;
        run_git(&self.local_repo_path, &["commit", "-m", &format!("sync #{next_counter}")])?;
        let commit_sha = run_git(&self.local_repo_path, &["rev-parse", "HEAD"])?.trim().to_string();

        let push_url = maybe_authenticated_url(&self.remote_url, &self.token);
        let refspec = format!("{BRANCH}:{BRANCH}");
        if let Err(err) = run_git_authenticated(&self.local_repo_path, &["push", &push_url, &refspec], &self.token) {
            let text = err.to_string();
            if text.contains("rejected") || text.contains("fetch first") || text.contains("non-fast-forward") {
                anyhow::bail!("push rejected — another device published first; run pull and try again");
            }
            return Err(err);
        }

        let new_manifest = bundle::fold_into_manifest(manifest, &bundle, commit_sha.clone(), SyncDirection::Push)?;
        manifest::save_manifest(&self.manifest_path, &new_manifest)?;
        let outcome = GitPushOutcome {
            commit_sha,
            files_changed: bundle.vault_files.len() + bundle.deleted_vault_files.len(),
            config_changed: bundle.config_toml.is_some() || bundle.config_deleted,
            oauth_files: bundle.oauth_files.len() + bundle.deleted_oauth_files.len(),
        };
        Ok(Some(outcome))
    }

    /// Fetches `main`, then replays every commit this device hasn't applied yet (in order) onto
    /// the vault/config — a full replay from the root when `manifest.last_tx_id` is `None` or is
    /// no longer a recognized ancestor (a fresh/paired device that never synced from this backend
    /// before), otherwise just the commits after it. See module docs for why this — not "just the
    /// latest" — is what resolves the new-device gap Arweave's owner-based discovery has.
    pub async fn pull(&self) -> anyhow::Result<GitPullOutcome> {
        let _one_sync_at_a_time = crate::lock::SyncLock::acquire(&crate::lock::lock_path_for(&self.manifest_path)).await?;
        let secrets = self.load_secrets()?;
        let manifest = manifest::load_manifest(&self.manifest_path)?;

        ensure_local_repo(&self.local_repo_path, &self.remote_url)?;

        let Some(remote_head) = ls_remote_head(&self.local_repo_path, &self.remote_url, &self.token, BRANCH)? else {
            return Ok(GitPullOutcome::up_to_date("nenhum snapshot foi publicado ainda"));
        };

        if manifest.last_tx_id.as_deref() == Some(remote_head.as_str()) {
            return Ok(GitPullOutcome::up_to_date("already up to date"));
        }

        let fetch_url = maybe_authenticated_url(&self.remote_url, &self.token);
        run_git_authenticated(&self.local_repo_path, &["fetch", &fetch_url, BRANCH], &self.token)?;

        let shas_to_replay = commits_to_replay(&self.local_repo_path, manifest.last_tx_id.as_deref())?;
        if shas_to_replay.is_empty() {
            return Ok(GitPullOutcome::up_to_date("already up to date"));
        }

        let ignore = SyncIgnore::load(&self.vault)?;
        let mut current_manifest = manifest;
        let mut files_written = 0;
        let mut files_deleted = 0;
        let mut files_ignored = 0;
        let mut config_updated = false;
        let mut oauth_files = 0;
        let mut oauth_logins: Vec<String> = Vec::new();
        let mut warnings = Vec::new();

        for sha in &shas_to_replay {
            let blob = run_git_bytes(&self.local_repo_path, &["show", &format!("{sha}:{BUNDLE_FILE_NAME}")])?;
            let decoded = bundle::decrypt_bundle(&blob, &secrets.vault_key)?;

            for relative in decoded.vault_files.keys().chain(decoded.deleted_vault_files.iter()) {
                if ignore.matches(relative) {
                    continue;
                }
                let current_hash = std::fs::read(self.vault.root().join(relative)).ok().map(|bytes| diff::sha256_hex(&bytes));
                let known_hash = current_manifest.vault_files.get(relative).cloned();
                if current_hash != known_hash {
                    warnings.push(format!("local change to {relative} was overwritten by the pull"));
                }
            }
            if decoded.config_toml.is_some() || decoded.config_deleted {
                let current_hash = std::fs::read(&self.config_path).ok().map(|bytes| diff::sha256_hex(&bytes));
                if current_hash != current_manifest.config_hash {
                    warnings.push("local change to config.toml was overwritten by the pull".to_string());
                }
            }

            warnings.extend(oauth::overwrite_warnings(&decoded, self.oauth_dir.as_deref(), &current_manifest));

            let report = bundle::apply_bundle(&decoded, &self.vault, &self.config_path)?;
            let touched = oauth::apply(&decoded, self.oauth_dir.as_deref())?;
            oauth_files += touched.len();
            for name in touched {
                if !oauth_logins.contains(&name) {
                    oauth_logins.push(name);
                }
            }
            files_written += report.files_written;
            files_deleted += report.files_deleted;
            files_ignored += report.files_ignored;
            config_updated = config_updated || report.config_updated || report.config_deleted;
            current_manifest = bundle::fold_into_manifest(current_manifest, &decoded, sha.clone(), SyncDirection::Pull)?;
        }

        manifest::save_manifest(&self.manifest_path, &current_manifest)?;
        let outcome =
            GitPullOutcome { commits_applied: shas_to_replay.len(), files_written, files_deleted, files_ignored, config_updated, oauth_files, oauth_logins, warnings };
        Ok(outcome)
    }
}

/// `git init` (idempotent — a no-op if `.git` already exists) plus a local identity so `commit`
/// never fails with "please tell me who you are" on a machine with no global git config, and a
/// bare `origin` remote (credential-free — see module docs) so the repo is inspectable with plain
/// `git` commands by whoever debugs it.
fn ensure_local_repo(local_repo_path: &Path, remote_url: &str) -> anyhow::Result<()> {
    if !local_repo_path.join(".git").exists() {
        std::fs::create_dir_all(local_repo_path)?;
        run_git(local_repo_path, &["init"])?;
        run_git(local_repo_path, &["config", "user.name", "Warden Sync"])?;
        run_git(local_repo_path, &["config", "user.email", "sync@warden.local"])?;
        run_git(local_repo_path, &["remote", "add", "origin", remote_url])?;
    }
    Ok(())
}

/// Positions local `main` at exactly what *this device* last applied — `Some(sha)` (set by a
/// previous `push`/`pull`) checks `main` out at that exact commit; `None` (never synced from this
/// backend before) starts a brand new orphan history. Deliberately never looks at the remote's
/// actual current tip: that's what makes `git push` afterwards a trustworthy fast-forward check
/// instead of something this engine second-guesses itself.
fn checkout_local_main_at_known_position(local_repo_path: &Path, known_sha: Option<&str>) -> anyhow::Result<()> {
    match known_sha {
        Some(sha) => {
            run_git(local_repo_path, &["checkout", "-B", BRANCH, sha])?;
        }
        None => {
            run_git(local_repo_path, &["checkout", "--orphan", BRANCH])?;
            // Clears the index of whatever a previous (rejected, or different-branch) attempt
            // left staged — harmless no-op error when there's nothing tracked yet.
            let _ = run_git(local_repo_path, &["rm", "-rf", "--cached", "."]);
        }
    }
    Ok(())
}

/// The sha `refs/heads/<branch>` points to on the remote, or `None` if that branch doesn't exist
/// there yet (a brand-new repo with zero pushes). A lightweight network call — no full fetch.
fn ls_remote_head(local_repo_path: &Path, remote_url: &str, token: &str, branch: &str) -> anyhow::Result<Option<String>> {
    let url = maybe_authenticated_url(remote_url, token);
    let output = run_git_authenticated(local_repo_path, &["ls-remote", &url, &format!("refs/heads/{branch}")], token)?;
    Ok(output.split_whitespace().next().map(str::to_string))
}

/// Which commit shas (oldest first) still need replaying onto this device. `None`/an
/// unrecognized `known_sha` (not an ancestor of the current `main`, e.g. a fresh device that only
/// has a vault key from pairing) means the full history, same as a first-ever pull; otherwise just
/// what came after it.
fn commits_to_replay(local_repo_path: &Path, known_sha: Option<&str>) -> anyhow::Result<Vec<String>> {
    let is_known_ancestor = match known_sha {
        Some(sha) => Command::new("git")
            .current_dir(local_repo_path)
            .args(["merge-base", "--is-ancestor", sha, "FETCH_HEAD"])
            .status()
            .map(|status| status.success())
            .unwrap_or(false),
        None => false,
    };

    let range = if is_known_ancestor { format!("{}..FETCH_HEAD", known_sha.unwrap()) } else { "FETCH_HEAD".to_string() };
    let output = run_git(local_repo_path, &["rev-list", "--reverse", &range])?;
    Ok(output.lines().map(str::to_string).filter(|line| !line.is_empty()).collect())
}

/// Injects `x-access-token:<token>@` right after the scheme for an `https://` remote; anything
/// else (a plain filesystem path, `file://...`) passes through untouched — no credential to add,
/// and it's exactly what this module's own tests point at a real local bare repo with.
fn maybe_authenticated_url(remote_url: &str, token: &str) -> String {
    if token.is_empty() {
        return remote_url.to_string();
    }
    match remote_url.strip_prefix("https://") {
        Some(rest) => format!("https://x-access-token:{token}@{rest}"),
        None => remote_url.to_string(),
    }
}

/// Replaces every occurrence of `token` in `text` — used to scrub it out of `git`'s stderr before
/// it becomes part of an error message, since git sometimes echoes the URL it failed to reach
/// (including any embedded credential) back in "fatal: unable to access '...'" text.
fn redact(text: &str, token: &str) -> String {
    if token.is_empty() {
        text.to_string()
    } else {
        text.replace(token, "***")
    }
}

fn run_git(repo_dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    run_git_in(repo_dir, args)
}

fn run_git_authenticated(repo_dir: &Path, args: &[&str], token: &str) -> anyhow::Result<String> {
    run_git_in(repo_dir, args).map_err(|err| anyhow::anyhow!("{}", redact(&err.to_string(), token)))
}

fn run_git_in(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git").current_dir(dir).args(args).output().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("git not found on PATH — install git to use sync through a git remote")
        } else {
            anyhow::anyhow!("failed to run git {}: {err}", args.join(" "))
        }
    })?;

    if !output.status.success() {
        anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Same as `run_git_in`, but returns raw stdout bytes instead of a `String` — needed for
/// `git show <sha>:bundle.enc`, whose output is the encrypted (binary) blob, not UTF-8 text.
fn run_git_bytes(dir: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("git").current_dir(dir).args(args).output().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("git not found on PATH — install git to use sync through a git remote")
        } else {
            anyhow::anyhow!("failed to run git {}: {err}", args.join(" "))
        }
    })?;
    if !output.status.success() {
        anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_git_pull_needs_a_reload_for_a_new_config_or_a_login_and_for_nothing_else() {
        let mut outcome = GitPullOutcome::up_to_date("nothing new");
        assert!(!outcome.needs_reload());
        outcome.files_written = 3;
        assert!(!outcome.needs_reload(), "notes alone do not change what the orchestrator holds");
        outcome.config_updated = true;
        assert!(outcome.needs_reload());
        outcome.config_updated = false;
        outcome.oauth_logins = vec!["slack.json".to_string()];
        assert!(outcome.needs_reload(), "a login alone is enough");
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "warden-sync-git-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    /// A real bare git repo on local disk — this module's own tests point `GitSyncEngine` straight
    /// at it (no token needed for a local path), same "hermetic against the real protocol, not a
    /// mock" spirit `fake_arweave_gateway.rs` already uses in this crate.
    fn bare_remote(suffix: &str) -> PathBuf {
        let path = temp_dir(&format!("remote-{suffix}"));
        run_git_in(&std::env::temp_dir(), &["init", "--bare", &path.to_string_lossy()]).unwrap();
        path
    }

    fn engine(remote: &Path, suffix: &str) -> (GitSyncEngine, SyncSecrets, PathBuf) {
        let secrets = manifest::generate_secrets();
        let secrets_path = temp_dir(&format!("secrets-{suffix}"));
        manifest::save_secrets(&secrets_path, &secrets).unwrap();
        let manifest_path = temp_dir(&format!("manifest-{suffix}.json"));
        let engine = GitSyncEngine::new(
            temp_dir(&format!("vault-{suffix}")),
            temp_dir(&format!("config-{suffix}.toml")),
            secrets_path,
            manifest_path.clone(),
            temp_dir(&format!("repo-{suffix}")),
            remote.to_string_lossy().to_string(),
            String::new(),
        );
        (engine, secrets, manifest_path)
    }

    /// Two engines sharing the same `vault_key` (as pairing would give them) but independent
    /// vault/manifest/local-repo paths — simulates two separate devices talking to the same
    /// remote, the way `fake_pairing_peer.rs`-style tests simulate two ends of a protocol.
    fn paired_engine(remote: &Path, suffix: &str, vault_key: [u8; 32], device_id: &str) -> GitSyncEngine {
        let secrets = SyncSecrets { version: 1, device_id: device_id.to_string(), vault_key };
        let secrets_path = temp_dir(&format!("secrets-{suffix}"));
        manifest::save_secrets(&secrets_path, &secrets).unwrap();
        GitSyncEngine::new(
            temp_dir(&format!("vault-{suffix}")),
            temp_dir(&format!("config-{suffix}.toml")),
            secrets_path,
            temp_dir(&format!("manifest-{suffix}.json")),
            temp_dir(&format!("repo-{suffix}")),
            remote.to_string_lossy().to_string(),
            String::new(),
        )
    }

    #[tokio::test]
    async fn push_and_pull_wait_for_another_process_syncing_the_same_vault() {
        let remote = bare_remote("locked");
        let (engine, _, manifest_path) = engine(&remote, "locked");
        engine.vault.write("a.md", "hello").unwrap();
        // Another process (a desktop button, `warden-server sync now`) mid-round.
        let held = crate::lock::SyncLock::acquire(&crate::lock::lock_path_for(&manifest_path)).await.unwrap();

        let push = engine.push();
        tokio::pin!(push);
        assert!(tokio::time::timeout(std::time::Duration::from_millis(400), &mut push).await.is_err(), "push ran under someone else's round");
        drop(held);
        assert!(push.await.unwrap().is_some());

        let held = crate::lock::SyncLock::acquire(&crate::lock::lock_path_for(&manifest_path)).await.unwrap();
        let pull = engine.pull();
        tokio::pin!(pull);
        assert!(tokio::time::timeout(std::time::Duration::from_millis(400), &mut pull).await.is_err(), "pull ran under someone else's round");
        drop(held);
        pull.await.unwrap();

        // A push and a pull at once (the loop and a button): one after the other, both fine.
        engine.vault.write("b.md", "again").unwrap();
        let (pushed, pulled) = tokio::join!(engine.push(), engine.pull());
        assert!(pushed.unwrap().is_some());
        pulled.unwrap();
    }

    #[tokio::test]
    async fn push_is_none_when_nothing_changed() {
        let remote = bare_remote("nochange");
        let (engine, _, _) = engine(&remote, "nochange");
        assert!(engine.push().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn push_creates_a_commit_and_updates_the_manifest() {
        let remote = bare_remote("push");
        let (engine, _, manifest_path) = engine(&remote, "push");
        engine.vault.write("a.md", "hello").unwrap();

        let outcome = engine.push().await.unwrap().unwrap();
        assert_eq!(outcome.files_changed, 1);
        let new_manifest = manifest::load_manifest(&manifest_path).unwrap();
        assert_eq!(new_manifest.last_tx_id.as_deref(), Some(outcome.commit_sha.as_str()));
        assert_eq!(new_manifest.manifest_counter, 1);

        let log = run_git_in(&remote, &["log", "--oneline", BRANCH]).unwrap();
        assert!(log.contains("sync #1"));
    }

    #[tokio::test]
    async fn pull_from_an_empty_remote_is_a_noop() {
        let remote = bare_remote("empty-pull");
        let (engine, _, _) = engine(&remote, "empty-pull");
        let outcome = engine.pull().await.unwrap();
        assert_eq!(outcome.commits_applied, 0);
        assert!(outcome.warnings.iter().any(|w| w.contains("nenhum snapshot")));
    }

    #[tokio::test]
    async fn a_fresh_device_pull_replays_the_full_history() {
        let remote = bare_remote("history");
        let vault_key = [9u8; 32];

        let device_a = paired_engine(&remote, "history-a", vault_key, "device-a");
        device_a.vault.write("a.md", "one").unwrap();
        device_a.push().await.unwrap().unwrap();
        device_a.vault.write("b.md", "two").unwrap();
        device_a.push().await.unwrap().unwrap();

        // Device B has the same vault key (as pairing would give it) but never synced before.
        let device_b = paired_engine(&remote, "history-b", vault_key, "device-b");
        let outcome = device_b.pull().await.unwrap();
        assert_eq!(outcome.commits_applied, 2);
        assert_eq!(outcome.files_written, 2);
        assert_eq!(device_b.vault.read("a.md").unwrap(), "one");
        assert_eq!(device_b.vault.read("b.md").unwrap(), "two");
    }

    /// P54: a login saved on one device reaches a fresh one, a change to it alone is enough to push, and removing it removes it
    /// there too; a login edited here since the last sync is warned about before the pull overwrites it.
    #[tokio::test]
    async fn the_mcp_oauth_logins_follow_between_devices() {
        let remote = bare_remote("oauth");
        let vault_key = [9u8; 32];
        let logins = |suffix: &str| temp_dir(&format!("oauth-{suffix}"));

        let (a_dir, b_dir) = (logins("a"), logins("b"));
        let device_a = paired_engine(&remote, "oauth-a", vault_key, "device-a").with_oauth_dir(a_dir.clone());
        let device_b = paired_engine(&remote, "oauth-b", vault_key, "device-b").with_oauth_dir(b_dir.clone());

        // Only a login to send: that alone is a change.
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::write(a_dir.join("slack.json"), "t1").unwrap();
        let pushed = device_a.push().await.unwrap().expect("a login is a change");
        assert_eq!((pushed.oauth_files, pushed.files_changed, pushed.config_changed), (1, 0, false));
        assert!(device_a.push().await.unwrap().is_none(), "nothing more to send");

        let pulled = device_b.pull().await.unwrap();
        assert_eq!(pulled.oauth_files, 1);
        assert_eq!(pulled.oauth_logins, ["slack.json"], "a host reconnects the server this login belongs to");
        assert_eq!(std::fs::read_to_string(b_dir.join("slack.json")).unwrap(), "t1");
        assert!(pulled.warnings.is_empty());

        // A renews the token; B edits the same login meanwhile, so the pull overwrites it with a warning.
        std::fs::write(a_dir.join("slack.json"), "t2").unwrap();
        device_a.push().await.unwrap().unwrap();
        std::fs::write(b_dir.join("slack.json"), "edited on b").unwrap();
        let pulled = device_b.pull().await.unwrap();
        assert_eq!(std::fs::read_to_string(b_dir.join("slack.json")).unwrap(), "t2");
        assert_eq!(pulled.warnings, ["local change to the MCP login slack was overwritten by the pull"]);

        // A disconnects the server: it goes from B too.
        std::fs::remove_file(a_dir.join("slack.json")).unwrap();
        let pushed = device_a.push().await.unwrap().unwrap();
        assert_eq!(pushed.oauth_files, 1);
        let pulled = device_b.pull().await.unwrap();
        assert_eq!(pulled.oauth_logins, ["slack.json"], "a removed login counts too");
        assert!(!b_dir.join("slack.json").exists());
    }

    /// A device with no login folder neither sends the other's logins nor loses any: the engine without `with_oauth_dir`.
    #[tokio::test]
    async fn an_engine_without_a_login_folder_leaves_the_logins_alone() {
        let remote = bare_remote("oauth-off");
        let vault_key = [9u8; 32];
        let a_dir = temp_dir("oauth-off-a");
        let device_a = paired_engine(&remote, "oauth-off-a", vault_key, "device-a").with_oauth_dir(a_dir.clone());
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::write(a_dir.join("slack.json"), "t1").unwrap();
        device_a.push().await.unwrap().unwrap();

        let device_b = paired_engine(&remote, "oauth-off-b", vault_key, "device-b");
        let pulled = device_b.pull().await.unwrap();
        assert_eq!(pulled.oauth_files, 0, "no folder, no write");
        assert!(device_b.push().await.unwrap().is_none(), "and nothing counted as deleted");
    }

    #[tokio::test]
    async fn pull_skips_entries_matching_the_destination_syncignore() {
        let remote = bare_remote("syncignore");
        let vault_key = [9u8; 32];

        let device_a = paired_engine(&remote, "syncignore-a", vault_key, "device-a");
        device_a.vault.write("a.md", "one").unwrap();
        device_a.vault.write("secret.md", "shh").unwrap();
        device_a.push().await.unwrap().unwrap();

        let device_b = paired_engine(&remote, "syncignore-b", vault_key, "device-b");
        device_b.vault.write(".syncignore", "secret.md\n").unwrap();
        let outcome = device_b.pull().await.unwrap();
        assert_eq!(outcome.files_written, 1);
        assert_eq!(outcome.files_ignored, 1);
        assert_eq!(device_b.vault.read("a.md").unwrap(), "one");
        assert!(!device_b.vault.root().join("secret.md").exists());
        assert!(outcome.warnings.is_empty());
    }

    #[tokio::test]
    async fn a_partially_synced_device_only_replays_new_commits() {
        let remote = bare_remote("partial");
        let vault_key = [9u8; 32];

        let device_a = paired_engine(&remote, "partial-a", vault_key, "device-a");
        device_a.vault.write("a.md", "one").unwrap();
        device_a.push().await.unwrap().unwrap();

        let device_b = paired_engine(&remote, "partial-b", vault_key, "device-b");
        let first_pull = device_b.pull().await.unwrap();
        assert_eq!(first_pull.commits_applied, 1);

        device_a.vault.write("b.md", "two").unwrap();
        device_a.push().await.unwrap().unwrap();

        let second_pull = device_b.pull().await.unwrap();
        assert_eq!(second_pull.commits_applied, 1);
        assert_eq!(second_pull.files_written, 1);
        assert_eq!(device_b.vault.read("b.md").unwrap(), "two");
    }

    #[tokio::test]
    async fn a_stale_push_is_rejected_clearly_and_a_pull_recovers() {
        let remote = bare_remote("stale");
        let vault_key = [9u8; 32];

        let device_a = paired_engine(&remote, "stale-a", vault_key, "device-a");
        device_a.vault.write("a.md", "one").unwrap();
        device_a.push().await.unwrap().unwrap();

        // Device B never pulled device A's change — its push should be rejected, not silently
        // clobber device A's history.
        let device_b = paired_engine(&remote, "stale-b", vault_key, "device-b");
        device_b.vault.write("b.md", "two").unwrap();
        let err = device_b.push().await.unwrap_err();
        assert!(err.to_string().contains("pull"), "expected a message pointing at pull, got: {err}");

        device_b.pull().await.unwrap();
        let outcome = device_b.push().await.unwrap().unwrap();
        assert_eq!(outcome.files_changed, 1);
    }

    #[test]
    fn maybe_authenticated_url_injects_credential_only_for_https() {
        assert_eq!(
            maybe_authenticated_url("https://gitea.example.com/user/vault.git", "tok"),
            "https://x-access-token:tok@gitea.example.com/user/vault.git"
        );
        assert_eq!(maybe_authenticated_url("/tmp/some/bare/repo", "tok"), "/tmp/some/bare/repo");
        assert_eq!(maybe_authenticated_url("https://gitea.example.com/user/vault.git", ""), "https://gitea.example.com/user/vault.git");
    }

    #[test]
    fn redact_scrubs_the_token_out_of_error_text() {
        let text = "fatal: unable to access 'https://x-access-token:supersecret@host/repo.git'";
        assert_eq!(redact(text, "supersecret"), "fatal: unable to access 'https://x-access-token:***@host/repo.git'");
    }
}

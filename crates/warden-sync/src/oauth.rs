//! MCP OAuth logins in the sync (P54): `~/.config/warden/mcp_oauth/<server>.json`, one file per server, so a device that pairs and
//! pulls arrives already connected instead of repeating every "Connect". They travel in the same encrypted bundle as the
//! vault and `config.toml`, with a hash per file in the manifest (`SyncManifest.oauth_files`) to tell what changed.
//!
//! The directory is optional on every engine (`with_oauth_dir`): `None` leaves logins out of both directions, which is what
//! the tests and a device without MCP servers get.

use std::collections::HashSet;
use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::bundle::SyncBundle;
use crate::diff::sha256_hex;
use crate::manifest::SyncManifest;

/// What changed in the OAuth directory since the manifest was last saved. Names are file names (`slack.json`).
#[derive(Debug, Default, PartialEq)]
pub struct OauthDiff {
    pub changed: Vec<String>,
    pub deleted: Vec<String>,
}

impl OauthDiff {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.deleted.is_empty()
    }
}

/// Whether `name` is a login file as `warden_bootstrap::oauth_credential_store_path` writes it: letters, digits, `_` and `-`,
/// then `.json`. Anything else in a bundle is not written, so a bundle can never reach outside the directory.
pub fn is_login_file_name(name: &str) -> bool {
    match name.strip_suffix(".json") {
        Some(stem) => !stem.is_empty() && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        None => false,
    }
}

/// The logins that are new, changed or gone since `manifest`. Without a directory there is nothing to compare, and nothing the
/// manifest knows counts as deleted.
pub fn diff_oauth(dir: Option<&Path>, manifest: &SyncManifest) -> anyhow::Result<OauthDiff> {
    let Some(dir) = dir else { return Ok(OauthDiff::default()) };
    let mut seen = HashSet::new();
    let mut changed = Vec::new();
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if !is_login_file_name(&name) || !entry.file_type()?.is_file() {
                    continue;
                }
                let hash = sha256_hex(&std::fs::read(entry.path())?);
                if manifest.oauth_files.get(&name) != Some(&hash) {
                    changed.push(name.clone());
                }
                seen.insert(name);
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    let mut deleted: Vec<String> = manifest.oauth_files.keys().filter(|name| !seen.contains(*name)).cloned().collect();
    changed.sort();
    deleted.sort();
    Ok(OauthDiff { changed, deleted })
}

/// Puts the changed logins (and the names of the deleted ones) into `bundle`.
pub fn attach(bundle: &mut SyncBundle, dir: Option<&Path>, diff: &OauthDiff) -> anyhow::Result<()> {
    let Some(dir) = dir else { return Ok(()) };
    for name in &diff.changed {
        bundle.oauth_files.insert(name.clone(), BASE64.encode(std::fs::read(dir.join(name))?));
    }
    bundle.deleted_oauth_files = diff.deleted.clone();
    Ok(())
}

/// Writes the logins of a decrypted bundle into `dir` and removes the ones it deletes; the count is files written plus removed.
/// With no directory, or for a name that is not a login file, nothing is touched.
pub fn apply(bundle: &SyncBundle, dir: Option<&Path>) -> anyhow::Result<usize> {
    let Some(dir) = dir else { return Ok(0) };
    let mut count = 0;
    for (name, encoded) in &bundle.oauth_files {
        if !is_login_file_name(name) {
            continue;
        }
        std::fs::create_dir_all(dir)?;
        let path = dir.join(name);
        std::fs::write(&path, BASE64.decode(encoded.as_bytes())?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        count += 1;
    }
    for name in &bundle.deleted_oauth_files {
        if !is_login_file_name(name) {
            continue;
        }
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => count += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(count)
}

/// Warnings for the logins `bundle` is about to overwrite or remove although they changed here since the last sync, like the
/// vault's "mudança local em ... foi sobrescrita pelo pull".
pub fn overwrite_warnings(bundle: &SyncBundle, dir: Option<&Path>, manifest: &SyncManifest) -> Vec<String> {
    let Some(dir) = dir else { return Vec::new() };
    let mut names: Vec<&String> = bundle.oauth_files.keys().chain(bundle.deleted_oauth_files.iter()).filter(|n| is_login_file_name(n)).collect();
    names.sort();
    names
        .into_iter()
        .filter(|name| std::fs::read(dir.join(name)).ok().map(|bytes| sha256_hex(&bytes)) != manifest.oauth_files.get(*name).cloned())
        .map(|name| format!("mudança local no login MCP {} foi sobrescrita pelo pull", name.trim_end_matches(".json")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "warden-sync-oauth-test-{suffix}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn empty_bundle() -> SyncBundle {
        SyncBundle {
            schema_version: 1,
            manifest_counter: 1,
            created_at_ms: 0,
            device_id: "a".into(),
            vault_files: Default::default(),
            deleted_vault_files: Vec::new(),
            config_toml: None,
            config_deleted: false,
            oauth_files: Default::default(),
            deleted_oauth_files: Vec::new(),
        }
    }

    fn manifest_with(files: &[(&str, &str)]) -> SyncManifest {
        let mut manifest = SyncManifest::default();
        for (name, content) in files {
            manifest.oauth_files.insert((*name).to_string(), sha256_hex(content.as_bytes()));
        }
        manifest
    }

    #[test]
    fn only_files_named_like_a_login_count() {
        for good in ["slack.json", "my-server_2.json", "A.json"] {
            assert!(is_login_file_name(good), "{good}");
        }
        for bad in [".json", "slack", "slack.json.bak", "../x.json", "a/b.json", "a b.json", "a.b.json", ""] {
            assert!(!is_login_file_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_diff_finds_new_changed_and_deleted_logins_and_ignores_other_files() {
        let dir = temp_dir("diff");
        std::fs::write(dir.join("new.json"), "n").unwrap();
        std::fs::write(dir.join("same.json"), "s").unwrap();
        std::fs::write(dir.join("edited.json"), "now").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a login").unwrap();
        std::fs::create_dir_all(dir.join("folder.json")).unwrap();
        let manifest = manifest_with(&[("same.json", "s"), ("edited.json", "before"), ("gone.json", "g")]);

        let diff = diff_oauth(Some(&dir), &manifest).unwrap();
        assert_eq!(diff.changed, ["edited.json", "new.json"]);
        assert_eq!(diff.deleted, ["gone.json"]);
        assert!(diff_oauth(Some(&dir), &manifest_with(&[("same.json", "s")])).unwrap().deleted.is_empty());
    }

    #[test]
    fn without_a_directory_nothing_is_a_change_and_nothing_known_is_deleted() {
        let manifest = manifest_with(&[("kept.json", "k")]);
        assert!(diff_oauth(None, &manifest).unwrap().is_empty());
    }

    #[test]
    fn a_missing_directory_means_every_known_login_was_deleted() {
        let manifest = manifest_with(&[("a.json", "1")]);
        let missing = std::env::temp_dir().join("warden-sync-oauth-test-never-created");
        assert_eq!(diff_oauth(Some(&missing), &manifest).unwrap().deleted, ["a.json"]);
    }

    #[test]
    fn attach_then_apply_moves_logins_between_directories_and_removes_deleted_ones() {
        let from = temp_dir("from");
        std::fs::write(from.join("slack.json"), r#"{"token":"t1"}"#).unwrap();
        let mut bundle = empty_bundle();
        let diff = OauthDiff { changed: vec!["slack.json".into()], deleted: vec!["old.json".into()] };
        attach(&mut bundle, Some(&from), &diff).unwrap();

        let to = temp_dir("to");
        std::fs::write(to.join("old.json"), "x").unwrap();
        assert_eq!(apply(&bundle, Some(&to)).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(to.join("slack.json")).unwrap(), r#"{"token":"t1"}"#);
        assert!(!to.join("old.json").exists());
        // Removing what is already gone is no error and no count.
        assert_eq!(apply(&bundle, Some(&to)).unwrap(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(to.join("slack.json")).unwrap().permissions().mode() & 0o777, 0o600, "a token is for its owner only");
        }
    }

    #[test]
    fn a_bundle_never_writes_or_removes_outside_the_directory() {
        let dir = temp_dir("jail");
        let outside = dir.parent().unwrap().join("warden-sync-oauth-test-outside.json");
        std::fs::write(&outside, "keep").unwrap();
        let mut bundle = empty_bundle();
        bundle.oauth_files.insert("../warden-sync-oauth-test-outside.json".into(), BASE64.encode("evil"));
        bundle.oauth_files.insert("notes.txt".into(), BASE64.encode("evil"));
        bundle.deleted_oauth_files.push("../warden-sync-oauth-test-outside.json".into());

        assert_eq!(apply(&bundle, Some(&dir)).unwrap(), 0);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
        assert!(!dir.join("notes.txt").exists());
        std::fs::remove_file(outside).unwrap();
    }

    #[test]
    fn without_a_directory_a_bundle_s_logins_are_left_alone() {
        let mut bundle = empty_bundle();
        bundle.oauth_files.insert("slack.json".into(), BASE64.encode("t"));
        assert_eq!(apply(&bundle, None).unwrap(), 0);
        let mut other = empty_bundle();
        attach(&mut other, None, &OauthDiff { changed: vec!["slack.json".into()], deleted: Vec::new() }).unwrap();
        assert!(other.oauth_files.is_empty());
    }

    #[test]
    fn a_login_changed_here_since_the_last_sync_is_warned_about_before_it_is_overwritten() {
        let dir = temp_dir("warn");
        std::fs::write(dir.join("slack.json"), "edited here").unwrap();
        std::fs::write(dir.join("calm.json"), "same as known").unwrap();
        let manifest = manifest_with(&[("slack.json", "as last synced"), ("calm.json", "same as known")]);
        let mut bundle = empty_bundle();
        bundle.oauth_files.insert("slack.json".into(), BASE64.encode("theirs"));
        bundle.oauth_files.insert("calm.json".into(), BASE64.encode("theirs"));

        assert_eq!(overwrite_warnings(&bundle, Some(&dir), &manifest), ["mudança local no login MCP slack foi sobrescrita pelo pull"]);
    }
}

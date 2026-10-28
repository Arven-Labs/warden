//! The publisher's side of signed skill packages (P160), what `warden-server package ...` does: make a signing key, sign a package
//! file with it, and read back who a package says signed it. Kept out of `main.rs` so it can be tested. The private key lives
//! in a file only its owner can read (mode 0600 on Unix), is never printed, and is never overwritten: losing it would make
//! everything signed with it unprovable, and a second `keygen` over it would do exactly that.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use warden_bootstrap::skill_package::{self, signature_standing, TrustedPublisher};
use warden_core::skill::index::{Index, IndexEntry, Revoked, MAX_INDEX_BYTES};
use warden_core::skill::package::{Package, MAX_PACKAGE_BYTES};
use warden_core::skill::signing::{decode_private_key, public_key_of};

/// Makes a new signing key and writes its private half to `path`, which must not exist. Returns the public key (64 hex
/// characters): what others put in their `[[trusted_publishers]]` to trust what is signed with it.
pub fn keygen(path: &Path) -> anyhow::Result<String> {
    skill_package::create_signing_key(path).map_err(anyhow::Error::msg)
}

/// Packs the skill in the markdown file `skill` (the vault's format; the file's name is the skill's) into a package and writes it to
/// `out`, unsigned: `sign` comes next. This is what the app does when it shares a skill, for a skill that is a file in a repository.
/// Returns a line saying what was written.
pub fn export(skill: &Path, version: &str, author: Option<&str>, license: &str, out: &Path) -> anyhow::Result<String> {
    let name = skill.file_stem().and_then(|n| n.to_str()).with_context(|| format!("{} has no usable name", skill.display()))?;
    let size = std::fs::metadata(skill).with_context(|| format!("cannot read {}", skill.display()))?.len();
    if size > MAX_PACKAGE_BYTES as u64 {
        bail!("{} is {} KiB; a package is at most {} KiB", skill.display(), size.div_ceil(1024), MAX_PACKAGE_BYTES / 1024);
    }
    let raw = std::fs::read_to_string(skill).with_context(|| format!("cannot read {}", skill.display()))?;
    let package = Package::export_skill(&warden_core::skill::Skill::parse(name, &raw), version, author, license)?;
    std::fs::write(out, package.to_json()?).with_context(|| format!("cannot write {}", out.display()))?;
    Ok(format!("wrote {} ({} {}, hash {})", out.display(), package.id, package.version, package.hash))
}

/// Reads the package at `package`, signs it with the key in `key` and writes it to `out` (the package's own file when `None`).
/// Returns the signer's public key. A package that is not good as it stands (changed, damaged, or with a signature that does not
/// check out) is refused rather than signed.
pub fn sign(package: &Path, key: &Path, out: Option<&Path>) -> anyhow::Result<String> {
    let text = read_package(package)?;
    let mut parsed = Package::parse(&text)?;
    let private = decode_private_key(&std::fs::read_to_string(key).with_context(|| format!("cannot read {}", key.display()))?)?;
    parsed.sign(&private)?;
    let json = parsed.to_json()?;
    let target = out.unwrap_or(package);
    std::fs::write(target, json).with_context(|| format!("cannot write {}", target.display()))?;
    public_key_of(&private)
}

/// A line saying who signed the package at `package`, measured against `trusted`. A signature that is wrong is an error.
pub fn verify(package: &Path, trusted: &[TrustedPublisher]) -> anyhow::Result<String> {
    let parsed = Package::parse(&read_package(package)?)?;
    let standing = signature_standing(&parsed, trusted).map_err(anyhow::Error::msg)?;
    let what = format!("{} {} (hash {})", parsed.id, parsed.version, parsed.hash);
    Ok(match (standing.state, standing.signer, standing.signer_name) {
        ("trusted", Some(key), name) => format!("{what}: signed by {} ({key}), a key you trust", name.unwrap_or_else(|| "a trusted key".into())),
        ("unknown", Some(key), _) => format!("{what}: signed by {key}, a key that is not in your trusted_publishers"),
        _ => format!("{what}: not signed"),
    })
}

/// Makes the signed index of a curator (P160) from `packages` (package files, each read and checked as it stands) and writes it to
/// `out`. Each entry's `source` is `base_url`, a slash and the file's name: the place the files will be published. `revoke` is hashes
/// to take back, each `HASH` or `HASH=reason`. `sequence` is the index's number, the clock in seconds when `None`, so a later build
/// is always newer. Returns a line saying what was written.
pub fn index_build(name: &str, key: &Path, base_url: &str, revoke: &[String], packages: &[PathBuf], out: &Path, sequence: Option<u64>) -> anyhow::Result<String> {
    let private = decode_private_key(&std::fs::read_to_string(key).with_context(|| format!("cannot read {}", key.display()))?)?;
    let base = base_url.trim().trim_end_matches('/');
    let mut entries = Vec::new();
    for path in packages {
        let package = Package::parse(&read_package(path)?).with_context(|| format!("{} is not a usable package", path.display()))?;
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if file.is_empty() || !file.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            bail!("the file name '{file}' cannot go in an address: use letters, digits, dots, dashes and underscores");
        }
        entries.push(IndexEntry::from_package(&package, &format!("{base}/{file}"))?);
    }
    let revoked: Vec<Revoked> = revoke
        .iter()
        .map(|item| match item.split_once('=') {
            Some((hash, reason)) => Revoked { hash: hash.trim().to_string(), reason: Some(reason.trim().to_string()).filter(|r| !r.is_empty()) },
            None => Revoked { hash: item.trim().to_string(), reason: None },
        })
        .collect();
    let sequence = sequence.unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_secs()));
    let index = Index::build(name, sequence, entries, revoked, &private)?;
    std::fs::write(out, index.to_json()?).with_context(|| format!("cannot write {}", out.display()))?;
    Ok(format!("wrote {} ({} packages, {} taken back, number {}), signed by {}", out.display(), index.entries.len(), index.revoked.len(), index.sequence, public_key_of(&private)?))
}

/// A line saying what the index at `path` holds and who signed it; with `key`, the index must be signed by it. A bad or missing
/// signature is an error.
pub fn index_verify(path: &Path, key: Option<&str>) -> anyhow::Result<String> {
    let size = std::fs::metadata(path).with_context(|| format!("cannot read {}", path.display()))?.len();
    if size > MAX_INDEX_BYTES as u64 {
        bail!("{} is {} KiB; an index is at most {} KiB", path.display(), size.div_ceil(1024), MAX_INDEX_BYTES / 1024);
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let index = match key {
        Some(key) => Index::parse_signed_by(&text, key)?,
        None => Index::parse(&text)?,
    };
    let signer = index.signer()?;
    let note = if key.is_some() { "the key you gave" } else { "a key you did not check against anything" };
    Ok(format!("{} (number {}): {} packages, {} taken back, signed by {signer}, {note}", index.name, index.sequence, index.entries.len(), index.revoked.len()))
}

fn read_package(path: &Path) -> anyhow::Result<String> {
    let size = std::fs::metadata(path).with_context(|| format!("cannot read {}", path.display()))?.len();
    if size > MAX_PACKAGE_BYTES as u64 {
        bail!("{} is {} KiB; a package is at most {} KiB", path.display(), size.div_ceil(1024), MAX_PACKAGE_BYTES / 1024);
    }
    std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use warden_core::skill::Skill;

    fn dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("warden-package-cmd-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn package_file(dir: &Path) -> std::path::PathBuf {
        let skill = Skill { name: "greet".into(), description: "Greets.".into(), body: "Say hello.".into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None };
        let path = dir.join("greet.warden-skill.json");
        std::fs::write(&path, Package::export_skill(&skill, "1.0.0", Some("Ana"), "MIT").unwrap().to_json().unwrap()).unwrap();
        path
    }

    #[test]
    fn a_skill_file_becomes_a_package_that_signs_and_reads_back() {
        let dir = dir();
        let skill = dir.join("greet.md");
        std::fs::write(&skill, "---\nname: greet\ndescription: Greets.\n---\nSay hello.\n").unwrap();
        let out = dir.join("greet.warden-skill.json");
        let line = export(&skill, "1.0.0", Some("Ana"), "MIT", &out).unwrap();
        assert!(line.contains("greet 1.0.0") && line.contains("sha256:"), "{line}");

        let package = Package::parse(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!((package.id.as_str(), package.author.as_deref(), package.license.as_str()), ("greet", Some("Ana"), "MIT"));
        assert!(package.signature.is_none(), "export leaves signing to `sign`");
        assert_eq!(package.as_skill().unwrap().body, "Say hello.");

        let key = dir.join("signing.key");
        let public = keygen(&key).unwrap();
        assert_eq!(sign(&out, &key, None).unwrap(), public);
        assert!(verify(&out, &[]).unwrap().contains(&public));

        // What cannot be a package is refused, not written.
        assert!(export(&skill, "not a version", None, "MIT", &dir.join("bad.json")).is_err());
        assert!(!dir.join("bad.json").exists());
        assert!(export(&dir.join("missing.md"), "1.0.0", None, "MIT", &dir.join("m.json")).unwrap_err().to_string().contains("cannot read"));
    }

    #[test]
    fn a_key_is_made_once_kept_private_and_never_overwritten() {
        let dir = dir();
        let key = dir.join("signing.key");
        let public = keygen(&key).unwrap();
        assert_eq!(public.len(), 64);
        assert_eq!(public_key_of(&decode_private_key(&std::fs::read_to_string(&key).unwrap()).unwrap()).unwrap(), public);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600, "only its owner can read it");
        }
        let before = std::fs::read_to_string(&key).unwrap();
        assert!(keygen(&key).unwrap_err().to_string().contains("never overwritten"));
        assert_eq!(std::fs::read_to_string(&key).unwrap(), before, "the first key is still there");
    }

    #[test]
    fn a_package_is_signed_and_read_back_by_its_signer_and_by_a_stranger() {
        let dir = dir();
        let (key, package) = (dir.join("signing.key"), package_file(&dir));
        let public = keygen(&key).unwrap();

        assert!(verify(&package, &[]).unwrap().contains("not signed"));
        assert_eq!(sign(&package, &key, None).unwrap(), public);
        assert!(verify(&package, &[]).unwrap().contains("not in your trusted_publishers"));
        let listed = [TrustedPublisher { key: public.clone(), name: "Fabio".into() }];
        let line = verify(&package, &listed).unwrap();
        assert!(line.contains("signed by Fabio") && line.contains("a key you trust") && line.contains(&public), "{line}");

        // Writing to another file leaves the original as it was.
        let (copy, other) = (dir.join("copy.json"), dir.join("other"));
        std::fs::create_dir_all(&other).unwrap();
        let unsigned = package_file(&other);
        sign(&unsigned, &key, Some(&copy)).unwrap();
        assert!(verify(&copy, &listed).unwrap().contains("a key you trust"));
        assert!(verify(&unsigned, &listed).unwrap().contains("not signed"), "the original is as it was");
    }

    #[test]
    fn an_index_is_built_from_packages_signed_and_read_back_by_its_key_only() {
        let dir = dir();
        let (key, package, out) = (dir.join("signing.key"), package_file(&dir), dir.join("index.json"));
        let public = keygen(&key).unwrap();
        let hash = Package::parse(&std::fs::read_to_string(&package).unwrap()).unwrap().hash;
        let taken = format!("sha256:{}", "ab".repeat(32));
        let line = index_build("Ana's picks", &key, "https://example.org/skills/", &[format!("{taken}=it asked for a password")], &[package.clone()], &out, Some(5)).unwrap();
        assert!(line.contains("1 packages, 1 taken back, number 5") && line.contains(&public), "{line}");

        let index = Index::parse_signed_by(&std::fs::read_to_string(&out).unwrap(), &public).unwrap();
        assert_eq!(index.entries[0].source, "https://example.org/skills/greet.warden-skill.json", "the slash at the end of the base is not doubled");
        assert_eq!(index.entries[0].hash, hash);
        assert_eq!(index.revoked_reason(&taken), Some("it asked for a password"));

        assert!(index_verify(&out, Some(&public)).unwrap().contains("the key you gave"));
        assert!(index_verify(&out, None).unwrap().contains("did not check"));
        assert!(index_verify(&out, Some(&"cd".repeat(32))).unwrap_err().to_string().contains("another key"));
        std::fs::write(&out, std::fs::read_to_string(&out).unwrap().replace("Ana's picks", "Mallory's picks")).unwrap();
        assert!(index_verify(&out, Some(&public)).unwrap_err().to_string().contains("changed or damaged"));
    }

    #[test]
    fn an_index_is_not_built_from_what_is_not_a_package_or_cannot_be_an_address() {
        let dir = dir();
        let (key, package, out) = (dir.join("signing.key"), package_file(&dir), dir.join("index.json"));
        keygen(&key).unwrap();
        let bad = dir.join("not a package.json");
        std::fs::write(&bad, "hello").unwrap();
        assert!(index_build("x", &key, "https://example.org", &[], &[bad], &out, None).is_err());
        let spaced = dir.join("with space.json");
        std::fs::copy(&package, &spaced).unwrap();
        assert!(index_build("x", &key, "https://example.org", &[], &[spaced], &out, None).unwrap_err().to_string().contains("cannot go in an address"));
        assert!(index_build("x", &key, "http://example.org", &[], &[package.clone()], &out, None).is_err(), "a source is https, ar:// or this machine");
        assert!(index_build("x", &dir.join("no-key"), "https://example.org", &[], &[package], &out, None).is_err());
        assert!(!out.exists(), "nothing was written");
    }

    #[test]
    fn a_package_that_is_not_good_or_a_file_that_is_not_a_key_is_not_signed() {
        let dir = dir();
        let (key, package) = (dir.join("signing.key"), package_file(&dir));
        keygen(&key).unwrap();

        let damaged = dir.join("damaged.json");
        std::fs::write(&damaged, std::fs::read_to_string(&package).unwrap().replace("Say hello.", "Say goodbye.")).unwrap();
        assert!(sign(&damaged, &key, None).unwrap_err().to_string().contains("changed or damaged"));

        let not_a_key = dir.join("not-a-key");
        std::fs::write(&not_a_key, "hello").unwrap();
        assert!(sign(&package, &not_a_key, None).is_err());
        assert!(verify(&package, &[]).unwrap().contains("not signed"), "the package was left alone");
        assert!(sign(&dir.join("missing.json"), &key, None).is_err());
    }
}

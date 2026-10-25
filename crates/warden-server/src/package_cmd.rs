//! The publisher's side of signed skill packages (P160), what `warden-server package ...` does: make a signing key, sign a package
//! file with it, and read back who a package says signed it. Kept out of `main.rs` so it can be tested. The private key lives
//! in a file only its owner can read (mode 0600 on Unix), is never printed, and is never overwritten: losing it would make
//! everything signed with it unprovable, and a second `keygen` over it would do exactly that.

use std::io::Write as _;
use std::path::Path;

use anyhow::{bail, Context};
use warden_bootstrap::skill_package::{signature_standing, TrustedPublisher};
use warden_core::skill::package::{Package, MAX_PACKAGE_BYTES};
use warden_core::skill::signing::{decode_private_key, encode_private_key, generate_keypair, public_key_of};

/// Makes a new signing key and writes its private half to `path`, which must not exist. Returns the public key (64 hex
/// characters): what others put in their `[[trusted_publishers]]` to trust what is signed with it.
pub fn keygen(path: &Path) -> anyhow::Result<String> {
    let (private, public) = generate_keypair()?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).with_context(|| format!("cannot create {} (a key file is never overwritten)", path.display()))?;
    writeln!(file, "{}", encode_private_key(&private)).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(public)
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

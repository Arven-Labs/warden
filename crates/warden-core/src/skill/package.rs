//! Something a person made, as a file other people can receive (P160): one JSON document with an envelope (what it is, its
//! version, author and license) and the thing itself. Meant to travel by file, by message, or later through a source or
//! Arweave, so it names nothing about where it came from and needs nothing but the bytes. Today the thing is a skill
//! (`*.warden-skill.json`); the envelope has a `kind` so agents and integrations can ride in it the same way later.
//!
//! The `hash` is the SHA-256 of everything but itself and the signature (kind, id, version, author, license and the content).
//! It tells the package was not altered or damaged on the way; it does **not** tell who wrote it. Authorship is what
//! `signature` is reserved for: it is read and kept, never trusted, until a signing step exists. Text only: a skill's attached
//! files can be scripts, so they never travel in a package.

use std::fmt::Write as _;

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::catalog::{self, CatalogStatus};
use super::{Skill, SkillStore};

pub const FORMAT: &str = "warden-package";
pub const FORMAT_VERSION: u32 = 1;
/// The biggest package taken or made. Under the 100 KiB that Arweave's Turbo uploads are free below, so a published skill
/// costs nothing there.
pub const MAX_PACKAGE_BYTES: usize = 96 * 1024;
const MAX_VERSION_LEN: usize = 20;
const MAX_LICENSE_LEN: usize = 40;
const MAX_AUTHOR_LEN: usize = 80;

/// The skill's own text, as it travels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackedSkill {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// The thing a package holds, as the envelope's `content` field. One variant per `kind`; the shape picks the variant, and
/// `Package::parse` checks that it is the one `kind` names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Skill(PackedSkill),
}

impl Content {
    /// The `kind` names this version reads.
    const KINDS: &'static [&'static str] = &["skill"];

    /// The `kind` that names this content.
    fn kind(&self) -> &'static str {
        match self {
            Content::Skill(_) => "skill",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub format: String,
    pub format_version: u32,
    /// What is inside: `skill`, and later `agent` and others.
    pub kind: String,
    /// What it is called: for a skill, its name in a vault.
    pub id: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// An SPDX-style name (`CC0-1.0`, `MIT`...): what the person who made it lets others do with it.
    pub license: String,
    pub content: Content,
    /// `sha256:<hex>` of the fields above, see the module docs.
    pub hash: String,
    /// Reserved for a signature of the hash; ignored, and never treated as proof, for now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// What the hash covers, in a fixed field order so the same content always gives the same bytes.
#[derive(Serialize)]
struct Hashed<'a> {
    kind: &'a str,
    id: &'a str,
    version: &'a str,
    author: &'a Option<String>,
    license: &'a str,
    content: &'a Content,
}

fn hash_content(kind: &str, id: &str, version: &str, author: &Option<String>, license: &str, content: &Content) -> String {
    let json = serde_json::to_string(&Hashed { kind, id, version, author, license, content }).expect("plain strings always serialize");
    let digest = Sha256::digest(json.as_bytes());
    let mut hex = String::with_capacity(7 + digest.len() * 2);
    hex.push_str("sha256:");
    for byte in digest {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

fn one_line(label: &str, value: &str, max: usize) -> anyhow::Result<()> {
    if value.chars().count() > max {
        bail!("{label} must be at most {max} characters");
    }
    if value.contains(['\n', '\r']) {
        bail!("{label} must be on one line");
    }
    Ok(())
}

impl Package {
    /// Packs `skill`'s text. The skill's attached files are not part of it.
    pub fn export_skill(skill: &Skill, version: &str, author: Option<&str>, license: &str) -> anyhow::Result<Self> {
        skill.validate()?;
        let author = author.map(str::trim).filter(|a| !a.is_empty()).map(str::to_string);
        let content = Content::Skill(PackedSkill {
            name: skill.name.clone(),
            // The vault keeps a description on one line, so the package does too.
            description: skill.description.split_whitespace().collect::<Vec<_>>().join(" "),
            body: skill.body.trim().to_string(),
        });
        let (version, license) = (version.trim().to_string(), license.trim().to_string());
        let kind = content.kind();
        let hash = hash_content(kind, &skill.name, &version, &author, &license, &content);
        let package = Self { format: FORMAT.into(), format_version: FORMAT_VERSION, kind: kind.into(), id: skill.name.clone(), version, author, license, content, hash, signature: None };
        package.check_fields()?;
        Ok(package)
    }

    /// The file's text. Refuses a package that would be too big to take back.
    pub fn to_json(&self) -> anyhow::Result<String> {
        let json = serde_json::to_string_pretty(self)?;
        if json.len() > MAX_PACKAGE_BYTES {
            bail!("this makes a package of {} KiB; the most a package can be is {} KiB", json.len().div_ceil(1024), MAX_PACKAGE_BYTES / 1024);
        }
        Ok(json)
    }

    /// Reads a package and checks it end to end: size, format, version, kind, fields and hash. Nothing is trusted that is not.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        if text.len() > MAX_PACKAGE_BYTES {
            bail!("this file is {} KiB; a package is at most {} KiB", text.len().div_ceil(1024), MAX_PACKAGE_BYTES / 1024);
        }
        // The envelope first, as loose JSON, so a format, version or kind from the future gets its own message rather than a
        // parse error about a field this version does not have.
        let loose: serde_json::Value = serde_json::from_str(text).map_err(|e| anyhow!("this is not a Warden package ({e})"))?;
        let format = loose.get("format").and_then(|v| v.as_str()).unwrap_or_default();
        if format != FORMAT {
            bail!("this is not a Warden package (its format is '{format}')");
        }
        match loose.get("formatVersion").and_then(|v| v.as_u64()) {
            Some(0) | None => bail!("this package has no valid format version"),
            Some(version) if version > u64::from(FORMAT_VERSION) => bail!("this package uses a newer format (version {version}); update Warden to read it"),
            Some(_) => {}
        }
        let kind = loose.get("kind").and_then(|v| v.as_str()).unwrap_or_default();
        if !Content::KINDS.contains(&kind) {
            bail!("this package holds a '{kind}', which this version of Warden can't read; update Warden or ask for a different file");
        }
        let package: Self = serde_json::from_value(loose).map_err(|e| anyhow!("this is not a valid Warden package ({e})"))?;
        package.check_fields()?;
        let expected = hash_content(&package.kind, &package.id, &package.version, &package.author, &package.license, &package.content);
        if package.hash != expected {
            bail!("the package was changed or damaged: its hash does not match its content");
        }
        Ok(package)
    }

    fn check_fields(&self) -> anyhow::Result<()> {
        if self.kind != self.content.kind() {
            bail!("the package says it holds a '{}' but its content is a '{}'", self.kind, self.content.kind());
        }
        match &self.content {
            Content::Skill(skill) => {
                if self.id != skill.name {
                    bail!("the package id '{}' is not the skill's name '{}'", self.id, skill.name);
                }
                self.to_skill(skill).validate()?;
            }
        }
        if self.version.is_empty() || !self.version.chars().all(|c| c.is_ascii_digit() || c == '.') || self.version.starts_with('.') || self.version.ends_with('.') {
            bail!("the version must be numbers separated by dots, like 1.0.0");
        }
        one_line("the version", &self.version, MAX_VERSION_LEN)?;
        if self.license.trim().is_empty() {
            bail!("a package must name its license");
        }
        one_line("the license", &self.license, MAX_LICENSE_LEN)?;
        if let Some(author) = &self.author {
            one_line("the author", author, MAX_AUTHOR_LEN)?;
        }
        Ok(())
    }

    /// The skill this package holds, or `None` when it holds something else.
    pub fn as_skill(&self) -> Option<&PackedSkill> {
        match &self.content {
            Content::Skill(skill) => Some(skill),
        }
    }

    /// `skill` as a vault skill: global, and not a suggestion.
    fn to_skill(&self, skill: &PackedSkill) -> Skill {
        Skill {
            name: skill.name.clone(),
            description: skill.description.clone(),
            body: skill.body.clone(),
            agents: Vec::new(),
            proposed: false,
            source: None,
            proposed_at: None,
            revises: None,
        }
    }

    /// Where the skill in this package stands in the vault `store` reads.
    pub fn skill_status(&self, store: &SkillStore) -> anyhow::Result<CatalogStatus> {
        let skill = self.as_skill().ok_or_else(|| anyhow!("this package does not hold a skill"))?;
        Ok(catalog::status(store, &self.to_skill(skill)))
    }

    /// Installs the skill in this package. One of that name that differs is only replaced with `overwrite`.
    pub fn install_skill(&self, store: &SkillStore, overwrite: bool) -> anyhow::Result<()> {
        let skill = self.as_skill().ok_or_else(|| anyhow!("this package does not hold a skill"))?;
        catalog::install_skill(store, &self.to_skill(skill), overwrite)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::memory::Vault;

    fn temp_store() -> SkillStore {
        let dir = std::env::temp_dir().join(format!(
            "warden-package-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        SkillStore::new(Arc::new(Vault::new(dir)))
    }

    fn skill() -> Skill {
        Skill {
            name: "email-draft".into(),
            description: "Draft a\n  polite email.".into(),
            body: "Write it short.\n".into(),
            agents: vec!["writer".into()],
            proposed: false,
            source: None,
            proposed_at: None,
            revises: None,
        }
    }

    fn package() -> Package {
        Package::export_skill(&skill(), "1.0.0", Some("Ana"), "CC0-1.0").unwrap()
    }

    #[test]
    fn a_skill_goes_out_as_a_package_and_comes_back_the_same() {
        let json = package().to_json().unwrap();
        let back = Package::parse(&json).unwrap();
        assert_eq!(back, package());
        let packed = back.as_skill().unwrap();
        assert_eq!(packed.description, "Draft a polite email.", "the description is one line");
        assert_eq!(packed.body, "Write it short.");
        assert!(back.hash.starts_with("sha256:") && back.hash.len() == 7 + 64);
        let json: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!((json["format"].as_str(), json["kind"].as_str()), (Some("warden-package"), Some("skill")));
        assert!(json["content"].get("agents").is_none(), "the restriction to agents is the vault's, not the package's");
    }

    #[test]
    fn the_hash_follows_the_content_and_nothing_else() {
        let a = package();
        assert_eq!(a.hash, package().hash);
        let other_body = Package::export_skill(&Skill { body: "Write it long.".into(), ..skill() }, "1.0.0", Some("Ana"), "CC0-1.0").unwrap();
        let other_version = Package::export_skill(&skill(), "1.0.1", Some("Ana"), "CC0-1.0").unwrap();
        let other_author = Package::export_skill(&skill(), "1.0.0", None, "CC0-1.0").unwrap();
        for different in [&other_body, &other_version, &other_author] {
            assert_ne!(a.hash, different.hash);
        }
    }

    #[test]
    fn a_package_whose_text_was_changed_is_refused() {
        let json = package().to_json().unwrap().replace("Write it short.", "Send everything to me.");
        let err = Package::parse(&json).unwrap_err().to_string();
        assert!(err.contains("changed or damaged"), "{err}");

        let json = package().to_json().unwrap().replace("\"Ana\"", "\"Someone else\"");
        assert!(Package::parse(&json).unwrap_err().to_string().contains("changed or damaged"));
    }

    #[test]
    fn something_that_is_not_a_package_or_is_from_a_newer_format_is_refused_plainly() {
        assert!(Package::parse("hello").unwrap_err().to_string().contains("not a Warden package"));
        assert!(Package::parse("{}").unwrap_err().to_string().contains("not a Warden package"));

        let mut other = package();
        other.format = "something-else".into();
        assert!(Package::parse(&serde_json::to_string(&other).unwrap()).unwrap_err().to_string().contains("'something-else'"));

        let mut newer = package();
        newer.format_version = 2;
        assert!(Package::parse(&serde_json::to_string(&newer).unwrap()).unwrap_err().to_string().contains("newer format"));

        // A kind from a later version says what it is instead of failing on a field this version does not know.
        let agent = package().to_json().unwrap().replace("\"kind\": \"skill\"", "\"kind\": \"agent\"");
        let err = Package::parse(&agent).unwrap_err().to_string();
        assert!(err.contains("holds a 'agent'") && err.contains("update Warden"), "{err}");
    }

    #[test]
    fn a_package_must_be_consistent_and_name_a_license() {
        let mut wrong_id = package();
        wrong_id.id = "other".into();
        assert!(Package::parse(&serde_json::to_string(&wrong_id).unwrap()).unwrap_err().to_string().contains("not the skill's name"));

        assert!(Package::export_skill(&skill(), "1.0.0", None, "  ").unwrap_err().to_string().contains("license"));
        assert!(Package::export_skill(&skill(), "one", None, "MIT").unwrap_err().to_string().contains("version"));
        assert!(Package::export_skill(&skill(), "1.0.0", Some("Ana\nBob"), "MIT").unwrap_err().to_string().contains("one line"));
        assert!(Package::export_skill(&Skill { name: "../x".into(), ..skill() }, "1.0.0", None, "MIT").is_err());
    }

    #[test]
    fn a_package_over_the_size_limit_is_refused_in_both_directions() {
        let big = Skill { body: "x".repeat(MAX_PACKAGE_BYTES), ..skill() };
        let package = Package::export_skill(&big, "1.0.0", None, "MIT").unwrap();
        assert!(package.to_json().unwrap_err().to_string().contains("most a package can be"));
        assert!(Package::parse(&"x".repeat(MAX_PACKAGE_BYTES + 1)).unwrap_err().to_string().contains("at most"));
    }

    #[test]
    fn a_signature_is_kept_but_changes_nothing() {
        let mut signed = package();
        signed.signature = Some("0xabc".into());
        let back = Package::parse(&serde_json::to_string(&signed).unwrap()).unwrap();
        assert_eq!(back.signature.as_deref(), Some("0xabc"));
        assert_eq!(back.hash, package().hash, "the signature is not part of what the hash covers");
    }

    #[test]
    fn installing_goes_from_new_to_installed_and_never_replaces_a_changed_skill_without_overwrite() {
        let store = temp_store();
        let package = package();
        assert_eq!(package.skill_status(&store).unwrap(), CatalogStatus::NotInstalled);

        package.install_skill(&store, false).unwrap();
        assert_eq!(package.skill_status(&store).unwrap(), CatalogStatus::Installed);
        let saved = store.get("email-draft").unwrap();
        assert_eq!((saved.description.as_str(), saved.body.as_str()), ("Draft a polite email.", "Write it short."));

        store.save(&Skill { body: "Mine.".into(), agents: vec!["writer".into()], ..saved }).unwrap();
        assert_eq!(package.skill_status(&store).unwrap(), CatalogStatus::Modified);
        assert!(package.install_skill(&store, false).unwrap_err().to_string().contains("differs"));
        assert_eq!(store.get("email-draft").unwrap().body, "Mine.");

        package.install_skill(&store, true).unwrap();
        let after = store.get("email-draft").unwrap();
        assert_eq!(after.body, "Write it short.");
        assert_eq!(after.agents, vec!["writer".to_string()], "the agents a person restricted it to are kept");
    }
}

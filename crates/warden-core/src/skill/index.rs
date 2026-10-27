//! What a curator publishes (P160): a signed list of the packages they stand behind, and the ones they take back. A person adds a
//! curator by name and public key (their own config, `[[curators]]`); Warden ships none. The index lists where each package is
//! (`source`, an `https://` address or an `ar://` one) and its `hash`, so whoever reads it can fetch the package from anywhere and
//! check it is the one the curator meant. It carries no package text, and installing still goes through the package's own preview.
//!
//! Like a package it is one JSON document with an envelope (`format`, `formatVersion`), a `hash` over everything but the hash and
//! the signature, and a `signature` over the hash (`signing`). Unlike a package, **the signature is required**: the curator's key
//! is what the reader trusts, so an index nobody signed, or whose signature does not check out, is refused. `sequence` only goes
//! up between one index of a curator and the next, so a reader that remembers the last one it saw can refuse an older copy
//! someone replays. `revoked` lists hashes the curator no longer vouches for, with a reason the person can read.

use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::package::{one_line, Package};
use super::signing::{self, Verification};

pub const FORMAT: &str = "warden-index";
pub const FORMAT_VERSION: u32 = 1;
/// The biggest index taken or made.
pub const MAX_INDEX_BYTES: usize = 512 * 1024;
pub const MAX_ENTRIES: usize = 500;
pub const MAX_REVOKED: usize = 500;
const MAX_NAME_LEN: usize = 80;
const MAX_VERSION_LEN: usize = 20;
const MAX_LICENSE_LEN: usize = 40;
const MAX_AUTHOR_LEN: usize = 80;
const MAX_DESCRIPTION_LEN: usize = 300;
const MAX_SOURCE_LEN: usize = 500;
const MAX_REASON_LEN: usize = 200;
/// An Arweave transaction id: 43 base64url characters.
const ARWEAVE_ID_LEN: usize = 43;

/// One package the curator lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    /// What the package is called (a skill's name).
    pub id: String,
    pub version: String,
    /// The package's own `hash` (`sha256:<hex>`): what the package fetched from `source` must have.
    pub hash: String,
    /// Where the package is: `https://…` or `ar://<id>`.
    pub source: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub license: String,
}

/// A package the curator no longer vouches for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revoked {
    pub hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub format: String,
    pub format_version: u32,
    /// What the curator calls this list.
    pub name: String,
    /// Goes up with each index the curator publishes (the build uses the clock, in seconds).
    pub sequence: u64,
    pub entries: Vec<IndexEntry>,
    pub revoked: Vec<Revoked>,
    /// `sha256:<hex>` of the fields above, see the module docs.
    pub hash: String,
    /// `ed25519:<public key>:<signature>` over the hash. Required: `parse` refuses an index without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// What the hash covers, in a fixed field order so the same content always gives the same bytes.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Hashed<'a> {
    name: &'a str,
    sequence: u64,
    entries: &'a [IndexEntry],
    revoked: &'a [Revoked],
}

fn hash_content(name: &str, sequence: u64, entries: &[IndexEntry], revoked: &[Revoked]) -> String {
    let json = serde_json::to_string(&Hashed { name, sequence, entries, revoked }).expect("plain strings always serialize");
    let digest = Sha256::digest(json.as_bytes());
    let mut hex = String::with_capacity(7 + digest.len() * 2);
    hex.push_str("sha256:");
    for byte in digest {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

/// Whether `hash` is written the way a package writes it: `sha256:` and 64 hex characters.
fn is_hash(hash: &str) -> bool {
    hash.strip_prefix("sha256:").is_some_and(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Whether `source` is somewhere a package can be fetched from: an `https://` address, `ar://` and an Arweave id, or plain
/// `http://` to this machine only (a curator someone tries on their own computer; the hub's fetch still refuses a loopback address
/// unless it was told to allow it). The index's own `hash` and signature, and the package's hash pinned in each entry, carry the
/// integrity, so this is about where a package may be asked for, not about trusting the wire.
pub fn is_source(source: &str) -> bool {
    if source.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    if let Some(id) = source.strip_prefix("ar://") {
        return id.len() == ARWEAVE_ID_LEN && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    }
    if let Some(rest) = source.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let name = host.rsplit_once(':').filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit())).map_or(host, |(name, _)| name);
        return matches!(name, "127.0.0.1" | "localhost" | "[::1]");
    }
    source.strip_prefix("https://").is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
}

impl IndexEntry {
    /// The entry for `package`, which will be found at `source`. The package is what `Package::parse` gave, so its fields are good.
    pub fn from_package(package: &Package, source: &str) -> anyhow::Result<Self> {
        let skill = package.as_skill().ok_or_else(|| anyhow!("only a skill can be listed in an index for now"))?;
        let entry = Self {
            id: package.id.clone(),
            version: package.version.clone(),
            hash: package.hash.clone(),
            source: source.trim().to_string(),
            description: skill.description.clone(),
            author: package.author.clone(),
            license: package.license.clone(),
        };
        entry.check()?;
        Ok(entry)
    }

    fn check(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            bail!("an entry has no id");
        }
        one_line("an entry's id", &self.id, MAX_NAME_LEN)?;
        if self.version.is_empty() || !self.version.chars().all(|c| c.is_ascii_digit() || c == '.') || self.version.starts_with('.') || self.version.ends_with('.') {
            bail!("the version of '{}' must be numbers separated by dots, like 1.0.0", self.id);
        }
        one_line("an entry's version", &self.version, MAX_VERSION_LEN)?;
        if !is_hash(&self.hash) {
            bail!("the hash of '{}' is not a sha256 hash", self.id);
        }
        if self.source.len() > MAX_SOURCE_LEN || !is_source(&self.source) {
            bail!("the source of '{}' must be an https:// address or an ar:// one", self.id);
        }
        one_line("an entry's description", &self.description, MAX_DESCRIPTION_LEN)?;
        if self.license.trim().is_empty() {
            bail!("'{}' does not name its license", self.id);
        }
        one_line("an entry's license", &self.license, MAX_LICENSE_LEN)?;
        if let Some(author) = &self.author {
            one_line("an entry's author", author, MAX_AUTHOR_LEN)?;
        }
        Ok(())
    }
}

impl Index {
    /// An index of `entries` and `revoked`, signed with the private key `pkcs8`.
    pub fn build(name: &str, sequence: u64, entries: Vec<IndexEntry>, revoked: Vec<Revoked>, pkcs8: &[u8]) -> anyhow::Result<Self> {
        let name = name.trim().to_string();
        let hash = hash_content(&name, sequence, &entries, &revoked);
        let mut index = Self { format: FORMAT.into(), format_version: FORMAT_VERSION, name, sequence, entries, revoked, hash, signature: None };
        index.check_fields()?;
        index.signature = Some(signing::sign_hash(&index.hash, pkcs8)?);
        Ok(index)
    }

    /// The file's text. Refuses an index that would be too big to take back.
    pub fn to_json(&self) -> anyhow::Result<String> {
        let json = serde_json::to_string_pretty(self)?;
        if json.len() > MAX_INDEX_BYTES {
            bail!("this makes an index of {} KiB; the most an index can be is {} KiB", json.len().div_ceil(1024), MAX_INDEX_BYTES / 1024);
        }
        Ok(json)
    }

    /// Reads an index and checks it end to end: size, format, version, fields, hash and signature. The signature has to be
    /// there and good; whose it is, is `signer`'s to say (or `parse_signed_by` checks).
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        if text.len() > MAX_INDEX_BYTES {
            bail!("this file is {} KiB; an index is at most {} KiB", text.len().div_ceil(1024), MAX_INDEX_BYTES / 1024);
        }
        let loose: serde_json::Value = serde_json::from_str(text).map_err(|e| anyhow!("this is not a Warden index ({e})"))?;
        let format = loose.get("format").and_then(|v| v.as_str()).unwrap_or_default();
        if format != FORMAT {
            bail!("this is not a Warden index (its format is '{format}')");
        }
        match loose.get("formatVersion").and_then(|v| v.as_u64()) {
            Some(0) | None => bail!("this index has no valid format version"),
            Some(version) if version > u64::from(FORMAT_VERSION) => bail!("this index uses a newer format (version {version}); update Warden to read it"),
            Some(_) => {}
        }
        let index: Self = serde_json::from_value(loose).map_err(|e| anyhow!("this is not a valid Warden index ({e})"))?;
        index.check_fields()?;
        let expected = hash_content(&index.name, index.sequence, &index.entries, &index.revoked);
        if index.hash != expected {
            bail!("the index was changed or damaged: its hash does not match its content");
        }
        index.signer()?;
        Ok(index)
    }

    /// `parse`, and the index must be signed by `key` (a public key in hex): the curator the person added.
    pub fn parse_signed_by(text: &str, key: &str) -> anyhow::Result<Self> {
        let index = Self::parse(text)?;
        let signer = index.signer()?;
        let wanted = signing::normalize_public_key(key).ok_or_else(|| anyhow!("the curator's key is not a public key"))?;
        if signer != wanted {
            bail!("this index is signed by another key ({}…) than the one of the curator ({}…)", &signer[..16], &wanted[..16]);
        }
        Ok(index)
    }

    /// The key that signed the index. An index without a signature, or with one that does not check out, is an error.
    pub fn signer(&self) -> anyhow::Result<String> {
        match signing::verify_field("index", self.signature.as_deref(), &self.hash)? {
            Verification::Valid { key } => Ok(key),
            Verification::Unsigned => bail!("the index is not signed: a curator's index has to be, or there is nothing to trust"),
        }
    }

    fn check_fields(&self) -> anyhow::Result<()> {
        if self.name.trim().is_empty() {
            bail!("an index must have a name");
        }
        one_line("the name", &self.name, MAX_NAME_LEN)?;
        if self.entries.len() > MAX_ENTRIES {
            bail!("an index lists at most {MAX_ENTRIES} packages");
        }
        if self.revoked.len() > MAX_REVOKED {
            bail!("an index revokes at most {MAX_REVOKED} packages");
        }
        let mut seen = HashSet::new();
        for entry in &self.entries {
            entry.check()?;
            if !seen.insert(entry.hash.as_str()) {
                bail!("'{}' is listed twice", entry.id);
            }
        }
        for revoked in &self.revoked {
            if !is_hash(&revoked.hash) {
                bail!("a revoked hash is not a sha256 hash");
            }
            if let Some(reason) = &revoked.reason {
                one_line("a reason", reason, MAX_REASON_LEN)?;
            }
        }
        Ok(())
    }

    /// Whether the curator took `hash` back.
    pub fn is_revoked(&self, hash: &str) -> bool {
        self.revoked.iter().any(|r| r.hash == hash)
    }

    /// Why the curator took `hash` back, when they said.
    pub fn revoked_reason(&self, hash: &str) -> Option<&str> {
        self.revoked.iter().find(|r| r.hash == hash).and_then(|r| r.reason.as_deref())
    }

    /// The entry whose package has `hash`.
    pub fn entry_by_hash(&self, hash: &str) -> Option<&IndexEntry> {
        self.entries.iter().find(|e| e.hash == hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::signing::generate_keypair;
    use crate::skill::Skill;

    fn skill(name: &str) -> Skill {
        Skill {
            name: name.into(),
            description: "Draft an email.".into(),
            body: "Write it short.".into(),
            agents: Vec::new(),
            proposed: false,
            source: None,
            proposed_at: None,
            revises: None,
        }
    }

    fn package(name: &str) -> Package {
        Package::export_skill(&skill(name), "1.0.0", Some("Ana"), "CC0-1.0").unwrap()
    }

    fn entry(name: &str) -> IndexEntry {
        IndexEntry::from_package(&package(name), "https://example.org/skills/x.warden-skill.json").unwrap()
    }

    fn index() -> (Index, Vec<u8>, String) {
        let (private, public) = generate_keypair().unwrap();
        let revoked = vec![Revoked { hash: format!("sha256:{}", "ab".repeat(32)), reason: Some("it leaked a token".into()) }];
        (Index::build("Ana's picks", 7, vec![entry("email-draft"), entry("review-pr")], revoked, &private).unwrap(), private, public)
    }

    /// The index's JSON with `edit` applied, read back through `parse`.
    fn edited(index: &Index, edit: impl FnOnce(&mut serde_json::Value)) -> anyhow::Result<Index> {
        let mut value: serde_json::Value = serde_json::from_str(&index.to_json().unwrap()).unwrap();
        edit(&mut value);
        Index::parse(&value.to_string())
    }

    #[test]
    fn an_index_reads_back_and_names_its_signer() {
        let (index, _, public) = index();
        let again = Index::parse(&index.to_json().unwrap()).unwrap();
        assert_eq!(again, index);
        assert_eq!(again.signer().unwrap(), public);
        assert_eq!(again.entries.len(), 2);
        assert!(again.entry_by_hash(&package("email-draft").hash).is_some());
    }

    #[test]
    fn the_curators_key_has_to_be_the_signers() {
        let (index, _, public) = index();
        let text = index.to_json().unwrap();
        assert!(Index::parse_signed_by(&text, &public).is_ok());
        assert!(Index::parse_signed_by(&text, &public.to_ascii_uppercase()).is_ok(), "a key is compared in lowercase");
        let (_, other) = generate_keypair().unwrap();
        assert!(Index::parse_signed_by(&text, &other).unwrap_err().to_string().contains("another key"));
        assert!(Index::parse_signed_by(&text, "nope").is_err());
    }

    #[test]
    fn an_index_nobody_signed_is_refused() {
        let (index, _, _) = index();
        let err = edited(&index, |v| {
            v.as_object_mut().unwrap().remove("signature");
        })
        .unwrap_err();
        assert!(err.to_string().contains("not signed"), "{err}");
    }

    #[test]
    fn a_changed_index_is_refused_by_its_hash_and_a_resigned_one_by_its_signature() {
        let (index, _, _) = index();
        let err = edited(&index, |v| v["entries"][0]["version"] = "9.9.9".into()).unwrap_err();
        assert!(err.to_string().contains("hash does not match"), "{err}");
        // Someone who also fixes the hash still cannot make the old signature fit.
        let mut forged = index.clone();
        forged.sequence = 8;
        forged.hash = hash_content(&forged.name, forged.sequence, &forged.entries, &forged.revoked);
        let err = Index::parse(&forged.to_json().unwrap()).unwrap_err();
        assert!(err.to_string().contains("signature does not match"), "{err}");
    }

    #[test]
    fn a_format_from_the_future_or_another_file_gets_its_own_message() {
        let (index, _, _) = index();
        assert!(edited(&index, |v| v["formatVersion"] = 2.into()).unwrap_err().to_string().contains("newer format"));
        assert!(edited(&index, |v| v["formatVersion"] = 0.into()).unwrap_err().to_string().contains("no valid format version"));
        assert!(edited(&index, |v| v["format"] = "warden-package".into()).unwrap_err().to_string().contains("not a Warden index"));
        assert!(Index::parse("nope").unwrap_err().to_string().contains("not a Warden index"));
    }

    #[test]
    fn what_is_too_big_or_badly_written_is_refused_when_built() {
        let (private, _) = generate_keypair().unwrap();
        let build = |entries: Vec<IndexEntry>, revoked: Vec<Revoked>| Index::build("x", 1, entries, revoked, &private);
        assert!(build(vec![entry("a"), entry("a")], vec![]).unwrap_err().to_string().contains("twice"));
        let many: Vec<Revoked> = (0..=MAX_REVOKED).map(|i| Revoked { hash: format!("sha256:{i:064x}"), reason: None }).collect();
        assert!(build(vec![], many).unwrap_err().to_string().contains("revokes at most"));
        assert!(build(vec![], vec![Revoked { hash: "sha256:short".into(), reason: None }]).is_err());
        assert!(build(vec![], vec![Revoked { hash: format!("sha256:{}", "0".repeat(64)), reason: Some("a\nb".into()) }]).is_err());
        assert!(Index::build("  ", 1, vec![], vec![], &private).is_err());
        assert!(Index::build("two\nlines", 1, vec![], vec![], &private).is_err());
        let mut bad = entry("a");
        bad.version = "1.x".into();
        assert!(build(vec![bad], vec![]).is_err());
        let mut no_license = entry("a");
        no_license.license = " ".into();
        assert!(build(vec![no_license], vec![]).is_err());
    }

    #[test]
    fn a_source_is_https_or_arweave_and_nothing_else() {
        let id = "a".repeat(43);
        assert!(is_source("https://example.org/a.json"));
        assert!(is_source(&format!("ar://{id}")));
        assert!(is_source("http://127.0.0.1:8080/a.json") && is_source("http://localhost/a.json") && is_source("http://[::1]:9/a"));
        for bad in ["http://example.org/a.json", "http://127.0.0.1.evil.org/a", "http://localhost@evil.org/a", "http://10.0.0.1/a", "file:///etc/passwd", "ftp://example.org/a", "https://", "https:///a", "ar://short", &format!("ar://{id}x"), "https://exa mple.org/a", "ar://%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%", "javascript:alert(1)", ""] {
            assert!(!is_source(bad), "{bad}");
        }
        let mut entry = entry("a");
        entry.source = "http://example.org/a.json".into();
        assert!(Index::build("x", 1, vec![entry], vec![], &generate_keypair().unwrap().0).unwrap_err().to_string().contains("https:// address or an ar:// one"));
    }

    #[test]
    fn revocation_is_by_hash_and_carries_its_reason() {
        let (index, _, _) = index();
        let revoked = format!("sha256:{}", "ab".repeat(32));
        assert!(index.is_revoked(&revoked));
        assert_eq!(index.revoked_reason(&revoked), Some("it leaked a token"));
        assert!(!index.is_revoked(&package("email-draft").hash));
        assert_eq!(index.revoked_reason(&package("email-draft").hash), None);
    }

    #[test]
    fn an_oversized_file_is_refused_before_it_is_read() {
        let (index, _, _) = index();
        let big = format!("{{\"format\":\"warden-index\",\"pad\":\"{}\"}}", "x".repeat(MAX_INDEX_BYTES));
        assert!(Index::parse(&big).unwrap_err().to_string().contains("at most"));
        assert_eq!(index.format, FORMAT);
        assert_eq!(index.format_version, FORMAT_VERSION);
    }

    #[test]
    fn the_same_content_always_gives_the_same_hash() {
        let (index, private, _) = index();
        let again = Index::build(&index.name, index.sequence, index.entries.clone(), index.revoked.clone(), &private).unwrap();
        assert_eq!(again.hash, index.hash);
        assert_eq!(again.signature, index.signature, "an Ed25519 signature is deterministic");
        let later = Index::build(&index.name, index.sequence + 1, index.entries.clone(), index.revoked.clone(), &private).unwrap();
        assert_ne!(later.hash, index.hash);
    }
}

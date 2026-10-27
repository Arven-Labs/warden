//! Curators (P160): people the owner chose to take skill packages from. A curator publishes a signed index (`warden_core::skill::index`)
//! at an address; the owner adds the curator by a name, the key that signs the index, and where it is (TOML `[[curators]]`). Warden
//! ships none and recommends none. This is the config side, the fetch of an index and of the package one of its entries points to, and
//! the small memory of the newest index seen from each curator, so an older one someone replays is refused.
//!
//! What a curator vouches for still goes through the package's own preview and install (`skill_package`): the index only says where a
//! package is and what its hash must be, and fetching one that the curator took back, or whose bytes are not the ones listed, is refused.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use warden_core::skill::index::{is_source, Index, IndexEntry, MAX_INDEX_BYTES};
use warden_core::skill::package::{Package, MAX_PACKAGE_BYTES};
use warden_core::skill::signing::normalize_public_key;
use warden_sync::arweave::parse_address;
pub use warden_sync::{ArweaveClient, HttpsFetcher};

const MAX_NAME_CHARS: usize = 80;
const MAX_URL_CHARS: usize = 500;
/// The most curators the list can hold.
pub const MAX_CURATORS: usize = 20;

/// A curator the person takes packages from (TOML `[[curators]]`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curator {
    /// What the person calls them: it is how a screen picks one, so no two share a name.
    pub name: String,
    /// The Ed25519 public key that signs their index, 64 hex characters.
    pub key: String,
    /// Where the index is: `https://…` or `ar://<id>`.
    pub url: String,
}

/// The entries of `list` that can be used: a real key (lowercase), a name on one line, a usable address, no key or name twice. A bad
/// entry is dropped and logged, never a reason for the hub not to start.
pub fn usable_curators(list: &[Curator]) -> Vec<Curator> {
    let mut usable: Vec<Curator> = Vec::new();
    for entry in list {
        match checked(entry) {
            Ok(curator) if !usable.iter().any(|c| c.key == curator.key || c.name.eq_ignore_ascii_case(&curator.name)) => usable.push(curator),
            Ok(_) => {}
            Err(why) => tracing::warn!("ignoring a curators entry: {why}"),
        }
    }
    usable
}

fn checked(entry: &Curator) -> Result<Curator, String> {
    let name = entry.name.trim().to_string();
    if name.is_empty() {
        return Err("a curator needs a name".into());
    }
    if name.contains(['\n', '\r']) {
        return Err("a name must be on one line".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("a name can be at most {MAX_NAME_CHARS} characters"));
    }
    let key = normalize_public_key(&entry.key).ok_or_else(|| format!("'{}' is not a key: a public key is 64 hex characters (0-9, a-f)", entry.key.trim().chars().take(24).collect::<String>()))?;
    let url = entry.url.trim().to_string();
    if url.len() > MAX_URL_CHARS || !is_source(&url) {
        return Err(format!("the address of '{name}' must be an https:// address or an ar:// one"));
    }
    Ok(Curator { name, key, url })
}

/// `list` as a person saves it from a screen: the strict side of `usable_curators`. A save that has a bad entry says which, so nothing
/// the person typed vanishes.
pub fn validate_curators(list: &[Curator]) -> Result<Vec<Curator>, String> {
    if list.len() > MAX_CURATORS {
        return Err(format!("at most {MAX_CURATORS} curators can be listed"));
    }
    let mut valid: Vec<Curator> = Vec::new();
    for entry in list {
        let curator = checked(entry)?;
        if valid.iter().any(|c| c.key == curator.key) {
            return Err(format!("the key {}… is in the list twice", &curator.key[..16]));
        }
        if valid.iter().any(|c| c.name.eq_ignore_ascii_case(&curator.name)) {
            return Err(format!("the name '{}' is in the list twice", curator.name));
        }
        valid.push(curator);
    }
    Ok(valid)
}

/// Replaces `[[curators]]` in the config at `path` with `list` (checked by `validate_curators`), leaving the rest of the file as it
/// was read, and returns what was saved.
pub fn set_curators(path: &Path, list: &[Curator]) -> Result<Vec<Curator>, String> {
    let valid = validate_curators(list)?;
    let mut config = crate::load_config_from_path(path, false).map_err(|e| format!("{e:#}"))?;
    config.curators = valid.clone();
    crate::save_config(path, &config).map_err(|e| format!("{e:#}"))?;
    Ok(valid)
}

/// The index `curator` publishes, read and checked: signed by the curator's key, and not older than `last_seen` (the newest
/// `sequence` this curator has shown before, `0` when none). The address is an `ar://` one through `arweave`, anything else through
/// `fetcher`.
pub async fn fetch_index(fetcher: &HttpsFetcher, arweave: &ArweaveClient, curator: &Curator, last_seen: u64) -> Result<Index, String> {
    let bytes = fetch(fetcher, arweave, &curator.url, MAX_INDEX_BYTES).await?;
    let text = String::from_utf8(bytes).map_err(|_| "what is at the curator's address is not an index (it is not text)".to_string())?;
    let index = Index::parse_signed_by(&text, &curator.key).map_err(|e| format!("what is at the address of '{}' is not a usable index: {e:#}", curator.name))?;
    if index.sequence < last_seen {
        return Err(format!("this index of '{}' is older than one already seen (number {} against {last_seen}), so it is not used", curator.name, index.sequence));
    }
    Ok(index)
}

/// The text of the package `entry` lists, for the caller to preview and install like any other. Refused when the curator took the
/// package back, and when what is at the source is not the package with the hash the index gave.
pub async fn fetch_indexed_package(fetcher: &HttpsFetcher, arweave: &ArweaveClient, index: &Index, entry: &IndexEntry) -> Result<String, String> {
    if index.is_revoked(&entry.hash) {
        return Err(match index.revoked_reason(&entry.hash) {
            Some(reason) => format!("the curator took '{}' back: {reason}", entry.id),
            None => format!("the curator took '{}' back", entry.id),
        });
    }
    let bytes = fetch(fetcher, arweave, &entry.source, MAX_PACKAGE_BYTES).await?;
    let text = String::from_utf8(bytes).map_err(|_| "what is at that address is not a Warden package (it is not text)".to_string())?;
    let package = Package::parse(&text).map_err(|e| format!("what is at that address is not a usable Warden package: {e:#}"))?;
    if package.hash != entry.hash {
        return Err(format!("what is at the address of '{}' is not the package the curator listed (its hash is different), so it is not used", entry.id));
    }
    Ok(text)
}

async fn fetch(fetcher: &HttpsFetcher, arweave: &ArweaveClient, address: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    if address.starts_with("ar://") {
        let tx_id = parse_address(address).map_err(|e| format!("{e:#}"))?;
        arweave.fetch_tx_data_capped(&tx_id, max_bytes).await.map_err(|e| format!("{e:#}"))
    } else {
        fetcher.fetch_capped(address, max_bytes).await.map_err(|e| format!("{e:#}"))
    }
}

/// The newest `sequence` seen from each curator, by the curator's key, in a small JSON file.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SeenSequences(BTreeMap<String, u64>);

impl SeenSequences {
    /// The file at `path`; missing or unreadable reads as nothing seen (the check then only refuses what it can prove).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn get(&self, key: &str) -> u64 {
        self.0.get(key).copied().unwrap_or(0)
    }

    /// Remembers `sequence` for `key` when it is newer than what was there, and writes the file; `Ok` also when nothing changed.
    pub fn remember(&mut self, path: &Path, key: &str, sequence: u64) -> std::io::Result<()> {
        if sequence <= self.get(key) {
            return Ok(());
        }
        self.0.insert(key.to_string(), sequence);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&self.0).expect("a map of numbers serializes"))
    }
}

#[cfg(test)]
mod tests {
    use warden_core::skill::signing::generate_keypair;
    use warden_core::skill::Skill;

    use super::*;

    fn curator(name: &str, key: &str) -> Curator {
        Curator { name: name.into(), key: key.into(), url: "https://example.org/index.json".into() }
    }

    fn key(n: u8) -> String {
        format!("{n:02x}").repeat(32)
    }

    #[test]
    fn a_saved_list_has_real_keys_addresses_and_names_and_none_twice() {
        let ok = validate_curators(&[curator(" Ana ", &key(0xAB).to_ascii_uppercase()), curator("Bia", &key(2))]).unwrap();
        assert_eq!(ok[0], curator("Ana", &key(0xab)), "trimmed, with the key in lowercase");
        assert!(validate_curators(&[]).unwrap().is_empty());
        let err = |list: &[Curator]| validate_curators(list).unwrap_err();
        assert!(err(&[curator("Ana", "nope")]).contains("not a key"));
        assert!(err(&[curator(" ", &key(1))]).contains("needs a name"));
        assert!(err(&[curator("a\nb", &key(1))]).contains("one line"));
        assert!(err(&[curator(&"x".repeat(81), &key(1))]).contains("at most 80"));
        assert!(err(&[curator("Ana", &key(1)), curator("Bia", &key(1))]).contains("twice"));
        assert!(err(&[curator("Ana", &key(1)), curator("ANA", &key(2))]).contains("name 'ANA' is in the list twice"));
        let mut http = curator("Ana", &key(1));
        http.url = "http://example.org/index.json".into();
        assert!(err(&[http]).contains("https:// address or an ar:// one"));
        let many: Vec<Curator> = (0..=MAX_CURATORS as u8).map(|n| curator(&format!("c{n}"), &key(n + 1))).collect();
        assert!(err(&many).contains("at most 20"));
    }

    #[test]
    fn reading_the_file_drops_a_bad_entry_and_goes_on() {
        let list = [curator("Ana", &key(1)), curator("Bad", "nope"), curator("Ana again", &key(1)), curator("ana", &key(3)), curator("Bia", &key(4))];
        let names: Vec<_> = usable_curators(&list).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Ana", "Bia"]);
    }

    #[test]
    fn the_newest_sequence_seen_is_remembered_and_never_goes_back() {
        let dir = std::env::temp_dir().join(format!("warden-curator-seen-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("curators_seen.json");
        let mut seen = SeenSequences::load(&path);
        assert_eq!(seen.get("k"), 0);
        seen.remember(&path, "k", 5).unwrap();
        seen.remember(&path, "k", 3).unwrap();
        assert_eq!(SeenSequences::load(&path).get("k"), 5);
        seen.remember(&path, "k", 9).unwrap();
        assert_eq!(SeenSequences::load(&path).get("k"), 9);
        assert_eq!(SeenSequences::load(&path).get("other"), 0);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(SeenSequences::load(&path).get("k"), 0, "a damaged file reads as nothing seen");
        let _ = std::fs::remove_dir_all(dir);
    }

    // The fetches, against a server of our own that is reachable only because the fetcher is told to allow private addresses.
    struct World {
        base: String,
        public: String,
        index_text: String,
        package: Package,
    }

    fn skill() -> Skill {
        Skill { name: "greet".into(), description: "Greets.".into(), body: "Say hello warmly.".into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None }
    }

    async fn world(revoke: bool, sequence: u64, serve_other_package: bool) -> World {
        let package = Package::export_skill(&skill(), "1.0.0", Some("Ana"), "MIT").unwrap();
        let other = Package::export_skill(&Skill { body: "Something else.".into(), ..skill() }, "1.0.0", Some("Ana"), "MIT").unwrap();
        let served = if serve_other_package { other.to_json().unwrap() } else { package.to_json().unwrap() };
        let (private, public) = generate_keypair().unwrap();

        let router = axum::Router::new().route("/package.json", axum::routing::get(move || {
            let served = served.clone();
            async move { served }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        // The index has to name where the package is, so it is made once the address is known, and served from the same router.
        let entry = IndexEntry::from_package(&package, &format!("{base}/package.json")).unwrap();
        let revoked = if revoke { vec![warden_core::skill::index::Revoked { hash: package.hash.clone(), reason: Some("it asked for a password".into()) }] } else { vec![] };
        let index = Index::build("Ana", sequence, vec![entry], revoked, &private).unwrap();
        let index_text = index.to_json().unwrap();
        let body = index_text.clone();
        let router = router.route("/index.json", axum::routing::get(move || {
            let body = body.clone();
            async move { body }
        }));
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        World { base, public, index_text, package }
    }

    fn arweave() -> ArweaveClient {
        ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1")
    }

    fn ana(world: &World) -> Curator {
        Curator { name: "Ana".into(), key: world.public.clone(), url: format!("{}/index.json", world.base) }
    }

    #[tokio::test]
    async fn the_index_of_a_curator_is_read_and_its_package_comes_back() {
        let w = world(false, 10, false).await;
        let fetcher = HttpsFetcher::new(true);
        let index = fetch_index(&fetcher, &arweave(), &ana(&w), 0).await.unwrap();
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.sequence, 10);
        let text = fetch_indexed_package(&fetcher, &arweave(), &index, &index.entries[0]).await.unwrap();
        assert_eq!(Package::parse(&text).unwrap(), w.package);
        assert_eq!(fetch_index(&fetcher, &arweave(), &ana(&w), 10).await.unwrap().sequence, 10, "the same number is not older");
    }

    #[tokio::test]
    async fn an_index_signed_by_another_key_or_older_than_one_seen_is_refused() {
        let w = world(false, 10, false).await;
        let fetcher = HttpsFetcher::new(true);
        let mut wrong = ana(&w);
        wrong.key = key(7);
        assert!(fetch_index(&fetcher, &arweave(), &wrong, 0).await.unwrap_err().contains("another key"));
        let err = fetch_index(&fetcher, &arweave(), &ana(&w), 11).await.unwrap_err();
        assert!(err.contains("older than one already seen"), "{err}");
        // The text that was served is the one the test built: reading it directly agrees.
        assert!(Index::parse_signed_by(&w.index_text, &w.public).is_ok());
    }

    #[tokio::test]
    async fn a_package_the_curator_took_back_or_that_is_not_the_one_listed_is_refused() {
        let fetcher = HttpsFetcher::new(true);
        let taken_back = world(true, 1, false).await;
        let index = fetch_index(&fetcher, &arweave(), &ana(&taken_back), 0).await.unwrap();
        let err = fetch_indexed_package(&fetcher, &arweave(), &index, &index.entries[0]).await.unwrap_err();
        assert_eq!(err, "the curator took 'greet' back: it asked for a password");

        let swapped = world(false, 1, true).await;
        let index = fetch_index(&fetcher, &arweave(), &ana(&swapped), 0).await.unwrap();
        let err = fetch_indexed_package(&fetcher, &arweave(), &index, &index.entries[0]).await.unwrap_err();
        assert!(err.contains("not the package the curator listed"), "{err}");
    }

    #[tokio::test]
    async fn by_default_a_curator_on_this_machine_is_not_reached() {
        let w = world(false, 1, false).await;
        let err = fetch_index(&HttpsFetcher::default(), &arweave(), &ana(&w), 0).await.unwrap_err();
        assert!(err.contains("only https://"), "{err}");
    }
}

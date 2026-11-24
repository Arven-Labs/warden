//! P-49 (round 5, investigation): does `SyncEngine::arca_replace_key` lose the entries of the `secret` (MCP logins) and `config`
//! classes of the group when the device that swaps its key has no folder for them?
//!
//! The question (Sessions 127 and 129): `begin_epoch` lost them on a first attempt (`prev = None`, arca#198, fixed in arca-core
//! `499628b`). `arca_replace_key` does not call `begin_epoch`: it wipes the local replica and the state, takes a snapshot of the
//! folder as it is at the new epoch (no `prev`), pulls the hub, seeds the state with the head the person confirmed and saves the key
//! last. Is its first manifest, which lists no `secret` and no `config` (the device has no folder for them), a base that makes the
//! next merge drop what only the hub lists?
//!
//! What was read (arca-core `499628b`, `Engine::merge_inner`, and `arca_sync.rs`): the cycle after the swap finds two tips of the
//! new epoch in the local store (the snapshot of the swap, and the head pulled from the hub). `merge_inner` takes the base from
//! `common_ancestor(...)`, which is `None` for two roots (no common manifest in the graph), and falls back to an empty list
//! (`map_or(&[], ...)`). With an empty base, `merge_files` sees a path that only one side lists as created on that side, and keeps
//! it. So the hypothesis is that nothing is lost, and these tests are expected to be green: only the CI can confirm. If one is red,
//! the failing assertion names what was lost, and the code is not touched here.
//!
//! The scenario, with a real HTTP hub in this process (`arca_core::server::handle` behind an `axum` router, over a `DiskStore`, the
//! way the hub of `warden-server` serves the same contract; that hub is not reachable from this crate, because `warden-server`
//! depends on `warden-sync` and not the other way round):
//! - `b` is a plain arca engine with a folder for each of the three classes. It publishes `x.md` (vault), `settings.json` (config)
//!   and `github.json` (secret) at epoch 0. It is not a `SyncEngine`: Warden never has a folder for `config`.
//! - `a` and `d` are `SyncEngine`s with only the vault (no logins folder, so `secret_accepted` is false). Both sync until they hold
//!   the same head, which lists the three classes.
//! - `a` writes `from-a.md`, rotates to epoch 1 by `PendingRotation::confirm` (what the screens call) and syncs: the hub now holds
//!   the epoch-1 head, which carries the other classes re-encrypted (since `499628b`).
//! - `d`, which stayed at epoch 0, writes its local files, swaps the key with `arca_replace_key` (key, epoch and head code as `a`
//!   hands them over) and runs one cycle.
//!
//! The epochs here are 0 (before) and 1 (after): a key made by `init_fresh` starts at epoch 0 and one rotation makes it 1 (the task
//! text of this investigation called them 1 and 2).
//!
//! Each test states its premise first and checks it in the same run (the head of the hub at the new epoch lists the other classes,
//! `d` has no folder for them, the swap and the cycle reached the hub), then the result by opening the manifests of the hub and of
//! `d` with the keys of the epoch. The case with no local file and the case with local files are two tests so one does not hide the
//! other. A file name or a text here is not one of the fixtures of the sibling tests.
//!
//! Not here: what happens to a file that `d` EDITED while behind (the swap's snapshot and the head pulled would both list the path
//! with different texts and no common ancestor, which by my reading of `merge_files` is a conflict; not run), and a hub that hides
//! the head (`arca_mode.rs` of warden-server).
#![cfg(feature = "arca")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use arca_core::crypto::{EpochKeys, VaultKey};
use arca_core::disk::DiskStore;
use arca_core::engine::{Engine, Roots, Snapshot};
use arca_core::envelope::Header;
use arca_core::heads::{tips, Head};
use arca_core::hub::HubStore;
use arca_core::manifest::{Class, FileEntry, Manifest};
use arca_core::object::ObjectKind;
use arca_core::server;
use arca_core::state::DeviceState;
use arca_core::store::BlobStore;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use sha2::{Digest, Sha256};
use warden_sync::arca_mode::{folder_path_for, load_mode, mode_path_for, state_path_for, store_path_for};
use warden_sync::arca_rotation::PendingRotation;
use warden_sync::manifest::load_secrets;
use warden_sync::SyncEngine;

/// What the group holds. Not the fixtures of the sibling tests (`note.md`, `hello`, `github.json` with `gh-secret`).
const SHARED_NOTE: (&str, &str) = ("x.md", "Plano de retorno da frota, rev. 7: café às 9h30\n");
const SETTINGS: (&str, &str) = ("settings.json", r#"{"tema":"claro","limite":42}"#);
const LOGIN: (&str, &str) = ("github.json", r#"{"token":"tok-fake-4711"}"#);
/// Written on `a` after `d` last synced, so only `a` (and the hub, once `a` syncs) has it.
const ONLY_ON_A: (&str, &str) = ("from-a.md", "Escrito só no a, antes da rotação.\n");
/// A vault file that `d` has and the hub does not.
const ONLY_ON_D: (&str, &str) = ("local-only-d.md", "Só existe no d: três itens\n1. pão\n2. café\n3. filtro\n");

fn unique_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("warden-sync-replace-key-classes-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha256_of(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

// ---- the hub ----

/// What the hub serves from and what it was asked, so a test can tell that something reached it.
#[derive(Clone)]
struct HubState {
    store: DiskStore,
    requests: Arc<AtomicUsize>,
}

struct Hub {
    url: String,
    state: HubState,
}

/// Every call goes to `arca_core::server::handle` (the contract of `/arca/v1/`), with no authentication: the token that a
/// `HubStore` sends is not looked at, which is not what this test is about.
async fn serve_arca(State(state): State<HubState>, method: Method, uri: Uri, body: Bytes) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    let target = uri.path_and_query().map_or("", |p| p.as_str());
    let reply = server::handle(&state.store, method.as_str(), target, &body).await;
    (StatusCode::from_u16(reply.status).unwrap(), [(header::CONTENT_TYPE, reply.content_type)], reply.body).into_response()
}

async fn hub() -> Hub {
    let state = HubState { store: DiskStore::open(unique_dir("hub").join("objects")).unwrap(), requests: Arc::default() };
    let app = Router::new().fallback(serve_arca).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Hub { url: format!("http://{addr}"), state }
}

impl Hub {
    fn store_of(&self, device: &str) -> HubStore {
        HubStore::new(&self.url, &format!("token-of-{device}")).unwrap()
    }

    fn requests(&self) -> usize {
        self.state.requests.load(Ordering::SeqCst)
    }

    /// How many objects (blobs and manifests) the hub holds.
    async fn objects(&self) -> usize {
        self.state.store.list(ObjectKind::Blob).await.unwrap().len() + self.state.store.list(ObjectKind::Manifest).await.unwrap().len()
    }
}

// ---- reading manifests ----

/// The tips of the manifests of `epoch` that `store` holds, opened with the key of that epoch. A manifest of that epoch that does
/// not open fails the test, so the view never hides what is there.
async fn tips_of(store: &DiskStore, key: &[u8; 32], epoch: u32) -> Vec<Head> {
    let keys = EpochKeys::derive(&VaultKey::from_bytes(*key), epoch);
    let mut manifests = Vec::new();
    for id in store.list(ObjectKind::Manifest).await.unwrap() {
        let bytes = store.get(&id).await.unwrap().expect("a listed manifest is there");
        if Header::decode(&bytes).unwrap().0.key_epoch != epoch {
            continue;
        }
        let manifest = Manifest::open(&keys, &bytes).unwrap_or_else(|e| panic!("the manifest {id} of epoch {epoch} does not open: {e:?}"));
        manifests.push((id, manifest));
    }
    tips(manifests)
}

async fn the_one_tip(store: &DiskStore, key: &[u8; 32], epoch: u32, what: &str) -> Head {
    let mut found = tips_of(store, key, epoch).await;
    assert_eq!(found.len(), 1, "{what}: expected one tip of epoch {epoch}, found {}", found.len());
    found.remove(0)
}

/// `"<Class>:<path>"` of every entry, sorted.
fn tags(manifest: &Manifest) -> Vec<String> {
    let mut found: Vec<String> = manifest.files.iter().map(|file| format!("{:?}:{}", file.class, file.path)).collect();
    found.sort();
    found
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = names.iter().map(ToString::to_string).collect();
    found.sort();
    found
}

fn entry<'a>(manifest: &'a Manifest, class: Class, path: &str) -> &'a FileEntry {
    manifest.files.iter().find(|file| file.class == class && file.path == path).unwrap_or_else(|| panic!("{class:?}:{path} is not listed; the manifest lists {:?}", tags(manifest)))
}

// ---- the devices ----

/// A `SyncEngine` with only the vault: no logins folder is wired in (`with_oauth_dir`), and Warden never has one for `config`.
struct Device {
    base: PathBuf,
    engine: SyncEngine,
}

impl Device {
    fn vault(&self) -> PathBuf {
        self.base.join("vault")
    }

    fn manifest_path(&self) -> PathBuf {
        self.base.join("sync_manifest.json")
    }

    fn store(&self) -> DiskStore {
        DiskStore::open(store_path_for(&self.manifest_path())).unwrap()
    }

    fn write(&self, (path, text): (&str, &str)) {
        std::fs::write(self.vault().join(path), text).unwrap();
    }

    /// The files of the vault folder (it holds no subfolder in these tests), name to text.
    fn files(&self) -> BTreeMap<String, String> {
        let mut found = BTreeMap::new();
        for entry in std::fs::read_dir(self.vault()).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file(), "{:?} in the vault folder is not a file", entry.path());
            found.insert(entry.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(entry.path()).unwrap());
        }
        found
    }

    /// The name of everything under `base` (folders end in `/`), sorted.
    fn listing(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut pending = vec![self.base.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let relative = path.strip_prefix(&self.base).unwrap().to_string_lossy().into_owned();
                if entry.file_type().unwrap().is_dir() {
                    pending.push(path);
                    names.push(format!("{relative}/"));
                } else {
                    names.push(relative);
                }
            }
        }
        names.sort();
        names
    }

    fn secrets(&self) -> warden_sync::manifest::SyncSecrets {
        load_secrets(&self.base.join("sync_secrets.json")).unwrap().unwrap()
    }
}

fn device(name: &str) -> Device {
    let base = unique_dir(name);
    std::fs::create_dir_all(base.join("vault")).unwrap();
    let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json"));
    Device { base, engine }
}

fn files_of(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(path, text)| ((*path).to_string(), (*text).to_string())).collect()
}

/// A device that is not the one of a `SyncEngine`: it has a folder for each of the three classes, writes them, and publishes the
/// first manifest of the group at epoch 0 to the hub.
async fn publish_as_a_device_with_all_three_classes(key: [u8; 32], hub: &HubStore) {
    let base = unique_dir("b");
    for (class_dir, (path, text)) in [("vault", SHARED_NOTE), ("config", SETTINGS), ("secret", LOGIN)] {
        std::fs::create_dir_all(base.join(class_dir)).unwrap();
        std::fs::write(base.join(class_dir).join(path), text).unwrap();
    }
    let engine = Engine::new(DiskStore::open(base.join("store")).unwrap(), EpochKeys::derive(&VaultKey::from_bytes(key), 0), "device-b").unwrap();
    let roots = Roots { vault: Some(base.join("vault")), config: Some(base.join("config")), secret: Some(base.join("secret")) };
    let Snapshot::Created { .. } = engine.snapshot(&roots, 1).await.unwrap() else { panic!("the first snapshot of b is a new manifest") };
    let pushed = engine.push(hub).await.unwrap();
    assert!(!pushed.manifests_withheld, "the push of b was incomplete");
}

/// One cycle that must go through; the clock moves so every manifest has its own time.
async fn cycle(device: &Device, hub: &HubStore, clock: &mut u64, what: &str) -> warden_sync::arca_sync::ArcaSyncReport {
    *clock += 1;
    device.engine.arca_sync(hub, *clock).await.unwrap_or_else(|e| panic!("{what}: the cycle failed: {e:#}"))
}

/// The scenario of the module doc. `local_in_d` are the vault files `d` writes after it last synced (and before the swap).
async fn replacing_the_key_keeps_the_other_classes(name: &str, local_in_d: &[(&str, &str)]) {
    let hub = hub().await;
    let mut clock = 100;

    // ---- the group at epoch 0: b publishes the three classes, a and d (vault only) sync until they hold the same head ----
    let a = device(&format!("{name}-a"));
    a.engine.init_fresh().unwrap();
    let key_0 = a.engine.export_vault_key().unwrap();
    publish_as_a_device_with_all_three_classes(key_0, &hub.store_of("b")).await;
    clock += 1;
    a.engine.arca_enable(false, clock).await.unwrap();
    let (a_hub, d_hub) = (hub.store_of("a"), hub.store_of("d"));
    cycle(&a, &a_hub, &mut clock, "a, first").await;

    let d = device(&format!("{name}-d"));
    d.engine.adopt_vault_key(key_0, 0).unwrap();
    clock += 1;
    d.engine.arca_enable(false, clock).await.unwrap();
    cycle(&d, &d_hub, &mut clock, "d, first").await;
    cycle(&a, &a_hub, &mut clock, "a, second").await;

    let head_0 = the_one_tip(&hub.state.store, &key_0, 0, "the hub before the rotation").await;
    assert_eq!(tags(&head_0.manifest), sorted(&["Vault:x.md", "Config:settings.json", "Secret:github.json"]), "premise: the head of the hub at epoch 0 lists the three classes");
    assert_eq!(d.files(), files_of(&[SHARED_NOTE]), "premise: d has the shared note and nothing else in its vault");

    // ---- a rotates to epoch 1 and syncs ----
    a.write(ONLY_ON_A);
    let rotation = PendingRotation::generate().unwrap();
    let phrase = rotation.phrase().unwrap();
    let words: Vec<&str> = phrase.words().collect();
    let typed: Vec<&str> = rotation.positions().iter().map(|position| words[position - 1]).collect();
    clock += 1;
    rotation.confirm(&typed, true, &a.engine, clock).await.unwrap();
    cycle(&a, &a_hub, &mut clock, "a, after the rotation").await;
    let (key_1, epoch_1, seed) = (a.secrets().vault_key, a.secrets().key_epoch, a.engine.arca_head_id().await.unwrap());
    assert_eq!(epoch_1, 1, "premise: a rotated once");
    assert_ne!(key_1, key_0, "premise: a has another key");

    // PREMISE 1: the head of the hub at the new epoch lists the other classes (the new epoch carries them re-encrypted), and it is
    // the head `a` shows as its head code.
    let head_before = the_one_tip(&hub.state.store, &key_1, epoch_1, "the hub after the rotation").await;
    assert_eq!(head_before.id, seed, "premise: the head code that a hands over is the head of the hub");
    assert_eq!(
        tags(&head_before.manifest),
        sorted(&["Vault:x.md", "Vault:from-a.md", "Config:settings.json", "Secret:github.json"]),
        "premise: the epoch-1 head of the hub lists the config and the secret entries"
    );
    assert_eq!(entry(&head_before.manifest, Class::Config, SETTINGS.0).sha256.0, sha256_of(SETTINGS.1), "premise: the config entry is the one b wrote");
    assert_eq!(entry(&head_before.manifest, Class::Secret, LOGIN.0).sha256.0, sha256_of(LOGIN.1), "premise: the secret entry is the one b wrote");

    // PREMISE 2: d stayed at epoch 0, has no folder for those classes, and sees the new epoch.
    d.engine.arca_sync(&d_hub, clock + 1).await.expect_err("d, still at epoch 0, stops on the newer epoch");
    assert!(!d.engine.arca_pending_epochs().await.unwrap().is_empty(), "premise: d sees the newer epoch");
    assert_eq!(d.secrets().key_epoch, 0, "premise: d is still at epoch 0");
    assert!(!load_mode(&mode_path_for(&d.manifest_path())).unwrap().expect("d is in arca mode").secret_accepted, "premise: d did not accept the logins");
    assert!(!d.base.join("mcp_oauth").exists(), "premise: d has no logins folder");
    for local in local_in_d {
        d.write(*local);
    }
    let expected_in_d_before: Vec<(&str, &str)> = [SHARED_NOTE].into_iter().chain(local_in_d.iter().copied()).collect();
    assert_eq!(d.files(), files_of(&expected_in_d_before), "premise: d's vault is the shared note and its own files");
    for forbidden in ["settings", "github", "conflito", "mcp_oauth", "arca-conflicts/"] {
        assert!(d.listing().iter().all(|name| !name.contains(forbidden)), "premise: nothing of d is named like {forbidden}: {:?}", d.listing());
    }

    // ---- the swap and one cycle ----
    let (requests_before, objects_before) = (hub.requests(), hub.objects().await);
    clock += 1;
    d.engine.arca_replace_key(&d_hub, key_1, epoch_1, seed, clock).await.expect("the swap goes through");
    assert!(hub.requests() > requests_before, "premise: the swap talked to the hub (it pulls)");
    let state = DeviceState::load(&state_path_for(&d.manifest_path())).unwrap().expect("the swap seeds the state");
    assert_eq!((state.key_epoch, state.last_accepted.0), (epoch_1, seed.hash), "the state holds the head the person confirmed");
    assert_eq!(d.secrets().vault_key, key_1, "the swap saved the new key");
    assert!(d.engine.arca_pending_epochs().await.unwrap().is_empty(), "nothing newer is pending after the swap");

    let report = cycle(&d, &d_hub, &mut clock, "d, after the swap").await;

    // PREMISE 3: the cycle reached the hub (d pushed what it wrote: a merge manifest at least), so the head below is not the old one
    // read twice.
    assert!(hub.objects().await > objects_before, "premise: the cycle after the swap wrote objects to the hub");
    let head_after = the_one_tip(&hub.state.store, &key_1, epoch_1, "the hub after d's cycle").await;
    assert_ne!(head_after.id, head_before.id, "premise: the hub has a new head, written by d");
    assert_eq!(head_after.manifest.device, d.secrets().device_id, "premise: the new head is d's");

    // RESULT: the hub's head still lists the other classes, with the same content; nothing that was listed is gone; what d had is up.
    let mut expected_tags = vec!["Vault:x.md".to_string(), "Vault:from-a.md".to_string(), "Config:settings.json".to_string(), "Secret:github.json".to_string()];
    expected_tags.extend(local_in_d.iter().map(|(path, _)| format!("Vault:{path}")));
    expected_tags.sort();
    assert_eq!(tags(&head_after.manifest), expected_tags, "the hub's head after the swap and one cycle");
    for before in &head_before.manifest.files {
        let after = entry(&head_after.manifest, before.class, &before.path);
        assert_eq!(after.sha256, before.sha256, "{:?}:{} changed its content", before.class, before.path);
    }
    assert_eq!(entry(&head_after.manifest, Class::Config, SETTINGS.0).sha256.0, sha256_of(SETTINGS.1));
    assert_eq!(entry(&head_after.manifest, Class::Secret, LOGIN.0).sha256.0, sha256_of(LOGIN.1));
    for (path, text) in local_in_d {
        assert_eq!(entry(&head_after.manifest, Class::Vault, path).sha256.0, sha256_of(text), "{path}: d's file went up with its own content");
    }

    // d's own head is the hub's, and its folder pointer follows it.
    let head_of_d = the_one_tip(&d.store(), &key_1, epoch_1, "d's own store").await;
    assert_eq!(head_of_d.id, head_after.id, "d's head is the head of the hub");
    assert_eq!(std::fs::read_to_string(folder_path_for(&d.manifest_path())).unwrap().trim(), head_after.id.to_string(), "the folder pointer of d follows the head");

    // No conflict, no copy, and nothing of the other classes written under the device's folder.
    assert_eq!(report.conflicts, 0, "no conflict was settled: {report:?}");
    assert_eq!(report.secret_losers_saved, 0, "{report:?}");
    let mut expected_in_d: Vec<(&str, &str)> = vec![SHARED_NOTE, ONLY_ON_A];
    expected_in_d.extend(local_in_d.iter().copied());
    assert_eq!(d.files(), files_of(&expected_in_d), "d's vault: the shared note, what only a had, and d's own files (no `.conflito-*` copy)");
    for forbidden in ["settings", "github", "conflito", "mcp_oauth"] {
        assert!(d.listing().iter().all(|name| !name.contains(forbidden)), "d's folder holds something named like {forbidden}: {:?}", d.listing());
    }
    assert!(d.listing().iter().all(|name| !name.starts_with("arca-conflicts/")), "the conflicts folder of d holds a copy: {:?}", d.listing());
    assert!(!d.base.join("mcp_oauth").exists(), "d did not get a logins folder it never had");

    // The vault of d converges with a's.
    cycle(&a, &a_hub, &mut clock, "a, after d's cycle").await;
    assert_eq!(a.files(), d.files(), "the vault of a and the vault of d are the same");
    let head_last = the_one_tip(&hub.state.store, &key_1, epoch_1, "the hub after a's last cycle").await;
    assert_eq!(head_last.id, head_after.id, "a took d's head as it was, and wrote nothing over it");
}

#[tokio::test]
async fn a_device_that_swaps_its_key_leaves_the_secret_and_config_entries_on_the_hub() {
    replacing_the_key_keeps_the_other_classes("swap-plain", &[]).await;
}

/// d has a vault file that the hub does not: it goes up with the first cycle and the other classes stay.
#[tokio::test]
async fn a_local_vault_file_of_the_swapping_device_goes_up_and_the_other_classes_stay() {
    replacing_the_key_keeps_the_other_classes("swap-local", &[ONLY_ON_D]).await;
}

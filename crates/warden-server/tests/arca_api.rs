//! The arca storage routes (`/arca/v1/`) on the hub: an approved device keeps encrypted objects here for its other
//! devices to pull (arca's phase 2b-2). The contract is `arca_core::server`; what these tests pin is what the hub adds:
//! who may call (the device token, checked before the body is read), whose folder it is (one per owner), and how much a
//! caller can cost (a body cap that is enforced before reading, a byte quota per owner, a cap on requests at once).
//! Spoken to with plain HTTP over a real socket, and with arca's own client (`HubStore`) as the second witness.

mod support;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arca_core::backend::Backend;
use arca_core::disk::DiskStore;
use arca_core::engine::{Engine, Roots, Snapshot};
use arca_core::hub::HubStore;
use arca_core::server::{MAX_BLOB_OBJECT_BYTES, MAX_MANIFEST_OBJECT_BYTES};
use arca_core::store::BlobStore;
use arca_core::{open, seal, Alg, EpochKeys, Header, ObjectId, ObjectKind, VaultKey, FORMAT_VERSION, NONCE_LEN};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use warden_core::memory::Vault;
use warden_core::orchestrator::Orchestrator;
use warden_server::arca_api::ArcaLimits;
use warden_server::arca_pairing::{key_fingerprint, verification_code};
use warden_truthid::crypto::{ecies_decrypt, ecies_encrypt, generate_ecies_keypair};
use warden_server::settings::WRONG_KEY_DELAY;
use warden_server::{PairingProof, PairingStore, Server};

struct Hub {
    addr: SocketAddr,
    devices: PairingStore,
    arca_dir: PathBuf,
}

/// A hub with the arca routes on (`Some(limits)`) or off (`None`).
async fn hub(arca: Option<ArcaLimits>) -> Hub {
    let dir = support::unique_temp_dir("warden-server-arca");
    let orchestrator = Orchestrator::new(Arc::new(support::MockProvider::replying("hi")), Arc::new(Vault::new(dir.join("vault"))));
    let devices_path = dir.join("devices.json");
    let arca_dir = dir.join("arca");
    let mut server = Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), dir.join("conversations"), devices_path.clone())
        .await
        .unwrap();
    if let Some(limits) = arca {
        server = server.with_arca_limits(arca_dir.clone(), limits);
    }
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Hub { addr, devices: PairingStore::new(devices_path), arca_dir }
}

impl Hub {
    /// The token of a device of the root (the first one pairs with the key, the rest with an invite), not approved yet.
    fn root_device(&self, id: &str) -> String {
        let proof = if self.devices.list().unwrap().is_empty() {
            PairingProof::PairingKey
        } else {
            PairingProof::Invite(self.devices.create_invite("test", Duration::from_secs(60)).unwrap())
        };
        self.devices.authenticate_as(id, id, None, proof).unwrap().unwrap().issued_token.unwrap()
    }

    /// The token of a device of the member `user`, not approved yet.
    fn member_device(&self, id: &str, user: &str) -> String {
        self.devices.authenticate_as(id, id, None, PairingProof::Member(user.into())).unwrap().unwrap().issued_token.unwrap()
    }

    fn approved_root(&self, id: &str) -> String {
        let token = self.root_device(id);
        self.devices.approve(id).unwrap();
        token
    }

    fn approved_member(&self, id: &str, user: &str) -> String {
        let token = self.member_device(id, user);
        self.devices.approve(id).unwrap();
        token
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<&[u8]>) -> Response {
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n", self.addr);
        if let Some(token) = token {
            head.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        if let Some(body) = body {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        let mut request = head.into_bytes();
        request.extend_from_slice(body.unwrap_or_default());
        self.raw(&request).await
    }

    /// The bytes of a whole request as given; the answer is read to the end (the hub closes after each one).
    async fn raw(&self, request: &[u8]) -> Response {
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        stream.write_all(request).await.unwrap();
        read_response(&mut stream).await
    }
}

struct Response {
    status: u16,
    head: String,
    body: Vec<u8>,
}

async fn read_response<R: tokio::io::AsyncRead + Unpin>(stream: &mut R) -> Response {
    let mut raw = Vec::new();
    // A caller that never gets an answer fails the test instead of hanging it.
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw)).await.expect("no answer within 10 s").unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or_else(|| panic!("not an HTTP answer: {:?}", String::from_utf8_lossy(&raw)));
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    Response { status, head, body: raw[split + 4..].to_vec() }
}

/// An object with a valid envelope of `kind` (the content is opaque to the hub, which has no key).
fn object(kind: ObjectKind, content: &[u8]) -> (ObjectId, Vec<u8>) {
    let header = Header { format_version: FORMAT_VERSION as u16, alg: Alg::XChaCha20Poly1305, kind, key_epoch: 1, nonce: [7u8; NONCE_LEN] };
    let mut bytes = header.encode().to_vec();
    bytes.extend_from_slice(content);
    (ObjectId::of(kind, &bytes), bytes)
}

fn path(id: &ObjectId) -> String {
    format!("/arca/v1/objects/{id}")
}

fn list_path(kind: &str) -> String {
    format!("/arca/v1/objects?kind={kind}")
}

fn ids(response: &Response) -> Vec<String> {
    assert_eq!(response.status, 200, "{}", String::from_utf8_lossy(&response.body));
    let json: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    json["ids"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
}

/// Opens a `PUT` for an object of `size` bytes, sends part of the body and keeps the connection: the hub is left waiting
/// for the rest, which is a request being served.
async fn hold_put(hub: &Hub, token: &str, id: &ObjectId, size: usize) -> TcpStream {
    let mut stream = TcpStream::connect(hub.addr).await.unwrap();
    let head = format!("PUT {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {token}\r\nContent-Length: {size}\r\n\r\n", path(id), hub.addr);
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(&[0u8; 10]).await.unwrap();
    stream
}

/// Calls until the hub answers something other than `503`, or `want_busy`: until it answers `503`. Gives the hub time to
/// have registered what the test did before (a held request, a closed connection).
async fn poll(hub: &Hub, token: &str, want_busy: bool) -> Response {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let response = hub.call("GET", &list_path("blob"), Some(token), None).await;
        if (response.status == 503) == want_busy {
            return response;
        }
        assert!(Instant::now() < deadline, "the hub never became {} (last answer {})", if want_busy { "busy" } else { "free" }, response.status);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// What an approved device can do
// ---------------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_approved_device_stores_and_reads_back_an_object() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.approved_root("dev-a");
    let (id, bytes) = object(ObjectKind::Blob, b"some encrypted content");

    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 204);
    let got = hub.call("GET", &path(&id), Some(&token), None).await;
    assert_eq!((got.status, got.body), (200, bytes));
    assert_eq!(hub.call("HEAD", &path(&id), Some(&token), None).await.status, 200);
    let (missing, _) = object(ObjectKind::Blob, b"never sent");
    assert_eq!(hub.call("GET", &path(&missing), Some(&token), None).await.status, 404);
    assert_eq!(ids(&hub.call("GET", &list_path("blob"), Some(&token), None).await).len(), 1);
}

#[tokio::test]
async fn arcas_own_client_works_against_the_real_hub_and_two_devices_converge() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let remote = Backend::Hub(HubStore::new(&hub.url(), &hub.approved_root("dev-a")).unwrap());

    // The client's own operations.
    let (id, bytes) = object(ObjectKind::Manifest, b"a manifest");
    remote.put(&id, &bytes).await.unwrap();
    assert!(remote.has(&id).await.unwrap());
    assert_eq!(remote.get(&id).await.unwrap(), Some(bytes));
    assert_eq!(remote.list(ObjectKind::Manifest).await.unwrap(), vec![id]);

    // Two devices: A publishes a snapshot, B pulls it and ends up with the same tip.
    let base = support::unique_temp_dir("warden-server-arca-engine");
    let device = |name: &str| {
        let vault = base.join(format!("{name}-vault"));
        std::fs::create_dir_all(&vault).unwrap();
        let local = Backend::Disk(DiskStore::open(base.join(format!("{name}-store"))).unwrap());
        let keys = EpochKeys::derive(&VaultKey::from_bytes([3u8; 32]), 1);
        (Engine::new(local, keys, name).unwrap(), Roots { vault: Some(vault.clone()), config: None, secret: None }, vault)
    };
    let (a, a_roots, a_vault) = device("a");
    let (b, _, _) = device("b");
    std::fs::write(a_vault.join("note.md"), b"hello").unwrap();
    let Snapshot::Created { id: tip, .. } = a.snapshot(&a_roots, 1).await.unwrap() else { panic!("expected a new snapshot") };
    a.push(&remote).await.unwrap();
    b.pull(&remote).await.unwrap();
    let heads = b.heads().await.unwrap();
    assert!(heads.rejected.is_empty());
    assert_eq!(heads.tips.iter().map(|t| t.id).collect::<Vec<_>>(), vec![tip]);
    let _ = std::fs::remove_dir_all(&base);
}

#[tokio::test]
async fn the_routes_answer_404_when_the_hub_does_not_offer_arca() {
    let hub = hub(None).await;
    let token = hub.approved_root("dev-a");
    let (id, bytes) = object(ObjectKind::Blob, b"x");
    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 404);
    assert_eq!(hub.call("GET", &list_path("blob"), Some(&token), None).await.status, 404);
}

// ---------------------------------------------------------------------------------------------------------------------
// Who may call
// ---------------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_call_without_a_valid_token_is_401_and_waits_like_a_wrong_key() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.approved_root("dev-a");
    let (id, bytes) = object(ObjectKind::Blob, b"x");
    let (hub, bytes) = (&hub, &bytes);
    let wrong = [None, Some("not-a-token".to_string()), Some(format!("{token}x")), Some(String::new())];
    let calls = wrong.iter().map(|wrong| async move {
        let started = Instant::now();
        let response = match wrong {
            Some(t) => hub.call("PUT", &path(&id), Some(t), Some(bytes)).await,
            None => hub.call("PUT", &path(&id), None, Some(bytes)).await,
        };
        (response.status, started.elapsed())
    });
    for (status, elapsed) in futures_util::future::join_all(calls).await {
        assert_eq!(status, 401);
        assert!(elapsed >= WRONG_KEY_DELAY, "answered in {elapsed:?}, before the wrong-key delay");
    }
    // Another scheme than Bearer is no token either, and nothing was stored.
    let basic = hub.raw(format!("PUT {} HTTP/1.1\r\nHost: x\r\nAuthorization: Basic {token}\r\nContent-Length: 0\r\n\r\n", path(&id)).as_bytes()).await;
    assert_eq!(basic.status, 401);
    assert!(ids(&hub.call("GET", &list_path("blob"), Some(&token), None).await).is_empty());
}

#[tokio::test]
async fn a_pending_device_is_403_and_so_is_a_revoked_one_on_its_very_next_call() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.root_device("dev-a");
    let (id, bytes) = object(ObjectKind::Blob, b"x");
    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 403);

    hub.devices.approve("dev-a").unwrap();
    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 204);

    hub.devices.revoke("dev-a").unwrap();
    assert_eq!(hub.call("GET", &path(&id), Some(&token), None).await.status, 403);
    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 403);
}

// ---------------------------------------------------------------------------------------------------------------------
// Whose folder it is
// ---------------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn owners_never_see_each_others_objects_and_devices_of_one_owner_share_them() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let root = hub.approved_root("dev-root");
    let alice_1 = hub.approved_member("alice-1", "alice");
    let alice_2 = hub.approved_member("alice-2", "alice");
    let bob = hub.approved_member("bob-1", "bob");
    let (id, bytes) = object(ObjectKind::Blob, b"alice's");

    assert_eq!(hub.call("PUT", &path(&id), Some(&alice_1), Some(&bytes)).await.status, 204);
    // Her other device reads it...
    assert_eq!(hub.call("GET", &path(&id), Some(&alice_2), None).await.body, bytes);
    // ...nobody else does, even knowing the id: not the root, not another member.
    for other in [&root, &bob] {
        assert_eq!(hub.call("GET", &path(&id), Some(other), None).await.status, 404);
        assert_eq!(hub.call("HEAD", &path(&id), Some(other), None).await.status, 404);
        assert!(ids(&hub.call("GET", &list_path("blob"), Some(other), None).await).is_empty());
    }
    // And what they store does not show up for her.
    let (other_id, other_bytes) = object(ObjectKind::Blob, b"bob's");
    assert_eq!(hub.call("PUT", &path(&other_id), Some(&bob), Some(&other_bytes)).await.status, 204);
    assert_eq!(ids(&hub.call("GET", &list_path("blob"), Some(&alice_1), None).await).len(), 1);
}

#[tokio::test]
async fn a_members_name_never_becomes_a_path() {
    let hub = hub(Some(ArcaLimits::default())).await;
    hub.approved_root("dev-root");
    let (id, bytes) = object(ObjectKind::Blob, b"x");
    for (n, user) in ["../../evil", "a/b", "..", "/abs", "ünï\u{0}code"].into_iter().enumerate() {
        let token = hub.approved_member(&format!("m-{n}"), user);
        assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 204, "member {user:?}");
    }
    // Everything landed inside the arca folder, in names that are fixed shapes: `owner` or `member-` and 64 hex digits.
    let mut folders = std::fs::read_dir(&hub.arca_dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect::<Vec<_>>();
    folders.sort();
    assert_eq!(folders.len(), 5, "{folders:?}");
    for name in &folders {
        let hex = name.strip_prefix("member-").unwrap_or_else(|| panic!("unexpected folder {name:?}"));
        assert!(hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{name:?}");
    }
    assert!(!hub.arca_dir.parent().unwrap().join("evil").exists());
}

// ---------------------------------------------------------------------------------------------------------------------
// What a caller can cost
// ---------------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_body_over_the_cap_is_413_before_any_of_it_is_read() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.approved_root("dev-a");
    let blob = ObjectId::of(ObjectKind::Blob, b"x");
    let manifest = ObjectId::of(ObjectKind::Manifest, b"x");
    // Only the head is sent: if the hub waited for the body, this would hang until the test's timeout.
    for (id, over) in [(blob, MAX_BLOB_OBJECT_BYTES + 1), (manifest, MAX_MANIFEST_OBJECT_BYTES + 1)] {
        let head = format!("PUT {} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Length: {over}\r\n\r\n", path(&id));
        assert_eq!(hub.raw(head.as_bytes()).await.status, 413);
    }
}

#[tokio::test]
async fn an_object_exactly_at_the_cap_is_accepted() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.approved_root("dev-a");
    // The cap is on the whole object (envelope included): the content takes what is left of it.
    let (_, empty) = object(ObjectKind::Manifest, b"");
    let content = vec![0u8; MAX_MANIFEST_OBJECT_BYTES as usize - empty.len()];
    let (id, bytes) = object(ObjectKind::Manifest, &content);
    assert_eq!(bytes.len() as u64, MAX_MANIFEST_OBJECT_BYTES);
    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 204);
}

#[tokio::test]
async fn a_stranger_cannot_make_the_hub_read_a_body() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let blob = ObjectId::of(ObjectKind::Blob, b"x");
    // The proof comes first: with no (or a wrong) token the answer is 401 whatever the declared size.
    let head = format!("PUT {} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n", path(&blob), MAX_BLOB_OBJECT_BYTES);
    assert_eq!(hub.raw(head.as_bytes()).await.status, 401);
}

#[tokio::test]
async fn a_put_needs_a_content_length_and_a_get_takes_no_body() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.approved_root("dev-a");
    let (id, bytes) = object(ObjectKind::Blob, b"x");
    let auth = format!("Authorization: Bearer {token}");

    let no_length = hub.raw(format!("PUT {} HTTP/1.1\r\nHost: x\r\n{auth}\r\n\r\n", path(&id)).as_bytes()).await;
    assert_eq!(no_length.status, 411);
    let chunked = hub.raw(format!("PUT {} HTTP/1.1\r\nHost: x\r\n{auth}\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", path(&id)).as_bytes()).await;
    assert_eq!(chunked.status, 411);
    assert_eq!(hub.call("GET", &path(&id), Some(&token), Some(b"junk")).await.status, 400);
    // Nothing of that was stored, and the right call still works.
    assert_eq!(hub.call("HEAD", &path(&id), Some(&token), None).await.status, 404);
    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 204);
}

#[tokio::test]
async fn the_status_of_a_call_that_is_not_an_object_put_comes_from_arca_not_from_the_cap() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.approved_root("dev-a");
    let huge = MAX_BLOB_OBJECT_BYTES * 2;
    let auth = format!("Authorization: Bearer {token}");
    for (target, want) in [
        ("/arca/v1/objects?kind=blob".to_string(), 405),   // a PUT on the list
        ("/arca/v1/objects/blob/not-an-id".to_string(), 400), // a PUT with no valid id
        ("/arca/v1/nothing".to_string(), 404),
    ] {
        let head = format!("PUT {target} HTTP/1.1\r\nHost: x\r\n{auth}\r\nContent-Length: {huge}\r\n\r\n");
        assert_eq!(hub.raw(head.as_bytes()).await.status, want, "{target}");
    }
}

#[tokio::test]
async fn a_put_of_junk_is_400_and_stores_nothing() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.approved_root("dev-a");
    let junk = b"not an arca envelope, but its hash matches".to_vec();
    let id = ObjectId::of(ObjectKind::Blob, &junk);
    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&junk)).await.status, 400);
    let (other, _) = object(ObjectKind::Blob, b"another");
    let (_, bytes) = object(ObjectKind::Blob, b"this one");
    assert_eq!(hub.call("PUT", &path(&other), Some(&token), Some(&bytes)).await.status, 400, "hash that is not the id's");
    assert!(ids(&hub.call("GET", &list_path("blob"), Some(&token), None).await).is_empty());
}

#[tokio::test]
async fn a_very_long_target_is_414() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let token = hub.approved_root("dev-a");
    let long = format!("/arca/v1/objects?kind=blob&after={}", "a".repeat(4096));
    assert_eq!(hub.call("GET", &long, Some(&token), None).await.status, 414);
}

#[tokio::test]
async fn an_owner_cannot_keep_more_bytes_than_the_quota_and_the_rest_is_untouched() {
    // An object here is 36 bytes of envelope plus 64 of content: 100. A file takes a whole 4 KiB block on disk, and the quota
    // counts blocks, so 8 KiB hold exactly two of them.
    let hub = hub(Some(ArcaLimits { max_owner_bytes: 8192, ..ArcaLimits::default() })).await;
    let alice = hub.approved_member("alice-1", "alice");
    let bob = hub.approved_member("bob-1", "bob");
    let objects: Vec<_> = (0..3u8).map(|n| object(ObjectKind::Blob, &[n; 64])).collect();
    assert!(objects.iter().all(|(_, bytes)| bytes.len() == 100));

    assert_eq!(hub.call("PUT", &path(&objects[0].0), Some(&alice), Some(&objects[0].1)).await.status, 204);
    // Exactly at the quota is still allowed...
    assert_eq!(hub.call("PUT", &path(&objects[1].0), Some(&alice), Some(&objects[1].1)).await.status, 204);
    // ...one object more is not, and a refused one is not stored.
    assert_eq!(hub.call("PUT", &path(&objects[2].0), Some(&alice), Some(&objects[2].1)).await.status, 507);
    assert_eq!(hub.call("GET", &path(&objects[2].0), Some(&alice), None).await.status, 404);
    // What she already keeps stays readable, and sending it again costs nothing.
    assert_eq!(hub.call("GET", &path(&objects[0].0), Some(&alice), None).await.body, objects[0].1);
    assert_eq!(hub.call("PUT", &path(&objects[0].0), Some(&alice), Some(&objects[0].1)).await.status, 204);
    // Another owner has a quota of their own.
    assert_eq!(hub.call("PUT", &path(&objects[2].0), Some(&bob), Some(&objects[2].1)).await.status, 204);
}

#[tokio::test]
async fn many_tiny_objects_cost_a_block_each_not_their_few_bytes() {
    let hub = hub(Some(ArcaLimits { max_owner_bytes: 4 * 4096, ..ArcaLimits::default() })).await;
    let token = hub.approved_root("dev-a");
    // The smallest object the hub accepts is the bare envelope (36 bytes, nothing after it); the nonce is what makes each one a
    // new object. Four of them fill the quota, and the fifth is past it.
    for n in 0..5u8 {
        let mut header = Header { format_version: FORMAT_VERSION as u16, alg: Alg::XChaCha20Poly1305, kind: ObjectKind::Blob, key_epoch: 1, nonce: [7u8; NONCE_LEN] };
        header.nonce[0] = n;
        let bytes = header.encode().to_vec();
        assert_eq!(bytes.len(), 36);
        let id = ObjectId::of(ObjectKind::Blob, &bytes);
        let status = hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status;
        assert_eq!(status, if n < 4 { 204 } else { 507 }, "object {n}");
    }
}

#[tokio::test]
async fn a_body_that_stops_arriving_is_cut_after_the_idle_timeout() {
    let hub = hub(Some(ArcaLimits { io_idle_timeout: Duration::from_millis(300), ..ArcaLimits::default() })).await;
    let token = hub.approved_root("dev-a");
    let (id, _) = object(ObjectKind::Blob, b"x");
    let mut stream = hold_put(&hub, &token, &id, 1_000_000).await;
    let started = Instant::now();
    let response = read_response(&mut stream).await;
    assert_eq!(response.status, 408);
    assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
}

#[tokio::test]
async fn a_body_that_trickles_in_is_cut_too() {
    let hub = hub(Some(ArcaLimits { io_idle_timeout: Duration::from_millis(400), ..ArcaLimits::default() })).await;
    let token = hub.approved_root("dev-a");
    let (id, _) = object(ObjectKind::Blob, b"x");
    let head = format!("PUT {} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Length: 1000000\r\n\r\n", path(&id));
    let (mut read, mut write) = TcpStream::connect(hub.addr).await.unwrap().into_split();
    write.write_all(head.as_bytes()).await.unwrap();
    // A byte every 100 ms never lets the idle timeout run out, but it is far below the minimum rate.
    let dripper = tokio::spawn(async move {
        for _ in 0..100 {
            if write.write_all(b"z").await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    let started = Instant::now();
    let response = read_response(&mut read).await;
    dripper.abort();
    assert_eq!(response.status, 408);
    assert!(started.elapsed() < Duration::from_secs(4), "took {:?}", started.elapsed());
}

#[tokio::test]
async fn a_client_that_never_reads_its_answer_does_not_keep_its_slot() {
    let hub = hub(Some(ArcaLimits { max_concurrent_per_owner: 1, io_idle_timeout: Duration::from_millis(300), ..ArcaLimits::default() })).await;
    let token = hub.approved_root("dev-a");
    // An answer far bigger than the socket buffers on both sides, so writing it has to wait for the reader.
    let (id, bytes) = object(ObjectKind::Manifest, &vec![1u8; 60 * 1024 * 1024]);
    assert_eq!(hub.call("PUT", &path(&id), Some(&token), Some(&bytes)).await.status, 204);

    let mut stalled = TcpStream::connect(hub.addr).await.unwrap();
    stalled.write_all(format!("GET {} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\n\r\n", path(&id)).as_bytes()).await.unwrap();
    // First it holds the slot (the other call is told to come back), then the hub gives up on it and frees it.
    assert_eq!(poll(&hub, &token, true).await.status, 503);
    assert_eq!(poll(&hub, &token, false).await.status, 200);
    drop(stalled);
}

#[tokio::test]
async fn an_owner_with_a_request_in_flight_is_told_to_come_back_and_others_are_not() {
    let hub = hub(Some(ArcaLimits { max_concurrent_per_owner: 1, ..ArcaLimits::default() })).await;
    let alice = hub.approved_member("alice-1", "alice");
    let alice_too = hub.approved_member("alice-2", "alice");
    let bob = hub.approved_member("bob-1", "bob");
    let (id, _) = object(ObjectKind::Blob, b"x");

    let held = hold_put(&hub, &alice, &id, 100).await;
    let busy = poll(&hub, &alice_too, true).await;
    assert!(busy.head.to_ascii_lowercase().contains("retry-after:"), "{}", busy.head);
    // Bob is not slowed by Alice's request.
    assert_eq!(hub.call("GET", &list_path("blob"), Some(&bob), None).await.status, 200);

    // When her connection is gone the hub frees the slot.
    drop(held);
    assert_eq!(poll(&hub, &alice, false).await.status, 200);
}

#[tokio::test]
async fn the_whole_hub_has_a_cap_on_requests_in_flight_too() {
    let hub = hub(Some(ArcaLimits { max_concurrent: 1, ..ArcaLimits::default() })).await;
    let alice = hub.approved_member("alice-1", "alice");
    let bob = hub.approved_member("bob-1", "bob");
    let (id, _) = object(ObjectKind::Blob, b"x");

    let held = hold_put(&hub, &alice, &id, 100).await;
    assert_eq!(poll(&hub, &bob, true).await.status, 503);
    drop(held);
    assert_eq!(poll(&hub, &bob, false).await.status, 200);
}

// ---------------------------------------------------------------------------------------------------------------------
// Handing the vault key to a new device (arca's phase 2b-3): B offers a public key, A encrypts the key to it, B reads it once
// ---------------------------------------------------------------------------------------------------------------------

const OFFER: &str = "/arca/v1/pairing/offer";
const OFFERS: &str = "/arca/v1/pairing/offers";
const GRANT: &str = "/arca/v1/pairing/grant";

fn offer_body(public_key: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "public_key": public_key })).unwrap()
}

/// A grant names the device and the key the giver saw in its offer: the hub refuses it if the offer has changed since.
fn grant_path(device_id: &str, key: &str) -> String {
    format!("{GRANT}/{device_id}?key={key}")
}

/// The same key in the uncompressed SEC1 form (`04` + x + y), made from a compressed one.
fn uncompressed(public_key: &str) -> String {
    use k256::elliptic_curve::sec1::ToSec1Point;
    let bytes: Vec<u8> = (0..public_key.len() / 2).map(|i| u8::from_str_radix(&public_key[2 * i..2 * i + 2], 16).unwrap()).collect();
    let key = k256::PublicKey::from_sec1_bytes(&bytes).unwrap();
    key.to_sec1_point(false).as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// The directory where the owner of the root's devices keeps its pairing files.
fn root_pairing_dir(hub: &Hub) -> PathBuf {
    hub.arca_dir.join("owner.pairing")
}

/// `(device_id, public_key)` of every offer in an answer to `OFFERS`.
fn offers(response: &Response) -> Vec<(String, String)> {
    assert_eq!(response.status, 200, "{}", String::from_utf8_lossy(&response.body));
    let json: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    json["offers"].as_array().unwrap().iter().map(|o| (o["device_id"].as_str().unwrap().to_string(), o["public_key"].as_str().unwrap().to_string())).collect()
}

#[tokio::test]
async fn a_device_offers_a_key_and_another_device_of_the_same_owner_sees_it() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let key = generate_ecies_keypair().public_hex;

    assert_eq!(hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&key))).await.status, 204);
    assert_eq!(offers(&hub.call("GET", OFFERS, Some(&a), None).await), vec![("dev-b".to_string(), key)]);
}

#[tokio::test]
async fn a_pending_or_revoked_device_cannot_offer_list_or_read() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let approved = hub.approved_root("dev-a");
    let pending = hub.root_device("dev-p");
    let key = generate_ecies_keypair().public_hex;

    for (method, path, body) in [("PUT", OFFER, Some(offer_body(&key))), ("GET", OFFERS, None), ("GET", GRANT, None)] {
        assert_eq!(hub.call(method, path, Some(&pending), body.as_deref()).await.status, 403, "{method} {path} while pending");
    }
    hub.devices.revoke("dev-a").unwrap();
    assert_eq!(hub.call("PUT", OFFER, Some(&approved), Some(&offer_body(&key))).await.status, 403);
    // And nothing was stored by the refused calls.
    let admin = hub.approved_root("dev-c");
    assert!(offers(&hub.call("GET", OFFERS, Some(&admin), None).await).is_empty());
}

#[tokio::test]
async fn the_pairing_routes_need_a_known_token() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let key = generate_ecies_keypair().public_hex;
    assert_eq!(hub.call("PUT", OFFER, None, Some(&offer_body(&key))).await.status, 401);
    assert_eq!(hub.call("GET", OFFERS, Some("not-a-token"), None).await.status, 401);
    assert_eq!(hub.call("GET", GRANT, Some("not-a-token"), None).await.status, 401);
}

#[tokio::test]
async fn offers_stay_inside_their_owner() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let alice_new = hub.approved_member("alice-2", "alice");
    let alice = hub.approved_member("alice-1", "alice");
    let bob = hub.approved_member("bob-1", "bob");
    let key = generate_ecies_keypair().public_hex;

    assert_eq!(hub.call("PUT", OFFER, Some(&alice_new), Some(&offer_body(&key))).await.status, 204);
    assert_eq!(offers(&hub.call("GET", OFFERS, Some(&alice), None).await).len(), 1);
    assert!(offers(&hub.call("GET", OFFERS, Some(&bob), None).await).is_empty());
    // Bob cannot grant to Alice's device either: for him it has no offer.
    assert_eq!(hub.call("PUT", &grant_path("alice-2", &key), Some(&bob), Some(b"x")).await.status, 404);
}

#[tokio::test]
async fn a_grant_reaches_only_the_device_it_is_for_and_only_once() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let c = hub.approved_root("dev-c");
    let key = generate_ecies_keypair().public_hex;
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&key))).await;

    let blob = b"sealed-for-b".to_vec();
    assert_eq!(hub.call("PUT", &grant_path("dev-b", &key), Some(&a), Some(&blob)).await.status, 204);
    // C is another approved device of the same owner: it never gets B's grant.
    assert_eq!(hub.call("GET", GRANT, Some(&c), None).await.status, 404);
    let got = hub.call("GET", GRANT, Some(&b), None).await;
    assert_eq!((got.status, got.body), (200, blob));
    assert_eq!(hub.call("GET", GRANT, Some(&b), None).await.status, 404, "a grant is read once");
}

#[tokio::test]
async fn a_grant_needs_an_offer_and_not_for_oneself() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let key = generate_ecies_keypair().public_hex;

    assert_eq!(hub.call("PUT", &grant_path("dev-b", &key), Some(&a), Some(b"x")).await.status, 404, "no offer from dev-b");
    hub.call("PUT", OFFER, Some(&a), Some(&offer_body(&key))).await;
    assert_eq!(hub.call("PUT", &grant_path("dev-a", &key), Some(&a), Some(b"x")).await.status, 400, "a device does not grant to itself");
}

#[tokio::test]
async fn a_device_revoked_after_the_offer_cannot_grant() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let key = generate_ecies_keypair().public_hex;
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&key))).await;

    hub.devices.revoke("dev-a").unwrap();
    assert_eq!(hub.call("PUT", &grant_path("dev-b", &key), Some(&a), Some(b"x")).await.status, 403);
    assert_eq!(hub.call("GET", GRANT, Some(&b), None).await.status, 404, "nothing was left for dev-b");
}

#[tokio::test]
async fn offers_and_grants_expire() {
    let hub = hub(Some(ArcaLimits { pairing_ttl: Duration::from_millis(300), ..ArcaLimits::default() })).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let c = hub.approved_root("dev-c");
    let (key_b, key_c) = (generate_ecies_keypair().public_hex, generate_ecies_keypair().public_hex);
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&key_b))).await;
    assert_eq!(hub.call("PUT", &grant_path("dev-b", &key_b), Some(&a), Some(b"x")).await.status, 204);
    hub.call("PUT", OFFER, Some(&c), Some(&offer_body(&key_c))).await;

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(hub.call("GET", GRANT, Some(&b), None).await.status, 404, "the grant is too old");
    assert!(offers(&hub.call("GET", OFFERS, Some(&a), None).await).is_empty(), "the offer is too old");
    assert_eq!(hub.call("PUT", &grant_path("dev-c", &key_c), Some(&a), Some(b"x")).await.status, 404, "and so nothing to grant to");
}

#[tokio::test]
async fn a_malformed_or_oversized_offer_is_refused() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let b = hub.approved_root("dev-b");

    for bad in [offer_body("not hex"), offer_body("02aabb"), offer_body(&"00".repeat(33)), b"not json".to_vec(), b"{}".to_vec()] {
        assert_eq!(hub.call("PUT", OFFER, Some(&b), Some(&bad)).await.status, 400, "{}", String::from_utf8_lossy(&bad));
    }
    assert_eq!(hub.call("PUT", OFFER, Some(&b), Some(&vec![b'x'; 64 * 1024])).await.status, 413);
    let admin = hub.approved_root("dev-a");
    assert!(offers(&hub.call("GET", OFFERS, Some(&admin), None).await).is_empty());
}

#[tokio::test]
async fn an_owner_has_a_cap_on_waiting_offers_and_a_device_replaces_its_own() {
    let hub = hub(Some(ArcaLimits { max_pending_offers: 2, ..ArcaLimits::default() })).await;
    let tokens: Vec<String> = (0..4).map(|i| hub.approved_root(&format!("dev-{i}"))).collect();
    let key = || offer_body(&generate_ecies_keypair().public_hex);

    assert_eq!(hub.call("PUT", OFFER, Some(&tokens[0]), Some(&key())).await.status, 204);
    assert_eq!(hub.call("PUT", OFFER, Some(&tokens[1]), Some(&key())).await.status, 204);
    assert_eq!(hub.call("PUT", OFFER, Some(&tokens[2]), Some(&key())).await.status, 507);
    // A device that already has an offer replaces it; that is not a third one.
    assert_eq!(hub.call("PUT", OFFER, Some(&tokens[0]), Some(&key())).await.status, 204);
    assert_eq!(offers(&hub.call("GET", OFFERS, Some(&tokens[3]), None).await).len(), 2);
}

#[tokio::test]
async fn an_oversized_grant_is_refused() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let key = generate_ecies_keypair().public_hex;
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&key))).await;
    assert_eq!(hub.call("PUT", &grant_path("dev-b", &key), Some(&a), Some(&vec![0u8; 64 * 1024])).await.status, 413);
}

#[test]
fn the_verification_code_is_four_groups_of_four_hex_digits_and_depends_on_the_key() {
    let (k1, k2) = (generate_ecies_keypair().public_hex, generate_ecies_keypair().public_hex);
    let code = verification_code("dev-b", &k1);
    assert_four_groups(&code);
    assert_eq!(code, verification_code("dev-b", &k1), "deterministic");
    assert_ne!(code, verification_code("dev-b", &k2));
}

fn assert_four_groups(code: &str) {
    let groups: Vec<&str> = code.split('-').collect();
    assert_eq!(groups.len(), 4, "{code}");
    assert!(groups.iter().all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))), "{code}");
}

#[test]
fn the_verification_code_depends_on_the_device_it_is_for() {
    let key = generate_ecies_keypair().public_hex;
    assert_ne!(verification_code("dev-b", &key), verification_code("dev-c", &key));
}

#[test]
fn the_same_key_in_either_encoding_or_spelling_gets_the_same_code() {
    let key = generate_ecies_keypair().public_hex;
    let code = verification_code("dev-b", &key);
    assert_eq!(verification_code("dev-b", &uncompressed(&key)), code);
    assert_eq!(verification_code("dev-b", &key.to_uppercase()), code);
    assert_eq!(verification_code("dev-b", &format!("0x{key}")), code);
}

#[test]
fn the_key_fingerprint_is_four_groups_stable_and_depends_on_the_key() {
    let (a, b) = ([7u8; 32], [8u8; 32]);
    assert_four_groups(&key_fingerprint(&a));
    assert_eq!(key_fingerprint(&a), key_fingerprint(&a));
    assert_ne!(key_fingerprint(&a), key_fingerprint(&b));
    // It is not the pairing code of anything: another label.
    assert_ne!(key_fingerprint(&a), verification_code("dev-b", &generate_ecies_keypair().public_hex));
}

#[tokio::test]
async fn a_hub_that_swaps_the_public_key_shows_a_different_code() {
    let (real, swapped) = (generate_ecies_keypair().public_hex, generate_ecies_keypair().public_hex);
    let shown_on_b = verification_code("dev-b", &real);
    // What a hostile hub would hand to A in place of B's key.
    assert_ne!(verification_code("dev-b", &swapped), shown_on_b);

    // And through the real hub, A computes from what the hub gave it: the code matches B's only if the key is B's.
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&real))).await;
    let (id_seen_by_a, seen_by_a) = offers(&hub.call("GET", OFFERS, Some(&a), None).await).remove(0);
    assert_eq!(verification_code(&id_seen_by_a, &seen_by_a), shown_on_b);
}

#[tokio::test]
async fn the_vault_key_travels_from_one_device_to_another_and_opens_what_the_first_sealed() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let vault_key = VaultKey::generate().unwrap();
    let sealed = seal(&EpochKeys::derive(&vault_key, 1), ObjectKind::Blob, b"made on device A").unwrap();

    // B: a key pair that lives only here, and its public half offered.
    let pair = generate_ecies_keypair();
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&pair.public_hex))).await;
    // A: picks B's offer and encrypts the key to the key it was given.
    let (device_id, public_key) = offers(&hub.call("GET", OFFERS, Some(&a), None).await).remove(0);
    let blob = ecies_encrypt(&vault_key.export_bytes()[..], &public_key).unwrap();
    assert_eq!(hub.call("PUT", &grant_path(&device_id, &public_key), Some(&a), Some(&blob)).await.status, 204);
    // B: reads it, decrypts it, and now opens A's object.
    let got = hub.call("GET", GRANT, Some(&b), None).await;
    assert_eq!(got.status, 200);
    let bytes: [u8; 32] = ecies_decrypt(&got.body, &pair.secret).unwrap().try_into().unwrap();
    let received = VaultKey::from_bytes(bytes);
    assert_eq!(*received.export_bytes(), *vault_key.export_bytes());
    assert_eq!(&open(&EpochKeys::derive(&received, 1), ObjectKind::Blob, &sealed.bytes).unwrap()[..], b"made on device A");
    // What went through the hub was ciphertext: the key's bytes are not in it.
    assert!(!got.body.windows(32).any(|w| w == &vault_key.export_bytes()[..]));
    // The two devices end up showing the same fingerprint, each computed from the key it has.
    assert_eq!(key_fingerprint(&vault_key.export_bytes()), key_fingerprint(&received.export_bytes()));
}

#[tokio::test]
async fn a_key_the_hub_made_up_is_caught_by_the_fingerprint_not_by_the_hub() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let real = VaultKey::generate().unwrap();
    let pair = generate_ecies_keypair();
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&pair.public_hex))).await;

    // A device with no key of its own (here dev-c, approved but never given the vault key) grants in A's place: the hub has no
    // way to tell, ECIES encrypts to anyone who knows B's public key.
    let forged = VaultKey::generate().unwrap();
    let c = hub.approved_root("dev-c");
    let blob = ecies_encrypt(&forged.export_bytes()[..], &pair.public_hex).unwrap();
    assert_eq!(hub.call("PUT", &grant_path("dev-b", &pair.public_hex), Some(&c), Some(&blob)).await.status, 204);

    let got = hub.call("GET", GRANT, Some(&b), None).await;
    let bytes: [u8; 32] = ecies_decrypt(&got.body, &pair.secret).unwrap().try_into().unwrap();
    // B cannot tell by itself, but the person comparing A's fingerprint with B's can.
    assert_ne!(key_fingerprint(&real.export_bytes()), key_fingerprint(&bytes));
    let _ = a;
}

#[tokio::test]
async fn a_grant_names_the_key_it_saw_and_a_missing_or_malformed_one_is_400() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let key = generate_ecies_keypair().public_hex;
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&key))).await;

    for bad in [format!("{GRANT}/dev-b"), format!("{GRANT}/dev-b?key="), format!("{GRANT}/dev-b?key=zz"), format!("{GRANT}/dev-b?other={key}")] {
        assert_eq!(hub.call("PUT", &bad, Some(&a), Some(b"x")).await.status, 400, "{bad}");
    }
    assert_eq!(offers(&hub.call("GET", OFFERS, Some(&a), None).await).len(), 1, "the offer is still there");
}

#[tokio::test]
async fn a_grant_for_a_key_the_offer_no_longer_has_is_409_and_keeps_the_new_offer() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let (old, new) = (generate_ecies_keypair().public_hex, generate_ecies_keypair().public_hex);
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&old))).await;
    // B starts over before A grants.
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&new))).await;

    assert_eq!(hub.call("PUT", &grant_path("dev-b", &old), Some(&a), Some(b"for-the-old-key")).await.status, 409);
    assert_eq!(offers(&hub.call("GET", OFFERS, Some(&a), None).await), vec![("dev-b".to_string(), new)]);
    assert_eq!(hub.call("GET", GRANT, Some(&b), None).await.status, 404, "nothing was stored");
}

#[tokio::test]
async fn a_key_is_kept_and_listed_in_its_compressed_form_whatever_form_it_came_in() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let compressed = generate_ecies_keypair().public_hex;

    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&uncompressed(&compressed)))).await;
    assert_eq!(offers(&hub.call("GET", OFFERS, Some(&a), None).await), vec![("dev-b".to_string(), compressed.clone())]);
    // A grant that names the key in the other spelling still finds the offer.
    assert_eq!(hub.call("PUT", &grant_path("dev-b", &compressed.to_uppercase()), Some(&a), Some(b"x")).await.status, 204);
}

#[tokio::test]
async fn an_offer_of_a_device_revoked_since_is_neither_listed_nor_granted() {
    let hub = hub(Some(ArcaLimits::default())).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let key = generate_ecies_keypair().public_hex;
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&key))).await;

    hub.devices.revoke("dev-b").unwrap();
    assert!(offers(&hub.call("GET", OFFERS, Some(&a), None).await).is_empty());
    assert_eq!(hub.call("PUT", &grant_path("dev-b", &key), Some(&a), Some(b"x")).await.status, 404);
}

#[tokio::test]
async fn two_grants_at_once_for_the_same_device_leave_one_winner() {
    let hub = hub(Some(ArcaLimits { max_concurrent_per_owner: 8, ..ArcaLimits::default() })).await;
    let a = hub.approved_root("dev-a");
    let b = hub.approved_root("dev-b");
    let key = generate_ecies_keypair().public_hex;
    hub.call("PUT", OFFER, Some(&b), Some(&offer_body(&key))).await;

    let path = grant_path("dev-b", &key);
    let (first, second) = tokio::join!(hub.call("PUT", &path, Some(&a), Some(b"one")), hub.call("PUT", &path, Some(&a), Some(b"two")));
    let mut statuses = [first.status, second.status];
    statuses.sort();
    assert_eq!(statuses, [204, 404]);
}

#[tokio::test]
async fn offers_made_at_once_do_not_go_past_the_cap() {
    let hub = hub(Some(ArcaLimits { max_pending_offers: 2, max_concurrent_per_owner: 8, ..ArcaLimits::default() })).await;
    let t: Vec<String> = (0..6).map(|i| hub.approved_root(&format!("dev-{i}"))).collect();
    let body: Vec<Vec<u8>> = (0..6).map(|_| offer_body(&generate_ecies_keypair().public_hex)).collect();

    let (r0, r1, r2, r3, r4, r5) = tokio::join!(
        hub.call("PUT", OFFER, Some(&t[0]), Some(&body[0])),
        hub.call("PUT", OFFER, Some(&t[1]), Some(&body[1])),
        hub.call("PUT", OFFER, Some(&t[2]), Some(&body[2])),
        hub.call("PUT", OFFER, Some(&t[3]), Some(&body[3])),
        hub.call("PUT", OFFER, Some(&t[4]), Some(&body[4])),
        hub.call("PUT", OFFER, Some(&t[5]), Some(&body[5])),
    );
    let accepted = [r0, r1, r2, r3, r4, r5].iter().filter(|r| r.status == 204).count();
    assert_eq!(accepted, 2);
}

#[tokio::test]
async fn leftovers_of_a_crashed_call_are_swept_once_they_are_older_than_the_ttl() {
    let hub = hub(Some(ArcaLimits { pairing_ttl: Duration::from_millis(300), ..ArcaLimits::default() })).await;
    let a = hub.approved_root("dev-a");
    hub.call("PUT", OFFER, Some(&a), Some(&offer_body(&generate_ecies_keypair().public_hex))).await;
    let dir = root_pairing_dir(&hub);
    let old = std::time::SystemTime::now() - Duration::from_secs(3600);
    let leftovers = ["tmp-1-1-1", "taken-1-1-1"].map(|name| dir.join(name));
    for file in &leftovers {
        let f = std::fs::File::create(file).unwrap();
        f.set_modified(old).unwrap();
    }

    hub.call("GET", OFFERS, Some(&a), None).await;
    assert!(leftovers.iter().all(|f| !f.exists()), "old tmp- and taken- files are gone");
}

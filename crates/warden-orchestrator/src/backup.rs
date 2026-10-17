//! Backups of an instance's volume: an encrypted archive per backup, in the orchestrator's state folder.
//!
//! A backup is the volume's `tar.gz` (made by a throwaway container, see `ContainerRuntime::archive_volume`) encrypted
//! as it streams, so a volume of gigabytes is never held in memory. The format (`.wbk`):
//!
//! - 4 bytes of magic (`WBK1`) and an 8-byte random prefix for the nonces: the header, 12 bytes, bound to every chunk as
//!   associated data;
//! - then frames, each a 4-byte big-endian length and that many bytes of AES-256-GCM ciphertext of up to 1 MiB of the
//!   archive. It is the STREAM construction (`aead::stream`): the chunk's position and whether it is the last one are in
//!   its nonce, so a chunk moved, dropped or cut off at the end fails to open, not only one that was altered.
//!
//! The key is the orchestrator's own (`backup.key` in the state folder, mode 0600): made the first time a backup is taken.
//! It sits on the same host as the backups, which is the level of trust already accepted for the pairing keys in
//! `instances.json`; taking a copy of it somewhere else is the operator's job (and a later slice).

use std::io::Read;
use std::path::{Path, PathBuf};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::stream::{DecryptorBE32, EncryptorBE32};
use aes_gcm::aead::Payload;
use aes_gcm::{Aes256Gcm, KeyInit};
use anyhow::{bail, Context};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAGIC: &[u8; 4] = b"WBK1";
const PREFIX_LEN: usize = 8;
const HEADER_LEN: usize = MAGIC.len() + PREFIX_LEN;
/// How much of the archive goes in one frame.
const CHUNK: usize = 1024 * 1024;
/// How much of the archive can wait in the pipe between the archiving container and the encryption (and back, on restore).
pub const PIPE_BYTES: usize = 1024 * 1024;
/// The tag AES-GCM adds to every chunk.
const TAG: usize = 16;
/// The file name of a backup: `<epoch_ms>.wbk`.
const EXTENSION: &str = "wbk";

/// The key backups are encrypted with.
#[derive(Clone)]
pub struct BackupKey([u8; 32]);

impl BackupKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The key in `<state_dir>/backup.key`, which must be there: to open a backup, a new key would be no use.
    pub fn load(state_dir: &Path) -> anyhow::Result<Self> {
        let path = state_dir.join("backup.key");
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).with_context(|| format!("{} is not a backup key", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => bail!("there is no {}: without the key that made them, backups cannot be opened", path.display()),
            Err(err) => Err(err).with_context(|| format!("could not read {}", path.display())),
        }
    }

    /// The key in `<state_dir>/backup.key`, made (random, mode 0600) when there isn't one yet.
    pub fn load_or_create(state_dir: &Path) -> anyhow::Result<Self> {
        let path = state_dir.join("backup.key");
        match std::fs::read_to_string(&path) {
            Ok(text) => return Self::parse(&text).with_context(|| format!("{} is not a backup key", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
        }
        std::fs::create_dir_all(state_dir).with_context(|| format!("could not create {}", state_dir.display()))?;
        let bytes: [u8; 32] = random_bytes()?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let mut options = std::fs::OpenOptions::new();
        // `create_new`: if another process made the key a moment ago, this one reads it instead of replacing it.
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        match options.open(&path) {
            Ok(mut file) => {
                std::io::Write::write_all(&mut file, hex.as_bytes())?;
                file.sync_all()?;
                Ok(Self(bytes))
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::parse(&std::fs::read_to_string(&path)?).with_context(|| format!("{} is not a backup key", path.display()))
            }
            Err(err) => Err(err).with_context(|| format!("could not write {}", path.display())),
        }
    }

    fn parse(text: &str) -> anyhow::Result<Self> {
        let text = text.trim();
        anyhow::ensure!(text.len() == 64 && text.is_ascii(), "expected 64 hex characters");
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).context("not hexadecimal")?;
        }
        Ok(Self(bytes))
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(GenericArray::from_slice(&self.0))
    }
}

/// `N` random bytes from the system.
pub fn random_bytes<const N: usize>() -> anyhow::Result<[u8; N]> {
    let mut bytes = [0u8; N];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).context("could not read /dev/urandom")?;
    Ok(bytes)
}

/// Reads up to `size` bytes (fewer only at the end of the input) into `buf`.
async fn fill<R: AsyncRead + Unpin>(input: &mut R, buf: &mut Vec<u8>, size: usize) -> std::io::Result<()> {
    buf.clear();
    buf.resize(size, 0);
    let mut filled = 0;
    while filled < size {
        let n = input.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(())
}

/// Encrypts everything `input` gives into `out`. Both are taken by value: if this fails, `input` is dropped, so whatever
/// is writing into the other end of a pipe stops instead of waiting for a reader that is gone.
pub async fn encrypt_stream<R, W>(key: &BackupKey, mut input: R, mut out: W) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let prefix: [u8; PREFIX_LEN] = random_bytes()?;
    let mut header = [0u8; HEADER_LEN];
    header[..MAGIC.len()].copy_from_slice(MAGIC);
    header[MAGIC.len()..].copy_from_slice(&prefix);
    out.write_all(&header).await?;

    let mut encryptor = EncryptorBE32::from_aead(key.cipher(), GenericArray::from_slice(&prefix));
    let (mut current, mut next) = (Vec::new(), Vec::new());
    fill(&mut input, &mut current, CHUNK).await.context("could not read the archive")?;
    loop {
        // One chunk ahead, to know which one is the last: its nonce says so.
        fill(&mut input, &mut next, CHUNK).await.context("could not read the archive")?;
        let payload = Payload { msg: &current, aad: &header };
        let sealed = if next.is_empty() {
            let sealed = encryptor.encrypt_last(payload).map_err(|_| anyhow::anyhow!("could not encrypt the last chunk"))?;
            write_frame(&mut out, &sealed).await?;
            break;
        } else {
            encryptor.encrypt_next(payload).map_err(|_| anyhow::anyhow!("could not encrypt a chunk"))?
        };
        write_frame(&mut out, &sealed).await?;
        std::mem::swap(&mut current, &mut next);
    }
    out.flush().await?;
    out.shutdown().await?;
    Ok(())
}

async fn write_frame<W: AsyncWrite + Unpin>(out: &mut W, sealed: &[u8]) -> anyhow::Result<()> {
    out.write_all(&(sealed.len() as u32).to_be_bytes()).await?;
    out.write_all(sealed).await?;
    Ok(())
}

/// The length of the next frame, or `None` at a clean end of the file.
async fn read_frame_len<R: AsyncRead + Unpin>(input: &mut R) -> anyhow::Result<Option<usize>> {
    let mut first = [0u8; 1];
    if input.read(&mut first).await? == 0 {
        return Ok(None);
    }
    let mut rest = [0u8; 3];
    input.read_exact(&mut rest).await.context("the backup is cut off inside a chunk's length")?;
    let len = u32::from_be_bytes([first[0], rest[0], rest[1], rest[2]]) as usize;
    anyhow::ensure!((TAG..=CHUNK + TAG).contains(&len), "the backup is damaged (a chunk of {len} bytes)");
    Ok(Some(len))
}

/// Decrypts a `.wbk` from `input` into `out`. Fails (and writes nothing past the last chunk that opened) on a wrong key, a
/// byte changed, or a file cut short or with chunks moved. Taken by value like `encrypt_stream`, for the same reason.
pub async fn decrypt_stream<R, W>(key: &BackupKey, mut input: R, mut out: W) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut header = [0u8; HEADER_LEN];
    input.read_exact(&mut header).await.context("the file is too short to be a backup")?;
    anyhow::ensure!(&header[..MAGIC.len()] == MAGIC, "this is not a Warden backup (or it is of a newer version)");
    let mut decryptor = DecryptorBE32::from_aead(key.cipher(), GenericArray::from_slice(&header[MAGIC.len()..]));

    let wrong = || anyhow::anyhow!("a chunk of the backup does not open: wrong key, or the backup was changed or cut short");
    let Some(mut len) = read_frame_len(&mut input).await? else {
        bail!("the backup has no chunks: it is cut short");
    };
    let mut sealed = vec![0u8; len];
    loop {
        input.read_exact(&mut sealed[..len]).await.context("the backup is cut off inside a chunk")?;
        let payload = Payload { msg: &sealed[..len], aad: &header };
        match read_frame_len(&mut input).await? {
            Some(next_len) => {
                let plain = decryptor.decrypt_next(payload).map_err(|_| wrong())?;
                out.write_all(&plain).await?;
                len = next_len;
                sealed.resize(len, 0);
            }
            None => {
                let plain = decryptor.decrypt_last(payload).map_err(|_| wrong())?;
                out.write_all(&plain).await?;
                break;
            }
        }
    }
    out.flush().await?;
    out.shutdown().await?;
    Ok(())
}

/// Where a customer's backups are.
pub fn backups_dir(state_dir: &Path, customer_id: &str) -> PathBuf {
    state_dir.join("backups").join(customer_id)
}

/// A name for a backup made now that no other backup of the customer has: the time in milliseconds, moved forward if two
/// are made in the same one.
pub fn new_backup_path(dir: &Path, now_ms: u64) -> PathBuf {
    let mut ms = now_ms;
    loop {
        let path = dir.join(format!("{ms}.{EXTENSION}"));
        if !path.exists() {
            return path;
        }
        ms += 1;
    }
}

/// The customer's backups, oldest first.
pub fn list(state_dir: &Path, customer_id: &str) -> anyhow::Result<Vec<PathBuf>> {
    let dir = backups_dir(state_dir, customer_id);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("could not read {}", dir.display())),
    };
    let mut found: Vec<(u64, PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == EXTENSION))
        .filter_map(|path| Some((path.file_stem()?.to_str()?.parse::<u64>().ok()?, path)))
        .collect();
    found.sort();
    Ok(found.into_iter().map(|(_, path)| path).collect())
}

/// Deletes all but the `keep` newest backups of the customer; how many went.
pub fn prune(state_dir: &Path, customer_id: &str, keep: usize) -> anyhow::Result<usize> {
    let all = list(state_dir, customer_id)?;
    let old = all.len().saturating_sub(keep);
    for path in &all[..old] {
        std::fs::remove_file(path).with_context(|| format!("could not delete {}", path.display()))?;
    }
    Ok(old)
}

/// A backup named by the operator: a path, or `latest` for the customer's newest.
pub fn resolve(state_dir: &Path, customer_id: &str, which: &str) -> anyhow::Result<PathBuf> {
    if which != "latest" {
        let path = PathBuf::from(which);
        anyhow::ensure!(path.is_file(), "there is no backup file at {which}");
        return Ok(path);
    }
    list(state_dir, customer_id)?.pop().with_context(|| format!("'{customer_id}' has no backups"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> BackupKey {
        BackupKey::from_bytes([byte; 32])
    }

    async fn seal(key: &BackupKey, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encrypt_stream(key, data, &mut out).await.unwrap();
        out
    }

    async fn open(key: &BackupKey, sealed: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::new();
        decrypt_stream(key, sealed, &mut out).await?;
        Ok(out)
    }

    /// Bytes that don't repeat on any short period, so a chunk moved from one place to another would show.
    fn sample(len: usize) -> Vec<u8> {
        (0..len).map(|i| ((i * 31 + i / 251) % 256) as u8).collect()
    }

    #[tokio::test]
    async fn what_goes_in_comes_out_for_none_one_and_many_chunks() {
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, CHUNK * 2, CHUNK * 3 + 17] {
            let data = sample(len);
            let sealed = seal(&key(7), &data).await;
            assert_eq!(open(&key(7), &sealed).await.unwrap(), data, "{len} bytes");
        }
    }

    #[tokio::test]
    async fn the_archive_is_not_in_the_file_in_the_clear() {
        let data = b"the-customers-secret-note".repeat(100);
        let sealed = seal(&key(7), &data).await;
        assert!(!sealed.windows(25).any(|w| w == b"the-customers-secret-note"));
    }

    #[tokio::test]
    async fn two_backups_of_the_same_data_differ() {
        let data = sample(1000);
        assert_ne!(seal(&key(7), &data).await, seal(&key(7), &data).await);
    }

    #[tokio::test]
    async fn a_wrong_key_does_not_open_it() {
        let sealed = seal(&key(7), &sample(5000)).await;
        assert!(open(&key(8), &sealed).await.is_err());
    }

    #[tokio::test]
    async fn a_changed_byte_in_any_part_is_refused() {
        let sealed = seal(&key(7), &sample(CHUNK + 500)).await;
        for at in [0, 5, HEADER_LEN + 4 + 10, sealed.len() / 2, sealed.len() - 1] {
            let mut damaged = sealed.clone();
            damaged[at] ^= 0x01;
            assert!(open(&key(7), &damaged).await.is_err(), "byte {at}");
        }
    }

    #[tokio::test]
    async fn a_file_cut_short_is_refused_even_at_a_chunk_boundary() {
        let sealed = seal(&key(7), &sample(CHUNK * 2 + 10)).await;
        // Every cut: inside the header, inside a frame, and exactly between two frames.
        let first_frame_end = HEADER_LEN + 4 + CHUNK + TAG;
        for cut in [0, 3, HEADER_LEN, HEADER_LEN + 2, first_frame_end - 1, first_frame_end, sealed.len() - 1] {
            assert!(open(&key(7), &sealed[..cut]).await.is_err(), "cut at {cut}");
        }
    }

    #[tokio::test]
    async fn chunks_swapped_are_refused() {
        let sealed = seal(&key(7), &sample(CHUNK * 2 + 10)).await;
        let frame = 4 + CHUNK + TAG;
        let mut swapped = sealed.clone();
        let (a, b) = (HEADER_LEN, HEADER_LEN + frame);
        let first = sealed[a..a + frame].to_vec();
        swapped[a..a + frame].copy_from_slice(&sealed[b..b + frame]);
        swapped[b..b + frame].copy_from_slice(&first);
        assert!(open(&key(7), &swapped).await.is_err());
    }

    #[tokio::test]
    async fn something_that_is_not_a_backup_is_said_so() {
        let err = open(&key(7), b"just some text, long enough to have a header").await.unwrap_err().to_string();
        assert!(err.contains("not a Warden backup"), "{err}");
    }

    #[test]
    fn the_key_is_made_once_with_mode_0600_and_read_back() {
        let dir = std::env::temp_dir().join(format!("warden-orchestrator-backup-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = BackupKey::load_or_create(&dir).unwrap();
        let again = BackupKey::load_or_create(&dir).unwrap();
        assert_eq!(first.0, again.0);
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(dir.join("backup.key")).unwrap().permissions()) & 0o777, 0o600);
        std::fs::write(dir.join("backup.key"), "not hex").unwrap();
        assert!(BackupKey::load_or_create(&dir).is_err(), "a damaged key is an error, never replaced by a new one");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backups_are_listed_oldest_first_and_pruned_to_the_newest() {
        let dir = std::env::temp_dir().join(format!("warden-orchestrator-backup-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let customer_dir = backups_dir(&dir, "acme");
        std::fs::create_dir_all(&customer_dir).unwrap();
        for ms in [900u64, 1000, 20, 5000] {
            std::fs::write(customer_dir.join(format!("{ms}.wbk")), b"x").unwrap();
        }
        std::fs::write(customer_dir.join("notes.txt"), b"x").unwrap();
        let names = |paths: Vec<PathBuf>| paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(list(&dir, "acme").unwrap()), ["20.wbk", "900.wbk", "1000.wbk", "5000.wbk"], "by time, not by text");
        assert_eq!(prune(&dir, "acme", 2).unwrap(), 2);
        assert_eq!(names(list(&dir, "acme").unwrap()), ["1000.wbk", "5000.wbk"]);
        assert!(customer_dir.join("notes.txt").exists(), "only backups are pruned");
        assert_eq!(resolve(&dir, "acme", "latest").unwrap(), customer_dir.join("5000.wbk"));
        assert!(resolve(&dir, "beta", "latest").is_err());
        assert!(list(&dir, "beta").unwrap().is_empty());
        assert_eq!(new_backup_path(&customer_dir, 5000), customer_dir.join("5001.wbk"), "a name taken moves forward");
        std::fs::remove_dir_all(&dir).ok();
    }
}

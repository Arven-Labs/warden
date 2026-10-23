//! TLS for the endpoint itself (P168 b): PEM certificate and key files → a `TlsAcceptor`.
//!
//! The same shape as the hub's (`warden-server/src/tls.rs`): the operator brings the certificate (Let's Encrypt through certbot,
//! `tailscale cert`, a company CA) and the files are read again on the next handshake whenever their modification time
//! changes, so a renewal needs no restart. Not here: ACME, a self-signed certificate, client certificates (the site mãe
//! signs its calls with an HMAC instead, `HOSPEDAGEM.md` section B).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::Context;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

/// What accepts the TLS handshake of a connection.
#[derive(Clone)]
pub struct GatewayTls {
    pub(crate) acceptor: TlsAcceptor,
}

impl GatewayTls {
    /// Loads `cert_path` (PEM chain, leaf first) and `key_path` (PEM private key). Fails right away on unreadable or invalid
    /// files, or a key that is not the certificate's: a later reload failure only logs and keeps the old certificate.
    pub fn from_pem_files(cert_path: impl Into<PathBuf>, key_path: impl Into<PathBuf>) -> anyhow::Result<GatewayTls> {
        // `ring` on purpose: the workspace compiles both `ring` and `aws-lc-rs`, and rustls panics rather than pick one.
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let resolver = ReloadingCertResolver::new(cert_path.into(), key_path.into(), provider.clone())?;
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .context("the TLS provider doesn't support the default protocol versions")?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(resolver));
        Ok(GatewayTls { acceptor: TlsAcceptor::from(Arc::new(config)) })
    }
}

/// Serves whatever is in the certificate and key files now, reading them again when either one's modification time changes.
/// One `stat` pair per handshake, which is nothing next to the handshake.
#[derive(Debug)]
struct ReloadingCertResolver {
    cert_path: PathBuf,
    key_path: PathBuf,
    provider: Arc<CryptoProvider>,
    current: Mutex<LoadedCert>,
}

#[derive(Debug)]
struct LoadedCert {
    key: Arc<CertifiedKey>,
    mtimes: (SystemTime, SystemTime),
}

impl ReloadingCertResolver {
    fn new(cert_path: PathBuf, key_path: PathBuf, provider: Arc<CryptoProvider>) -> anyhow::Result<ReloadingCertResolver> {
        let current = load_cert(&cert_path, &key_path, &provider)?;
        Ok(ReloadingCertResolver { cert_path, key_path, provider, current: Mutex::new(current) })
    }
}

impl ResolvesServerCert for ReloadingCertResolver {
    fn resolve(&self, _client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let mut current = self.current.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let changed = match (modified(&self.cert_path), modified(&self.key_path)) {
            (Ok(cert), Ok(key)) => (cert, key) != current.mtimes,
            // Mid-rewrite or deleted: keep serving what is loaded rather than fail every handshake.
            _ => false,
        };
        if changed {
            match load_cert(&self.cert_path, &self.key_path, &self.provider) {
                Ok(fresh) => {
                    tracing::info!(cert = %self.cert_path.display(), "reloaded the TLS certificate");
                    *current = fresh;
                }
                // Also possible mid-rewrite (the certificate already new, the key still old): `mtimes` was not updated, so the
                // next handshake tries again.
                Err(err) => tracing::warn!("the TLS certificate changed but could not be reloaded, keeping the old one: {err:#}"),
            }
        }
        Some(current.key.clone())
    }
}

fn modified(path: &Path) -> std::io::Result<SystemTime> {
    std::fs::metadata(path)?.modified()
}

fn load_cert(cert_path: &Path, key_path: &Path, provider: &CryptoProvider) -> anyhow::Result<LoadedCert> {
    // The mtimes are read before the contents: if a write lands in between, the next handshake sees a newer mtime than the
    // one stored and loads again, instead of missing it.
    let mtimes = (modified(cert_path)?, modified(key_path)?);
    let certs = CertificateDer::pem_file_iter(cert_path)
        .with_context(|| format!("failed to read the TLS certificate {}", cert_path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("invalid PEM in {}", cert_path.display()))?;
    anyhow::ensure!(!certs.is_empty(), "no certificate found in {}", cert_path.display());
    let key = PrivateKeyDer::from_pem_file(key_path).with_context(|| format!("failed to read the TLS private key {}", key_path.display()))?;
    let signing_key = provider.key_provider.load_private_key(key).context("unsupported TLS private key")?;
    let certified = CertifiedKey::new(certs, signing_key);
    certified.keys_match().context("the TLS certificate and private key don't match")?;
    Ok(LoadedCert { key: Arc::new(certified), mtimes })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rcgen::{CertificateParams, KeyPair};

    use super::*;

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("warden-gateway-tls-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A certificate for `localhost` and its key, as PEM.
    fn pair() -> (String, String) {
        let key = KeyPair::generate().unwrap();
        let cert = CertificateParams::new(vec!["localhost".to_string()]).unwrap().self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    fn write(dir: &Path, cert: &str, key: &str) -> (PathBuf, PathBuf) {
        let (cert_path, key_path) = (dir.join("gateway.crt"), dir.join("gateway.key"));
        std::fs::write(&cert_path, cert).unwrap();
        std::fs::write(&key_path, key).unwrap();
        (cert_path, key_path)
    }

    #[test]
    fn a_matching_pair_loads() {
        let (cert, key) = pair();
        let (cert_path, key_path) = write(&temp_dir(), &cert, &key);
        assert!(GatewayTls::from_pem_files(cert_path, key_path).is_ok());
    }

    #[test]
    fn a_missing_file_fails_at_startup() {
        let dir = temp_dir();
        let (cert, key) = pair();
        let (cert_path, key_path) = write(&dir, &cert, &key);
        assert!(GatewayTls::from_pem_files(dir.join("nope.crt"), &key_path).is_err());
        assert!(GatewayTls::from_pem_files(&cert_path, dir.join("nope.key")).is_err());
    }

    #[test]
    fn a_file_with_no_certificate_fails_at_startup() {
        let (_, key) = pair();
        let (cert_path, key_path) = write(&temp_dir(), "this is not a certificate\n", &key);
        let err = GatewayTls::from_pem_files(cert_path, key_path).err().expect("no certificate must fail");
        assert!(format!("{err:#}").contains("no certificate found"), "{err:#}");
    }

    #[test]
    fn a_key_that_is_not_the_certificates_fails_at_startup() {
        let (cert, _) = pair();
        let (_, other_key) = pair();
        let (cert_path, key_path) = write(&temp_dir(), &cert, &other_key);
        let err = GatewayTls::from_pem_files(cert_path, key_path).err().expect("a mismatched key must fail");
        assert!(format!("{err:#}").contains("don't match"), "{err:#}");
    }
}

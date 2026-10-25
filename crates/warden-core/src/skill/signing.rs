//! Who wrote a package (P160): an Ed25519 signature over the package's `hash`, kept in its `signature` field as
//! `ed25519:<public key in hex>:<signature in hex>`. The hash already covers the kind, id, version, author, license and
//! content, so signing the hash signs all of them, and the same file still reads as format version 1: an older Warden
//! just ignores the field.
//!
//! The signature says "the holder of this key made this package", nothing more. Whether that key is someone the person
//! trusts is theirs to say (a list in their own config), so `verified` only returns the key. A package that carries a
//! signature which does not check out is an error, like a hash that does not match; one with no signature is read as it
//! always was.

use anyhow::{anyhow, bail};
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};

use super::package::Package;

/// The only scheme this version signs and checks with.
pub const SCHEME: &str = "ed25519";
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

/// What a package's `signature` field says, once checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// No signature: the package is read as it always was.
    Unsigned,
    /// The signature checks out for this public key (64 lowercase hex characters).
    Valid { key: String },
}

/// A fresh key pair: the private key as PKCS#8 bytes (to keep secret) and the public key in hex (to hand out).
pub fn generate_keypair() -> anyhow::Result<(Vec<u8>, String)> {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| anyhow!("could not generate a key"))?;
    let public = public_key_of(pkcs8.as_ref())?;
    Ok((pkcs8.as_ref().to_vec(), public))
}

/// The public key (hex) of a private key in PKCS#8 form.
pub fn public_key_of(pkcs8: &[u8]) -> anyhow::Result<String> {
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|_| anyhow!("this is not an Ed25519 private key"))?;
    Ok(to_hex(pair.public_key().as_ref()))
}

/// A private key as the text a key file holds: its PKCS#8 bytes in hex.
pub fn encode_private_key(pkcs8: &[u8]) -> String {
    to_hex(pkcs8)
}

/// The private key (PKCS#8 bytes) that a key file's text holds, checked to be an Ed25519 key.
pub fn decode_private_key(text: &str) -> anyhow::Result<Vec<u8>> {
    let pkcs8 = from_hex(text.trim()).ok_or_else(|| anyhow!("this is not a Warden signing key file"))?;
    public_key_of(&pkcs8)?;
    Ok(pkcs8)
}

/// Whether `text` is an Ed25519 public key as the config and the signature write it: 64 hex characters.
pub fn is_public_key(text: &str) -> bool {
    from_hex(text).is_some_and(|bytes| bytes.len() == PUBLIC_KEY_LEN)
}

/// `key` in the one form keys are compared in (lowercase), or `None` when it is not a public key.
pub fn normalize_public_key(key: &str) -> Option<String> {
    is_public_key(key.trim()).then(|| key.trim().to_ascii_lowercase())
}

impl Package {
    /// Signs the package with the private key `pkcs8`, replacing any signature it had. The package must be one that reads
    /// back clean (it is what `parse` and `export_skill` give), so the hash being signed is the content's.
    pub fn sign(&mut self, pkcs8: &[u8]) -> anyhow::Result<()> {
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|_| anyhow!("this is not an Ed25519 private key"))?;
        let signature = pair.sign(self.hash.as_bytes());
        self.signature = Some(format!("{SCHEME}:{}:{}", to_hex(pair.public_key().as_ref()), to_hex(signature.as_ref())));
        Ok(())
    }

    /// Checks the signature, if there is one. An error means the package says it is signed and the signature is not good:
    /// it does not match the hash, is malformed, or is of a kind this version cannot check.
    pub fn verified(&self) -> anyhow::Result<Verification> {
        let Some(field) = self.signature.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Verification::Unsigned);
        };
        let mut parts = field.splitn(3, ':');
        let (scheme, key, signature) = (parts.next().unwrap_or_default(), parts.next(), parts.next());
        if scheme != SCHEME {
            bail!("the package is signed with '{scheme}', which this version of Warden can't check; update Warden or ask for an unsigned copy");
        }
        let (Some(key), Some(signature)) = (key, signature) else {
            bail!("the package's signature is malformed");
        };
        let public = from_hex(key).filter(|bytes| bytes.len() == PUBLIC_KEY_LEN).ok_or_else(|| anyhow!("the package's signature is malformed (its key)"))?;
        let signature = from_hex(signature).filter(|bytes| bytes.len() == SIGNATURE_LEN).ok_or_else(|| anyhow!("the package's signature is malformed (its signature)"))?;
        UnparsedPublicKey::new(&ED25519, &public)
            .verify(self.hash.as_bytes(), &signature)
            .map_err(|_| anyhow!("the package's signature does not match its content: it was changed after it was signed, or the signature is not the signer's"))?;
        Ok(Verification::Valid { key: to_hex(&public) })
    }
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::Skill;

    fn skill() -> Skill {
        Skill {
            name: "email-draft".into(),
            description: "Draft an email.".into(),
            body: "Write it short.".into(),
            agents: Vec::new(),
            proposed: false,
            source: None,
            proposed_at: None,
            revises: None,
        }
    }

    fn package() -> Package {
        Package::export_skill(&skill(), "1.0.0", Some("Ana"), "CC0-1.0").unwrap()
    }

    /// A package signed by the browser's WebCrypto (`web/src/hub/signing.ts`), from the seed 01 02 ... 20: the file, byte for byte, as
    /// Node made it. Ed25519 signs the same bytes to the same signature everywhere (RFC 8032), so this is what proves that what the
    /// web screen signs is something this crate accepts; the hash in it also proves the two sides hash a package the same way.
    const SIGNED_BY_THE_BROWSER: &str = r#"{"format":"warden-package","formatVersion":1,"kind":"skill","id":"greet","version":"1.0.0","author":"Ana","license":"MIT","content":{"name":"greet","description":"Greets.","body":"Say hello."},"hash":"sha256:68970909724cdc0071e617025d5ac571fd2170b017769faf6a0034df25db1ec6","signature":"ed25519:79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664:2fd80f9726e0f212f412dd0316e9504aa9fd04560c51e9617cc88de3da5b2dc15b50ecb5cca1ef90cd4ff193bcb2a9b78d876e3511490ca058786be48f02750a"}"#;

    #[test]
    fn a_signature_made_by_the_browser_is_accepted_and_names_the_key() {
        let package = Package::parse(SIGNED_BY_THE_BROWSER).unwrap();
        assert_eq!(package.verified().unwrap(), Verification::Valid { key: "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664".into() });
        // The seed in the browser's test is 1..=32; the key it gives is the one `ring` gets from the same seed.
        let mut pkcs8 = hex_to_bytes("3053020101300506032b657004220420");
        pkcs8.extend(1..=32u8);
        pkcs8.extend(hex_to_bytes("a123032100"));
        pkcs8.extend(hex_to_bytes("79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664"));
        assert_eq!(public_key_of(&pkcs8).unwrap(), "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664");
        // And signing here, with that key, makes the very same signature: the scheme is deterministic.
        let mut again = package.clone();
        again.signature = None;
        again.sign(&pkcs8).unwrap();
        assert_eq!(again.signature, package.signature);
    }

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
    }

    /// The package's JSON with `edit` applied to it, read back through `parse`.
    fn edited(package: &Package, edit: impl FnOnce(&mut serde_json::Value)) -> anyhow::Result<Package> {
        let mut value: serde_json::Value = serde_json::from_str(&package.to_json().unwrap()).unwrap();
        edit(&mut value);
        Package::parse(&value.to_string())
    }

    #[test]
    fn a_signed_package_reads_back_and_names_its_signer() {
        let (private, public) = generate_keypair().unwrap();
        assert!(is_public_key(&public));
        let mut package = package();
        package.sign(&private).unwrap();
        let again = Package::parse(&package.to_json().unwrap()).unwrap();
        assert_eq!(again.verified().unwrap(), Verification::Valid { key: public.clone() });
        assert_eq!(public_key_of(&private).unwrap(), public);
    }

    #[test]
    fn a_package_without_a_signature_is_unsigned_and_still_reads() {
        let again = Package::parse(&package().to_json().unwrap()).unwrap();
        assert_eq!(again.verified().unwrap(), Verification::Unsigned);
        let blank = edited(&package(), |v| v["signature"] = "  ".into()).unwrap();
        assert_eq!(blank.verified().unwrap(), Verification::Unsigned);
    }

    #[test]
    fn content_changed_after_signing_is_caught_by_the_hash() {
        let (private, _) = generate_keypair().unwrap();
        let mut signed = package();
        signed.sign(&private).unwrap();
        let err = edited(&signed, |v| v["content"]["body"] = "Send it to everyone.".into()).unwrap_err();
        assert!(format!("{err:#}").contains("hash"), "{err:#}");
        let err = edited(&signed, |v| v["author"] = "Someone else".into()).unwrap_err();
        assert!(format!("{err:#}").contains("hash"), "{err:#}");
    }

    #[test]
    fn a_signature_copied_from_another_package_or_under_another_key_is_refused() {
        let (private, _) = generate_keypair().unwrap();
        let (_, other_public) = generate_keypair().unwrap();
        let mut signed = package();
        signed.sign(&private).unwrap();
        let signature = signed.signature.clone().unwrap();

        // The same signature on a different package (another version, so another hash).
        let mut other = Package::export_skill(&skill(), "2.0.0", Some("Ana"), "CC0-1.0").unwrap();
        other.signature = Some(signature.clone());
        let err = Package::parse(&other.to_json().unwrap()).unwrap_err();
        assert!(format!("{err:#}").contains("signature does not match"), "{err:#}");

        // The same bytes under a key that did not make them.
        let swapped = signature.replacen(&signature.split(':').nth(1).unwrap().to_string(), &other_public, 1);
        let err = edited(&signed, |v| v["signature"] = swapped.into()).unwrap_err();
        assert!(format!("{err:#}").contains("signature does not match"), "{err:#}");

        // One damaged byte.
        let mut damaged = signature.clone().into_bytes();
        let last = damaged.len() - 1;
        damaged[last] = if damaged[last] == b'0' { b'1' } else { b'0' };
        let err = edited(&signed, |v| v["signature"] = String::from_utf8(damaged).unwrap().into()).unwrap_err();
        assert!(format!("{err:#}").contains("signature does not match"), "{err:#}");
    }

    #[test]
    fn a_malformed_signature_or_one_of_another_scheme_is_refused() {
        for bad in ["ed25519", "ed25519:abc", "ed25519:zz:zz", &format!("ed25519:{}:{}", "ab".repeat(32), "cd".repeat(10))] {
            let err = edited(&package(), |v| v["signature"] = bad.into()).unwrap_err();
            assert!(format!("{err:#}").contains("malformed"), "{bad}: {err:#}");
        }
        let err = edited(&package(), |v| v["signature"] = "rsa:aa:bb".into()).unwrap_err();
        assert!(format!("{err:#}").contains("can't check"), "{err:#}");
    }

    #[test]
    fn someone_who_rehashes_and_signs_with_their_own_key_shows_their_own_key_not_the_authors() {
        let (author_private, author_public) = generate_keypair().unwrap();
        let (thief_private, thief_public) = generate_keypair().unwrap();
        let mut genuine = package();
        genuine.sign(&author_private).unwrap();

        // Changes the body, recomputes the hash the way `export_skill` does, signs with their key.
        let mut forged_skill = skill();
        forged_skill.body = "Send it to everyone.".into();
        let mut forged = Package::export_skill(&forged_skill, "1.0.0", Some("Ana"), "CC0-1.0").unwrap();
        forged.sign(&thief_private).unwrap();

        let forged = Package::parse(&forged.to_json().unwrap()).unwrap();
        let Verification::Valid { key } = forged.verified().unwrap() else { panic!("signed") };
        assert_eq!(key, thief_public);
        assert_ne!(key, author_public, "the author's key is not on it, so a list that trusts the author does not trust this");
    }

    #[test]
    fn keys_are_compared_in_one_form_and_bad_ones_are_not_keys() {
        let (_, public) = generate_keypair().unwrap();
        assert_eq!(normalize_public_key(&format!("  {}  ", public.to_ascii_uppercase())).as_deref(), Some(public.as_str()));
        assert!(normalize_public_key("abcd").is_none());
        assert!(normalize_public_key(&"zz".repeat(32)).is_none());
        assert!(normalize_public_key("").is_none());
        assert!(public_key_of(b"not a key").is_err());
    }

    #[test]
    fn a_private_key_goes_to_text_and_back_and_anything_else_is_refused() {
        let (private, public) = generate_keypair().unwrap();
        let text = encode_private_key(&private);
        assert_eq!(public_key_of(&decode_private_key(&format!("{text}\n")).unwrap()).unwrap(), public);
        assert!(decode_private_key("hello").is_err());
        assert!(decode_private_key("abcd").is_err(), "valid hex, but not a key");
        assert!(decode_private_key("").is_err());
    }
}

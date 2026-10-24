//! The signature on every call and webhook of the contract.
//!
//! The header is `X-Warden-Signature: t=<unix seconds>,v1=<hex>`, where the MAC is
//! `HMAC-SHA256(secret, "<t>.<METHOD>.<path and query>.<body>")`. The method and the path are signed so a request captured
//! on the wire cannot be replayed against another account's URL, and the timestamp so it cannot be replayed later: a
//! signature more than `TOLERANCE_SECS` away from the receiver's clock (either way) is refused. The comparison is
//! constant-time. The format follows the Stripe style that `warden-server` already verifies for inbound third-party
//! webhooks, extended with the method and the path.
//!
//! The two directions use two different secrets (the site mãe's calls to the gateway, and the gateway's webhooks): a leak
//! of one does not let anyone speak as the other.

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const SIGNATURE_HEADER: &str = "X-Warden-Signature";
/// Sent on webhooks: the event id, so the receiver can drop a delivery it has already handled.
pub const IDEMPOTENCY_HEADER: &str = "Idempotency-Key";
/// How far the signature's timestamp may be from the receiver's clock, in seconds.
pub const TOLERANCE_SECS: i64 = 300;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureError {
    /// The header is missing or not `t=…,v1=…`.
    Malformed,
    /// The timestamp is outside the tolerance.
    Stale,
    /// The MAC doesn't match: wrong secret, or the method, path or body changed.
    Mismatch,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SignatureError::Malformed => "the signature header is missing or malformed",
            SignatureError::Stale => "the signature is too old or too far in the future",
            SignatureError::Mismatch => "the signature doesn't match",
        })
    }
}

impl std::error::Error for SignatureError {}

fn mac(secret: &str, timestamp: i64, method: &str, path: &str, body: &[u8]) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("an HMAC key can be any length");
    mac.update(format!("{timestamp}.{}.{path}.", method.to_ascii_uppercase()).as_bytes());
    mac.update(body);
    mac
}

/// The header value for a call made at `timestamp`: `t=<timestamp>,v1=<hex>`.
pub fn sign(secret: &str, timestamp: i64, method: &str, path: &str, body: &[u8]) -> String {
    let tag = mac(secret, timestamp, method, path, body).finalize().into_bytes();
    format!("t={timestamp},v1={}", hex::encode(tag))
}

/// Checks a header against what was received. `now` is the receiver's clock, in unix seconds.
pub fn verify(secret: &str, header: Option<&str>, method: &str, path: &str, body: &[u8], now: i64) -> Result<(), SignatureError> {
    let header = header.ok_or(SignatureError::Malformed)?;
    let mut timestamp = None;
    let mut candidate = None;
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => timestamp = v.parse::<i64>().ok(),
            Some(("v1", v)) => candidate = hex::decode(v).ok(),
            _ => {}
        }
    }
    let (Some(timestamp), Some(candidate)) = (timestamp, candidate) else {
        return Err(SignatureError::Malformed);
    };
    if (now - timestamp).abs() > TOLERANCE_SECS {
        return Err(SignatureError::Stale);
    }
    mac(secret, timestamp, method, path, body).verify_slice(&candidate).map_err(|_| SignatureError::Mismatch)
}

/// Like `verify`, for a secret that is being rotated: the call is good if **any** of the secrets signed it. The errors that
/// do not depend on the secret (`Malformed`, `Stale`) are the same whichever is tried, so they are returned at once.
pub fn verify_any(secrets: &[&str], header: Option<&str>, method: &str, path: &str, body: &[u8], now: i64) -> Result<(), SignatureError> {
    let mut last = SignatureError::Mismatch;
    for secret in secrets {
        match verify(secret, header, method, path, body, now) {
            Ok(()) => return Ok(()),
            Err(SignatureError::Mismatch) => last = SignatureError::Mismatch,
            Err(other) => return Err(other),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const PATH: &str = "/v1/accounts/acc_1/credits";
    const BODY: &[u8] = br#"{"amount_micro_usd":1000000,"payment_ref":"p1"}"#;

    fn signed() -> String {
        sign("secret", NOW, "POST", PATH, BODY)
    }

    #[test]
    fn a_signature_verifies() {
        assert_eq!(verify("secret", Some(&signed()), "POST", PATH, BODY, NOW), Ok(()));
        assert_eq!(verify("secret", Some(&signed()), "post", PATH, BODY, NOW), Ok(()), "the method is case-insensitive");
    }

    #[test]
    fn verify_any_accepts_a_call_signed_with_any_of_the_secrets() {
        let header = signed();
        assert_eq!(verify_any(&["secret"], Some(&header), "POST", PATH, BODY, NOW), Ok(()));
        assert_eq!(verify_any(&["new", "secret"], Some(&header), "POST", PATH, BODY, NOW), Ok(()), "the old one still works");
        assert_eq!(verify_any(&["secret", "new"], Some(&header), "POST", PATH, BODY, NOW), Ok(()), "whatever the order");
        assert_eq!(verify_any(&["one", "two"], Some(&header), "POST", PATH, BODY, NOW), Err(SignatureError::Mismatch));
        assert_eq!(verify_any(&[], Some(&header), "POST", PATH, BODY, NOW), Err(SignatureError::Mismatch), "no secret accepts nothing");
    }

    #[test]
    fn verify_any_still_refuses_a_stale_or_malformed_header_whatever_the_secrets() {
        assert_eq!(verify_any(&["new", "secret"], Some(&signed()), "POST", PATH, BODY, NOW + 301), Err(SignatureError::Stale));
        assert_eq!(verify_any(&["new", "secret"], Some("garbage"), "POST", PATH, BODY, NOW), Err(SignatureError::Malformed));
        assert_eq!(verify_any(&["secret"], None, "POST", PATH, BODY, NOW), Err(SignatureError::Malformed));
    }

    #[test]
    fn the_header_has_the_documented_shape() {
        let header = signed();
        let (t, v1) = header.split_once(',').unwrap();
        assert_eq!(t, format!("t={NOW}"));
        assert!(v1.starts_with("v1=") && v1.len() == 3 + 64, "{header}");
    }

    #[test]
    fn anything_that_changes_breaks_it() {
        let header = signed();
        let check = |secret: &str, method: &str, path: &str, body: &[u8]| verify(secret, Some(&header), method, path, body, NOW);
        assert_eq!(check("other", "POST", PATH, BODY), Err(SignatureError::Mismatch));
        assert_eq!(check("secret", "GET", PATH, BODY), Err(SignatureError::Mismatch));
        assert_eq!(check("secret", "POST", "/v1/accounts/acc_2/credits", BODY), Err(SignatureError::Mismatch));
        assert_eq!(check("secret", "POST", PATH, br#"{"amount_micro_usd":9000000,"payment_ref":"p1"}"#), Err(SignatureError::Mismatch));
    }

    #[test]
    fn an_old_or_future_timestamp_is_refused() {
        let header = signed();
        assert_eq!(verify("secret", Some(&header), "POST", PATH, BODY, NOW + TOLERANCE_SECS), Ok(()));
        assert_eq!(verify("secret", Some(&header), "POST", PATH, BODY, NOW + TOLERANCE_SECS + 1), Err(SignatureError::Stale));
        assert_eq!(verify("secret", Some(&header), "POST", PATH, BODY, NOW - TOLERANCE_SECS - 1), Err(SignatureError::Stale));
    }

    #[test]
    fn a_new_timestamp_with_an_old_mac_is_refused() {
        // Changing `t` to look fresh must not work: the MAC covers it.
        let forged = signed().replacen(&format!("t={NOW}"), &format!("t={}", NOW + 100), 1);
        assert_eq!(verify("secret", Some(&forged), "POST", PATH, BODY, NOW + 100), Err(SignatureError::Mismatch));
    }

    #[test]
    fn missing_or_malformed_headers_are_refused() {
        for header in [None, Some(""), Some("garbage"), Some("t=abc,v1=00"), Some("t=1"), Some("v1=00"), Some("t=1,v1=zz")] {
            assert_eq!(verify("secret", header, "POST", PATH, BODY, NOW), Err(SignatureError::Malformed), "{header:?}");
        }
    }

    #[test]
    fn a_get_without_a_body_signs_the_empty_body() {
        let header = sign("secret", NOW, "GET", "/v1/accounts/acc_1/balance", b"");
        assert_eq!(verify("secret", Some(&header), "GET", "/v1/accounts/acc_1/balance", b"", NOW), Ok(()));
        assert_eq!(verify("secret", Some(&header), "GET", "/v1/accounts/acc_1/balance", b"x", NOW), Err(SignatureError::Mismatch));
    }
}

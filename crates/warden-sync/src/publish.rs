//! Publishing a package (P160) to Arweave through the TruthID app's `pin()`: the same exchange `push` uses for the vault, but
//! the content is the package's own JSON in plain text, on purpose, because what is published is meant to be read by anyone.
//! The caller shows the QR (`begin.pending.qr_payload_json()`) *before* awaiting `run_publish`, so the person can scan it.
//!
//! Only a package that reads back clean is published (`Package::parse`: size, format, fields and hash), so nothing invalid
//! gets pinned for good.

use warden_core::skill::package::Package;
use warden_truthid::{PendingPin, PinResult};

/// What the QR shows and what will be pinned, ready to wait on the phone.
pub struct BeginPublish {
    pub pending: PendingPin,
    content: Vec<u8>,
    pub id: String,
    pub version: String,
    pub hash: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PublishOutcome {
    /// The Arweave transaction id: the package is at `ar://<tx_id>`.
    pub tx_id: String,
    pub id: String,
    pub version: String,
    pub hash: String,
}

/// Checks `package_json` and starts a `PendingPin` session for it. Does no network I/O yet.
pub fn begin_publish(package_json: &str) -> anyhow::Result<BeginPublish> {
    let package = Package::parse(package_json)?;
    let pending = PendingPin::begin_with_default_timeout("Warden")?;
    Ok(BeginPublish { pending, content: package_json.as_bytes().to_vec(), id: package.id, version: package.version, hash: package.hash })
}

/// Waits for the TruthID phone to pin the package, sweeping every host of the local networks.
pub async fn run_publish(begin: BeginPublish) -> anyhow::Result<PublishOutcome> {
    let hosts = warden_truthid::lan::candidate_hosts()?;
    run_publish_with_hosts(begin, hosts).await
}

/// Same as `run_publish`, but sweeps only `hosts` — lets tests target a fake phone on `127.0.0.1`.
pub async fn run_publish_with_hosts(begin: BeginPublish, hosts: Vec<std::net::Ipv4Addr>) -> anyhow::Result<PublishOutcome> {
    let BeginPublish { pending, content, id, version, hash } = begin;
    let pin_result = pending.run_with_hosts(&content, hosts).await?;
    let tx_id = apply_pin_result(&pin_result)?;
    Ok(PublishOutcome { tx_id, id, version, hash })
}

/// The transaction id of a pin that worked, or why it did not.
fn apply_pin_result(pin_result: &PinResult) -> anyhow::Result<String> {
    if pin_result.status != "pinned" {
        anyhow::bail!(pin_result.error.clone().unwrap_or_else(|| format!("TruthID pin failed with status {:?}", pin_result.status)));
    }
    pin_result
        .cid
        .as_deref()
        .and_then(|cid| cid.strip_prefix("ar://"))
        .filter(|tx_id| !tx_id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("TruthID pin succeeded but returned no ar:// id"))
}

#[cfg(test)]
mod tests {
    use warden_core::skill::package::MAX_PACKAGE_BYTES;
    use warden_core::skill::Skill;

    use super::*;

    fn pin(status: &str, cid: Option<&str>, error: Option<&str>) -> PinResult {
        PinResult {
            status: status.into(),
            cid: cid.map(str::to_string),
            content_hash: None,
            providers_ok: None,
            providers_failed: None,
            error: error.map(str::to_string),
        }
    }

    fn package_json(body: &str) -> String {
        let skill = Skill { name: "greet".into(), description: "Says hello.".into(), body: body.into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None };
        Package::export_skill(&skill, "1.0.0", Some("Ana"), "CC0-1.0").unwrap().to_json().unwrap()
    }

    #[test]
    fn a_pinned_result_gives_the_transaction_id() {
        assert_eq!(apply_pin_result(&pin("pinned", Some("ar://abc123"), None)).unwrap(), "abc123");
    }

    #[test]
    fn a_pin_that_did_not_work_says_why() {
        let err = apply_pin_result(&pin("rejected", None, Some("the person said no"))).unwrap_err().to_string();
        assert_eq!(err, "the person said no");
        let err = apply_pin_result(&pin("failed", None, None)).unwrap_err().to_string();
        assert!(err.contains("failed"), "{err}");
    }

    #[test]
    fn a_pinned_result_without_an_arweave_id_is_an_error() {
        for cid in [None, Some("ipfs://abc"), Some("ar://"), Some("abc")] {
            assert!(apply_pin_result(&pin("pinned", cid, None)).is_err(), "{cid:?}");
        }
    }

    #[test]
    fn only_a_package_that_reads_back_clean_is_published() {
        let good = package_json("Say hello.");
        let begin = begin_publish(&good).unwrap();
        assert_eq!((begin.id.as_str(), begin.version.as_str()), ("greet", "1.0.0"));
        assert!(begin.hash.starts_with("sha256:"));
        assert_eq!(begin.content, good.as_bytes());

        assert!(begin_publish("not a package").unwrap_err().to_string().contains("not a Warden package"));
        let altered = good.replace("Say hello.", "Say goodbye.");
        assert!(begin_publish(&altered).unwrap_err().to_string().contains("changed or damaged"));
        assert!(begin_publish(&"x".repeat(MAX_PACKAGE_BYTES + 1)).is_err());
    }
}

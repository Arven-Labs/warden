//! The hub a vault in arca mode syncs with, as a client the arca can use: the saved hub (`hubs.json`) and the device token this
//! computer was issued for it (`remote_hub.json`). Shared by every front end on this computer (the desktop and the CLI), so none
//! of them rebuilds how a token is found or which addresses are refused.
//!
//! Never asks the person for an address or a token: a hub this computer never signed in to is a sentence saying so.

use std::path::Path;

use warden_sync::arca_sync::{HubError, HubStore};

use crate::remote_identities::{RemoteIdentities, RemoteIdentity};
use crate::saved_hubs::{self, SavedHub};

/// The token this computer was issued for `hub`, or the sentence that tells the person to sign in first.
pub fn device_token<'a>(hub: &SavedHub, identity: &'a RemoteIdentity) -> Result<&'a str, String> {
    identity
        .device_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| format!("not signed in to '{}' yet: sign in to this hub first", hub.name))
}

/// The hub the sync talks to, with the token this computer was issued for it. Refuses plain `http` to another machine (the
/// token would travel in the clear).
pub fn hub_store(hub: &SavedHub, identity: &RemoteIdentity) -> Result<HubStore, String> {
    let token = device_token(hub, identity)?;
    HubStore::new(&hub.url, token).map_err(|e| match e {
        HubError::InsecureUrl => format!("'{}' is plain http to another machine, so the token would travel in the clear: save the hub with an https address", hub.name),
        HubError::InvalidUrl => format!("'{}' is not a hub address the sync can use (an https address, with nothing after the host)", hub.url),
        HubError::InvalidToken => format!("the token saved for '{}' cannot be used: sign in to this hub again", hub.name),
        other => other.to_string(),
    })
}

/// The saved hub `hub_id`, with the token this computer holds for it, ready to sync with. `device_name` only matters the first
/// time, when the computer has no identity for that hub yet.
pub fn saved_hub_store(hubs_path: &Path, config_file: &Path, hub_id: &str, device_name: &str) -> Result<HubStore, String> {
    let hub = saved_hubs::find(hubs_path, hub_id).map_err(|e| format!("{e:#}"))?.ok_or_else(|| format!("there is no saved hub '{hub_id}' (it may have been removed)"))?;
    let identity = RemoteIdentities::beside(config_file).load_or_create(hub_id, device_name).map_err(|e| format!("{e:#}"))?;
    hub_store(&hub, &identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub(url: &str) -> SavedHub {
        SavedHub { id: "hub-00000001".into(), name: "Home".into(), url: url.into() }
    }

    fn identity(token: Option<&str>) -> RemoteIdentity {
        RemoteIdentity { device_id: "dev-1".into(), device_token: token.map(String::from) }
    }

    #[test]
    fn a_hub_the_computer_never_signed_in_to_is_refused_with_a_sentence_that_says_what_to_do() {
        for token in [None, Some("")] {
            let error = hub_store(&hub("https://hub.example/"), &identity(token)).unwrap_err();
            assert!(error.contains("sign in"), "{error}");
        }
    }

    #[test]
    fn plain_http_to_another_machine_is_refused_and_https_and_loopback_are_not() {
        let error = hub_store(&hub("http://192.168.0.10:7000/"), &identity(Some("tok"))).unwrap_err();
        assert!(error.contains("https"), "{error}");
        assert!(hub_store(&hub("https://hub.example/"), &identity(Some("tok"))).is_ok());
        assert!(hub_store(&hub("http://127.0.0.1:7000/"), &identity(Some("tok"))).is_ok());
    }

    #[test]
    fn the_error_never_carries_the_token() {
        let error = hub_store(&hub("http://192.168.0.10:7000/"), &identity(Some("secret-token-123"))).unwrap_err();
        assert!(!error.contains("secret-token-123"), "{error}");
    }
}

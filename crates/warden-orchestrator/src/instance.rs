//! What the orchestrator knows about one customer's instance.

use serde::{Deserialize, Serialize};

/// The port every hub listens on inside its container (the image's `EXPOSE`).
pub const HUB_PORT: u16 = 7420;

/// What a caller asks for when it provisions an instance: the body of `PUT /v1/instances/{customer_id}` in the contract
/// (`HOSPEDAGEM.md` B1). Provisioning twice with the same spec is a no-op; with a different one it is refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstanceSpec {
    pub customer_id: String,
    /// The image to run, e.g. `ghcr.io/arven-labs/warden-server:0.1.0`.
    pub image: String,
    /// Memory limit of the container, in MiB. `None`: no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u64>,
    /// CPU limit of the container, in cores. `None`: no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpus: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Meant to be running.
    Active,
    /// Stopped on purpose; the data stays.
    Suspended,
}

/// An instance as the orchestrator keeps it: the spec plus what it decided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    #[serde(flatten)]
    pub spec: InstanceSpec,
    pub state: State,
    /// The host port the hub is published on (always on 127.0.0.1: the reverse proxy comes in front, in another slice).
    pub port: u16,
    /// The hub's pairing key, made when the instance was. The orchestrator reads `/status` with it.
    pub pairing_key: String,
    pub created_at_ms: u64,
}

impl Instance {
    pub fn container_name(&self) -> String {
        container_name(&self.spec.customer_id)
    }

    pub fn volume_name(&self) -> String {
        volume_name(&self.spec.customer_id)
    }
}

pub fn container_name(customer_id: &str) -> String {
    format!("warden-{customer_id}")
}

pub fn volume_name(customer_id: &str) -> String {
    format!("warden-data-{customer_id}")
}

/// A customer id ends up in container and volume names, so it is held to what Docker takes: lowercase letters, digits,
/// `-` and `_`, starting with a letter or digit, at most 40 characters.
pub fn validate_customer_id(id: &str) -> anyhow::Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 40
        && id.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    anyhow::ensure!(ok, "a customer id is 1 to 40 lowercase letters, digits, - or _, starting with a letter or digit (got '{id}')");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_ids_are_held_to_what_docker_takes() {
        for good in ["a", "acme", "acme-1", "a_b", "0abc"] {
            assert!(validate_customer_id(good).is_ok(), "{good}");
        }
        for bad in ["", "-a", "_a", "Acme", "a b", "a/b", "a.b", "é", &"x".repeat(41)] {
            assert!(validate_customer_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn names_are_derived_from_the_customer() {
        assert_eq!(container_name("acme"), "warden-acme");
        assert_eq!(volume_name("acme"), "warden-data-acme");
    }
}

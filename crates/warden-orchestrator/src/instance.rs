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
    /// The `config.toml` a new instance starts with, put in its volume before the first start and never over one that is
    /// already there. A hub with no model provider configured refuses to start, so an instance needs one from the first
    /// run (later: the gateway's `Provider::Cloud`, P139). It may hold secrets: never printed, see `differs_from`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
}

impl InstanceSpec {
    /// Which settings differ from `other`, by name. For the message of a refused provision: the spec itself is not printed,
    /// because the config in it can carry a token.
    pub fn differs_from(&self, other: &InstanceSpec) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.image != other.image {
            names.push("image");
        }
        if self.memory_mb != other.memory_mb {
            names.push("memory_mb");
        }
        if self.cpus != other.cpus {
            names.push("cpus");
        }
        if self.config != other.config {
            names.push("config");
        }
        names
    }
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
    /// When the data is erased, for an instance whose delete was asked for: the container is stopped and the record and
    /// the volume stay until then (`HOSPEDAGEM.md` question 4). `None`: no delete is pending. Resuming cancels it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_after_ms: Option<u64>,
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
    fn the_differences_are_named_and_the_config_is_never_in_them() {
        let base = InstanceSpec { customer_id: "acme".into(), image: "img:1".into(), memory_mb: None, cpus: None, config: Some("token = \"s3cret\"".into()) };
        assert!(base.differs_from(&base.clone()).is_empty());
        let other = InstanceSpec { image: "img:2".into(), memory_mb: Some(256), config: None, ..base.clone() };
        let names = base.differs_from(&other);
        assert_eq!(names, ["image", "memory_mb", "config"]);
        assert!(!format!("{names:?}").contains("s3cret"));
    }

    #[test]
    fn a_record_written_before_the_deadline_existed_still_loads_and_has_none() {
        let old = r#"{"customer_id":"acme","image":"img:1","state":"suspended","port":20001,"pairing_key":"k","created_at_ms":1}"#;
        let instance: Instance = serde_json::from_str(old).unwrap();
        assert_eq!(instance.delete_after_ms, None);
        assert!(!serde_json::to_string(&instance).unwrap().contains("delete_after_ms"), "no deadline, no field");

        let pending = Instance { delete_after_ms: Some(42), ..instance };
        let back: Instance = serde_json::from_str(&serde_json::to_string(&pending).unwrap()).unwrap();
        assert_eq!(back.delete_after_ms, Some(42));
    }

    #[test]
    fn names_are_derived_from_the_customer() {
        assert_eq!(container_name("acme"), "warden-acme");
        assert_eq!(volume_name("acme"), "warden-data-acme");
    }
}

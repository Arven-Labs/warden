//! One pass of the recycler: ask every active instance how loaded it is, and restart the idle heavy ones.
//!
//! The decision is `RecyclePolicy::decide`; this is only what acts on it. An instance that cannot be asked, or whose
//! restart fails, is reported and left; it never stops the pass for the others.

use crate::instance::State;
use crate::manager::InstanceManager;
use crate::recycle::{Decision, Reason, RecyclePolicy};
use crate::runtime::ContainerRuntime;
use crate::status::StatusSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It was idle and heavy, and was restarted.
    Recycled,
    /// Left alone, for this reason.
    Kept(Reason),
    /// Could not be asked or restarted. The message says what failed.
    Failed(String),
}

/// Suspended instances are not looked at: they are stopped on purpose, and there is no one to ask.
pub async fn recycle_pass<R: ContainerRuntime, S: StatusSource>(
    manager: &InstanceManager<R>,
    source: &S,
    policy: &RecyclePolicy,
) -> anyhow::Result<Vec<(String, Outcome)>> {
    let mut report = Vec::new();
    for instance in manager.list()?.into_iter().filter(|i| i.state == State::Active) {
        let id = instance.spec.customer_id.clone();
        let outcome = match source.fetch(&instance).await {
            Err(err) => Outcome::Failed(format!("{err:#}")),
            Ok(status) => match policy.decide(&status) {
                Decision::Keep(reason) => Outcome::Kept(reason),
                Decision::Recycle => match manager.restart(&instance).await {
                    Ok(()) => Outcome::Recycled,
                    Err(err) => Outcome::Failed(format!("{err:#}")),
                },
            },
        };
        report.push((id, outcome));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use warden_server_protocol::HubStatus;

    use super::*;
    use crate::fake::FakeRuntime;
    use crate::instance::{Instance, InstanceSpec};
    use crate::store::InstanceStore;

    /// Answers with a fixed status per customer; a customer with none is unreachable.
    struct Fixed(HashMap<String, HubStatus>);

    #[async_trait]
    impl StatusSource for Fixed {
        async fn fetch(&self, instance: &Instance) -> anyhow::Result<HubStatus> {
            self.0.get(&instance.spec.customer_id).cloned().ok_or_else(|| anyhow::anyhow!("connection refused"))
        }
    }

    fn manager(name: &str) -> InstanceManager<FakeRuntime> {
        let dir = std::env::temp_dir().join(format!("warden-orchestrator-recycler-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        InstanceManager::new(InstanceStore::new(dir), FakeRuntime::default())
    }

    fn spec(id: &str) -> InstanceSpec {
        InstanceSpec { customer_id: id.into(), image: "img".into(), memory_mb: None, cpus: None }
    }

    fn policy() -> RecyclePolicy {
        RecyclePolicy { rss_threshold_bytes: 500, min_idle_secs: 100, min_uptime_secs: 1000 }
    }

    fn heavy_and_idle() -> HubStatus {
        HubStatus { version: "0.1.0".into(), uptime_secs: 5000, rss_bytes: Some(900), connections: 0, in_flight: 0, idle_secs: 600 }
    }

    fn outcome(report: &[(String, Outcome)], id: &str) -> Outcome {
        report.iter().find(|(c, _)| c == id).unwrap_or_else(|| panic!("{id} is not in the report")).1.clone()
    }

    #[tokio::test]
    async fn only_the_idle_heavy_instance_is_restarted() {
        let m = manager("only-heavy");
        for id in ["heavy", "busy", "light"] {
            m.provision(spec(id)).await.unwrap();
        }
        let source = Fixed(HashMap::from([
            ("heavy".to_string(), heavy_and_idle()),
            ("busy".to_string(), HubStatus { in_flight: 2, ..heavy_and_idle() }),
            ("light".to_string(), HubStatus { rss_bytes: Some(50), ..heavy_and_idle() }),
        ]));
        let report = recycle_pass(&m, &source, &policy()).await.unwrap();
        assert_eq!(outcome(&report, "heavy"), Outcome::Recycled);
        assert_eq!(outcome(&report, "busy"), Outcome::Kept(Reason::Busy));
        assert_eq!(outcome(&report, "light"), Outcome::Kept(Reason::Light));
        assert_eq!(m.runtime().restarts(), ["warden-heavy"]);
        assert_eq!(m.runtime().container("warden-heavy").unwrap().starts, 2);
    }

    #[tokio::test]
    async fn a_suspended_instance_is_not_asked_nor_restarted() {
        let m = manager("suspended");
        m.provision(spec("paused")).await.unwrap();
        m.suspend("paused").await.unwrap();
        let source = Fixed(HashMap::from([("paused".to_string(), heavy_and_idle())]));
        let report = recycle_pass(&m, &source, &policy()).await.unwrap();
        assert!(report.is_empty());
        assert!(m.runtime().restarts().is_empty());
        assert!(!m.runtime().container("warden-paused").unwrap().running, "still stopped");
    }

    #[tokio::test]
    async fn an_unreachable_or_unrestartable_instance_does_not_stop_the_others() {
        let m = manager("failures");
        for id in ["down", "stuck", "fine"] {
            m.provision(spec(id)).await.unwrap();
        }
        m.runtime().break_container("warden-stuck");
        let source = Fixed(HashMap::from([("stuck".to_string(), heavy_and_idle()), ("fine".to_string(), heavy_and_idle())]));
        let report = recycle_pass(&m, &source, &policy()).await.unwrap();
        assert!(matches!(outcome(&report, "down"), Outcome::Failed(msg) if msg.contains("connection refused")));
        assert!(matches!(outcome(&report, "stuck"), Outcome::Failed(_)));
        assert_eq!(outcome(&report, "fine"), Outcome::Recycled, "the one after the failures is still handled");
    }
}

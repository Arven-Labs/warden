//! The orchestrator of hosted Warden instances (P138, `HOSPEDAGEM.md` phase 4).
//!
//! One container per customer, driven through the `docker` CLI: `manager` provisions, suspends, resumes and deletes, each
//! idempotent by customer id; `recycler` restarts the instance that is idle *and* heavy, on the verdict of `recycle`.
//!
//! Why recycle (K.6 item 5): a hub that once loaded the history search model keeps ~560 MB of resident memory even after
//! unloading it (K.2), and scale-to-zero was ruled out (it would break the bots, the scheduled tasks, the sync and the
//! nodes' connections). So an instance that is idle *and* heavy is stopped and started again, which is the only way proven
//! to give the memory back.
//!
//! Everything that touches the world sits behind a trait (`ContainerRuntime`, `StatusSource`), so the rest is tested with
//! `fake::FakeRuntime` and a canned status. `backup` takes the encrypted copies of a volume and restores and checks them;
//! `upgrade` moves instances to a new image in waves, putting the old one back where the new one does not come up.
//! A delete is scheduled (`schedule_delete`): the container stops and the data stays until the deadline, when
//! `purge_expired` erases it (`purge-expired --yes`; without `--yes` it only lists). `api` is the site mother's HTTP
//! contract (B1), signed, served by `serve`. Not here yet: the reverse proxy with TLS and a domain per customer, and the
//! loop (`serve` and `purge-expired --every`) as a service.

pub mod api;
pub mod backup;
pub mod fake;
pub mod instance;
pub mod manager;
pub mod recycle;
pub mod recycler;
pub mod runtime;
pub mod ship;
pub mod status;
pub mod store;
pub mod upgrade;

pub use backup::BackupKey;
pub use instance::{Instance, InstanceSpec, State};
pub use manager::InstanceManager;
pub use recycle::{Decision, Reason, RecyclePolicy};
pub use recycler::{recycle_pass, Outcome};
pub use runtime::{ContainerRuntime, DockerCli};
pub use ship::{BackupSink, CommandSink};
pub use status::{HttpStatus, StatusSource};
pub use upgrade::{UpgradeOptions, UpgradeOutcome};
pub use store::InstanceStore;

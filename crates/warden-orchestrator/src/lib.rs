//! The orchestrator of hosted Warden instances (P138, `HOSPEDAGEM.md` phase 4).
//!
//! What exists so far is the recycling policy (K.6 item 5). A hub that once loaded the history search model keeps
//! ~560 MB of resident memory even after unloading it (K.2), and scale-to-zero was ruled out (it would break the bots,
//! the scheduled tasks, the sync and the nodes' connections). So an instance that is idle *and* heavy is stopped and
//! started again, which is the only way proven to give the memory back.

pub mod recycle;

pub use recycle::{Decision, Reason, RecyclePolicy};

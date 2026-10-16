//! The `--log-format` flag of `serve` (P143 d). The subscriber itself, and its tests, live in
//! `warden_bootstrap::logging`, shared with the other binaries.

use clap::ValueEnum;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

/// Installs the global subscriber. Called once, by `serve` — the commands that print for a person (`api-keys create`,
/// `webhooks token`) don't log, so they never call it.
pub fn init(format: LogFormat) {
    warden_bootstrap::logging::init(format == LogFormat::Json);
}

//! Reading the SSH audit log (P47) back for the screens and the CLI. The tools write one JSON line per call
//! (`warden_core::tool::ssh::AuditLog`, `~/.config/warden/ssh_audit.jsonl`, rotated into `.1` to `.3`): the host, the agent, what was
//! asked and whether a person approved it, and how it ended. The log never holds the output of a command, and the command itself may hold
//! a secret, so what this returns is for the owner only (the hub hands a member nothing).
//!
//! Read when a screen asks and never kept: a line that is not JSON (a log cut short by a crash) is skipped, not a failure.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The most entries a read hands back (the newest ones).
pub const SSH_AUDIT_LIMIT: usize = 100;
/// The rotated files kept next to the log (`AUDIT_KEEP` in `warden-core`), `.1` being the newest.
const ROTATED_FILES: usize = 3;

/// One call of an SSH tool, as the log kept it.
#[derive(Debug, Clone, PartialEq)]
pub struct SshAuditEntry {
    pub at_ms: u64,
    pub host_id: String,
    /// The agent that asked; `None` when the call came from no agent in particular.
    pub agent: Option<String>,
    /// `exec`, `upload` or `download`.
    pub action: String,
    /// `not_required`, `approved`, `denied` (a person said no or did not answer) or `unavailable` (nobody could be asked).
    pub approval: String,
    /// The command line of an `exec`.
    pub command: Option<String>,
    pub local_path: Option<String>,
    pub remote_path: Option<String>,
    pub overwrite: bool,
    pub exit_code: Option<i64>,
    pub timed_out: bool,
    pub bytes: Option<u64>,
    /// Why the call failed or was refused, when it did.
    pub error: Option<String>,
}

impl SshAuditEntry {
    /// What the call was about: the command, or `local → remote` for a copy (the way a person reads it).
    pub fn detail(&self) -> String {
        match (&self.command, &self.local_path, &self.remote_path) {
            (Some(command), _, _) => command.clone(),
            (None, local, remote) => {
                let (local, remote) = (local.as_deref().unwrap_or("?"), remote.as_deref().unwrap_or("?"));
                let (from, to) = if self.action == "download" { (remote, local) } else { (local, remote) };
                format!("{from} → {to}{}", if self.overwrite { " (overwrite)" } else { "" })
            }
        }
    }

    /// How it ended, in a few words; empty when the call never ran and there is no reason to give.
    pub fn outcome(&self) -> String {
        if let Some(error) = &self.error {
            return format!("error: {error}");
        }
        if self.timed_out {
            return "timed out".to_string();
        }
        match (self.exit_code, self.bytes) {
            (Some(code), _) => format!("exit {code}"),
            (None, Some(bytes)) => format!("{bytes} bytes"),
            (None, None) => String::new(),
        }
    }

    /// One line for a terminal: `2026-10-12 14:03:11  prod  exec  approved  uptime  → exit 0`.
    pub fn summary_line(&self) -> String {
        let when = format_time(self.at_ms);
        let agent = self.agent.as_deref().map(|a| format!(" [{a}]")).unwrap_or_default();
        let outcome = self.outcome();
        let tail = if outcome.is_empty() { String::new() } else { format!("  → {outcome}") };
        format!("{when}  {}{agent}  {}  {}  {}{tail}", self.host_id, self.action, self.approval, self.detail())
    }
}

/// `2026-10-12 14:03:11` in UTC, from milliseconds since the epoch.
fn format_time(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// The log's files from the newest: the log itself, then `.1`, `.2`, `.3`.
fn files_newest_first(path: &Path) -> Vec<PathBuf> {
    let mut files = vec![path.to_path_buf()];
    for n in 1..=ROTATED_FILES {
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(".{n}"));
        files.push(PathBuf::from(name));
    }
    files
}

fn parse_line(line: &str) -> Option<SshAuditEntry> {
    let value: Value = serde_json::from_str(line).ok()?;
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let outcome = value.get("outcome").unwrap_or(&Value::Null);
    Some(SshAuditEntry {
        at_ms: value.get("ts").and_then(Value::as_u64)?,
        host_id: text("host_id")?,
        agent: text("agent"),
        action: text("action").unwrap_or_default(),
        approval: text("approval").unwrap_or_default(),
        command: text("command"),
        local_path: text("local_path"),
        remote_path: text("remote_path"),
        overwrite: value.get("overwrite").and_then(Value::as_bool).unwrap_or(false),
        exit_code: outcome.get("exit_code").and_then(Value::as_i64),
        timed_out: outcome.get("timed_out").and_then(Value::as_bool).unwrap_or(false),
        bytes: outcome.get("bytes").and_then(Value::as_u64),
        error: outcome.get("error").and_then(Value::as_str).map(str::to_string),
    })
}

/// The newest `limit` calls in the log at `path` and the files it was rotated into, newest first. A missing log is an empty list.
pub fn read_ssh_audit(path: &Path, limit: usize) -> Vec<SshAuditEntry> {
    let mut entries = Vec::new();
    for file in files_newest_first(path) {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        // Lines are appended, so the newest is last.
        for line in text.lines().rev() {
            if let Some(entry) = parse_line(line) {
                entries.push(entry);
                if entries.len() >= limit {
                    return entries;
                }
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use warden_core::tool::ssh::AuditLog;

    fn temp_log(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "warden-ssh-audit-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("ssh_audit.jsonl")
    }

    fn line(ts: u64, host: &str, extra: Value) -> Value {
        let mut base = json!({ "ts": ts, "host_id": host, "agent": null, "action": "exec", "approval": "not_required", "outcome": { "exit_code": 0 } });
        base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        base
    }

    #[test]
    fn a_missing_log_is_an_empty_list() {
        assert!(read_ssh_audit(&temp_log("missing"), SSH_AUDIT_LIMIT).is_empty());
    }

    #[test]
    fn it_reads_what_the_tools_write_newest_first_across_the_rotated_files_and_up_to_the_limit() {
        let path = temp_log("rotated");
        // Written by the same type the tools use, so the format can't drift unnoticed. A tiny size makes it rotate.
        let log = AuditLog::with_rotation(&path, 150, 3);
        for n in 1..=8u64 {
            log.record(&line(n * 1000, "prod", json!({ "command": format!("echo {n}"), "agent": "ops" }))).unwrap();
        }
        let all = read_ssh_audit(&path, SSH_AUDIT_LIMIT);
        assert_eq!(all.iter().map(|e| e.at_ms).collect::<Vec<_>>(), [8000, 7000, 6000, 5000, 4000, 3000, 2000, 1000][..all.len()]);
        assert!(all.len() > 2, "the log was rotated and the older files are read too: {}", all.len());
        assert_eq!(all[0].command.as_deref(), Some("echo 8"));
        assert_eq!(all[0].agent.as_deref(), Some("ops"));
        assert_eq!(read_ssh_audit(&path, 2).iter().map(|e| e.at_ms).collect::<Vec<_>>(), [8000, 7000]);
    }

    #[test]
    fn a_line_that_is_not_json_is_skipped() {
        let path = temp_log("corrupt");
        let good = serde_json::to_string(&line(1000, "prod", json!({ "command": "ls" }))).unwrap();
        std::fs::write(&path, format!("{good}\nthis is not json\n{{\"ts\": 5, \"host_id\":\n")).unwrap();
        let entries = read_ssh_audit(&path, 10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].host_id, "prod");
    }

    #[test]
    fn an_exec_a_copy_a_timeout_and_a_refusal_read_as_a_person_would() {
        let exec = parse_line(&line(1_760_277_791_000, "prod", json!({ "command": "uptime", "agent": "ops" })).to_string()).unwrap();
        assert_eq!(exec.detail(), "uptime");
        assert_eq!(exec.outcome(), "exit 0");
        assert_eq!(exec.summary_line(), "2025-10-12 14:03:11  prod [ops]  exec  not_required  uptime  → exit 0");

        let up = parse_line(
            &line(1000, "prod", json!({ "action": "upload", "local_path": "/tmp/a", "remote_path": "/srv/a", "overwrite": true, "outcome": { "bytes": 2048 } })).to_string(),
        )
        .unwrap();
        assert_eq!((up.detail().as_str(), up.outcome().as_str()), ("/tmp/a → /srv/a (overwrite)", "2048 bytes"));
        let down = parse_line(&line(1000, "prod", json!({ "action": "download", "local_path": "/tmp/a", "remote_path": "/srv/a", "outcome": { "bytes": 1 } })).to_string()).unwrap();
        assert_eq!(down.detail(), "/srv/a → /tmp/a", "a download goes from the server");

        let slow = parse_line(&line(1000, "prod", json!({ "command": "sleep 99", "outcome": { "exit_code": null, "timed_out": true } })).to_string()).unwrap();
        assert_eq!(slow.outcome(), "timed out");
        let refused = parse_line(&line(1000, "prod", json!({ "command": "reboot", "approval": "denied", "outcome": { "error": "refused: not approved" } })).to_string()).unwrap();
        assert_eq!((refused.approval.as_str(), refused.outcome().as_str()), ("denied", "error: refused: not approved"));
    }

    #[test]
    fn the_time_is_utc_and_right_across_a_leap_day() {
        assert_eq!(format_time(0), "1970-01-01 00:00:00");
        assert_eq!(format_time(951_782_400_000), "2000-02-29 00:00:00");
        assert_eq!(format_time(1_760_277_791_000), "2025-10-12 14:03:11");
    }
}

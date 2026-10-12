//! The log of the SSH tools' calls (P47) for the screens: `ListSshAudit` reads what `warden_core::tool::ssh::AuditLog` wrote
//! (`warden_bootstrap::ssh_audit`). Read only and the owner's: the log holds the commands agents ran, which may hold a secret, so a
//! member gets an empty list, as with the feed of activity.

use std::path::Path;

use warden_bootstrap::ssh_audit::{read_ssh_audit, SshAuditEntry, SSH_AUDIT_LIMIT};
use warden_server_protocol::protocol::SshAuditEntryDto;
use warden_server_protocol::ServerMessage;

/// An entry as the screens get it. Also what the desktop shows for this computer.
pub fn to_dto(entry: SshAuditEntry) -> SshAuditEntryDto {
    SshAuditEntryDto {
        at_ms: entry.at_ms,
        host_id: entry.host_id,
        agent: entry.agent,
        action: entry.action,
        approval: entry.approval,
        command: entry.command,
        local_path: entry.local_path,
        remote_path: entry.remote_path,
        overwrite: entry.overwrite,
        exit_code: entry.exit_code,
        timed_out: entry.timed_out,
        bytes: entry.bytes,
        error: entry.error,
    }
}

/// Answers `ListSshAudit`. `path` is `None` on a hub that keeps no such log.
pub fn handle_list_ssh_audit(path: Option<&Path>, is_owner: bool, request_id: u64) -> ServerMessage {
    let entries = match path {
        Some(path) if is_owner => read_ssh_audit(path, SSH_AUDIT_LIMIT).into_iter().map(to_dto).collect(),
        _ => Vec::new(),
    };
    ServerMessage::SshAuditList { request_id, entries }
}

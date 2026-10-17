//! `GET /status`: the hub's own load, for the orchestrator that decides whether to recycle it (P138, `HOSPEDAGEM.md`
//! K.6 item 5). Unlike the public `/health`, it needs the pairing key as a bearer token: the numbers say when the
//! hub is quiet, which is nobody else's business.

use tokio::io::{self, AsyncWrite};
use warden_server_protocol::HubStatus;

use crate::activity::HubActivity;
use crate::attempt_limit::Reservation;
use crate::pairing_key::PairingKey;
use crate::settings::{keys_match, WRONG_KEY_DELAY};
use crate::web_ui::{write_response, RequestHead};

/// `true`: the key was wrong or missing, a guess the caller counts (P142).
pub(crate) async fn serve<S: AsyncWrite + Unpin>(stream: &mut S, head: &RequestHead, key: &PairingKey, activity: &HubActivity, attempt: &Reservation) -> io::Result<bool> {
    let head_only = match head.method.as_str() {
        "GET" => false,
        "HEAD" => true,
        _ => {
            write_response(stream, "405 Method Not Allowed", &[("Allow", "GET, HEAD")], "text/plain; charset=utf-8", b"method not allowed", false).await?;
            return Ok(false);
        }
    };
    let presented = head.header("authorization").and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))).map(str::trim);
    if !presented.is_some_and(|presented| keys_match(presented, &key.current())) {
        // A missing or wrong key waits first, like every other place a key is checked, so guessing is slow.
        tokio::time::sleep(WRONG_KEY_DELAY).await;
        write_response(stream, "401 Unauthorized", &[("WWW-Authenticate", "Bearer")], "text/plain; charset=utf-8", b"unauthorized", head_only).await?;
        return Ok(true);
    }
    attempt.release();
    let snapshot = activity.snapshot();
    let status = HubStatus {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: snapshot.uptime_secs,
        rss_bytes: rss_bytes(),
        connections: snapshot.connections,
        in_flight: snapshot.in_flight,
        idle_secs: snapshot.idle_secs,
    };
    let body = serde_json::to_vec(&status).unwrap_or_default();
    write_response(stream, "200 OK", &[("Cache-Control", "no-store")], "application/json", &body, head_only).await?;
    Ok(false)
}

/// Resident memory of this process, or `None` where it can't be read (anywhere but Linux).
pub fn rss_bytes() -> Option<u64> {
    parse_vm_rss(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// The `VmRSS:   12345 kB` line of `/proc/<pid>/status`, in bytes.
fn parse_vm_rss(status: &str) -> Option<u64> {
    let line = status.lines().find_map(|line| line.strip_prefix("VmRSS:"))?;
    let kib: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;
    kib.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::parse_vm_rss;

    #[test]
    fn vm_rss_is_read_in_bytes_from_the_kib_line() {
        let status = "Name:\twarden-server\nVmPeak:\t  200000 kB\nVmRSS:\t   24832 kB\nThreads:\t4\n";
        assert_eq!(parse_vm_rss(status), Some(24832 * 1024));
    }

    #[test]
    fn a_status_without_the_line_or_with_a_strange_one_gives_none() {
        assert_eq!(parse_vm_rss("Name:\tx\n"), None);
        assert_eq!(parse_vm_rss("VmRSS:\tlots\n"), None);
        assert_eq!(parse_vm_rss("VmRSS:\t12 MB\n"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn this_process_has_a_resident_size_on_linux() {
        assert!(super::rss_bytes().is_some_and(|bytes| bytes > 0));
    }
}

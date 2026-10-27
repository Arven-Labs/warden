//! A fetch of something at an address a stranger chose (P160: a curator's index, and the package an entry points to), made so that
//! the address cannot be used to reach what is only reachable from this machine: another device on the network, a cloud's
//! metadata service, the hub itself. Nothing here is trusted: the bytes are checked by hash and signature by the caller, so this
//! is only about *where* the request may go and how much may come back.
//!
//! - only `https://` is followed (and `http://` when `allow_private` is on, see below);
//! - the name is resolved by our own resolver, which drops every address that is not public (loopback, private, link-local,
//!   shared-address, documentation, multicast, reserved, and the IPv4 ones written inside an IPv6 address); a name with none left
//!   is refused. An address written as an IP number does not go through a resolver, so it is checked on the URL itself, on the
//!   first request and on every redirect (at most three);
//! - the body is cut at `max_bytes`, by `Content-Length` and again while reading, and the whole thing gives up after 30 s.
//!
//! `allow_private` turns the address checks off (and lets `http://` through), for a curator on the owner's own network and for tests
//! against a server on this machine. It is the hub's `--allow-private-curators`, off unless the owner says so.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::Url;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 3;

/// Whether `ip` is an address on the public internet: not one that only means something on this machine or this network.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local() // 169.254/16, which is where cloud metadata services live
        || ip.is_broadcast()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b)) // shared address space (CGNAT)
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2) // documentation
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || (a == 198 && b == 51 && c == 100) // documentation
        || (a == 203 && b == 0 && c == 113) // documentation
        || a >= 240) // reserved
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    // An IPv4 address inside an IPv6 one (`::ffff:10.0.0.1`, or the NAT64 prefix `64:ff9b::/96`) is judged as the IPv4 it holds.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let segments = ip.segments();
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        let [hi, lo] = [segments[6], segments[7]];
        return is_public_v4(Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00 // unique local, fc00::/7
        || (segments[0] & 0xffc0) == 0xfe80 // link-local, fe80::/10
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)) // documentation
}

/// What is wrong with going to `url`, or `None` when it is fine. The part that does not need a lookup: the scheme, and an address
/// written as an IP number.
fn refusal(url: &Url, allow_private: bool) -> Option<String> {
    match url.scheme() {
        "https" => {}
        "http" if allow_private => {}
        other => return Some(format!("only https:// addresses are followed, not {other}://")),
    }
    if allow_private {
        return None;
    }
    let literal = match url.host() {
        Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        Some(url::Host::Domain(_)) => None,
        None => return Some("this address has no host".into()),
    };
    literal.filter(|ip| !is_public_ip(*ip)).map(|ip| format!("{ip} is not a public address, so it is not fetched"))
}

/// Resolves a name and keeps only its public addresses.
struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let found: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let public: Vec<SocketAddr> = found.into_iter().filter(|addr| is_public_ip(addr.ip())).collect();
            if public.is_empty() {
                return Err(format!("{host} does not point to a public address, so it is not fetched").into());
            }
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

/// Fetches addresses a stranger chose, see the module docs.
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpsFetcher {
    allow_private: bool,
}

impl HttpsFetcher {
    pub fn new(allow_private: bool) -> Self {
        Self { allow_private }
    }

    /// The bytes at `url` when they fit in `max_bytes`.
    pub async fn fetch_capped(&self, url: &str, max_bytes: usize) -> anyhow::Result<Vec<u8>> {
        let url = Url::parse(url.trim()).map_err(|_| anyhow::anyhow!("'{}' is not an address", url.trim()))?;
        if let Some(why) = refusal(&url, self.allow_private) {
            anyhow::bail!(why);
        }
        let allow_private = self.allow_private;
        let redirects = reqwest::redirect::Policy::custom(move |attempt| {
            if let Some(why) = refusal(attempt.url(), allow_private) {
                attempt.error(format!("the address sent us on to somewhere that is not followed: {why}"))
            } else if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        });
        let mut builder = reqwest::Client::builder().redirect(redirects).timeout(TIMEOUT);
        if !self.allow_private {
            builder = builder.dns_resolver(Arc::new(PublicOnly));
        }
        let client = builder.build()?;

        let resp = client.get(url).send().await.map_err(describe)?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            anyhow::bail!("nothing at that address (404)");
        }
        let mut resp = resp.error_for_status()?;
        let too_big = || anyhow::anyhow!("what is at that address is bigger than the {} KiB allowed", max_bytes / 1024);
        if resp.content_length().is_some_and(|len| len > max_bytes as u64) {
            return Err(too_big());
        }
        let mut data = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(describe)? {
            if data.len() + chunk.len() > max_bytes {
                return Err(too_big());
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }
}

/// A failed request in words, with the cause our resolver or redirect policy gave (reqwest keeps it in the error's source chain).
fn describe(err: reqwest::Error) -> anyhow::Error {
    let mut message = err.to_string();
    let mut source = std::error::Error::source(&err);
    while let Some(cause) = source {
        message = format!("{message}: {cause}");
        source = cause.source();
    }
    anyhow::anyhow!(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn only_public_addresses_are_public() {
        for public in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "100.63.255.255", "100.128.0.0", "2606:4700:4700::1111", "2a00:1450:4009::200e"] {
            assert!(is_public_ip(ip(public)), "{public}");
        }
        for private in [
            "0.0.0.0",
            "127.0.0.1",
            "127.255.255.254",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "100.64.0.1",      // CGNAT
            "100.127.255.255",
            "192.0.0.1",
            "192.0.2.7",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.9",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:10.0.0.1",   // an IPv4 inside an IPv6
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::a00:1",    // NAT64 of 10.0.0.1
        ] {
            assert!(!is_public_ip(ip(private)), "{private}");
        }
        assert!(is_public_ip(ip("::ffff:8.8.8.8")) && is_public_ip(ip("64:ff9b::808:808")));
    }

    #[test]
    fn the_address_itself_is_checked_before_any_lookup() {
        let refused = |url: &str, allow| refusal(&Url::parse(url).unwrap(), allow);
        assert!(refused("https://example.org/a", false).is_none());
        assert!(refused("https://8.8.8.8/a", false).is_none());
        for bad in ["https://127.0.0.1/a", "https://10.1.2.3:8443/a", "https://[::1]/a", "https://169.254.169.254/latest/meta-data", "https://[::ffff:10.0.0.1]/a", "https://0.0.0.0/a", "https://2130706433/a"] {
            assert!(refused(bad, false).is_some(), "{bad}");
        }
        for bad in ["http://example.org/a", "ftp://example.org/a", "file:///etc/passwd", "ar://x"] {
            assert!(refused(bad, false).is_some(), "{bad}");
        }
        // With `allow_private`, plain http and private addresses go through, but the other schemes still do not.
        assert!(refused("http://127.0.0.1:8080/a", true).is_none());
        assert!(refused("https://10.0.0.1/a", true).is_none());
        assert!(refused("ftp://example.org/a", true).is_some());
    }
}

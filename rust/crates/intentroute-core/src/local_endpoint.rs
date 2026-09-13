//! Upstream proxy endpoint boundary, ported from `LocalProxyEndpoint.cs`:
//! only a listener on a **literal loopback IP** is supported. Hostnames, LAN
//! and public addresses, and IPv4-mapped IPv6 spellings are rejected. The
//! normalized host is the canonical `IpAddr` string form, matching the C#
//! `IPAddress.ToString()` canonicalization.

use std::net::IpAddr;

pub fn try_normalize(host: &str, port: i32) -> Result<String, &'static str> {
    if !(1..=65535).contains(&port) {
        return Err("Proxy port must be between 1 and 65535.");
    }

    let candidate = host.trim();
    let address: IpAddr = candidate
        .parse()
        .map_err(|_| "Proxy host must be a literal loopback IP address such as 127.0.0.1 or ::1.")?;

    match address {
        IpAddr::V4(v4) if v4.octets()[0] == 127 => Ok(v4.to_string()),
        IpAddr::V6(v6) if is_ipv4_mapped(&v6) => {
            Err("Only a proxy listener on a literal loopback IP address is supported.")
        }
        IpAddr::V6(v6) if v6.segments() == [0, 0, 0, 0, 0, 0, 0, 1] => Ok(v6.to_string()),
        _ => Err("Only a proxy listener on a literal loopback IP address is supported."),
    }
}

fn is_ipv4_mapped(v6: &std::net::Ipv6Addr) -> bool {
    let segments = v6.segments();
    segments[0] == 0
        && segments[1] == 0
        && segments[2] == 0
        && segments[3] == 0
        && segments[4] == 0
        && segments[5] == 0xffff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_loopback_literals() {
        assert_eq!(try_normalize("127.0.0.1", 10808).unwrap(), "127.0.0.1");
        assert_eq!(try_normalize(" 127.0.0.1 ", 10808).unwrap(), "127.0.0.1");
        assert_eq!(try_normalize("::1", 10808).unwrap(), "::1");
        // 127.0.0.0/8 is loopback on Windows/.NET semantics.
        assert_eq!(try_normalize("127.5.4.3", 1).unwrap(), "127.5.4.3");
    }

    #[test]
    fn rejects_non_loopback_and_mapped_forms() {
        assert!(try_normalize("localhost", 10808).is_err());
        assert!(try_normalize("192.168.1.2", 10808).is_err());
        assert!(try_normalize("8.8.8.8", 10808).is_err());
        assert!(try_normalize("::ffff:127.0.0.1", 10808).is_err());
        assert!(try_normalize("fd00::1", 10808).is_err());
    }

    #[test]
    fn rejects_bad_ports() {
        assert!(try_normalize("127.0.0.1", 0).is_err());
        assert!(try_normalize("127.0.0.1", 65536).is_err());
    }
}

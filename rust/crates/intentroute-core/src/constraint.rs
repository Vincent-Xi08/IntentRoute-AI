//! Host / IP-CIDR / port list validation, ported from
//! `RuleConstraintValidator.cs` with identical semantics:
//!
//! - separators: `, ; | \n \r \t space`
//! - hosts: exact DNS names or `*.suffix`; empty list passes; ≤ 1000 chars
//! - IPs: IPv4/IPv6 literals or CIDR (all-digit prefix within family range);
//!   ≤ 1000 chars
//! - ports: 1–65535 or ascending `a-b` / `a:b` ranges; ≤ 500 chars
//!
//! One deliberate, documented divergence: the C# version relies on
//! `Uri.CheckHostName` / `IPAddress.TryParse`, which accept a few exotic
//! forms (e.g. IPv4-mapped IPv6 spellings) that `std::net` also accepts for
//! addresses but that host-name validation re-implements explicitly here.

const SEPARATORS: [char; 7] = [',', ';', '|', '\n', '\r', '\t', ' '];
const MAX_HOST_LIST: usize = 1_000;
const MAX_IP_LIST: usize = 1_000;
const MAX_PORT_LIST: usize = 500;

fn split_list(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(SEPARATORS)
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// Valid DNS-label check shared by exact hosts and `*.suffix` tails.
fn is_dns_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.contains("..") {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    labels.iter().all(|label| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

pub fn is_valid_host_list(raw: &str) -> bool {
    if raw.is_empty() {
        return true;
    }
    if raw.len() > MAX_HOST_LIST {
        return false;
    }
    split_list(raw).all(|token| {
        let host = token.strip_prefix("*.").unwrap_or(token);
        is_dns_host(host)
    })
}

fn is_valid_cidr_prefix(prefix: &str, ipv6: bool) -> bool {
    if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // No leading-zero canonicalization concern: C# parses with int.TryParse on
    // the digit string, accepting "007" — replicate that by parsing as u32.
    let Ok(value) = prefix.parse::<u32>() else {
        return false;
    };
    let max = if ipv6 { 128 } else { 32 };
    value <= max
}

pub fn is_valid_ip_list(raw: &str) -> bool {
    if raw.is_empty() {
        return true;
    }
    if raw.len() > MAX_IP_LIST {
        return false;
    }
    split_list(raw).all(|token| match token.split_once('/') {
        None => token.parse::<std::net::IpAddr>().is_ok(),
        Some((address, prefix)) => {
            match address.parse::<std::net::IpAddr>() {
                Ok(std::net::IpAddr::V4(_)) => is_valid_cidr_prefix(prefix, false),
                Ok(std::net::IpAddr::V6(_)) => is_valid_cidr_prefix(prefix, true),
                Err(_) => false,
            }
        }
    })
}

fn parse_port(token: &str) -> Option<u32> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value = token.parse::<u32>().ok()?;
    (1..=65535).contains(&value).then_some(value)
}

pub fn is_valid_port_list(raw: &str) -> bool {
    if raw.is_empty() {
        return true;
    }
    if raw.len() > MAX_PORT_LIST {
        return false;
    }
    split_list(raw).all(|token| {
        let separator = if token.contains('-') {
            '-'
        } else if token.contains(':') {
            ':'
        } else {
            return parse_port(token).is_some();
        };
        match token.split_once(separator) {
            Some((start, end)) => match (parse_port(start), parse_port(end)) {
                (Some(a), Some(b)) => a <= b,
                _ => false,
            },
            None => false,
        }
    })
}

/// Human-oriented failure reasons mirroring the editor's three error strings.
pub fn explain(hosts: &str, ips: &str, ports: &str) -> Vec<&'static str> {
    let mut errors = Vec::new();
    if !is_valid_host_list(hosts) {
        errors.push("invalid host list");
    }
    if !is_valid_ip_list(ips) {
        errors.push("invalid IP/CIDR list");
    }
    if !is_valid_port_list(ports) {
        errors.push("invalid port list");
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ports of the C# RuleConstraintValidatorTests vectors that carry the
    // interesting boundary semantics.
    #[test]
    fn hosts_accept_exact_suffix_and_mixed_lists() {
        assert!(is_valid_host_list(""));
        assert!(is_valid_host_list("github.com"));
        assert!(is_valid_host_list("*.github.com"));
        assert!(is_valid_host_list("a.com, b.com;*.c.com|d.com"));
        assert!(is_valid_host_list("xn--fiqs8s.example"));
    }

    #[test]
    fn hosts_reject_bad_shapes() {
        assert!(!is_valid_host_list("localhost")); // single label
        assert!(!is_valid_host_list("*..com"));
        assert!(!is_valid_host_list("*.com")); // bare suffix tail is single-label
        assert!(!is_valid_host_list("-bad.com"));
        assert!(!is_valid_host_list("bad-.com"));
        assert!(!is_valid_host_list("a..b.com"));
        assert!(!is_valid_host_list("under_score.example"));
        assert!(!is_valid_host_list(&("a".repeat(64) + ".com")));
    }

    #[test]
    fn hosts_enforce_length_cap() {
        let long = "aaaa.com ".repeat(120);
        assert!(!is_valid_host_list(long.trim_end()));
    }

    #[test]
    fn ips_accept_literals_and_cidrs() {
        assert!(is_valid_ip_list(""));
        assert!(is_valid_ip_list("10.0.0.1"));
        assert!(is_valid_ip_list("::1"));
        assert!(is_valid_ip_list("10.0.0.0/8, fd00::/8"));
        assert!(is_valid_ip_list("192.168.0.0/16; 2001:db8::/32"));
    }

    #[test]
    fn ips_reject_bad_prefixes_and_garbage() {
        assert!(!is_valid_ip_list("10.0.0.0/33"));
        assert!(!is_valid_ip_list("fd00::/129"));
        assert!(!is_valid_ip_list("10.0.0.0/x"));
        assert!(!is_valid_ip_list("10.0.0.0/"));
        assert!(!is_valid_ip_list("999.1.1.1"));
        assert!(!is_valid_ip_list("10.0.0.0/-1"));
    }

    #[test]
    fn ports_accept_single_and_ascending_ranges() {
        assert!(is_valid_port_list(""));
        assert!(is_valid_port_list("443"));
        assert!(is_valid_port_list("443, 80"));
        assert!(is_valid_port_list("1000-2000"));
        assert!(is_valid_port_list("1000:2000"));
        assert!(is_valid_port_list("1-65535"));
        assert!(is_valid_port_list("443-443"));
    }

    #[test]
    fn ports_reject_out_of_range_and_descending() {
        assert!(!is_valid_port_list("0"));
        assert!(!is_valid_port_list("65536"));
        assert!(!is_valid_port_list("2000-1000"));
        assert!(!is_valid_port_list("1000-"));
        assert!(!is_valid_port_list("-1000"));
        assert!(!is_valid_port_list("443-65536"));
    }

    #[test]
    fn explain_reports_each_failed_dimension() {
        assert!(explain("", "", "").is_empty());
        assert_eq!(explain("bad", "1.2.3.4/99", "0"), [
            "invalid host list",
            "invalid IP/CIDR list",
            "invalid port list"
        ]);
    }
}

//! Shadowing and broad-scope detection for the policy check panel
//! (parity slice 9). This ports the highest-value findings from the WPF
//! `PolicyIntelligence` engine:
//!
//! - **Shadowing**: an enabled rule can never match because an earlier rule
//!   in canonical order covers everything it covers (process is a superset,
//!   domains/IPs/ports contain, protocol includes).
//! - **Broad scope**: an enabled rule has no destination constraints at all
//!   (no hosts, IPs, or ports), so it covers all traffic for its process.
//!
//! Both are proven exact-superset relations — the WPF engine additionally
//! proves partial overlaps (containment hints), which remain WPF-only.

use crate::rule::ProxyRule;
use crate::runtime_order::canonical_order;
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

/// Analyzes enabled rules in canonical order for shadowing and broad scope.
pub fn analyze(rules: &[ProxyRule]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let enabled: Vec<ProxyRule> = rules.iter().filter(|r| r.is_enabled).cloned().collect();
    let ordered = canonical_order(enabled);

    // Broad scope: no destination constraints at all.
    for (index, rule) in ordered.iter().enumerate() {
        if rule.target_hosts.trim().is_empty()
            && rule.target_ips.trim().is_empty()
            && rule.target_ports.trim().is_empty()
        {
            findings.push(Finding {
                code: "PIR-BROAD",
                severity: Severity::Info,
                detail: format!(
                    "#{} {} covers all traffic for {}",
                    index + 1,
                    rule.exe_name,
                    if rule.exe_name.trim() == "*" { "every process" } else { "its process" }
                ),
            });
        }
    }

    // Shadowing: earlier rule covers everything a later rule covers.
    for later_index in 1..ordered.len() {
        for earlier_index in 0..later_index {
            if shadows(&ordered[earlier_index], &ordered[later_index]) {
                findings.push(Finding {
                    code: "PIR-SHADOW",
                    severity: Severity::Critical,
                    detail: format!(
                        "#{} {} is unreachable: #{} {} covers everything it matches",
                        later_index + 1,
                        ordered[later_index].exe_name,
                        earlier_index + 1,
                        ordered[earlier_index].exe_name
                    ),
                });
                break; // one shadowing report per shadowed rule is enough
            }
        }
    }

    findings
}

/// True when `earlier` covers every input `later` would match.
fn shadows(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    process_covers(earlier, later)
        && (earlier.target_hosts.trim().is_empty() || hosts_cover(earlier, later))
        && (earlier.target_ips.trim().is_empty() || ips_cover(earlier, later))
        && (earlier.target_ports.trim().is_empty() || ports_cover(earlier, later))
        && protocol_covers(earlier, later)
}

fn process_covers(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    let e = earlier.exe_name.trim();
    e == "*" || e.eq_ignore_ascii_case(later.exe_name.trim())
}

fn protocol_covers(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    let e = earlier.protocol.trim().to_uppercase();
    let l = later.protocol.trim().to_uppercase();
    match (e.as_str(), l.as_str()) {
        // Earlier "Both"/empty covers any later protocol.
        ("", _) | ("BOTH", _) | ("TCP/UDP", _) => true,
        // Earlier TCP covers later TCP only (not UDP).
        ("TCP", "TCP") => true,
        ("UDP", "UDP") => true,
        _ => false,
    }
}

fn hosts_cover(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    let earlier_tokens = split_list(&earlier.target_hosts);
    // Unrestricted earlier matches everything — but that case is handled
    // by the caller's `is_empty()` fast path, so here earlier has tokens.
    let later_tokens = split_list(&later.target_hosts);
    // Later unrestricted: earlier must also be unrestricted (already handled).
    // Later has tokens: every later token must be covered by an earlier token.
    later_tokens.is_empty()
        || later_tokens.iter().all(|lt| {
            earlier_tokens.iter().any(|et| host_token_covers(et, lt))
        })
}

fn host_token_covers(earlier: &str, later: &str) -> bool {
    let e = earlier.trim().to_lowercase();
    let l = later.trim().to_lowercase();
    if let Some(suffix) = e.strip_prefix("*.") {
        // *.suffix covers suffix itself and any subdomain of suffix.
        l == suffix || l.ends_with(&format!(".{suffix}"))
    } else {
        // Exact host covers only itself.
        l == e
    }
}

fn ips_cover(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    let earlier_tokens = split_list(&earlier.target_ips);
    let later_tokens = split_list(&later.target_ips);
    later_tokens.is_empty()
        || later_tokens.iter().all(|lt| {
            earlier_tokens.iter().any(|et| ip_token_covers(et, lt))
        })
}

fn ip_token_covers(earlier: &str, later: &str) -> bool {
    let Some((e_addr, e_prefix)) = parse_cidr(earlier) else {
        return earlier.trim() == later.trim(); // bare exact match
    };
    let Some((l_addr, l_prefix)) = parse_cidr(later) else {
        return false; // later isn't a valid IP/CIDR
    };
    // Both are CIDRs (parse_cidr normalizes bare IPs to /32 or /128).
    // Later is covered when same family, earlier's prefix ≤ later's prefix,
    // and they share the same network address at earlier's prefix length.
    match (e_addr, l_addr) {
        (IpAddr::V4(en), IpAddr::V4(ln)) => {
            e_prefix <= l_prefix && {
                let mask = v4_mask(e_prefix);
                (u32::from(en) & mask) == (u32::from(ln) & mask)
            }
        }
        (IpAddr::V6(en), IpAddr::V6(ln)) => {
            e_prefix <= l_prefix && {
                let shift = 128 - e_prefix.min(128);
                (u128::from(en) >> shift) == (u128::from(ln) >> shift)
            }
        }
        _ => false,
    }
}

fn ports_cover(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    let earlier_tokens = split_list(&earlier.target_ports);
    let later_tokens = split_list(&later.target_ports);
    later_tokens.is_empty()
        || later_tokens.iter().all(|lt| {
            earlier_tokens.iter().any(|et| port_token_covers(et, lt))
        })
}

fn port_token_covers(earlier: &str, later: &str) -> bool {
    let (es, ee) = parse_port_range(earlier);
    let (ls, le) = parse_port_range(later);
    // Earlier range covers later range when earlier ⊇ later.
    es <= ls && le <= ee
}

fn split_list(raw: &str) -> Vec<String> {
    raw.split([',', ';', '|', '\n', '\r', '\t', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}

fn parse_cidr(token: &str) -> Option<(IpAddr, u32)> {
    match token.split_once('/') {
        Some((addr, prefix)) => {
            let addr = addr.parse::<IpAddr>().ok()?;
            let prefix = prefix.parse::<u32>().ok()?;
            let max = match addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            (prefix <= max).then_some((addr, prefix))
        }
        None => token.parse::<IpAddr>().ok().map(|a| {
            let prefix = match a {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            (a, prefix)
        }),
    }
}

fn v4_mask(prefix: u32) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix.min(32))
    }
}

fn in_v4_cidr(net: IpAddr, prefix: u32, ip: IpAddr) -> bool {
    if let (IpAddr::V4(n), IpAddr::V4(i)) = (net, ip) {
        let mask = v4_mask(prefix);
        (u32::from(n) & mask) == (u32::from(i) & mask)
    } else {
        false
    }
}

fn in_v6_cidr(net: IpAddr, prefix: u32, ip: IpAddr) -> bool {
    if let (IpAddr::V6(n), IpAddr::V6(i)) = (net, ip) {
        if prefix == 0 {
            return true;
        }
        let shift = 128 - prefix.min(128);
        (u128::from(n) >> shift) == (u128::from(i) >> shift)
    } else {
        false
    }
}

fn parse_port_range(token: &str) -> (u16, u16) {
    let sep = if token.contains('-') { '-' } else if token.contains(':') { ':' } else { '\0' };
    if sep == '\0' {
        token.parse::<u16>().map(|p| (p, p)).unwrap_or((0, 0))
    } else {
        token
            .split_once(sep)
            .and_then(|(a, b)| {
                let start = a.parse::<u16>().ok()?;
                let end = b.parse::<u16>().ok()?;
                Some((start, end))
            })
            .unwrap_or((0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::ProxyMode;

    fn rule(exe: &str, priority: i32) -> ProxyRule {
        let mut r = ProxyRule::new("t", exe);
        r.mode = ProxyMode::Direct;
        r.is_enabled = true;
        r.priority = priority;
        r
    }

    #[test]
    fn broad_scope_detects_unconstrained_rule() {
        let findings = analyze(&[rule("app.exe", 10)]);
        assert!(findings.iter().any(|f| f.code == "PIR-BROAD"));
    }

    #[test]
    fn broad_scope_not_fired_for_constrained_rule() {
        let mut r = rule("app.exe", 10);
        r.target_hosts = "github.com".into();
        assert!(!analyze(&[r]).iter().any(|f| f.code == "PIR-BROAD"));
    }

    #[test]
    fn shadow_global_covers_everything() {
        let global = rule("*", 10);
        let specific = rule("app.exe", 20);
        let findings = analyze(&[global, specific]);
        assert!(
            findings.iter().any(|f| f.code == "PIR-SHADOW"
                && f.detail.contains("app.exe")
                && f.detail.contains("unreachable"))
        );
    }

    #[test]
    fn shadow_domain_superset() {
        let mut wide = rule("app.exe", 10);
        wide.target_hosts = "*.github.com".into();
        let mut narrow = rule("app.exe", 20);
        narrow.target_hosts = "api.github.com".into();
        let findings = analyze(&[wide, narrow]);
        assert!(findings.iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn shadow_ip_cidr_superset() {
        let mut wide = rule("app.exe", 10);
        wide.target_ips = "10.0.0.0/8".into();
        let mut narrow = rule("app.exe", 20);
        narrow.target_ips = "10.1.0.0/16".into();
        let findings = analyze(&[wide, narrow]);
        assert!(findings.iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn no_shadow_when_domains_dont_cover() {
        let mut first = rule("app.exe", 10);
        first.target_hosts = "github.com".into();
        let mut second = rule("app.exe", 20);
        second.target_hosts = "gitlab.com".into();
        assert!(!analyze(&[first, second]).iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn no_shadow_when_later_more_specific() {
        let mut first = rule("app.exe", 10);
        first.target_ports = "443".into();
        let mut second = rule("app.exe", 20);
        second.target_ports = "80,443".into(); // later broader than earlier
        assert!(!analyze(&[first, second]).iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn no_shadow_for_disabled_rules() {
        let mut global = rule("*", 10);
        global.is_enabled = false;
        let specific = rule("app.exe", 20);
        assert!(!analyze(&[global, specific]).iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn protocol_both_covers_tcp() {
        let mut both = rule("app.exe", 10);
        both.protocol = "Both".into();
        let mut tcp_only = rule("app.exe", 20);
        tcp_only.protocol = "TCP".into();
        assert!(analyze(&[both, tcp_only]).iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn protocol_tcp_does_not_cover_udp() {
        let mut tcp = rule("app.exe", 10);
        tcp.protocol = "TCP".into();
        let mut udp = rule("app.exe", 20);
        udp.protocol = "UDP".into();
        assert!(!analyze(&[tcp, udp]).iter().any(|f| f.code == "PIR-SHADOW"));
    }
}

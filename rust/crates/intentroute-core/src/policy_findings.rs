//! Shadowing, broad-scope, and partial-overlap detection for the policy
//! check panel (parity slices 9–10). This ports the highest-value findings
//! from the WPF `PolicyIntelligence` engine:
//!
//! - **Shadowing**: an enabled rule can never match because an earlier rule
//!   in canonical order covers everything it covers (process is a superset,
//!   domains/IPs/ports contain, protocol includes).
//! - **Broad scope**: an enabled rule has no destination constraints at all
//!   (no hosts, IPs, or ports), so it covers all traffic for its process.
//! - **Partial overlap**: two enabled rules provably intersect without
//!   either covering the other. Shared traffic hits the earlier rule. This
//!   is a hint, not a shadowing/conflict proof — traffic outside the
//!   intersection still follows each rule's own scope. Domain versus IP
//!   constraints can never be proven to intersect, and domains/IPs are each
//!   only proven via tree containment (one token contains the other).

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

    // Partial overlap: provable intersection without containment in either
    // direction. Every overlapping pair is reported (not just the first).
    for later_index in 1..ordered.len() {
        for earlier_index in 0..later_index {
            let earlier = &ordered[earlier_index];
            let later = &ordered[later_index];
            if shadows(earlier, later) || shadows(later, earlier) {
                continue;
            }
            if !partial_overlap(earlier, later) {
                continue;
            }
            let same_mode = earlier.mode == later.mode;
            findings.push(Finding {
                code: "PIR-OVERLAP",
                severity: if same_mode { Severity::Info } else { Severity::Warning },
                detail: format!(
                    "#{} {} and #{} {} provably intersect but neither covers the other; shared traffic hits #{} first",
                    earlier_index + 1,
                    earlier.exe_name,
                    later_index + 1,
                    later.exe_name,
                    earlier_index + 1
                ),
            });
        }
    }

    findings
}

/// True when `earlier` covers every input `later` would match.
fn shadows(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    process_covers(earlier, later)
        && protocol_covers(earlier, later)
        && ports_cover(earlier, later)
        && destinations_cover(earlier, later)
}

/// True when the two rules provably intersect without either covering the
/// other. An empty destination constraint set on either side means that
/// dimension intersects everything; constrained sides intersect only via
/// tree containment (domain or IP), never domain-versus-IP.
fn partial_overlap(a: &ProxyRule, b: &ProxyRule) -> bool {
    process_may_overlap(a, b)
        && protocol_overlaps(a, b)
        && ports_overlap(a, b)
        && destinations_overlap(a, b)
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

fn protocol_overlaps(a: &ProxyRule, b: &ProxyRule) -> bool {
    let p = |r: &ProxyRule| r.protocol.trim().to_uppercase();
    let (x, y) = (p(a), p(b));
    let unrestricted = |s: &str| s.is_empty() || s == "BOTH" || s == "TCP/UDP";
    unrestricted(&x) || unrestricted(&y) || x == y
}

fn process_may_overlap(a: &ProxyRule, b: &ProxyRule) -> bool {
    let x = a.exe_name.trim();
    let y = b.exe_name.trim();
    x == "*" || y == "*" || x.eq_ignore_ascii_case(y)
}

/// Hosts and IPs form one destination dimension: a rule is destination-
/// filtered when either list is non-empty. An unfiltered earlier covers any
/// later; a filtered earlier cannot cover an unfiltered later. With both
/// filtered, every later host and IP token must be covered, independently
/// per list (an empty list is vacuously covered).
fn destinations_cover(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    let earlier_filtered = has_destination_filter(earlier);
    let later_filtered = has_destination_filter(later);
    if !earlier_filtered {
        return true;
    }
    if !later_filtered {
        return false;
    }
    hosts_cover(earlier, later) && ips_cover(earlier, later)
}

fn destinations_overlap(a: &ProxyRule, b: &ProxyRule) -> bool {
    if !has_destination_filter(a) || !has_destination_filter(b) {
        return true;
    }
    let a_hosts = split_list(&a.target_hosts);
    let b_hosts = split_list(&b.target_hosts);
    let domains_overlap = a_hosts.iter().any(|ah| {
        b_hosts
            .iter()
            .any(|bh| host_token_covers(ah, bh) || host_token_covers(bh, ah))
    });
    let a_ips = split_list(&a.target_ips);
    let b_ips = split_list(&b.target_ips);
    let networks_overlap = a_ips.iter().any(|ai| {
        b_ips
            .iter()
            .any(|bi| ip_token_covers(ai, bi) || ip_token_covers(bi, ai))
    });
    domains_overlap || networks_overlap
}

fn has_destination_filter(rule: &ProxyRule) -> bool {
    !rule.target_hosts.trim().is_empty() || !rule.target_ips.trim().is_empty()
}

fn hosts_cover(earlier: &ProxyRule, later: &ProxyRule) -> bool {
    let earlier_tokens = split_list(&earlier.target_hosts);
    let later_tokens = split_list(&later.target_hosts);
    // Called only when both rules are destination-filtered: an empty later
    // host list is vacuously covered; otherwise every later token needs an
    // earlier token covering it (an empty earlier list covers nothing).
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
}fn ip_token_covers(earlier: &str, later: &str) -> bool {
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
    if earlier_tokens.is_empty() {
        return true; // earlier unrestricted
    }
    let later_tokens = split_list(&later.target_ports);
    if later_tokens.is_empty() {
        return false; // earlier restricted cannot cover an unrestricted later
    }
    later_tokens.iter().all(|lt| {
        earlier_tokens.iter().any(|et| port_token_covers(et, lt))
    })
}

fn ports_overlap(a: &ProxyRule, b: &ProxyRule) -> bool {
    let a_tokens = split_list(&a.target_ports);
    let b_tokens = split_list(&b.target_ports);
    if a_tokens.is_empty() || b_tokens.is_empty() {
        return true;
    }
    a_tokens.iter().any(|at| {
        let (a_start, a_end) = parse_port_range(at);
        b_tokens.iter().any(|bt| {
            let (b_start, b_end) = parse_port_range(bt);
            a_start <= b_end && b_start <= a_end
        })
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

    // ---- partial overlap (parity slice 10) ----

    #[test]
    fn overlap_same_mode_is_info() {
        let mut wide = rule("app.exe", 10);
        wide.target_hosts = "*.github.com".into();
        let mut mixed = rule("app.exe", 20);
        mixed.target_hosts = "api.github.com,gitlab.com".into();
        let findings = analyze(&[wide, mixed]);
        let overlap = findings
            .iter()
            .find(|f| f.code == "PIR-OVERLAP")
            .expect("partial overlap expected");
        assert_eq!(overlap.severity, Severity::Info);
        assert!(!findings.iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn overlap_different_mode_is_warning() {
        let mut wide = rule("app.exe", 10);
        wide.target_hosts = "*.github.com".into();
        let mut mixed = rule("app.exe", 20);
        mixed.target_hosts = "api.github.com,gitlab.com".into();
        mixed.mode = ProxyMode::Proxy;
        let findings = analyze(&[wide, mixed]);
        let overlap = findings
            .iter()
            .find(|f| f.code == "PIR-OVERLAP")
            .expect("partial overlap expected");
        assert_eq!(overlap.severity, Severity::Warning);
    }

    #[test]
    fn no_overlap_for_disjoint_hosts() {
        let mut first = rule("app.exe", 10);
        first.target_hosts = "github.com".into();
        let mut second = rule("app.exe", 20);
        second.target_hosts = "gitlab.com".into();
        assert!(!analyze(&[first, second]).iter().any(|f| f.code == "PIR-OVERLAP"));
    }

    #[test]
    fn no_overlap_between_domain_and_ip_constraints() {
        let mut host_rule = rule("app.exe", 10);
        host_rule.target_hosts = "github.com".into();
        let mut ip_rule = rule("app.exe", 20);
        ip_rule.target_ips = "10.0.0.0/8".into();
        // A domain constraint and an IP constraint can never be proven to
        // intersect, so no overlap finding is emitted.
        assert!(!analyze(&[host_rule, ip_rule]).iter().any(|f| f.code == "PIR-OVERLAP"));
    }

    #[test]
    fn overlap_via_ip_superset_with_extra_token() {
        let mut wide = rule("app.exe", 10);
        wide.target_ips = "10.0.0.0/8".into();
        let mut mixed = rule("app.exe", 20);
        mixed.target_ips = "10.5.0.0/16,1.2.3.4".into();
        let findings = analyze(&[wide, mixed]);
        // The /8 contains the /16 token but not the bare 1.2.3.4 token, so
        // neither side contains the other while the /16 proves intersection.
        assert!(findings.iter().any(|f| f.code == "PIR-OVERLAP"));
        assert!(!findings.iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn overlap_via_port_intersection() {
        let mut first = rule("app.exe", 10);
        first.target_ports = "80-90".into();
        let mut second = rule("app.exe", 20);
        second.target_ports = "85-95".into();
        let findings = analyze(&[first, second]);
        assert!(findings.iter().any(|f| f.code == "PIR-OVERLAP"));
        assert!(!findings.iter().any(|f| f.code == "PIR-SHADOW"));
    }

    #[test]
    fn overlap_not_reported_for_disjoint_ports() {
        let mut first = rule("app.exe", 10);
        first.target_ports = "80".into();
        let mut second = rule("app.exe", 20);
        second.target_ports = "443".into();
        assert!(!analyze(&[first, second]).iter().any(|f| f.code == "PIR-OVERLAP"));
    }

    #[test]
    fn overlap_skipped_when_exact_duplicates() {
        let mut first = rule("app.exe", 10);
        first.target_hosts = "github.com".into();
        let mut second = rule("app.exe", 20);
        second.target_hosts = "github.com".into();
        let findings = analyze(&[first, second]);
        // Exact containment in both directions reports shadowing only.
        assert!(findings.iter().any(|f| f.code == "PIR-SHADOW"));
        assert!(!findings.iter().any(|f| f.code == "PIR-OVERLAP"));
    }

    #[test]
    fn overlap_cross_process_only_via_global() {
        let mut global = rule("*", 10);
        global.target_hosts = "*.github.com".into();
        let mut other = rule("app.exe", 20);
        other.target_hosts = "api.github.com,gitlab.com".into();
        // The global *.github.com matcher covers api.github.com but not
        // gitlab.com, so the per-process rule neither shadows it nor is
        // shadowed: they provably share api.github.com traffic.
        let findings = analyze(&[global, other]);
        assert!(findings.iter().any(|f| f.code == "PIR-OVERLAP"));
        assert!(!findings.iter().any(|f| f.code == "PIR-SHADOW"));
    }

    // ---- containment-direction regressions (fixed with slice 10) ----
    // A restricted earlier rule followed by an unrestricted later rule is
    // reverse containment: the later superset cannot shadow what runs first,
    // and the WPF engine stays silent on such pairs — only the earlier rule
    // still wins the shared traffic, which is the intended order anyway.

    #[test]
    fn no_shadow_when_earlier_port_restricted_and_later_not() {
        let mut earlier = rule("app.exe", 10);
        earlier.target_ports = "443".into();
        let later = rule("app.exe", 20);
        let findings = analyze(&[earlier, later]);
        // Earlier only covers port 443; the unrestricted later also matches
        // port 80, so the later rule is NOT shadowed (slice-9 false positive).
        assert!(!findings.iter().any(|f| f.code == "PIR-SHADOW"));
        assert!(!findings.iter().any(|f| f.code == "PIR-OVERLAP"));
    }

    #[test]
    fn no_shadow_when_earlier_host_restricted_and_later_not() {
        let mut earlier = rule("app.exe", 10);
        earlier.target_hosts = "github.com".into();
        let later = rule("app.exe", 20);
        let findings = analyze(&[earlier, later]);
        assert!(!findings.iter().any(|f| f.code == "PIR-SHADOW"));
        assert!(!findings.iter().any(|f| f.code == "PIR-OVERLAP"));
    }

    #[test]
    fn shadow_still_holds_when_earlier_unrestricted_ports() {
        let mut earlier = rule("app.exe", 10);
        earlier.target_hosts = "*.github.com".into();
        let mut later = rule("app.exe", 20);
        later.target_hosts = "api.github.com".into();
        later.target_ports = "443".into();
        // Earlier covers all destinations including api.github.com on every
        // port, so the port-restricted later rule remains fully shadowed.
        let findings = analyze(&[earlier, later]);
        assert!(findings.iter().any(|f| f.code == "PIR-SHADOW"));
        assert!(!findings.iter().any(|f| f.code == "PIR-OVERLAP"));
    }
}

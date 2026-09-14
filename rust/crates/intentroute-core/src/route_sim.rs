//! Route Decision Simulator, ported from the WPF Route Decision Simulator
//! (parity slice 6). A strict local what-if evaluation: takes one exact
//! process name, one concrete domain or literal IP, one port, and TCP/UDP;
//! walks **enabled** rules in Canonical Runtime Order and returns the first
//! proven match. Returns None (global fallback) when no rule matches —
//! it never resolves DNS, probes anything, or observes traffic.

use crate::config::AppConfig;
use crate::rule::{ProxyMode, ProxyRule};
use crate::runtime_order::canonical_order;
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteDecision {
    /// A rule matched; carries the rule's process name and action.
    Matched { exe_name: String, mode: ProxyMode },
    /// No enabled rule matched; the global mode decides.
    NoMatch,
    /// The query itself is invalid (empty process, bad port, etc.).
    InvalidQuery(String),
}

#[derive(Debug, Clone)]
pub struct RouteQuery {
    pub process: String,
    pub destination: String,
    pub is_ip: bool,
    pub port: u16,
    pub is_udp: bool,
}

/// Evaluates the query against the enabled rules in canonical order.
pub fn simulate(query: &RouteQuery, config: &AppConfig) -> RouteDecision {
    let process = query.process.trim();
    if process.is_empty() {
        return RouteDecision::InvalidQuery("process name is required".into());
    }
    if query.destination.trim().is_empty() {
        return RouteDecision::InvalidQuery("destination is required".into());
    }
    if query.port == 0 {
        return RouteDecision::InvalidQuery("port must be 1–65535".into());
    }

    let ordered = canonical_order(
        config
            .rules
            .iter()
            .filter(|r| r.is_enabled)
            .cloned()
            .collect(),
    );

    for rule in &ordered {
        if rule_matches(rule, query) {
            return RouteDecision::Matched {
                exe_name: rule.exe_name.clone(),
                mode: rule.mode,
            };
        }
    }

    RouteDecision::NoMatch
}

fn rule_matches(rule: &ProxyRule, query: &RouteQuery) -> bool {
    // Process name: exact or * (global).
    let exe = rule.exe_name.trim();
    if exe != "*" && !exe.eq_ignore_ascii_case(query.process.trim()) {
        return false;
    }

    // Protocol: rule's network must include the query's transport.
    let protocol = rule.protocol.trim().to_ascii_uppercase();
    let transport = if query.is_udp { "udp" } else { "tcp" };
    match protocol.as_str() {
        "" | "BOTH" | "TCP/UDP" => {}
        "TCP" => if transport != "tcp" { return false },
        "UDP" => if transport != "udp" { return false },
        _ => return false, // unsupported protocol can't match anything
    }

    // Domain or IP constraint: if the rule has one and the query is the
    // wrong kind, it can't match. Empty = unrestricted.
    let has_host = !rule.target_hosts.trim().is_empty();
    let has_ip = !rule.target_ips.trim().is_empty();

    if query.is_ip {
        if has_host && !has_ip {
            return false; // rule only constrains domains, query is IP
        }
        if has_ip && !ip_matches(&rule.target_ips, &query.destination) {
            return false;
        }
    } else {
        if has_ip && !has_host {
            return false; // rule only constrains IPs, query is a domain
        }
        if has_host && !host_matches(&rule.target_hosts, &query.destination) {
            return false;
        }
    }

    // Port constraint.
    if !rule.target_ports.trim().is_empty() && !port_matches(&rule.target_ports, query.port) {
        return false;
    }

    true
}

fn host_matches(raw: &str, destination: &str) -> bool {
    let dest = destination.trim().to_lowercase();
    raw.split([',', ';', '|', '\n', '\r', '\t', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .any(|token| {
            if let Some(suffix) = token.strip_prefix("*.") {
                dest.ends_with(&format!(".{suffix}").to_lowercase()) || dest == suffix.to_lowercase()
            } else {
                dest == token.to_lowercase()
            }
        })
}

fn ip_matches(raw: &str, destination: &str) -> bool {
    let Ok(dest_ip) = destination.trim().parse::<IpAddr>() else {
        return false;
    };
    raw.split([',', ';', '|', '\n', '\r', '\t', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .any(|token| {
            match token.split_once('/') {
                Some((addr_str, prefix_str)) => {
                    let Ok(addr) = addr_str.parse::<IpAddr>() else { return false };
                    let Ok(prefix) = prefix_str.parse::<u32>() else { return false };
                    match (addr, dest_ip) {
                        (IpAddr::V4(net), IpAddr::V4(ip)) => {
                            let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix.min(32)) };
                            (u32::from(net) & mask) == (u32::from(ip) & mask)
                        }
                        (IpAddr::V6(net), IpAddr::V6(ip)) => {
                            let bits = u128::from(net);
                            let target = u128::from(ip);
                            if prefix == 0 { return true; }
                            let shift = 128 - prefix.min(128);
                            (bits >> shift) == (target >> shift)
                        }
                        _ => false, // family mismatch
                    }
                }
                None => {
                    // Bare IP: exact match (the builder normalizes to /32 or /128,
                    // but stored data may have the bare form).
                    token.parse::<IpAddr>().is_ok_and(|a| a == dest_ip)
                }
            }
        })
}

fn port_matches(raw: &str, port: u16) -> bool {
    raw.split([',', ';', '|', '\n', '\r', '\t', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .any(|token| {
            let sep = if token.contains('-') { '-' } else if token.contains(':') { ':' } else { return token.parse::<u16>().is_ok_and(|p| p == port) };
            match token.split_once(sep) {
                Some((a, b)) => {
                    let (Ok(start), Ok(end)) = (a.parse::<u16>(), b.parse::<u16>()) else { return false };
                    port >= start && port <= end
                }
                None => false,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalMode, ProxyServer, ProxyType};

    fn query(process: &str, dest: &str, is_ip: bool, port: u16, udp: bool) -> RouteQuery {
        RouteQuery {
            process: process.into(),
            destination: dest.into(),
            is_ip,
            port,
            is_udp: udp,
        }
    }

    fn rule(exe: &str, mode: ProxyMode) -> ProxyRule {
        let mut r = ProxyRule::new("t", exe);
        r.mode = mode;
        r.is_enabled = true;
        r.priority = 10;
        r
    }

    fn config(rules: Vec<ProxyRule>) -> AppConfig {
        AppConfig {
            global_mode: GlobalMode::DirectAll,
            rules,
            proxy_servers: vec![ProxyServer {
                id: "s1".into(),
                name: String::new(),
                proxy_type: ProxyType::Socks5,
                host: "127.0.0.1".into(),
                port: 10808,
                username: String::new(),
                password: String::new(),
                enabled: true,
            }],
            proxy_chains: vec![],
        }
    }

    #[test]
    fn process_match_returns_rule_action() {
        let cfg = config(vec![rule("chrome.exe", ProxyMode::Block)]);
        let result = simulate(&query("chrome.exe", "example.com", false, 443, false), &cfg);
        assert_eq!(
            result,
            RouteDecision::Matched { exe_name: "chrome.exe".into(), mode: ProxyMode::Block }
        );
    }

    #[test]
    fn process_mismatch_falls_through() {
        let cfg = config(vec![rule("chrome.exe", ProxyMode::Block)]);
        let result = simulate(&query("curl.exe", "example.com", false, 443, false), &cfg);
        assert_eq!(result, RouteDecision::NoMatch);
    }

    #[test]
    fn global_rule_matches_any_process() {
        let cfg = config(vec![rule("*", ProxyMode::Direct)]);
        let result = simulate(&query("anything.exe", "anywhere.com", false, 80, true), &cfg);
        assert_eq!(
            result,
            RouteDecision::Matched { exe_name: "*".into(), mode: ProxyMode::Direct }
        );
    }

    #[test]
    fn host_constraint_filters_by_domain() {
        let mut r = rule("chrome.exe", ProxyMode::Proxy);
        r.target_hosts = "github.com, *.github.com".into();
        let cfg = config(vec![r]);

        assert_eq!(
            simulate(&query("chrome.exe", "github.com", false, 443, false), &cfg),
            RouteDecision::Matched { exe_name: "chrome.exe".into(), mode: ProxyMode::Proxy }
        );
        assert_eq!(
            simulate(&query("chrome.exe", "api.github.com", false, 443, false), &cfg),
            RouteDecision::Matched { exe_name: "chrome.exe".into(), mode: ProxyMode::Proxy }
        );
        assert_eq!(
            simulate(&query("chrome.exe", "gitlab.com", false, 443, false), &cfg),
            RouteDecision::NoMatch
        );
    }

    #[test]
    fn ip_cidr_constraint_filters_by_range() {
        let mut r = rule("app.exe", ProxyMode::Direct);
        r.target_ips = "10.0.0.0/8".into();
        let cfg = config(vec![r]);

        assert!(matches!(
            simulate(&query("app.exe", "10.1.2.3", true, 443, false), &cfg),
            RouteDecision::Matched { .. }
        ));
        assert_eq!(
            simulate(&query("app.exe", "192.168.1.1", true, 443, false), &cfg),
            RouteDecision::NoMatch
        );
    }

    #[test]
    fn port_constraint_filters() {
        let mut r = rule("game.exe", ProxyMode::Block);
        r.target_ports = "7000-8000".into();
        let cfg = config(vec![r]);

        assert!(matches!(
            simulate(&query("game.exe", "x.com", false, 7500, true), &cfg),
            RouteDecision::Matched { .. }
        ));
        assert_eq!(
            simulate(&query("game.exe", "x.com", false, 443, true), &cfg),
            RouteDecision::NoMatch
        );
    }

    #[test]
    fn protocol_constraint_filters_transport() {
        let mut r = rule("dns.exe", ProxyMode::Direct);
        r.protocol = "UDP".into();
        let cfg = config(vec![r]);

        assert!(matches!(
            simulate(&query("dns.exe", "8.8.8.8", true, 53, true), &cfg),
            RouteDecision::Matched { .. }
        ));
        assert_eq!(
            simulate(&query("dns.exe", "8.8.8.8", true, 53, false), &cfg),
            RouteDecision::NoMatch
        );
    }

    #[test]
    fn disabled_rules_are_skipped() {
        let mut r = rule("chrome.exe", ProxyMode::Block);
        r.is_enabled = false;
        let cfg = config(vec![r]);
        assert_eq!(
            simulate(&query("chrome.exe", "x.com", false, 80, false), &cfg),
            RouteDecision::NoMatch
        );
    }

    #[test]
    fn canonical_order_determines_first_match() {
        let mut first = rule("specific.exe", ProxyMode::Block);
        first.priority = 5;
        let mut second = rule("*", ProxyMode::Direct);
        second.priority = 10;
        let cfg = config(vec![second.clone(), first.clone()]);

        let result = simulate(&query("specific.exe", "x.com", false, 80, false), &cfg);
        assert_eq!(
            result,
            RouteDecision::Matched { exe_name: "specific.exe".into(), mode: ProxyMode::Block }
        );
    }

    #[test]
    fn invalid_query_rejected() {
        let cfg = config(vec![rule("a.exe", ProxyMode::Direct)]);
        assert!(matches!(
            simulate(&query("", "x.com", false, 80, false), &cfg),
            RouteDecision::InvalidQuery(_)
        ));
        assert!(matches!(
            simulate(&query("a.exe", "", false, 80, false), &cfg),
            RouteDecision::InvalidQuery(_)
        ));
    }
}

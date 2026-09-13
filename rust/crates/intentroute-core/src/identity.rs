//! Full rule identity key, ported from `RuleIdentity.cs`.
//!
//! The key is `exe-name + normalized(hosts) + normalized(ips) + normalized(ports)
//! + protocol + mode` joined with U+001F; list normalization dedupes
//! (case-insensitively), sorts, and joins with `", "`, hosts lowercased. The
//! same key backs AI-draft dedupe and import skip classification.

use crate::rule::{ProxyMode, ProxyRule};

const SEPARATORS: [char; 7] = [',', ';', '|', '\n', '\r', '\t', ' '];
const UNIT_SEPARATOR: char = '\u{1f}';

pub fn normalize_list(raw: &str, lowercase: bool) -> String {
    let mut values: Vec<String> = raw
        .split(SEPARATORS)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| {
            if lowercase {
                token.to_lowercase()
            } else {
                token.to_string()
            }
        })
        .collect();
    values.sort_unstable_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    values.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    values.join(", ")
}

pub fn rule_identity_key(rule: &ProxyRule) -> String {
    [
        rule.exe_name.trim().to_lowercase(),
        normalize_list(&rule.target_hosts, true),
        normalize_list(&rule.target_ips, false),
        normalize_list(&rule.target_ports, false),
        rule.protocol.trim().to_string(),
        mode_member_name(rule.mode).to_string(),
    ]
    .join(&UNIT_SEPARATOR.to_string())
}

fn mode_member_name(mode: ProxyMode) -> &'static str {
    mode.name()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::ProxyRule;

    fn rule(exe: &str, hosts: &str, ips: &str, ports: &str, protocol: &str, mode: ProxyMode) -> ProxyRule {
        let mut r = ProxyRule::new("t", exe);
        r.target_hosts = hosts.to_string();
        r.target_ips = ips.to_string();
        r.target_ports = ports.to_string();
        r.protocol = protocol.to_string();
        r.mode = mode;
        r
    }

    #[test]
    fn constraint_lists_are_normalized_to_same_identity() {
        let left = rule("chrome.exe", "GitHub.com, *.github.com", "10.0.0.1;10.0.0.2", "443 80", "TCP", ProxyMode::Proxy);
        let right = rule("CHROME.EXE", "*.github.com, github.com", "10.0.0.2, 10.0.0.1", "80,443", "TCP", ProxyMode::Proxy);
        assert_eq!(rule_identity_key(&left), rule_identity_key(&right));
    }

    #[test]
    fn different_constraints_or_mode_differ() {
        let base = rule("chrome.exe", "github.com", "", "", "TCP", ProxyMode::Proxy);
        let other_hosts = rule("chrome.exe", "gitlab.com", "", "", "TCP", ProxyMode::Proxy);
        let other_mode = rule("chrome.exe", "github.com", "", "", "TCP", ProxyMode::Direct);
        assert_ne!(rule_identity_key(&base), rule_identity_key(&other_hosts));
        assert_ne!(rule_identity_key(&base), rule_identity_key(&other_mode));
    }

    #[test]
    fn duplicate_tokens_collapse_case_insensitively() {
        assert_eq!(normalize_list("B.com, b.com, a.com", true), "a.com, b.com");
        assert_eq!(normalize_list("10.0.0.2 10.0.0.2", false), "10.0.0.2");
    }
}

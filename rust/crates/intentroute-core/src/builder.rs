//! sing-box 1.13+ configuration builder, ported from
//! `SingBoxConfigBuilder.cs`. Behavior parity:
//!
//! - proxy chains are rejected (no runtime mapping exists);
//! - upstream servers must be literal loopback listeners, tagged
//!   `proxy-<sanitized id>` (non `[A-Za-z0-9_-]` → `_`, empty id →
//!   `server-<n>`); the first enabled server is the default proxy;
//! - rules compile in Canonical Runtime Order (enabled rules only):
//!   exact `process_name` (bare `*` omits the matcher), `domain` /
//!   `domain_suffix` from exact / `*.suffix` hosts (other wildcards are
//!   errors), bare IPs normalized to `/32` / `/128` CIDRs, single ports as
//!   `port` and `a-b`/`a:b` ranges normalized to `a:b` `port_range`,
//!   protocol mapping `""`/Both/TCP-UDP → `[tcp, udp]`, TCP/UDP → single
//!   string; modes map to route-direct / reject / route-proxy;
//! - `final` is the default proxy under ProxyAll (which requires one enabled
//!   server) and `direct` otherwise;
//! - the full JSON keeps passwords for the managed runtime file only; the
//!   redacted JSON replaces every outbound password with `***`; error text is
//!   scrubbed of password assignments.

use crate::config::{AppConfig, ProxyServer, ProxyType};
use crate::local_endpoint::try_normalize;
use crate::rule::{ProxyMode, ProxyRule};
use serde_json::{json, Map, Value};

pub const DIRECT_TAG: &str = "direct";
pub const TUN_INBOUND_TAG: &str = "tun-in";
pub const TUN_IPV4_ADDRESS: &str = "172.19.0.1/30";
pub const TUN_IPV6_ADDRESS: &str = "fdfe:dcba:9876::1/126";

const SEPARATORS: [char; 7] = [',', ';', '|', '\n', '\r', '\t', ' '];

#[derive(Debug, Clone)]
pub struct BuildResult {
    /// Full config JSON including secrets — for writing the managed runtime
    /// file only; never display or log this form.
    pub config_json: String,
    /// Config JSON with every outbound password replaced by `***`.
    pub redacted_json: String,
}

fn split_list(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(SEPARATORS)
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

fn safe_rule_name(rule: &ProxyRule) -> &str {
    if !rule.exe_name.trim().is_empty() {
        &rule.exe_name
    } else if !rule.note.trim().is_empty() {
        &rule.note
    } else {
        &rule.id
    }
}

fn safe_server_name(server: &ProxyServer) -> &str {
    if server.name.trim().is_empty() {
        &server.id
    } else {
        &server.name
    }
}

fn sanitize_error(message: &str) -> String {
    // Mirrors the C# regexes: password=secret / "password": "secret" → ***.
    let mut scrubbed = String::with_capacity(message.len());
    let bytes = message.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &message[i..];
        if let Some(stripped) = strip_password_assignment(rest) {
            scrubbed.push_str(stripped);
            i += stripped.len();
            // C# keeps the JSON quotes around the masked value; a bare token
            // is masked without quotes.
            if message[i..].starts_with('"') {
                scrubbed.push_str("\"***\"");
                i += 1;
                while i < message.len() && message.as_bytes()[i] != b'"' {
                    i += 1;
                }
                i += 1;
            } else {
                scrubbed.push_str("***");
                while i < message.len()
                    && !matches!(bytes[i], b' ' | b',' | b';' | b'\t' | b'\n' | b'\r')
                {
                    i += 1;
                }
            }
        } else {
            let ch = message[i..].chars().next().unwrap();
            scrubbed.push(ch);
            i += ch.len_utf8();
        }
    }
    scrubbed
}

/// Matches a leading `password` / `"password"` key with `=` or `:` (any
/// casing/spelling of spacing), returning the consumed key text on a match.
fn strip_password_assignment(rest: &str) -> Option<&str> {
    let lower = rest.to_ascii_lowercase();
    for key in ["password", "\"password\""] {
        let len = key.len();
        if lower.starts_with(key) {
            let mut consumed = len;
            let tail = &rest[consumed..];
            let spaces = tail.len() - tail.trim_start_matches([' ', '\t']).len();
            consumed += spaces;
            if rest[consumed..].starts_with(['=', ':']) {
                consumed += 1;
                let tail = &rest[consumed..];
                let spaces = tail.len() - tail.trim_start_matches([' ', '\t']).len();
                consumed += spaces;
                return Some(&rest[..consumed]);
            }
        }
    }
    None
}

fn make_outbound_tag(server: &ProxyServer, index: usize) -> String {
    let id = if server.id.trim().is_empty() {
        format!("server-{}", index + 1)
    } else {
        server.id.clone()
    };
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    format!("proxy-{safe}")
}

fn build_proxy_outbound(server: &ProxyServer, tag: &str, normalized_host: &str) -> Value {
    let mut outbound = match server.proxy_type {
        ProxyType::Socks5 => json!({
            "type": "socks",
            "tag": tag,
            "server": normalized_host,
            "server_port": server.port,
            "version": "5"
        }),
        ProxyType::Http => json!({
            "type": "http",
            "tag": tag,
            "server": normalized_host,
            "server_port": server.port
        }),
        ProxyType::Https => json!({
            "type": "http",
            "tag": tag,
            "server": normalized_host,
            "server_port": server.port,
            "tls": { "enabled": true }
        }),
    };

    let map = outbound.as_object_mut().expect("outbound is an object");
    if !server.username.is_empty() {
        map.insert("username".into(), Value::String(server.username.clone()));
    }
    if !server.password.is_empty() {
        map.insert("password".into(), Value::String(server.password.clone()));
    }
    outbound
}

fn ordered_enabled_rules(config: &AppConfig) -> Vec<&ProxyRule> {
    // Canonical Runtime Order over enabled rules: priority, creation
    // timestamp, then persisted source order (stable sort provides the
    // tiebreak over the source-indexed enumeration).
    let mut indexed: Vec<(usize, &ProxyRule)> = config
        .rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.is_enabled)
        .collect();
    indexed.sort_by(|(ia, a), (ib, b)| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| ia.cmp(ib))
    });
    indexed.into_iter().map(|(_, rule)| rule).collect()
}

fn parse_hosts(raw: &str, rule: &ProxyRule) -> Result<(Vec<String>, Vec<String>), String> {
    let mut exact = Vec::new();
    let mut suffixes = Vec::new();
    for token in split_list(raw) {
        if let Some(raw_suffix) = token.strip_prefix("*.") {
            let suffix = raw_suffix.trim().trim_start_matches('.');
            if suffix.is_empty() || suffix.contains(['*', '?']) {
                return Err(format!(
                    "Rule '{}' has unsupported host pattern '{token}'. Use exact hosts or *.suffix.",
                    safe_rule_name(rule)
                ));
            }
            suffixes.push(suffix.to_string());
        } else if token.contains(['*', '?']) {
            return Err(format!(
                "Rule '{}' has unsupported host pattern '{token}'. Use exact hosts or *.suffix.",
                safe_rule_name(rule)
            ));
        } else {
            exact.push(token.trim().trim_end_matches('.').to_string());
        }
    }
    Ok((exact, suffixes))
}

fn parse_ip_cidrs(raw: &str, rule: &ProxyRule) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    for token in split_list(raw) {
        match token.split_once('/') {
            Some((address, prefix)) => {
                let Ok(address) = address.parse::<std::net::IpAddr>() else {
                    return Err(invalid_ip(rule, token));
                };
                if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(invalid_ip(rule, token));
                }
                let Ok(prefix) = prefix.parse::<u32>() else {
                    return Err(invalid_ip(rule, token));
                };
                let max = match address {
                    std::net::IpAddr::V4(_) => 32,
                    std::net::IpAddr::V6(_) => 128,
                };
                if prefix > max {
                    return Err(invalid_ip(rule, token));
                }
                values.push(token.to_string());
            }
            None => {
                let Ok(address) = token.parse::<std::net::IpAddr>() else {
                    return Err(invalid_ip(rule, token));
                };
                let suffix = match address {
                    std::net::IpAddr::V6(_) => "/128",
                    std::net::IpAddr::V4(_) => "/32",
                };
                values.push(format!("{token}{suffix}"));
            }
        }
    }
    Ok(values)
}

fn invalid_ip(rule: &ProxyRule, token: &str) -> String {
    format!("Rule '{}' has invalid IP/CIDR '{token}'.", safe_rule_name(rule))
}

fn parse_port(token: &str) -> Option<u32> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value = token.parse::<u32>().ok()?;
    (1..=65535).contains(&value).then_some(value)
}

fn parse_ports(raw: &str, rule: &ProxyRule) -> Result<(Vec<u32>, Vec<String>), String> {
    let mut ports = Vec::new();
    let mut ranges = Vec::new();
    for token in split_list(raw) {
        let separator = if token.contains('-') {
            '-'
        } else if token.contains(':') {
            ':'
        } else {
            match parse_port(token) {
                Some(port) => {
                    ports.push(port);
                    continue;
                }
                None => {
                    return Err(format!(
                        "Rule '{}' has invalid port '{token}'.",
                        safe_rule_name(rule)
                    ))
                }
            }
        };

        match token.split_once(separator) {
            Some((start, end)) => match (parse_port(start), parse_port(end)) {
                (Some(a), Some(b)) if a <= b => ranges.push(format!("{a}:{b}")),
                _ => {
                    return Err(format!(
                        "Rule '{}' has invalid port range '{token}'.",
                        safe_rule_name(rule)
                    ))
                }
            },
            None => {
                return Err(format!(
                    "Rule '{}' has invalid port range '{token}'.",
                    safe_rule_name(rule)
                ))
            }
        }
    }
    Ok((ports, ranges))
}

fn parse_network(protocol: &str, rule: &ProxyRule) -> Result<Vec<&'static str>, String> {
    let trimmed = protocol.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if trimmed.is_empty()
        || lowered == "both"
        || lowered == "tcp/udp"
    {
        return Ok(vec!["tcp", "udp"]);
    }
    if lowered == "tcp" {
        return Ok(vec!["tcp"]);
    }
    if lowered == "udp" {
        return Ok(vec!["udp"]);
    }
    Err(format!(
        "Rule '{}' has unsupported protocol '{trimmed}'. Use TCP, UDP, or Both.",
        safe_rule_name(rule)
    ))
}

fn has_nontrivial_exe_wildcard(exe_name: &str) -> bool {
    let trimmed = exe_name.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return false;
    }
    trimmed.contains(['*', '?', '[', ']'])
}

fn try_build_route_rule(
    rule: &ProxyRule,
    outbound_tags_by_server_id: &std::collections::HashMap<String, String>,
    default_proxy_tag: Option<&str>,
) -> Result<Value, String> {
    if rule.exe_name.trim().is_empty() {
        return Err("Rule executable name is required; use '*' explicitly for a global rule.".into());
    }
    if !rule.proxy_chain_id.trim().is_empty() {
        return Err(format!(
            "Rule '{}' references proxy chain '{}', which is not supported.",
            safe_rule_name(rule),
            rule.proxy_chain_id
        ));
    }
    if has_nontrivial_exe_wildcard(&rule.exe_name) {
        return Err(format!(
            "Rule '{}' uses unsupported executable wildcard '{}'. Only exact process names are allowed.",
            safe_rule_name(rule),
            rule.exe_name
        ));
    }

    let mut route = Map::new();
    let exe = rule.exe_name.trim();
    if !exe.is_empty() && exe != "*" {
        route.insert("process_name".into(), json!([exe]));
    }

    let (exact_hosts, suffix_hosts) = parse_hosts(&rule.target_hosts, rule)?;
    if !exact_hosts.is_empty() {
        route.insert("domain".into(), json!(exact_hosts));
    }
    if !suffix_hosts.is_empty() {
        route.insert("domain_suffix".into(), json!(suffix_hosts));
    }

    let ip_cidrs = parse_ip_cidrs(&rule.target_ips, rule)?;
    if !ip_cidrs.is_empty() {
        route.insert("ip_cidr".into(), json!(ip_cidrs));
    }

    let (ports, ranges) = parse_ports(&rule.target_ports, rule)?;
    if !ports.is_empty() {
        route.insert("port".into(), json!(ports));
    }
    if !ranges.is_empty() {
        route.insert("port_range".into(), json!(ranges));
    }

    let networks = parse_network(&rule.protocol, rule)?;
    if networks.len() == 1 {
        route.insert("network".into(), Value::String(networks[0].into()));
    } else if networks.len() > 1 {
        route.insert("network".into(), json!(networks));
    }

    match rule.mode {
        ProxyMode::Direct => {
            route.insert("action".into(), json!("route"));
            route.insert("outbound".into(), json!(DIRECT_TAG));
        }
        ProxyMode::Block => {
            route.insert("action".into(), json!("reject"));
        }
        ProxyMode::Proxy => {
            let outbound_tag = if !rule.proxy_id.trim().is_empty() {
                match outbound_tags_by_server_id.get(rule.proxy_id.trim()) {
                    Some(tag) => tag.clone(),
                    None => {
                        return Err(format!(
                            "Rule '{}' references missing or disabled proxy server '{}'.",
                            safe_rule_name(rule),
                            rule.proxy_id
                        ))
                    }
                }
            } else {
                match default_proxy_tag {
                    Some(tag) => tag.to_string(),
                    None => {
                        return Err(format!(
                            "Rule '{}' requires a proxy but no enabled proxy server is available.",
                            safe_rule_name(rule)
                        ))
                    }
                }
            };
            route.insert("action".into(), json!("route"));
            route.insert("outbound".into(), Value::String(outbound_tag));
        }
    }

    Ok(Value::Object(route))
}

fn redact_secrets(root: &Value) -> Value {
    let mut clone = root.clone();
    if let Some(outbounds) = clone
        .get_mut("outbounds")
        .and_then(Value::as_array_mut)
    {
        for outbound in outbounds.iter_mut() {
            if let Some(map) = outbound.as_object_mut() {
                if let Some(password) = map.get_mut("password") {
                    *password = Value::String("***".into());
                }
            }
        }
    }
    clone
}

/// Builds the sing-box configuration. On success returns the full JSON (for
/// the managed runtime file only) and the password-redacted JSON.
pub fn build(config: &AppConfig) -> Result<BuildResult, String> {
    if !config.proxy_chains.is_empty() {
        return Err(
            "Proxy chains are not supported because no sing-box runtime mapping is implemented."
                .into(),
        );
    }

    let enabled_servers: Vec<&ProxyServer> = config
        .proxy_servers
        .iter()
        .filter(|server| server.enabled)
        .collect();

    let mut outbound_tags_by_server_id = std::collections::HashMap::new();
    let mut outbounds = vec![json!({ "type": "direct", "tag": DIRECT_TAG })];
    let mut default_proxy_tag: Option<String> = None;

    for (index, server) in enabled_servers.iter().enumerate() {
        let normalized_host = try_normalize(&server.host, server.port).map_err(|error| {
            format!("Proxy server '{}' is invalid: {error}", safe_server_name(server))
        })?;
        let tag = make_outbound_tag(server, index);
        if !server.id.is_empty() {
            outbound_tags_by_server_id.insert(server.id.clone(), tag.clone());
        }
        outbounds.push(build_proxy_outbound(server, &tag, &normalized_host));
        default_proxy_tag.get_or_insert(tag);
    }

    let final_outbound = match config.global_mode {
        crate::config::GlobalMode::ProxyAll => match &default_proxy_tag {
            Some(tag) => tag.clone(),
            None => {
                return Err(
                    "GlobalMode is ProxyAll but no enabled proxy server is available.".into()
                )
            }
        },
        crate::config::GlobalMode::DirectAll => DIRECT_TAG.to_string(),
    };

    let mut route_rules: Vec<Value> = Vec::new();
    for rule in ordered_enabled_rules(config) {
        route_rules.push(try_build_route_rule(
            rule,
            &outbound_tags_by_server_id,
            default_proxy_tag.as_deref(),
        )?);
    }

    let root = json!({
        "log": { "level": "info", "timestamp": true },
        "inbounds": [{
            "type": "tun",
            "tag": TUN_INBOUND_TAG,
            "address": [TUN_IPV4_ADDRESS, TUN_IPV6_ADDRESS],
            "auto_route": true,
            "strict_route": true,
            "stack": "system"
        }],
        "outbounds": outbounds,
        "route": {
            "auto_detect_interface": true,
            "rules": route_rules,
            "final": final_outbound
        }
    });

    Ok(BuildResult {
        config_json: serde_json::to_string_pretty(&root)
            .map_err(|error| sanitize_error(&format!("Failed to build sing-box config: {error}")))?,
        redacted_json: serde_json::to_string_pretty(&redact_secrets(&root))
            .map_err(|error| sanitize_error(&format!("Failed to build sing-box config: {error}")))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalMode, ProxyServer};

    fn server(id: &str, host: &str, port: i32, enabled: bool, password: &str) -> ProxyServer {
        ProxyServer {
            id: id.into(),
            name: String::new(),
            proxy_type: ProxyType::Socks5,
            host: host.into(),
            port,
            username: String::new(),
            password: password.into(),
            enabled,
        }
    }

    fn rule(id: &str, exe: &str, mode: ProxyMode) -> ProxyRule {
        let mut rule = ProxyRule::new(id, exe);
        rule.mode = mode;
        rule
    }

    fn base_config() -> AppConfig {
        let mut config = AppConfig::empty();
        config.proxy_servers = vec![server("s1", "127.0.0.1", 10808, true, "hunter2")];
        config
    }

    #[test]
    fn builds_minimal_config_with_tun_and_direct_final() {
        let result = build(&base_config()).unwrap();
        let root: Value = serde_json::from_str(&result.config_json).unwrap();
        assert_eq!(root["route"]["final"], "direct");
        assert_eq!(root["inbounds"][0]["type"], "tun");
        assert_eq!(root["inbounds"][0]["address"][0], TUN_IPV4_ADDRESS);
        assert_eq!(root["inbounds"][0]["address"][1], TUN_IPV6_ADDRESS);
        // First outbound is direct, second is the socks proxy with credentials.
        assert_eq!(root["outbounds"][0]["tag"], "direct");
        assert_eq!(root["outbounds"][1]["tag"], "proxy-s1");
        assert_eq!(root["outbounds"][1]["server_port"], 10808);
        assert_eq!(root["outbounds"][1]["password"], "hunter2");
    }

    #[test]
    fn redacted_output_masks_passwords_only() {
        let result = build(&base_config()).unwrap();
        assert!(result.redacted_json.contains("\"password\": \"***\""));
        assert!(!result.redacted_json.contains("hunter2"));
        assert!(result.config_json.contains("hunter2"));
    }

    #[test]
    fn proxy_rule_compiles_constraints_to_route_semantics() {
        let mut config = base_config();
        let mut r = rule("r1", "chrome.exe", ProxyMode::Proxy);
        r.target_hosts = "github.com, *.github.com".into();
        r.target_ips = "10.0.0.1, fd00::/8".into();
        r.target_ports = "443, 1000-2000".into();
        r.protocol = "TCP".into();
        config.rules = vec![r];

        let built = build(&config).unwrap();
        let root: Value = serde_json::from_str(&built.config_json).unwrap();
        let rule_json = &root["route"]["rules"][0];
        assert_eq!(rule_json["process_name"], json!(["chrome.exe"]));
        assert_eq!(rule_json["domain"], json!(["github.com"]));
        assert_eq!(rule_json["domain_suffix"], json!(["github.com"]));
        assert_eq!(rule_json["ip_cidr"], json!(["10.0.0.1/32", "fd00::/8"]));
        assert_eq!(rule_json["port"], json!([443]));
        assert_eq!(rule_json["port_range"], json!(["1000:2000"]));
        assert_eq!(rule_json["network"], "tcp");
        assert_eq!(rule_json["action"], "route");
        assert_eq!(rule_json["outbound"], "proxy-s1");
    }

    #[test]
    fn both_protocol_emits_tcp_udp_array_and_global_rule_omits_process() {
        let mut config = base_config();
        let mut global = rule("g", "*", ProxyMode::Block);
        global.protocol = "Both".into();
        config.rules = vec![global];

        let built = build(&config).unwrap();
        let root: Value = serde_json::from_str(&built.config_json).unwrap();
        let rule_json = &root["route"]["rules"][0];
        assert!(rule_json.get("process_name").is_none());
        assert_eq!(rule_json["network"], json!(["tcp", "udp"]));
        assert_eq!(rule_json["action"], "reject");
    }

    #[test]
    fn proxy_all_requires_enabled_server_and_uses_it_as_final() {
        let mut config = AppConfig::empty();
        config.global_mode = GlobalMode::ProxyAll;
        assert_eq!(
            build(&config).unwrap_err(),
            "GlobalMode is ProxyAll but no enabled proxy server is available."
        );

        config.proxy_servers = vec![
            server("s1", "127.0.0.1", 10808, false, ""),
            server("s2", "::1", 1080, true, ""),
        ];
        let built = build(&config).unwrap();
        let root: Value = serde_json::from_str(&built.config_json).unwrap();
        assert_eq!(root["route"]["final"], "proxy-s2");
        assert_eq!(root["outbounds"][1]["server"], "::1");
    }

    #[test]
    fn rejects_chains_wildcards_and_missing_references() {
        let mut chained = base_config();
        chained.proxy_chains = vec![json!({ "Id": "c1" })];
        assert!(build(&chained)
            .unwrap_err()
            .contains("Proxy chains are not supported"));

        let mut wildcard = base_config();
        wildcard.rules = vec![rule("r", "chrome*.exe", ProxyMode::Direct)];
        assert!(build(&wildcard)
            .unwrap_err()
            .contains("unsupported executable wildcard"));

        let mut missing_ref = base_config();
        let mut r = rule("r", "a.exe", ProxyMode::Proxy);
        r.proxy_id = "gone".into();
        missing_ref.rules = vec![r];
        assert!(build(&missing_ref)
            .unwrap_err()
            .contains("references missing or disabled proxy server"));

        let mut no_servers = AppConfig::empty();
        no_servers.rules = vec![rule("r", "a.exe", ProxyMode::Proxy)];
        assert!(build(&no_servers)
            .unwrap_err()
            .contains("requires a proxy but no enabled proxy server"));
    }

    #[test]
    fn rejects_bad_constraints_with_rule_context() {
        let mut config = base_config();
        let mut r = rule("r", "a.exe", ProxyMode::Direct);
        r.target_hosts = "bad*.host".into();
        config.rules = vec![r.clone()];
        assert!(build(&config).unwrap_err().contains("unsupported host pattern 'bad*.host'"));

        let mut config = base_config();
        let mut r = rule("r", "a.exe", ProxyMode::Direct);
        r.target_ips = "10.0.0.0/33".into();
        config.rules = vec![r];
        assert!(build(&config).unwrap_err().contains("invalid IP/CIDR '10.0.0.0/33'"));

        let mut config = base_config();
        let mut r = rule("r", "a.exe", ProxyMode::Direct);
        r.target_ports = "2000-1000".into();
        config.rules = vec![r];
        assert!(build(&config).unwrap_err().contains("invalid port range '2000-1000'"));

        let mut config = base_config();
        let mut r = rule("r", "a.exe", ProxyMode::Direct);
        r.protocol = "QUIC".into();
        config.rules = vec![r];
        assert!(build(&config).unwrap_err().contains("unsupported protocol 'QUIC'"));
    }

    #[test]
    fn disabled_rules_are_skipped_and_order_follows_canonical_runtime() {
        let mut config = base_config();
        let mut low = rule("low", "low.exe", ProxyMode::Direct);
        low.priority = 10;
        let mut high = rule("high", "high.exe", ProxyMode::Block);
        high.priority = 5;
        let mut disabled = rule("off", "off.exe", ProxyMode::Direct);
        disabled.priority = 1;
        disabled.is_enabled = false;
        config.rules = vec![low, high, disabled];

        let built = build(&config).unwrap();
        let root: Value = serde_json::from_str(&built.config_json).unwrap();
        let rules = root["route"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["process_name"], json!(["high.exe"]));
        assert_eq!(rules[1]["process_name"], json!(["low.exe"]));
    }

    #[test]
    fn server_tag_sanitizes_ids_and_errors_scrub_passwords() {
        let mut config = AppConfig::empty();
        config.proxy_servers = vec![server("my proxy/1", "127.0.0.1", 10808, true, "topsecret")];
        let built = build(&config).unwrap();
        let root: Value = serde_json::from_str(&built.config_json).unwrap();
        assert_eq!(root["outbounds"][1]["tag"], "proxy-my_proxy_1");

        let mut bad_host = AppConfig::empty();
        bad_host.proxy_servers = vec![server("s", "example.com", 80, true, "topsecret")];
        let error = build(&bad_host).unwrap_err();
        assert!(error.contains("literal loopback"));
        assert!(!error.contains("topsecret"));

        assert!(sanitize_error("password=topsecret failed").contains("password=***"));
        assert!(sanitize_error(r#""password": "topsecret""#).contains(r#""password": "***""#));
    }
}

//! Application-configuration model slice needed by the sing-box builder:
//! proxy servers, proxy type, global mode, and the container that pairs them
//! with rules. JSON shape matches the C# `AppConfig` store (numeric enums,
//! PascalCase members, required `Id` on servers).

use crate::rule::ProxyRule;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyType {
    Socks5,
    Http,
    Https,
}

impl ProxyType {
    pub fn as_u8(self) -> u8 {
        match self {
            ProxyType::Socks5 => 0,
            ProxyType::Http => 1,
            ProxyType::Https => 2,
        }
    }
}

impl TryFrom<u8> for ProxyType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ProxyType::Socks5),
            1 => Ok(ProxyType::Http),
            2 => Ok(ProxyType::Https),
            _ => Err(()),
        }
    }
}

impl Serialize for ProxyType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.as_u8())
    }
}

impl<'de> Deserialize<'de> for ProxyType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u8::deserialize(deserializer)?;
        ProxyType::try_from(value)
            .map_err(|_| serde::de::Error::custom("unsupported ProxyType value (expected 0-2)"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalMode {
    ProxyAll,
    DirectAll,
}

impl GlobalMode {
    pub fn as_u8(self) -> u8 {
        match self {
            GlobalMode::ProxyAll => 0,
            GlobalMode::DirectAll => 1,
        }
    }
}

impl TryFrom<u8> for GlobalMode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(GlobalMode::ProxyAll),
            1 => Ok(GlobalMode::DirectAll),
            _ => Err(()),
        }
    }
}

impl Serialize for GlobalMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.as_u8())
    }
}

impl<'de> Deserialize<'de> for GlobalMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u8::deserialize(deserializer)?;
        GlobalMode::try_from(value)
            .map_err(|_| serde::de::Error::custom("unsupported GlobalMode value (expected 0-1)"))
    }
}

fn default_enabled() -> bool {
    true
}

fn default_proxy_type() -> ProxyType {
    ProxyType::Socks5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyServer {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "ProxyType", default = "default_proxy_type")]
    pub proxy_type: ProxyType,
    #[serde(rename = "Host", default)]
    pub host: String,
    #[serde(rename = "Port", default)]
    pub port: i32,
    #[serde(rename = "Username", default)]
    pub username: String,
    #[serde(rename = "Password", default)]
    pub password: String,
    #[serde(rename = "Enabled", default = "default_enabled")]
    pub enabled: bool,
}

fn default_global_mode() -> GlobalMode {
    GlobalMode::DirectAll
}

fn default_rules() -> Vec<ProxyRule> {
    Vec::new()
}

fn default_servers() -> Vec<ProxyServer> {
    Vec::new()
}

/// The builder only needs the rule list, server list, chain count, and global
/// mode; unknown members of the persisted configuration are tolerated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(rename = "GlobalMode", default = "default_global_mode")]
    pub global_mode: GlobalMode,
    #[serde(rename = "Rules", default = "default_rules")]
    pub rules: Vec<ProxyRule>,
    #[serde(rename = "ProxyServers", default = "default_servers")]
    pub proxy_servers: Vec<ProxyServer>,
    #[serde(rename = "ProxyChains", default)]
    pub proxy_chains: Vec<serde_json::Value>,
}

impl AppConfig {
    pub fn empty() -> Self {
        Self {
            global_mode: GlobalMode::DirectAll,
            rules: Vec::new(),
            proxy_servers: Vec::new(),
            proxy_chains: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config_with_defaults() {
        let json = r#"{"Rules": [], "ProxyServers": []}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.global_mode, GlobalMode::DirectAll);
        assert!(config.proxy_chains.is_empty());
    }

    #[test]
    fn parses_server_with_numeric_type() {
        let json = r#"{
            "GlobalMode": 0,
            "Rules": [],
            "ProxyServers": [
                {"Id": "s1", "Host": "127.0.0.1", "Port": 10808, "ProxyType": 1,
                 "Username": "u", "Password": "p", "Enabled": true}
            ]
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.global_mode, GlobalMode::ProxyAll);
        let server = &config.proxy_servers[0];
        assert_eq!(server.proxy_type, ProxyType::Http);
        assert_eq!(server.port, 10808);
    }

    #[test]
    fn server_id_is_required() {
        let json = r#"{"Rules": [], "ProxyServers": [{"Host": "127.0.0.1"}]}"#;
        assert!(serde_json::from_str::<AppConfig>(json).is_err());
    }

    #[test]
    fn unknown_proxy_type_is_rejected() {
        let json = r#"{"Rules": [], "ProxyServers": [{"Id": "s1", "ProxyType": 7}]}"#;
        assert!(serde_json::from_str::<AppConfig>(json).is_err());
    }
}

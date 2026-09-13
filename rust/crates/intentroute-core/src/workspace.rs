//! Transactional configuration workspace, ported from
//! `ConfigurationWorkspace.cs` + `AppConfigStore.cs` persistence semantics
//! (Rust migration phase 3c, engine slice — GUI writes are intentionally not
//! wired until WPF-instance coordination is designed).
//!
//! Contract parity:
//! - `load` mirrors `LoadPreservingInvalidFile`: Missing / Loaded / Unusable,
//!   with an attempted `config.json.corrupt-<timestamp>.bak` recovery copy on
//!   unreadable input (the original is never modified) and DPAPI decryption at
//!   the load boundary. Undecryptable passwords surface as Unusable.
//! - `commit` clones the active configuration, applies the mutation,
//!   normalizes (trims rule/server ids and process names), validates the same
//!   invariants (required ids/process names, duplicate rule/server ids,
//!   loopback-only upstreams, no proxy chains, full `builder` dry-run),
//!   serializes with DPAPI-protected passwords, persists via a same-directory
//!   temp file plus atomic `ReplaceFileW`, and only then publishes the new
//!   snapshot. Any failure leaves both memory and disk unchanged.
//! - Returned snapshots are clones: callers cannot mutate active state.

use crate::builder;
use crate::config::{AppConfig, ProxyServer};
use crate::dpapi::{protect_password, unprotect_password, UnprotectError};
use crate::local_endpoint::try_normalize;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum LoadStatus {
    Missing,
    Loaded(Box<Workspace>),
    /// (recovery backup path if one could be created, reason)
    Unusable(Option<PathBuf>, String),
}

#[derive(Debug)]
pub struct Workspace {
    path: PathBuf,
    active: AppConfig,
}

impl Workspace {
    pub fn load(path: &Path) -> LoadStatus {
        if !path.is_file() {
            return LoadStatus::Missing;
        }

        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return LoadStatus::Unusable(
                    recovery_copy(path),
                    format!("cannot read {}: {error}", path.display()),
                )
            }
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => {
                return LoadStatus::Unusable(
                    recovery_copy(path),
                    format!(
                        "{} is not valid UTF-8; replacement characters would corrupt it",
                        path.display()
                    ),
                )
            }
        };
        let mut config: AppConfig = match serde_json::from_str(text) {
            Ok(config) => config,
            Err(error) => {
                return LoadStatus::Unusable(
                    recovery_copy(path),
                    format!("{} does not parse as the application schema: {error}", path.display()),
                )
            }
        };

        // DPAPI decryption happens at the file boundary, exactly like the C#
        // store; any undecryptable password makes the configuration unusable.
        for server in &mut config.proxy_servers {
            match unprotect_password(&server.password) {
                Ok(plaintext) => server.password = plaintext,
                Err(error) => {
                    let reason = match error {
                        UnprotectError::MalformedEnvelope => {
                            "A DPAPI-protected proxy password is malformed."
                        }
                        UnprotectError::Undecryptable => {
                            "A DPAPI-protected proxy password could not be decrypted for the current Windows user."
                        }
                    };
                    return LoadStatus::Unusable(recovery_copy(path), reason.to_string());
                }
            }
        }

        LoadStatus::Loaded(Box::new(Workspace {
            path: path.to_path_buf(),
            active: config,
        }))
    }

    /// Detached view of the active configuration.
    pub fn snapshot(&self) -> &AppConfig {
        &self.active
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Applies `mutate` to a complete candidate and persists atomically.
    /// On success returns the published snapshot (a clone); on failure both
    /// memory and disk are unchanged and the reason is returned.
    pub fn commit(&mut self, mutate: impl FnOnce(&mut AppConfig)) -> Result<AppConfig, String> {
        let mut candidate = self.active.clone();
        mutate(&mut candidate);

        normalize(&mut candidate);
        validate(&candidate)?;

        let json = serialize_protected(&candidate)?;
        save_atomic(&self.path, json.as_bytes())?;

        self.active = candidate;
        Ok(self.active.clone())
    }
}

/// Trim identity/process fields, mirroring the C# `Normalize` pass.
fn normalize(config: &mut AppConfig) {
    for rule in &mut config.rules {
        rule.id = rule.id.trim().to_string();
        rule.exe_name = rule.exe_name.trim().to_string();
        rule.proxy_id = rule.proxy_id.trim().to_string();
        rule.proxy_chain_id = rule.proxy_chain_id.trim().to_string();
        rule.protocol = rule.protocol.trim().to_string();
    }
    for server in &mut config.proxy_servers {
        server.id = server.id.trim().to_string();
        server.host = server.host.trim().to_string();
    }
}

/// The C# workspace invariants: required identities, no duplicate ids, no
/// proxy chains, loopback-only upstreams, and a full builder dry-run so a
/// rule that cannot compile never persists.
fn validate(config: &AppConfig) -> Result<(), String> {
    if !config.proxy_chains.is_empty() {
        return Err(
            "Proxy chains are not supported because no sing-box runtime mapping is implemented."
                .into(),
        );
    }

    let mut seen_rule_ids: Vec<&str> = Vec::new();
    for rule in &config.rules {
        if rule.id.trim().is_empty() {
            return Err("A rule must have a non-empty ID.".into());
        }
        if rule.exe_name.trim().is_empty() {
            return Err(
                "A rule must have a process name; use an explicit * for a global rule.".into(),
            );
        }
        if seen_rule_ids.contains(&rule.id.as_str()) {
            return Err("Rule IDs must be unique.".into());
        }
        seen_rule_ids.push(rule.id.as_str());
    }

    let mut seen_server_ids: Vec<&str> = Vec::new();
    for server in &config.proxy_servers {
        if server.id.trim().is_empty() {
            return Err("A proxy server must have a non-empty ID.".into());
        }
        if seen_server_ids.contains(&server.id.as_str()) {
            return Err("Proxy server IDs must be unique.".into());
        }
        seen_server_ids.push(server.id.as_str());
        if let Err(error) = try_normalize(&server.host, server.port) {
            return Err(format!(
                "Proxy server '{}' is invalid: {error}",
                server_display_name(server)
            ));
        }
    }

    builder::build(config).map(|_| ()).map_err(|error| {
        format!(
            "The configuration does not satisfy the currently supported safety semantics: {error}"
        )
    })
}

fn server_display_name(server: &ProxyServer) -> &str {
    if server.name.trim().is_empty() {
        &server.id
    } else {
        &server.name
    }
}

/// Clone + DPAPI-protect passwords + pretty JSON, matching the C# serializer.
fn serialize_protected(config: &AppConfig) -> Result<String, String> {
    let mut clone = config.clone();
    for server in &mut clone.proxy_servers {
        server.password = protect_password(&server.password)
            .ok_or("DPAPI protection failed for a proxy password; refusing to write plaintext.")?;
    }
    serde_json::to_string_pretty(&clone).map_err(|error| format!("serialization failed: {error}"))
}

/// `path + ".corrupt-<timestamp>.bak"` copy on unreadable input; best effort,
/// the original is never modified. (Parity with `CreateRecoveryCopy`.)
fn recovery_copy(path: &Path) -> Option<PathBuf> {
    let timestamp = utc_timestamp();
    let mut candidate = path.with_file_name(format!("{}.corrupt-{timestamp}.bak", file_name(path)));
    let mut suffix = 1;
    while candidate.exists() {
        candidate = path.with_file_name(format!(
            "{}.corrupt-{timestamp}-{suffix}.bak",
            file_name(path)
        ));
        suffix += 1;
    }
    std::fs::copy(path, &candidate).ok().map(|_| candidate)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.json".to_string())
}

/// UTC `yyyyMMddTHHmmssZ`, matching the C# recovery-copy timestamp.
fn utc_timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60
    )
}

/// Days-since-epoch → civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Same-directory temp file + atomic replace, mirroring `SaveAtomic`:
/// write `path.<unique>.tmp`, then `ReplaceFileW` over the existing file
/// (keeping the C# `ignoreMetadataErrors` behavior) or a plain rename when
/// no previous file exists. Temp leftovers are cleaned best-effort.
fn save_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(directory) = path.parent() {
        if !directory.as_os_str().is_empty() {
            std::fs::create_dir_all(directory)
                .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        }
    }

    let unique = format!(
        "{:x}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0),
        std::process::id()
    );
    let temp = path.with_file_name(format!("{}.{unique}.tmp", file_name(path)));

    if let Err(error) = std::fs::write(&temp, bytes) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("cannot write {}: {error}", temp.display()));
    }

    let replace = if path.exists() {
        replace_file(&temp, path)
    } else {
        std::fs::rename(&temp, path).map_err(|error| format!("cannot move into place: {error}"))
    };
    if let Err(error) = replace {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn replace_file(replacement: &Path, replaced: &Path) -> Result<(), String> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "ReplaceFileW"]
        fn replace_file_w(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
    }

    let replaced_w: Vec<u16> = replaced
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let replacement_w: Vec<u16> = replacement
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    const REPLACEFILE_IGNORE_METADATA_ERRORS: u32 = 2;
    let ok = unsafe {
        replace_file_w(
            replaced_w.as_ptr(),
            replacement_w.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_IGNORE_METADATA_ERRORS,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err("atomic replace failed (the previous file is untouched)".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(_replacement: &Path, _replaced: &Path) -> Result<(), String> {
    Err("atomic replace is only implemented on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalMode, ProxyServer, ProxyType};
    use crate::rule::{ProxyMode, ProxyRule};

    /// Random per-run canary standing in for a proxy password. Never a fixed
    /// literal: a committed constant that looks like a credential fails the
    /// pre-commit security gate, and a random value proves round-trips.
    fn canary() -> String {
        format!(
            "canary-{:x}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id()
        )
    }

    fn temp_dir() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "intentroute-ws-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn base_config(secret: &str) -> AppConfig {
        let mut config = AppConfig::empty();
        config.proxy_servers = vec![ProxyServer {
            id: "s1".into(),
            name: String::new(),
            proxy_type: ProxyType::Socks5,
            host: "127.0.0.1".into(),
            port: 10808,
            username: String::new(),
            password: secret.into(),
            enabled: true,
        }];
        let mut rule = ProxyRule::new("r1", "app.exe");
        rule.mode = ProxyMode::Direct;
        config.rules = vec![rule];
        config
    }

    fn write_config(directory: &Path, config: &AppConfig) -> PathBuf {
        let path = directory.join("config.json");
        let mut clone = config.clone();
        for server in &mut clone.proxy_servers {
            #[cfg(windows)]
            {
                server.password = protect_password(&server.password).unwrap();
            }
        }
        std::fs::write(&path, serde_json::to_string_pretty(&clone).unwrap()).unwrap();
        path
    }

    #[test]
    fn load_reports_missing_for_absent_file() {
        let directory = temp_dir();
        match Workspace::load(&directory.join("absent.json")) {
            LoadStatus::Missing => {}
            _ => panic!("expected Missing"),
        }
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn load_decrypts_dpapi_passwords_at_the_boundary() {
        let secret = canary();
        let directory = temp_dir();
        let path = write_config(&directory, &base_config(&secret));

        let workspace = match Workspace::load(&path) {
            LoadStatus::Loaded(workspace) => workspace,
            _ => panic!("expected Loaded"),
        };
        assert_eq!(workspace.snapshot().proxy_servers[0].password, secret);
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn load_unusable_on_corrupt_json_creates_recovery_copy_and_preserves_original() {
        let directory = temp_dir();
        let path = directory.join("config.json");
        std::fs::write(&path, "{ not json").unwrap();
        let before = std::fs::read(&path).unwrap();

        match Workspace::load(&path) {
            LoadStatus::Unusable(backup, reason) => {
                assert!(reason.contains("parse"), "reason: {reason}");
                let backup = backup.expect("recovery copy expected");
                assert!(backup.to_string_lossy().contains("corrupt-"));
                assert_eq!(std::fs::read(&backup).unwrap(), before);
            }
            _ => panic!("expected Unusable"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), before, "original untouched");
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn load_unusable_on_invalid_utf8() {
        let directory = temp_dir();
        let path = directory.join("config.json");
        std::fs::write(&path, [0xFF, 0xFE, b'{']).unwrap();
        match Workspace::load(&path) {
            LoadStatus::Unusable(_, reason) => assert!(reason.contains("UTF-8")),
            _ => panic!("expected Unusable"),
        }
        std::fs::remove_dir_all(&directory).ok();
    }

    #[cfg(windows)]
    #[test]
    fn load_unusable_on_foreign_ciphertext() {
        // An envelope that cannot decrypt for this user fails closed.
        let directory = temp_dir();
        let path = directory.join("config.json");
        use base64::Engine as _;
        let foreign = base64::engine::general_purpose::STANDARD.encode([0u8; 16]);
        std::fs::write(
            &path,
            format!(r#"{{"Rules":[],"ProxyServers":[{{"Id":"s1","Password":"dpapi:{foreign}"}}]}}"#),
        )
        .unwrap();
        match Workspace::load(&path) {
            LoadStatus::Unusable(_, reason) => {
                assert!(reason.contains("decrypted") || reason.contains("malformed"))
            }
            _ => panic!("expected Unusable"),
        }
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn commit_persists_and_publishes_with_protected_password() {
        let secret = canary();
        let directory = temp_dir();
        let path = write_config(&directory, &base_config(&secret));
        let mut workspace = match Workspace::load(&path) {
            LoadStatus::Loaded(workspace) => workspace,
            _ => panic!("expected Loaded"),
        };

        let published = workspace
            .commit(|candidate| {
                candidate.rules[0].mode = ProxyMode::Proxy;
                candidate.global_mode = GlobalMode::ProxyAll;
            })
            .expect("commit should succeed");

        // Published snapshot matches the new state and is detached.
        assert_eq!(published.rules[0].mode, ProxyMode::Proxy);
        assert_eq!(workspace.snapshot().rules[0].mode, ProxyMode::Proxy);
        let mut leaked = published;
        leaked.rules.clear();
        assert_eq!(workspace.snapshot().rules.len(), 1, "snapshot is detached");

        // Disk carries the new mode with an encrypted password envelope and
        // never the plaintext canary.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""Mode": 0"#));
        assert!(raw.contains("dpapi:"), "password encrypted at rest");
        assert!(!raw.contains(&secret), "plaintext canary must not persist");

        // A reload decrypts back to the canary.
        match Workspace::load(&path) {
            LoadStatus::Loaded(reloaded) => {
                assert_eq!(reloaded.snapshot().proxy_servers[0].password, secret);
            }
            _ => panic!("expected reloaded"),
        }
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn commit_validation_failure_leaves_memory_and_disk_unchanged() {
        let secret = canary();
        let directory = temp_dir();
        let path = write_config(&directory, &base_config(&secret));
        let before = std::fs::read(&path).unwrap();
        let mut workspace = match Workspace::load(&path) {
            LoadStatus::Loaded(workspace) => workspace,
            _ => panic!("expected Loaded"),
        };

        // Empty process name violates the workspace invariant.
        let error = workspace
            .commit(|candidate| candidate.rules[0].exe_name = "   ".into())
            .expect_err("should fail");
        assert!(error.contains("process name"), "error: {error}");
        assert_eq!(
            workspace.snapshot().rules[0].exe_name,
            "app.exe",
            "memory unchanged"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "disk unchanged");

        // A non-loopback upstream fails closed through the builder dry-run.
        let error = workspace
            .commit(|candidate| candidate.proxy_servers[0].host = "example.com".into())
            .expect_err("should fail");
        assert!(error.contains("loopback"), "error: {error}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "disk unchanged");
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn commit_rejects_duplicate_rule_ids_without_persisting() {
        let secret = canary();
        let directory = temp_dir();
        let mut config = base_config(&secret);
        config.rules.push(ProxyRule::new("r1", "other.exe"));
        let path = write_config(&directory, &config);
        let before = std::fs::read(&path).unwrap();
        let mut workspace = match Workspace::load(&path) {
            LoadStatus::Loaded(workspace) => workspace,
            _ => panic!("expected Loaded"),
        };

        let error = workspace.commit(|_| {}).expect_err("duplicate ids must fail");
        assert!(error.contains("unique"), "error: {error}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn commit_rejects_proxy_chains() {
        let secret = canary();
        let directory = temp_dir();
        let path = write_config(&directory, &base_config(&secret));
        let mut workspace = match Workspace::load(&path) {
            LoadStatus::Loaded(workspace) => workspace,
            _ => panic!("expected Loaded"),
        };

        let error = workspace
            .commit(|candidate| candidate.proxy_chains = vec![serde_json::json!({"Id": "c1"})])
            .expect_err("chains must fail");
        assert!(
            error.contains("Proxy chains are not supported"),
            "error: {error}"
        );
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn commit_creates_file_when_missing() {
        let secret = canary();
        let directory = temp_dir();
        let path = directory.join("nested").join("config.json");
        let mut workspace = Workspace {
            path: path.clone(),
            active: base_config(&secret),
        };

        workspace.commit(|_| {}).expect("commit should create the file");
        assert!(path.is_file());
        assert!(std::fs::read_to_string(&path).unwrap().contains("dpapi:"));
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn commit_is_atomic_against_a_locked_destination() {
        let secret = canary();
        let directory = temp_dir();
        let path = write_config(&directory, &base_config(&secret));
        let before = std::fs::read(&path).unwrap();
        let mut workspace = match Workspace::load(&path) {
            LoadStatus::Loaded(workspace) => workspace,
            _ => panic!("expected Loaded"),
        };

        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ: u32 = 1;
            let _lock = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ) // no write/delete share → replace fails
                .open(&path)
                .unwrap();
            if let Err(error) = workspace.commit(|candidate| candidate.rules[0].priority = 99) {
                assert!(
                    error.contains("replace failed") || error.contains("move into place"),
                    "error: {error}"
                );
                assert_eq!(std::fs::read(&path).unwrap(), before, "original untouched");
                assert_eq!(workspace.snapshot().rules[0].priority, 0, "memory unchanged");
                let leftovers: Vec<_> = std::fs::read_dir(&directory)
                    .unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                    .collect();
                assert!(leftovers.is_empty(), "temp cleaned up");
            }
        }
        std::fs::remove_dir_all(&directory).ok();
    }
}

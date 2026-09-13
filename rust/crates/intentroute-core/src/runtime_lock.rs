//! Management-lock interop, ported from `SingBoxRuntime.cs`: the WPF
//! application holds an exclusive `sing-box.runtime.lock` handle in the
//! configuration directory for its whole lifetime. Rust write paths acquire
//! the same lock for the duration of one load→commit transaction, so an
//! editing console and a running WPF instance can never mutate
//! `config.json` concurrently. Best-effort delete on release mirrors the C#
//! dispose behavior.

use std::path::{Path, PathBuf};

pub const RUNTIME_LOCK_FILE_NAME: &str = "sing-box.runtime.lock";

pub struct ManagementLockGuard {
    _file: std::fs::File,
    path: PathBuf,
}

impl ManagementLockGuard {
    pub fn acquire(config_directory: &Path) -> Result<Self, String> {
        let path = config_directory.join(RUNTIME_LOCK_FILE_NAME);
        let file = open_exclusive(&path).map_err(|_| {
            format!(
                "Another IntentRoute AI instance is already managing this configuration directory ({}).",
                path.display()
            )
        })?;
        Ok(Self { _file: file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ManagementLockGuard {
    fn drop(&mut self) {
        // Release (drop handle) happens implicitly; matching the C# dispose,
        // removing the leftover file is best-effort only.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Runs `body` while holding the management lock for `config_directory`.
/// The lock is released even when `body` fails.
pub fn with_management_lock<T>(
    config_directory: &Path,
    body: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _guard = ManagementLockGuard::acquire(config_directory)?;
    body()
}

#[cfg(windows)]
fn open_exclusive(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .share_mode(0) // FileShare.None — the C# semantics
        .open(path)
}

#[cfg(not(windows))]
fn open_exclusive(_path: &Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "management lock is only implemented on Windows",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn second_acquisition_is_exclusive_and_release_allows_reacquire() {
        let directory = std::env::temp_dir().join(format!(
            "intentroute-lock-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let lock_path = directory.join(RUNTIME_LOCK_FILE_NAME);

        let first = ManagementLockGuard::acquire(&directory).expect("first acquire");
        assert!(lock_path.exists());

        let second = ManagementLockGuard::acquire(&directory);
        assert!(
            second
                .err()
                .unwrap_or_default()
                .contains("already managing"),
            "second acquire must fail while held"
        );

        drop(first);
        // The C# dispose best-effort deletes the leftover file; a stale file
        // (if deletion raced) still allows reacquisition because exclusivity
        // comes from the open handle, not file existence.
        let _ = std::fs::remove_file(&lock_path);
        let third = ManagementLockGuard::acquire(&directory).expect("reacquire after release");
        drop(third);
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn with_management_lock_runs_body_and_releases() {
        let directory = std::env::temp_dir().join(format!(
            "intentroute-lockfn-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();

        let result = with_management_lock(&directory, || Ok(42));
        assert_eq!(result.unwrap(), 42);

        let result: Result<(), String> = with_management_lock(&directory, || Err("boom".into()));
        assert_eq!(result.unwrap_err(), "boom");

        // Lock is free again after both calls.
        let guard = ManagementLockGuard::acquire(&directory).expect("free after body");
        drop(guard);
        std::fs::remove_dir_all(&directory).ok();
    }
}

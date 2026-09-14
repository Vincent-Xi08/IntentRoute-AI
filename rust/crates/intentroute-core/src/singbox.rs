//! Managed sing-box process host for the monitor page (parity slice 11),
//! porting the execution-facing half of the WPF `SingBoxRuntime` +
//! `SingBoxExecutionBackend`:
//!
//! - **Discovery** (`INTENTROUTE_SING_BOX` → legacy env → beside the app →
//!   PATH) only *lists* candidates; nothing is executed without an
//!   explicitly passed path, preserving the WPF approval boundary.
//! - **Version probe** runs `<exe> version` (5 s timeout) and requires
//!   ≥ 1.13.0; errors are secret-redacted and truncated.
//! - **Config check** runs `<exe> check -c <file>` (15 s timeout) on a
//!   candidate file before anything is promoted or executed.
//! - **Managed run** acquires the same `sing-box.runtime.lock` the WPF
//!   runtime holds for its lifetime (mutual exclusion with a running WPF
//!   instance and with concurrent editors), writes the full generated
//!   config atomically, spawns `<exe> run -c` with `CREATE_NO_WINDOW`,
//!   piped stdio, and membership in a kill-on-close Job Object (so stop
//!   terminates the whole process tree), redacts every captured line at
//!   capture time into a bounded ring buffer, and deletes the
//!   password-bearing generated config on stop, failure, and drop.
//!
//! All process launches go through [`spawn`], which passes the executable
//! path and every argument as an argument list (the Rust equivalent of
//! `shell=False`); no shell is ever involved.
//!
//! Remaining WPF parity deliberately out of scope here: the
//! check-then-replace apply pipeline with rollback onto a previously
//! running process, orphaned-PID recovery via the runtime state file, and
//! the RunningStale state ladder.

use crate::runtime_lock::ManagementLockGuard;
use crate::runtime_log::LogLine;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Launcher, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const GENERATED_CONFIG_FILE_NAME: &str = "sing-box.generated.json";
pub const ENV_EXECUTABLE: &str = "INTENTROUTE_SING_BOX";
pub const LEGACY_ENV_EXECUTABLE: &str = "PROXYMANAGER_SING_BOX";
pub const DEFAULT_EXECUTABLE_NAMES: [&str; 2] = ["sing-box.exe", "sing-box"];

const MINIMUM_VERSION: (u32, u32, u32) = (1, 13, 0);
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const STARTUP_SETTLE: Duration = Duration::from_millis(300);
const MAX_VERSION_OUTPUT_CHARS: usize = 8_192;
const MAX_ERROR_CHARS: usize = 800;

type LogSink = Arc<Mutex<VecDeque<LogLine>>>;

/// Lists sing-box executable candidates in the WPF discovery order.
/// Discovery never executes anything.
pub fn discover_executables(app_directory: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for var in [ENV_EXECUTABLE, LEGACY_ENV_EXECUTABLE] {
        if let Ok(value) = std::env::var(var) {
            let value = value.trim();
            if !value.is_empty() {
                if let Some(resolved) = resolve_candidate(Path::new(value)) {
                    candidates.push(resolved);
                }
            }
        }
    }
    if let Some(directory) = app_directory {
        for name in DEFAULT_EXECUTABLE_NAMES {
            let candidate = directory.join(name);
            if candidate.is_file() {
                candidates.push(candidate);
            }
        }
    }
    if let Some(found) = find_on_path() {
        candidates.push(found);
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

/// Accepts a file path or a directory containing `sing-box.exe`/`sing-box`.
fn resolve_candidate(path_or_dir: &Path) -> Option<PathBuf> {
    if path_or_dir.is_file() {
        return Some(canonicalish(path_or_dir));
    }
    if path_or_dir.is_dir() {
        for name in DEFAULT_EXECUTABLE_NAMES {
            let candidate = path_or_dir.join(name);
            if candidate.is_file() {
                return Some(canonicalish(&candidate));
            }
        }
    }
    None
}

fn canonicalish(path: &Path) -> PathBuf {
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // `canonicalize` yields `\\?\`-prefixed verbatim paths; the product
    // surface (and the WPF `Path.GetFullPath`) uses plain drive paths.
    let text = full.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text.as_ref()))
}

fn find_on_path() -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    for segment in std::env::split_paths(&path_env) {
        for name in DEFAULT_EXECUTABLE_NAMES {
            let candidate = segment.join(name);
            if candidate.is_file() {
                return Some(canonicalish(&candidate));
            }
        }
    }
    None
}

/// Parses `sing-box version vX.Y.Z` lines; ported from the WPF regex
/// `^\s*sing-box\s+version\s+v?(\d+)\.(\d+)\.(\d+)(?:\s|$)` (case-insensitive).
pub fn parse_version_output(text: &str) -> Option<(u32, u32, u32)> {
    for line in text.lines() {
        let rest = strip_prefix_ci(line.trim_start(), "sing-box")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let rest = strip_prefix_ci(rest.trim_start(), "version")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let digits = rest.trim_start().strip_prefix('v').unwrap_or(rest.trim_start());
        let mut parts = digits.split('.');
        let major = leading_number(parts.next()?)?;
        let minor = leading_number(parts.next()?)?;
        let patch_part = parts.next()?;
        let patch = leading_number(patch_part)?;
        let after = patch_part.trim_start_matches(|c: char| c.is_ascii_digit());
        if !after.is_empty() && !after.starts_with(char::is_whitespace) {
            return None;
        }
        return Some((major, minor, patch));
    }
    None
}

fn leading_number(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let mut consumed = 0usize;
    for expected in prefix.chars() {
        let actual = text[consumed..].chars().next()?;
        if actual.to_lowercase().ne(expected.to_lowercase()) {
            return None;
        }
        consumed += actual.len_utf8();
    }
    Some(&text[consumed..])
}

/// Case-insensitive substring search returning a byte offset.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let first = needle.chars().next()?;
    for (offset, actual) in haystack.char_indices() {
        if actual.to_lowercase().ne(first.to_lowercase()) {
            continue;
        }
        if strip_prefix_ci(&haystack[offset..], needle).is_some() {
            return Some(offset);
        }
    }
    None
}

/// Secret redaction ported from the WPF `SingBoxRuntime.RedactSecrets`:
/// JSON `"password": "..."` values become `"***"`, and
/// `password|passwd|pwd|secret|token|credential` key/value separators
/// (`:` or `=`) hide the following non-whitespace run. Applied to every
/// captured log line, every error string, and export text.
pub fn redact_secrets(text: &str) -> String {
    redact_secret_assignments(&redact_json_passwords(text))
}

fn redact_json_passwords(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while let Some(found) = text[cursor..].find('"') {
        let open = cursor + found;
        let Some(key_close_rel) = text[open + 1..].find('"') else {
            break;
        };
        let key_close = open + 1 + key_close_rel;
        let key = &text[open + 1..key_close];
        if key.eq_ignore_ascii_case("password") {
            let after = &text[key_close + 1..];
            let mut i = after.len() - after.trim_start().len();
            if after[i..].starts_with(':') {
                i += 1;
                i += after[i..].len() - after[i..].trim_start().len();
                if after[i..].starts_with('"') {
                    let value_start = i + 1;
                    if let Some(value_close) = after[value_start..].find('"') {
                        // Copy through the value's opening quote, re-emit a
                        // quoted "***", resume after the closing quote.
                        output.push_str(&text[cursor..key_close + 1 + value_start]);
                        output.push_str("***\"");
                        cursor = key_close + 1 + value_start + value_close + 1;
                        continue;
                    }
                }
            }
        }
        output.push_str(&text[cursor..key_close + 1]);
        cursor = key_close + 1;
    }
    output.push_str(&text[cursor..]);
    output
}

const SECRET_KEYWORDS: [&str; 6] = ["password", "passwd", "pwd", "secret", "token", "credential"];

fn redact_secret_assignments(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < text.len() {
        let earliest = SECRET_KEYWORDS
            .iter()
            .filter_map(|keyword| {
                find_ci(&text[cursor..], keyword).map(|offset| (offset, keyword.len()))
            })
            .min_by_key(|(offset, _)| *offset);
        let Some((offset, keyword_len)) = earliest else { break };
        let keyword_start = cursor + offset;
        let after = &text[keyword_start + keyword_len..];
        let mut i = after.len() - after.trim_start().len();
        let separator = after[i..].chars().next();
        if separator != Some(':') && separator != Some('=') {
            output.push_str(&text[cursor..keyword_start + keyword_len]);
            cursor = keyword_start + keyword_len;
            continue;
        }
        i += separator.map_or(1, char::len_utf8);
        i += after[i..].len() - after[i..].trim_start().len();
        let value_len = after[i..]
            .find(char::is_whitespace)
            .unwrap_or(after.len() - i);
        if value_len == 0 {
            output.push_str(&text[cursor..keyword_start + keyword_len]);
            cursor = keyword_start + keyword_len;
            continue;
        }
        output.push_str(&text[cursor..keyword_start + keyword_len + i]);
        output.push_str("***");
        cursor = keyword_start + keyword_len + i + value_len;
    }
    output.push_str(&text[cursor..]);
    output
}

/// Runs `exe args…` to completion with redirected stdout/stderr and a hard
/// timeout; on timeout the child is killed. Output is combined (stdout
/// then stderr) and bounded like the WPF probe.
pub fn run_to_completion(exe: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = spawn(exe, args)
        .map_err(|error| format!("{} failed to start: {error}", exe.display()))?;
    let mut readers = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        readers.push(std::thread::spawn(move || read_bounded(pipe)));
    }
    if let Some(pipe) = child.stderr.take() {
        readers.push(std::thread::spawn(move || read_bounded(pipe)));
    }

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} wait failed: {error}", exe.display()));
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            for reader in readers.drain(..) {
                let _ = reader.join();
            }
            return Err(format!(
                "{} {} timed out after {}s.",
                exe.display(),
                args.first().copied().unwrap_or(""),
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let mut output = String::new();
    for reader in readers {
        if let Ok(text) = reader.join() {
            output.push_str(&text);
        }
    }
    let output = output.trim().to_string();
    if !status.success() {
        return Err(if output.is_empty() {
            format!("exited with code {}", status.code().unwrap_or(-1))
        } else {
            output
        });
    }
    Ok(output)
}

fn read_bounded<R: Read>(pipe: R) -> String {
    let mut reader = BufReader::new(pipe);
    let mut collected = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if collected.len() >= MAX_VERSION_OUTPUT_CHARS {
                    break;
                }
                let trimmed = line.trim_end_matches(['\r', '\n']);
                let remaining = MAX_VERSION_OUTPUT_CHARS - collected.len();
                if trimmed.len() <= remaining {
                    collected.push_str(trimmed);
                    collected.push('\n');
                } else {
                    let cut = trimmed
                        .char_indices()
                        .map(|(index, _)| index)
                        .chain(std::iter::once(trimmed.len()))
                        .find(|index| *index >= remaining)
                        .unwrap_or(trimmed.len());
                    collected.push_str(&trimmed[..cut]);
                    collected.push('\n');
                    break;
                }
            }
        }
    }
    collected
}

/// Probes `<exe> version`, parses the output, and enforces the 1.13 floor.
pub fn probe_version(exe: &Path) -> Result<String, String> {
    let output = run_to_completion(exe, &["version"], VERSION_TIMEOUT)
        .map_err(|error| redact_secrets(&truncate(&format!("sing-box version probe failed: {error}"))))?;
    let Some(version) = parse_version_output(&output) else {
        return Err("sing-box version output could not be recognized; v1.13 or newer is required.".into());
    };
    if version < MINIMUM_VERSION {
        return Err(format!(
            "sing-box {}.{}.{} is not supported; v1.13.0 or newer is required.",
            version.0, version.1, version.2
        ));
    }
    Ok(format!("{}.{}.{}", version.0, version.1, version.2))
}

/// Runs `<exe> check -c <config_path>` and maps failures to a redacted,
/// truncated error string.
pub fn check_config(exe: &Path, config_path: &Path) -> Result<(), String> {
    let config_argument = config_path.to_string_lossy().into_owned();
    run_to_completion(exe, &["check", "-c", &config_argument], CHECK_TIMEOUT)
        .map_err(|error| redact_secrets(&truncate(&format!("sing-box check failed: {error}"))))
        .map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    Stopped,
    Running,
    Failed,
}

#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    pub state: RuntimeState,
    pub is_running: bool,
    pub process_id: Option<u32>,
    pub version: Option<String>,
    pub executable_path: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub last_error: Option<String>,
}

struct Inner {
    logs: LogSink,
    child: Option<Child>,
    reader_threads: Vec<std::thread::JoinHandle<()>>,
    stopping: bool,
    executable_path: PathBuf,
    version: Option<String>,
    config_path: PathBuf,
    state: RuntimeState,
    last_error: Option<String>,
    _job: KillOnCloseJob,
    _guard: ManagementLockGuard,
}

/// A managed `sing-box run` process. Dropping the value stops the process
/// tree and deletes the generated config (which carries proxy passwords).
pub struct ManagedRuntime {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for ManagedRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ManagedRuntime")
    }
}

impl ManagedRuntime {
    /// Validates + starts a managed sing-box for `config_directory`:
    /// acquire the runtime lock → clean stale artifacts → write the
    /// candidate config → `<exe> check` → promote atomically → spawn
    /// `<exe> run -c` in a kill-on-close job → settle. On any failure
    /// nothing is left behind and the lock is released.
    pub fn start(
        config_directory: &Path,
        executable: &Path,
        config_json: &str,
        version: Option<&str>,
        max_log_lines: usize,
    ) -> Result<Self, String> {
        let guard = ManagementLockGuard::acquire(config_directory)?;
        Self::start_locked(guard, config_directory, executable, config_json, version, max_log_lines)
    }

    fn start_locked(
        guard: ManagementLockGuard,
        config_directory: &Path,
        executable: &Path,
        config_json: &str,
        version: Option<&str>,
        max_log_lines: usize,
    ) -> Result<ManagedRuntime, String> {
        cleanup_stale_runtime_artifacts(config_directory);
        std::fs::create_dir_all(config_directory)
            .map_err(|error| format!("failed to prepare config directory: {error}"))?;

        let config_path = config_directory.join(GENERATED_CONFIG_FILE_NAME);
        let candidate_path = config_directory.join(format!(
            "{}.{}.candidate",
            GENERATED_CONFIG_FILE_NAME,
            unique_suffix()
        ));
        // The full config carries proxy passwords; it goes straight to disk
        // and is never surfaced through an error path.
        std::fs::write(&candidate_path, config_json.as_bytes())
            .map_err(|error| format!("failed to write candidate config: {error}"))?;

        if let Err(error) = check_config(executable, &candidate_path) {
            let _ = std::fs::remove_file(&candidate_path);
            return Err(error);
        }

        crate::workspace::save_atomic(&config_path, config_json.as_bytes())
            .map_err(|error| format!("failed to promote config: {error}"))?;
        let _ = std::fs::remove_file(&candidate_path);

        let max_logs = max_log_lines.max(32);
        let spawn_result = spawn_managed_raw(
            executable,
            &["run", "-c", &config_path.to_string_lossy()],
            max_logs,
        );
        let (mut child, reader_threads, logs, job) = match spawn_result {
            Ok(spawned) => spawned,
            Err(error) => {
                let _ = std::fs::remove_file(&config_path);
                return Err(format!("failed to start sing-box: {error}"));
            }
        };

        std::thread::sleep(STARTUP_SETTLE);
        match child.try_wait() {
            Ok(Some(status)) => {
                drop(job); // terminates anything left in the job
                for thread in reader_threads {
                    let _ = thread.join();
                }
                let _ = std::fs::remove_file(&config_path);
                return Err(format!(
                    "sing-box exited during startup with code {}.",
                    status.code().unwrap_or(-1)
                ));
            }
            Ok(None) => {}
            Err(error) => {
                drop(job);
                for thread in reader_threads {
                    let _ = thread.join();
                }
                let _ = std::fs::remove_file(&config_path);
                return Err(format!("sing-box startup wait failed: {error}"));
            }
        }

        Ok(ManagedRuntime {
            inner: Mutex::new(Inner {
                logs,
                child: Some(child),
                reader_threads,
                stopping: false,
                executable_path: executable.to_path_buf(),
                version: version.map(str::to_string),
                config_path,
                state: RuntimeState::Running,
                last_error: None,
                _job: job,
                _guard: guard,
            }),
        })
    }

    /// Stops the managed process tree, deletes the generated config, joins
    /// the reader threads, and releases the runtime lock.
    pub fn stop(&self) -> RuntimeStatus {
        if let Ok(mut inner) = self.inner.lock() {
            stop_inner(&mut inner);
        }
        self.status()
    }

    pub fn recent_logs(&self) -> Vec<LogLine> {
        self.converge_on_exit();
        match self.inner.lock() {
            Ok(inner) => snapshot_logs(&inner),
            Err(poisoned) => snapshot_logs(&poisoned.into_inner()),
        }
    }

    pub fn clear_logs(&self) {
        if let Ok(inner) = self.inner.lock() {
            if let Ok(mut logs) = inner.logs.lock() {
                logs.clear();
            }
        }
    }

    pub fn status(&self) -> RuntimeStatus {
        self.converge_on_exit();
        match self.inner.lock() {
            Ok(inner) => status_of(&inner),
            Err(poisoned) => status_of(&poisoned.into_inner()),
        }
    }

    /// Lazily converges the state when the child died on its own: Failed
    /// state, an exit-code error line, and cleanup of the secret-bearing
    /// generated config (mirroring the WPF `OnProcessExited`).
    fn converge_on_exit(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.stopping {
                return;
            }
            let Some(child) = inner.child.as_mut() else { return };
            match child.try_wait() {
                Ok(Some(status)) => {
                    inner.child = None;
                    inner.state = RuntimeState::Failed;
                    inner.last_error = Some(format!(
                        "sing-box exited with code {}.",
                        status.code().unwrap_or(-1)
                    ));
                    for thread in std::mem::take(&mut inner.reader_threads) {
                        let _ = thread.join();
                    }
                    let _ = std::fs::remove_file(&inner.config_path);
                }
                Ok(None) => {}
                Err(_) => {
                    inner.child = None;
                    inner.state = RuntimeState::Failed;
                    inner.last_error = Some("sing-box exited with an unknown code.".into());
                }
            }
        }
    }
}

impl Drop for ManagedRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

fn stop_inner(inner: &mut Inner) {
    inner.stopping = true;
    let child = inner.child.take();
    inner._job = KillOnCloseJob::none();
    if let Some(mut child) = child {
        wait_briefly(&mut child);
    }
    let _ = std::fs::remove_file(&inner.config_path);
    for thread in std::mem::take(&mut inner.reader_threads) {
        let _ = thread.join();
    }
    inner.state = RuntimeState::Stopped;
    inner.last_error = None;
}

fn wait_briefly(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn snapshot_logs(inner: &Inner) -> Vec<LogLine> {
    match inner.logs.lock() {
        Ok(logs) => logs.iter().cloned().collect(),
        Err(poisoned) => poisoned.into_inner().iter().cloned().collect(),
    }
}

fn status_of(inner: &Inner) -> RuntimeStatus {
    let running = !inner.stopping && inner.child.is_some();
    RuntimeStatus {
        state: if running { RuntimeState::Running } else { inner.state },
        is_running: running,
        process_id: running.then(|| inner.child.as_ref().map_or(0, Child::id)),
        version: inner.version.clone(),
        executable_path: Some(inner.executable_path.clone()),
        config_path: Some(inner.config_path.clone()),
        last_error: inner.last_error.clone(),
    }
}

/// Spawns `exe args…` with piped stdio, no console window, and membership
/// in a kill-on-close job; returns the child, reader threads, the shared
/// ring buffer the threads fill with redacted timestamped lines, and the
/// job handle whose release terminates the tree.
fn spawn_managed_raw(
    exe: &Path,
    args: &[&str],
    max_logs: usize,
) -> Result<(Child, Vec<std::thread::JoinHandle<()>>, LogSink, KillOnCloseJob), String> {
    let job = KillOnCloseJob::create().map_err(|error| format!("job object: {error}"))?;
    let mut child = spawn(exe, args).map_err(|error| error.to_string())?;
    if let Err(error) = job.assign(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("job assign: {error}"));
    }
    let sink: LogSink = Arc::new(Mutex::new(VecDeque::new()));
    let mut threads = Vec::new();
    let mut pipes: Vec<Box<dyn Read + Send>> = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        pipes.push(Box::new(pipe));
    }
    if let Some(pipe) = child.stderr.take() {
        pipes.push(Box::new(pipe));
    }
    for pipe in pipes {
        let sink = Arc::clone(&sink);
        threads.push(std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\r', '\n']);
                        if trimmed.is_empty() {
                            continue;
                        }
                        let entry = LogLine {
                            time: local_time(),
                            message: redact_secrets(trimmed),
                        };
                        if let Ok(mut logs) = sink.lock() {
                            logs.push_back(entry);
                            while logs.len() > max_logs {
                                logs.pop_front();
                            }
                        }
                    }
                }
            }
        }));
    }
    Ok((child, threads, sink, job))
}

/// Local wall-clock time `HH:MM:SS` for log capture, via `GetLocalTime`.
fn local_time() -> String {
    #[repr(C)]
    struct SystemTimeW {
        _year: u16,
        _month: u16,
        _day_of_week: u16,
        _day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        _milliseconds: u16,
    }
    #[cfg(windows)]
    {
        extern "system" {
            fn GetLocalTime(system_time: *mut SystemTimeW);
        }
        let mut now = SystemTimeW {
            _year: 0,
            _month: 0,
            _day_of_week: 0,
            _day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            _milliseconds: 0,
        };
        unsafe { GetLocalTime(&mut now) };
        format!("{:02}:{:02}:{:02}", now.hour, now.minute, now.second)
    }
    #[cfg(not(windows))]
    {
        String::new()
    }
}

/// Launches `exe` with `args` as an argument list — piped stdio, stdin
/// closed, `CREATE_NO_WINDOW`, and the executable's directory as the
/// working directory, mirroring the WPF `CreateStartInfo`. The program and
/// every argument are passed as values to the launcher (the Rust
/// equivalent of `shell=False`); no shell is ever involved.
fn spawn(exe: &Path, args: &[&str]) -> std::io::Result<Child> {
    let mut command = Launcher::new(exe);
    command.args(args);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if let Some(directory) = exe.parent() {
        if !directory.as_os_str().is_empty() {
            command.current_dir(directory);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn()
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_ERROR_CHARS {
        return text.to_string();
    }
    let cut = text
        .char_indices()
        .nth(MAX_ERROR_CHARS)
        .map_or(text.len(), |(index, _)| index);
    format!("{}…", &text[..cut])
}

fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{:x}-{}", nanos, std::process::id())
}

fn cleanup_stale_runtime_artifacts(config_directory: &Path) {
    let _ = std::fs::remove_file(config_directory.join(GENERATED_CONFIG_FILE_NAME));
    let Ok(read_dir) = std::fs::read_dir(config_directory) else { return };
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let generated_leftover = name.starts_with("sing-box.generated.json.")
            && (name.ends_with(".candidate") || name.ends_with(".rollback"));
        let state_leftover = name.starts_with("sing-box.runtime-state.json");
        if generated_leftover || state_leftover {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// A Job Object created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: closing
/// the handle terminates every process still in the job (the managed child
/// and anything it spawned), mirroring the WPF
/// `Kill(entireProcessTree: true)` without launching helper processes.
struct KillOnCloseJob {
    handle: Option<RawHandle>,
}

#[cfg(windows)]
type RawHandle = *mut core::ffi::c_void;

#[cfg(not(windows))]
type RawHandle = ();

impl KillOnCloseJob {
    #[cfg(windows)]
    fn none() -> Self {
        Self { handle: None }
    }

    #[cfg(not(windows))]
    fn none() -> Self {
        Self { handle: None }
    }

    #[cfg(windows)]
    fn create() -> Result<Self, String> {
        #[repr(C)]
        struct BasicLimitInformation {
            per_process_user_time_limit: i64,
            per_job_user_time_limit: i64,
            limit_flags: u32,
            minimum_working_set_size: usize,
            maximum_working_set_size: usize,
            active_process_limit: u32,
            affinity: usize,
            priority_class: u32,
            scheduling_class: u32,
        }
        #[repr(C)]
        struct IoCounters {
            read_operation_count: u64,
            write_operation_count: u64,
            other_operation_count: u64,
            read_transfer_count: u64,
            write_transfer_count: u64,
            other_transfer_count: u64,
        }
        #[repr(C)]
        struct ExtendedLimitInformation {
            basic: BasicLimitInformation,
            io: IoCounters,
            process_memory_limit: usize,
            job_memory_limit: usize,
            peak_process_memory_used: usize,
            peak_job_memory_used: usize,
        }

        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

        extern "system" {
            fn CreateJobObjectW(attributes: *mut core::ffi::c_void, name: *const u16) -> RawHandle;
            fn SetInformationJobObject(
                job: RawHandle,
                information_class: u32,
                information: *mut core::ffi::c_void,
                length: u32,
            ) -> i32;
            fn CloseHandle(handle: RawHandle) -> i32;
        }

        let mut information = ExtendedLimitInformation {
            basic: BasicLimitInformation {
                per_process_user_time_limit: 0,
                per_job_user_time_limit: 0,
                limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                minimum_working_set_size: 0,
                maximum_working_set_size: 0,
                active_process_limit: 0,
                affinity: 0,
                priority_class: 0,
                scheduling_class: 0,
            },
            io: IoCounters {
                read_operation_count: 0,
                write_operation_count: 0,
                other_operation_count: 0,
                read_transfer_count: 0,
                write_transfer_count: 0,
                other_transfer_count: 0,
            },
            process_memory_limit: 0,
            job_memory_limit: 0,
            peak_process_memory_used: 0,
            peak_job_memory_used: 0,
        };

        unsafe {
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job.is_null() {
                return Err("CreateJobObjectW failed".into());
            }
            let ok = SetInformationJobObject(
                job,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                &mut information as *mut ExtendedLimitInformation as *mut core::ffi::c_void,
                std::mem::size_of::<ExtendedLimitInformation>() as u32,
            );
            if ok == 0 {
                CloseHandle(job);
                return Err("SetInformationJobObject failed".into());
            }
            Ok(Self { handle: Some(job) })
        }
    }

    #[cfg(not(windows))]
    fn create() -> Result<Self, String> {
        Err("job objects are only implemented on Windows".into())
    }

    #[cfg(windows)]
    fn assign(&self, child: &Child) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        extern "system" {
            fn AssignProcessToJobObject(job: RawHandle, process: RawHandle) -> i32;
        }
        let job = self.handle.ok_or("job already released")?;
        let ok = unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) };
        if ok == 0 {
            return Err("AssignProcessToJobObject failed".into());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn assign(&self, _child: &Child) -> Result<(), String> {
        Err("job objects are only implemented on Windows".into())
    }
}

#[cfg(windows)]
impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            extern "system" {
                fn CloseHandle(object: RawHandle) -> i32;
            }
            unsafe { CloseHandle(handle) };
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;

    fn temp_directory(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "intentroute-singbox-{label}-{}-{nanos:x}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn cmd() -> PathBuf {
        let system_root = std::env::var_os("SystemRoot").map_or_else(
            || PathBuf::from(r"C:\Windows"),
            PathBuf::from,
        );
        system_root.join("System32").join("cmd.exe")
    }

    /// A fake `sing-box` script: `version` prints a 1.13.0 banner, `check`
    /// succeeds, `run` prints one logrus line then sleeps until killed.
    fn fake_sing_box(directory: &Path, version: &str) -> PathBuf {
        let path = directory.join("fake-sing-box.cmd");
        let script = format!(
            "@echo off\r\nif \"%1\"==\"version\" echo sing-box version v{version} & exit /b 0\r\nif \"%1\"==\"check\" exit /b 0\r\necho INFO[0000] sing-box started\r\nping -n 60 127.0.0.1 > nul\r\n"
        );
        fs::write(&path, script).unwrap();
        path
    }

    #[test]
    fn parse_version_accepts_supported_banners() {
        assert_eq!(parse_version_output("sing-box version v1.13.0"), Some((1, 13, 0)));
        assert_eq!(parse_version_output("sing-box version 1.13.19"), Some((1, 13, 19)));
        assert_eq!(
            parse_version_output("  sing-box  version  v2.0.1 \r\nmore"),
            Some((2, 0, 1))
        );
        assert_eq!(parse_version_output("sing-box version v1.2.3 extra"), Some((1, 2, 3)));
    }

    #[test]
    fn parse_version_rejects_unrecognized_output() {
        assert_eq!(parse_version_output("some other tool 1.13.0"), None);
        assert_eq!(parse_version_output("sing-box version v1.13"), None);
        assert_eq!(parse_version_output("sing-box version v1.13.0-rc.1"), None);
        assert_eq!(parse_version_output("sing-boxversion v1.13.0"), None);
        assert_eq!(parse_version_output(""), None);
    }

    #[test]
    fn redact_secrets_hides_password_values() {
        let secret = format!("canary-{:x}", std::process::id());
        let redacted = redact_secrets(&format!("server password={secret} rejected"));
        assert!(redacted.contains("password=***"), "{redacted}");
        assert!(!redacted.contains(&secret));

        let redacted = redact_secrets(&format!("token: {secret}"));
        assert!(redacted.contains("token: ***"), "{redacted}");
        assert!(!redacted.contains(&secret));
    }

    #[test]
    fn redact_secrets_hides_json_passwords() {
        let secret = format!("canary-{:x}", std::process::id());
        let redacted = redact_secrets(&format!(
            "config {{\"password\": \"{secret}\", \"name\": \"keep\"}}"
        ));
        assert!(redacted.contains("\"password\": \"***\""), "{redacted}");
        assert!(redacted.contains("\"name\": \"keep\""));
        assert!(!redacted.contains(&secret));
    }

    #[test]
    fn redact_secrets_preserves_ordinary_text_and_cjk() {
        let text = "INFO[0001] 入站 127.0.0.1:10808 无敏感内容 started";
        assert_eq!(redact_secrets(text), text);
    }

    #[test]
    fn run_to_completion_captures_output() {
        let output = run_to_completion(&cmd(), &["/c", "echo probe-ok"], VERSION_TIMEOUT).unwrap();
        assert!(output.contains("probe-ok"), "{output}");
    }

    #[test]
    fn run_to_completion_maps_nonzero_exit() {
        let error = run_to_completion(&cmd(), &["/c", "exit /b 3"], VERSION_TIMEOUT).unwrap_err();
        assert!(error.contains("3") || error.contains("code"), "{error}");
    }

    #[test]
    fn run_to_completion_times_out_and_kills() {
        let error = run_to_completion(&cmd(), &["/c", "ping -n 30 127.0.0.1 > nul"], Duration::from_millis(400))
            .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
    }

    #[test]
    fn probe_version_reports_supported_version() {
        let directory = temp_directory("probe");
        let fake = fake_sing_box(&directory, "1.13.19");
        assert_eq!(probe_version(&fake).unwrap(), "1.13.19");
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn probe_version_rejects_below_minimum() {
        let directory = temp_directory("probe-old");
        let fake = fake_sing_box(&directory, "1.12.9");
        let error = probe_version(&fake).unwrap_err();
        assert!(error.contains("not supported"), "{error}");
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn managed_runtime_runs_captures_and_stops() {
        let directory = temp_directory("managed");
        let fake = fake_sing_box(&directory, "1.13.0");

        let runtime = ManagedRuntime::start(&directory, &fake, "{}", Some("1.13.0"), 64).unwrap();
        let status = runtime.status();
        assert!(status.is_running);
        assert_eq!(status.state, RuntimeState::Running);
        assert!(status.process_id.is_some());

        // The generated config exists while running and is removed on stop.
        let generated = directory.join(GENERATED_CONFIG_FILE_NAME);
        assert!(generated.is_file());

        // The startup log line lands in the ring buffer (redacted pipeline).
        let mut saw_line = false;
        for _ in 0..50 {
            let logs = runtime.recent_logs();
            if logs.iter().any(|entry| entry.message.contains("sing-box started")) {
                saw_line = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(saw_line, "expected the fake startup line to be captured");

        let final_status = runtime.stop();
        assert!(!final_status.is_running);
        assert_eq!(final_status.state, RuntimeState::Stopped);
        assert!(!generated.is_file(), "generated config must be deleted on stop");

        // Like the WPF Stop (process dies, runtime object lives), the lock
        // stays held until the runtime is dropped (the WPF Dispose).
        assert!(ManagementLockGuard::acquire(&directory).is_err(), "lock held after stop");
        drop(runtime);
        let guard = ManagementLockGuard::acquire(&directory).expect("lock free after drop");
        drop(guard);
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn managed_runtime_rejects_running_when_lock_held() {
        let directory = temp_directory("locked");
        let fake = fake_sing_box(&directory, "1.13.0");
        let _guard = ManagementLockGuard::acquire(&directory).unwrap();

        let error = ManagedRuntime::start(&directory, &fake, "{}", None, 64).unwrap_err();
        assert!(error.contains("already managing"), "{error}");
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn managed_runtime_reports_immediate_exit_as_failure() {
        let directory = temp_directory("crash");
        // `run` exits immediately: no echo branch matched for "check", so
        // make a dedicated crasher script.
        let path = directory.join("fake-sing-box.cmd");
        fs::write(
            &path,
            "@echo off\r\nif \"%1\"==\"version\" echo sing-box version v1.13.0 & exit /b 0\r\nif \"%1\"==\"check\" exit /b 0\r\nexit /b 2\r\n",
        )
        .unwrap();

        let error = ManagedRuntime::start(&directory, &path, "{}", None, 64).unwrap_err();
        assert!(error.contains("exited during startup"), "{error}");
        assert!(!directory.join(GENERATED_CONFIG_FILE_NAME).is_file());
        let guard = ManagementLockGuard::acquire(&directory).expect("lock released after failure");
        drop(guard);
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn managed_runtime_check_failure_keeps_directory_clean() {
        let directory = temp_directory("badcheck");
        // `check` exits 1 with a stderr message.
        let path = directory.join("fake-sing-box.cmd");
        fs::write(
            &path,
            "@echo off\r\nif \"%1\"==\"version\" echo sing-box version v1.13.0 & exit /b 0\r\necho config error & exit /b 1\r\n",
        )
        .unwrap();

        let error = ManagedRuntime::start(&directory, &path, "{}", None, 64).unwrap_err();
        assert!(error.contains("check failed"), "{error}");
        assert!(!directory.join(GENERATED_CONFIG_FILE_NAME).is_file());
        // No candidate leftovers either.
        for entry in fs::read_dir(&directory).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(!name.contains(".candidate"), "leftover candidate: {name}");
            assert!(!name.contains(".rollback"), "leftover rollback: {name}");
        }
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn managed_runtime_stale_artifacts_are_cleaned_on_start() {
        let directory = temp_directory("stale");
        let fake = fake_sing_box(&directory, "1.13.0");
        fs::write(directory.join(GENERATED_CONFIG_FILE_NAME), "stale").unwrap();
        fs::write(directory.join("sing-box.generated.json.deadbeef.candidate"), "stale").unwrap();
        fs::write(directory.join("sing-box.runtime-state.json"), "stale").unwrap();

        let runtime = ManagedRuntime::start(&directory, &fake, "{}", None, 64).unwrap();
        assert!(directory.join(GENERATED_CONFIG_FILE_NAME).is_file(), "fresh config written");
        assert!(!directory.join("sing-box.runtime-state.json").is_file());
        runtime.stop();
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn managed_runtime_converges_to_failed_when_child_dies() {
        let directory = temp_directory("die");
        // Runs for ~1 second, then exits on its own.
        let path = directory.join("fake-sing-box.cmd");
        fs::write(
            &path,
            "@echo off\r\nif \"%1\"==\"version\" echo sing-box version v1.13.0 & exit /b 0\r\nif \"%1\"==\"check\" exit /b 0\r\nping -n 2 127.0.0.1 > nul\r\nexit /b 5\r\n",
        )
        .unwrap();

        let runtime = ManagedRuntime::start(&directory, &path, "{}", None, 64).unwrap();
        // Poll until the lazy convergence observes the exit.
        let mut failed = false;
        for _ in 0..100 {
            let status = runtime.status();
            if status.state == RuntimeState::Failed {
                assert!(!status.is_running);
                let error = status.last_error.unwrap_or_default();
                assert!(error.contains("exited with code"), "{error}");
                failed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(failed, "runtime must converge to Failed after child exit");
        assert!(!directory.join(GENERATED_CONFIG_FILE_NAME).is_file());
        drop(runtime);
        let guard = ManagementLockGuard::acquire(&directory).expect("lock free after drop");
        drop(guard);
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn ring_buffer_is_bounded() {
        let directory = temp_directory("bounded");
        let path = directory.join("fake-sing-box.cmd");
        fs::write(
            &path,
            "@echo off\r\nif \"%1\"==\"version\" echo sing-box version v1.13.0 & exit /b 0\r\nif \"%1\"==\"check\" exit /b 0\r\nfor /L %%i in (1,1,60) do @echo line %%i\r\nping -n 30 127.0.0.1 > nul\r\n",
        )
        .unwrap();

        // The requested cap floors at 32 (the WPF `Math.Max(32, …)`), so the
        // 60 echoed lines must trim to the newest 32.
        let runtime = ManagedRuntime::start(&directory, &path, "{}", None, 3).unwrap();
        let mut bounded = false;
        for _ in 0..100 {
            let logs = runtime.recent_logs();
            assert!(logs.len() <= 32, "ring buffer exceeded cap: {}", logs.len());
            if logs.len() == 32
                && logs
                    .last()
                    .is_some_and(|entry| entry.message.contains("line 60"))
            {
                bounded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(bounded, "expected the buffer to trim to its floor cap of 32");
        runtime.stop();
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn captured_lines_are_redacted_at_capture_time() {
        let directory = temp_directory("redact");
        let secret = format!("canary-{:x}", std::process::id());
        let path = directory.join("fake-sing-box.cmd");
        fs::write(
            &path,
            format!(
                "@echo off\r\nif \"%1\"==\"version\" echo sing-box version v1.13.0 & exit /b 0\r\nif \"%1\"==\"check\" exit /b 0\r\necho INFO[0001] auth password={secret} rejected\r\nping -n 30 127.0.0.1 > nul\r\n"
            ),
        )
        .unwrap();

        let runtime = ManagedRuntime::start(&directory, &path, "{}", None, 64).unwrap();
        let mut redacted = false;
        for _ in 0..100 {
            let logs = runtime.recent_logs();
            if logs.iter().any(|entry| entry.message.contains("rejected")) {
                for entry in &logs {
                    assert!(!entry.message.contains(&secret), "leaked: {}", entry.message);
                    if entry.message.contains("rejected") {
                        assert!(entry.message.contains("password=***"), "{}", entry.message);
                    }
                }
                redacted = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(redacted, "expected the secret-bearing line to be captured");
        runtime.stop();
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn discover_executables_prefers_environment_variable() {
        let directory = temp_directory("discover");
        let fake = directory.join("sing-box.exe");
        fs::write(&fake, b"").unwrap();
        // `temp_dir` may be an 8.3 short path while discovery canonicalizes;
        // compare through the same normalization.
        let expected = canonicalish(&fake);

        let previous = std::env::var(ENV_EXECUTABLE).ok();
        std::env::set_var(ENV_EXECUTABLE, fake.as_os_str());
        let candidates = discover_executables(None);
        std::env::set_var(ENV_EXECUTABLE, "");
        match previous {
            Some(value) => std::env::set_var(ENV_EXECUTABLE, value),
            None => std::env::remove_var(ENV_EXECUTABLE),
        }

        assert!(
            candidates.iter().any(|candidate| candidate == &expected),
            "env candidate must be discovered: {candidates:?} vs {expected:?}"
        );
        fs::remove_dir_all(&directory).ok();
    }
}

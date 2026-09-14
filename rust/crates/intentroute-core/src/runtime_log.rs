//! Classification and filtering of the redacted sing-box console lines
//! surfaced by the managed runtime (parity slice 11), ported from the WPF
//! `RuntimeLogFilter`. Lines arrive in logrus console format: an optional
//! `yyyy-MM-dd HH:mm:ss` (or ISO `T`) timestamp followed by an uppercase
//! level token such as `INFO[0000]` or `PANIC`. Unrecognized lines are
//! treated as Info. Export snapshots are passed through
//! [`crate::singbox::redact_secrets`] a second time as defense in depth.

/// Severity ladder; the declaration order is the comparison order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    /// Stable lowercase name for UI display and persistence.
    pub fn name(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
            LogLevel::Fatal => "fatal",
        }
    }

    pub fn all() -> [LogLevel; 6] {
        [
            LogLevel::Trace,
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
            LogLevel::Fatal,
        ]
    }
}

/// One captured log line: the local capture time (already formatted) and the
/// redacted message, mirroring the WPF `RuntimeLogLineSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub time: String,
    pub message: String,
}

/// Recognizes the first level token in `line`; `None` means unrecognized
/// (the caller treats those lines as Info, like the WPF filter).
pub fn parse_level(line: &str) -> Option<LogLevel> {
    if line.is_empty() {
        return None;
    }
    let body = strip_timestamp_prefix(line);
    parse_leading_level(body)
}

/// A line passes when its level is at least `minimum_level` and, when a
/// non-blank `search` is given, the line contains it case-insensitively.
pub fn matches(line: &str, minimum_level: LogLevel, search: &str) -> bool {
    let level = parse_level(line).unwrap_or(LogLevel::Info);
    if level < minimum_level {
        return false;
    }
    let search = search.trim();
    if search.is_empty() {
        return true;
    }
    to_lowercase(line).contains(&to_lowercase(search))
}

/// Renders snapshots as `[Time] Message` lines joined with `\r\n` (the C#
/// `Environment.NewLine` on Windows) with no trailing newline; empty input
/// yields an empty string. Messages are redacted again as a second line of
/// defense.
pub fn build_export_text(lines: &[LogLine]) -> String {
    lines
        .iter()
        .map(|line| format!("[{}] {}", line.time, crate::singbox::redact_secrets(&line.message)))
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Strips an optional `yyyy-MM-dd HH:mm:ss(.fff)` / ISO-`T` prefix followed
/// by whitespace; returns `line` unchanged when no prefix matches.
fn strip_timestamp_prefix(line: &str) -> &str {
    let bytes = line.as_bytes();
    // 19 characters: yyyy-mm-dd?hh:mm:ss
    if bytes.len() < 19 {
        return line;
    }
    let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    for i in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !digit(i) {
            return line;
        }
    }
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return line;
    }
    let sep = bytes[10];
    if sep != b' ' && sep != b'T' {
        return line;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return line;
    }
    let mut rest = &line[19..];
    // Optional fractional seconds.
    if rest.starts_with('.') {
        let fraction_len = rest[1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if fraction_len > 0 {
            rest = &rest[1 + fraction_len..];
        }
    }
    let trimmed = rest.trim_start_matches([' ', '\t']);
    if trimmed.len() == rest.len() {
        return line; // timestamp must be followed by whitespace
    }
    trimmed
}

/// Matches an uppercase level token at the start of `body`; a following
/// ASCII letter rejects the token (so `Information` is not `INFO`).
fn parse_leading_level(body: &str) -> Option<LogLevel> {
    const TOKENS: [(&str, LogLevel); 7] = [
        ("TRACE", LogLevel::Trace),
        ("DEBUG", LogLevel::Debug),
        ("INFO", LogLevel::Info),
        ("WARN", LogLevel::Warn),
        ("ERROR", LogLevel::Error),
        ("FATAL", LogLevel::Fatal),
        ("PANIC", LogLevel::Fatal),
    ];
    for (token, level) in TOKENS {
        if body.starts_with(token) {
            let next = body[token.len()..].chars().next();
            if next.is_some_and(|c| c.is_ascii_alphabetic()) {
                return None;
            }
            return Some(level);
        }
    }
    None
}

fn to_lowercase(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test vectors ported from the C# RuntimeLogFilterTests.

    #[test]
    fn parse_level_recognizes_sing_box_console_tokens() {
        let cases = [
            ("INFO[0000] sing-box started", LogLevel::Info),
            ("ERROR[0003] bad", LogLevel::Error),
            ("WARN", LogLevel::Warn),
            ("2026-09-01 12:00:00 DEBUG msg", LogLevel::Debug),
            ("2026-09-01T12:00:00 INFO msg", LogLevel::Info),
            ("2026-09-01 12:00:00.123 TRACE detail", LogLevel::Trace),
            ("PANIC: boom", LogLevel::Fatal),
            ("FATAL x", LogLevel::Fatal),
        ];
        for (line, expected) in cases {
            assert_eq!(parse_level(line), Some(expected), "line: {line}");
        }
    }

    #[test]
    fn parse_level_unrecognized_falls_back_to_info() {
        assert_eq!(parse_level("started"), None);
        assert_eq!(parse_level(""), None);
    }

    #[test]
    fn parse_level_rejects_non_uppercase_or_overlong_tokens() {
        assert_eq!(parse_level("info x"), None);
        assert_eq!(parse_level("Information about the route"), None);
    }

    #[test]
    fn matches_passes_line_at_or_above_minimum_level() {
        assert!(matches("WARN outbound degraded", LogLevel::Info, ""));
        assert!(!matches("DEBUG dialing 127.0.0.1:1080", LogLevel::Info, ""));
    }

    #[test]
    fn matches_treats_unrecognized_lines_as_info() {
        assert!(!matches("started", LogLevel::Warn, ""));
        assert!(matches("started", LogLevel::Info, ""));
    }

    #[test]
    fn matches_search_is_case_insensitive() {
        assert!(matches(
            "INFO[0000] sing-box started",
            LogLevel::Trace,
            "SING-BOX"
        ));
    }

    #[test]
    fn matches_blank_search_matches_everything() {
        assert!(matches("INFO[0000] sing-box started", LogLevel::Info, ""));
        assert!(matches("INFO[0000] sing-box started", LogLevel::Info, "   "));
    }

    #[test]
    fn matches_combines_level_and_search() {
        assert!(matches(
            "ERROR[0001] inbound timeout on 127.0.0.1",
            LogLevel::Warn,
            "timeout"
        ));
        assert!(!matches(
            "DEBUG dialing 127.0.0.1:1080",
            LogLevel::Info,
            "1080"
        ));
    }

    #[test]
    fn export_text_joins_snapshots_with_crlf() {
        let lines = [
            LogLine { time: "t1".into(), message: "m1".into() },
            LogLine { time: "t2".into(), message: "m2".into() },
        ];
        assert_eq!(build_export_text(&lines), "[t1] m1\r\n[t2] m2");
    }

    #[test]
    fn export_text_empty_input_yields_empty_text() {
        assert_eq!(build_export_text(&[]), "");
    }

    #[test]
    fn export_text_preserves_non_secret_messages() {
        let lines = [LogLine { time: "12:00:00".into(), message: "INFO[0000] 无敏感内容".into() }];
        assert_eq!(build_export_text(&lines), "[12:00:00] INFO[0000] 无敏感内容");
    }

    #[test]
    fn export_text_redacts_json_passwords_as_second_line_of_defense() {
        let secret = format!("canary-{:x}", std::process::id());
        let lines = [LogLine {
            time: "12:00:01".into(),
            message: format!("ERROR[0001] config {{\"password\": \"{secret}\"}}"),
        }];
        let text = build_export_text(&lines);
        assert!(text.contains("\"password\": \"***\""), "text: {text}");
        assert!(!text.contains(&secret));
    }

    #[test]
    fn export_text_redacts_key_value_secrets() {
        let secret = format!("canary-{:x}", std::process::id());
        let lines = [LogLine {
            time: "12:00:02".into(),
            message: format!("INFO[0002] server password={secret} rejected"),
        }];
        let text = build_export_text(&lines);
        assert!(text.contains("password=***"), "text: {text}");
        assert!(!text.contains(&secret));
    }
}

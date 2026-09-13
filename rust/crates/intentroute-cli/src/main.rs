//! intentroute-cli — command-line companion for IntentRoute AI rule files.
//!
//! Subcommands (all fully local, read-only over the given files):
//!   check <rules.json>                        validate every rule's constraint lists
//!   import-preview <rules.json> [existing]    classify the import against existing rules
//!   order <rules.json>                        print the Canonical Runtime Order
//!   build-config <config.json> [--full]       emit the sing-box configuration
//!
//! <rules.json> may be either a rule export envelope ({"Rules": [...]}) or a
//! full application configuration — any JSON object with a "Rules" array is
//! accepted. Files are read as strict UTF-8 like the product's
//! `AppConfigStore.ReadStrictUtf8`; a null Rules array is rejected.
//!
//! `build-config` prints the password-redacted JSON by default. `--full`
//! prints the unredacted configuration (which contains any proxy password in
//! the source file) — intended solely to feed `sing-box check` via a file
//! redirect; do not paste it anywhere.

use intentroute_core::{canonical_order, constraint, plan_import, AppConfig, ProxyRule};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, path] if cmd == "check" => run_check(path),
        [cmd, path] if cmd == "order" => run_order(path),
        [cmd, path] if cmd == "import-preview" => run_import_preview(path, None),
        [cmd, path, existing] if cmd == "import-preview" => {
            run_import_preview(path, Some(existing))
        }
        [cmd, path] if cmd == "build-config" => run_build_config(path, false),
        [cmd, path, flag] if cmd == "build-config" && flag == "--full" => {
            run_build_config(path, true)
        }
        _ => {
            eprintln!("usage: intentroute check <rules.json>");
            eprintln!("       intentroute order <rules.json>");
            eprintln!("       intentroute import-preview <rules.json> [existing-config.json]");
            eprintln!("       intentroute build-config <config.json> [--full]");
            ExitCode::from(2)
        }
    }
}

/// Strict-UTF-8 read + extract the "Rules" array from an envelope or config.
fn load_rules(path: &str) -> Result<Vec<ProxyRule>, String> {
    let raw = std::fs::read(PathBuf::from(path))
        .map_err(|error| format!("cannot read {path}: {error}"))?;
    let text = std::str::from_utf8(&raw)
        .map_err(|_| format!("{path} is not valid UTF-8; the product rejects replacement characters"))?;
    let document: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("{path} is not valid JSON: {error}"))?;
    let rules_value = document
        .get("Rules")
        .ok_or_else(|| format!("{path} contains no Rules array"))?;
    let rules: Vec<ProxyRule> = serde_json::from_value(rules_value.clone())
        .map_err(|error| format!("Rules in {path} do not match the rule schema: {error}"))?;
    Ok(rules)
}

fn load_config(path: &str) -> Result<AppConfig, String> {
    let raw = std::fs::read(PathBuf::from(path))
        .map_err(|error| format!("cannot read {path}: {error}"))?;
    let text = std::str::from_utf8(&raw)
        .map_err(|_| format!("{path} is not valid UTF-8; the product rejects replacement characters"))?;
    serde_json::from_str(text).map_err(|error| format!("{path} does not match the config schema: {error}"))
}

fn run_build_config(path: &str, full: bool) -> ExitCode {
    let config = match load_config(path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    match intentroute_core::build_sing_box_config(&config) {
        Ok(result) => {
            let output = if full {
                result.config_json
            } else {
                result.redacted_json
            };
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            // Builder errors are already password-scrubbed on the Rust side.
            eprintln!("build failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_check(path: &str) -> ExitCode {
    let rules = match load_rules(path) {
        Ok(rules) => rules,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    let mut failures = 0usize;
    for (index, rule) in rules.iter().enumerate() {
        let errors = constraint::explain(&rule.target_hosts, &rule.target_ips, &rule.target_ports);
        if errors.is_empty() {
            println!("{:>3}  OK        {}", index + 1, rule.exe_name);
        } else {
            failures += 1;
            println!("{:>3}  INVALID   {}  [{}]", index + 1, rule.exe_name, errors.join(", "));
        }
    }
    println!(
        "\n{} rule(s) checked, {} invalid",
        rules.len(),
        failures
    );
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_order(path: &str) -> ExitCode {
    let rules = match load_rules(path) {
        Ok(rules) => rules,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    let ordered = canonical_order(rules);
    for (index, rule) in ordered.iter().enumerate() {
        println!(
            "{:>3}  pri {:>4}  {}  {}  {}",
            index + 1,
            rule.priority,
            rule.created_at,
            rule.exe_name,
            rule.mode.name()
        );
    }
    ExitCode::SUCCESS
}

fn run_import_preview(path: &str, existing_path: Option<&str>) -> ExitCode {
    let incoming = match load_rules(path) {
        Ok(rules) => rules,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let existing = match existing_path {
        Some(existing_path) => match load_rules(existing_path) {
            Ok(rules) => rules,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(2);
            }
        },
        None => Vec::new(),
    };

    let plan = plan_import(&existing, &incoming);
    println!(
        "{:>4}  {:<24} {}",
        "#", "process", "disposition"
    );
    for row in &plan.rows {
        println!("{:>4}  {:<24} {}", row.index, row.exe_name, row.disposition.as_str());
    }
    println!(
        "\nadd: {}   skip-existing: {}   skip-duplicate-in-file: {}",
        plan.add_count, plan.skip_existing_count, plan.skip_duplicate_in_file_count
    );
    if plan.has_additions() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

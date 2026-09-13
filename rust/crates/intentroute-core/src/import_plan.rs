//! Import classification, ported from `RuleImportPlanner.cs`.
//!
//! A rule whose full identity already exists in the current configuration is
//! skipped; a later rule whose identity duplicates an earlier rule **in the
//! same import file** is skipped as an in-file duplicate; everything else is
//! added. Identity comparison is case-insensitive over the full identity key,
//! exactly like the C# `HashSet<StringComparer.OrdinalIgnoreCase>`.

use crate::identity::rule_identity_key;
use crate::rule::ProxyRule;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportDisposition {
    Add,
    SkipExisting,
    SkipDuplicateInFile,
}

impl ImportDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportDisposition::Add => "add",
            ImportDisposition::SkipExisting => "skip-existing",
            ImportDisposition::SkipDuplicateInFile => "skip-duplicate-in-file",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportPreviewRow {
    pub index: usize,
    pub exe_name: String,
    pub disposition: ImportDisposition,
}

#[derive(Debug, Clone)]
pub struct ImportPlan {
    pub rows: Vec<ImportPreviewRow>,
    pub rules_to_add: Vec<ProxyRule>,
    pub add_count: usize,
    pub skip_existing_count: usize,
    pub skip_duplicate_in_file_count: usize,
}

impl ImportPlan {
    pub fn skip_count(&self) -> usize {
        self.skip_existing_count + self.skip_duplicate_in_file_count
    }

    pub fn has_additions(&self) -> bool {
        self.add_count > 0
    }
}

pub fn plan_import(existing: &[ProxyRule], incoming: &[ProxyRule]) -> ImportPlan {
    let mut existing_keys: HashSet<String> = HashSet::new();
    for rule in existing {
        existing_keys.insert(rule_identity_key(rule).to_lowercase());
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut rows = Vec::with_capacity(incoming.len());
    let mut rules_to_add = Vec::new();
    let mut skip_existing = 0usize;
    let mut skip_in_file = 0usize;

    for (index, rule) in incoming.iter().enumerate() {
        let key = rule_identity_key(rule).to_lowercase();
        let disposition = if existing_keys.contains(&key) {
            skip_existing += 1;
            ImportDisposition::SkipExisting
        } else if !seen.insert(key) {
            skip_in_file += 1;
            ImportDisposition::SkipDuplicateInFile
        } else {
            rules_to_add.push(rule.clone());
            ImportDisposition::Add
        };
        rows.push(ImportPreviewRow {
            index: index + 1,
            exe_name: rule.exe_name.clone(),
            disposition,
        });
    }

    ImportPlan {
        add_count: rules_to_add.len(),
        rules_to_add,
        skip_existing_count: skip_existing,
        skip_duplicate_in_file_count: skip_in_file,
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::ProxyMode;

    fn rule(exe: &str, hosts: &str, mode: ProxyMode) -> ProxyRule {
        let mut r = ProxyRule::new(format!("id-{}", exe), exe);
        r.target_hosts = hosts.to_string();
        r.mode = mode;
        r
    }

    // Port of RuleImportPreviewTests.Preview_ClassifiesAddSkipExistingAndInFileDuplicates.
    #[test]
    fn classifies_add_skip_existing_and_in_file_duplicates() {
        let existing = vec![rule("chrome.exe", "github.com", ProxyMode::Proxy)];
        let incoming = vec![
            rule("chrome.exe", "github.com", ProxyMode::Proxy),
            rule("chrome.exe", "openai.com", ProxyMode::Proxy),
            rule("chrome.exe", "openai.com", ProxyMode::Proxy),
            rule("curl.exe", "", ProxyMode::Direct),
        ];

        let plan = plan_import(&existing, &incoming);

        assert_eq!(plan.add_count, 2);
        assert_eq!(plan.skip_existing_count, 1);
        assert_eq!(plan.skip_duplicate_in_file_count, 1);
        assert_eq!(plan.skip_count(), 2);
        assert!(plan.has_additions());
        assert_eq!(
            plan.rows.iter().map(|r| r.disposition).collect::<Vec<_>>(),
            vec![
                ImportDisposition::SkipExisting,
                ImportDisposition::Add,
                ImportDisposition::SkipDuplicateInFile,
                ImportDisposition::Add,
            ]
        );
        assert_eq!(
            plan.rules_to_add.iter().map(|r| r.exe_name.clone()).collect::<Vec<_>>(),
            vec!["chrome.exe", "curl.exe"]
        );
    }

    #[test]
    fn empty_incoming_yields_nothing_to_add() {
        let plan = plan_import(&[rule("a.exe", "", ProxyMode::Proxy)], &[]);
        assert_eq!(plan.add_count, 0);
        assert!(!plan.has_additions());
        assert!(plan.rows.is_empty());
    }

    #[test]
    fn identity_equivalence_drives_skip_existing() {
        let existing = vec![rule("chrome.exe", "GitHub.com, *.github.com", ProxyMode::Proxy)];
        let incoming = vec![rule("CHROME.EXE", "*.github.com, github.com", ProxyMode::Proxy)];
        let plan = plan_import(&existing, &incoming);
        assert_eq!(plan.skip_existing_count, 1);
        assert_eq!(plan.add_count, 0);
    }
}

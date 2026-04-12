//! Diagnostics and strict-mode reporting for semantic rules.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct RuleReport {
    pub matched: usize,
    pub applied: usize,
    pub already_applied: usize,
    pub unsupported_sites: Vec<String>,
    pub missing_expected: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct SemanticDiagnostics {
    pub rules: BTreeMap<String, RuleReport>,
    pub changed_files: BTreeSet<PathBuf>,
}

impl SemanticDiagnostics {
    pub fn mark_changed(&mut self, path: &Path) {
        self.changed_files.insert(path.to_path_buf());
    }

    pub fn matched(&mut self, rule: &str) {
        self.rules.entry(rule.to_string()).or_default().matched += 1;
    }

    pub fn applied(&mut self, rule: &str) {
        self.rules.entry(rule.to_string()).or_default().applied += 1;
    }

    pub fn already_applied(&mut self, rule: &str) {
        self.rules
            .entry(rule.to_string())
            .or_default()
            .already_applied += 1;
    }

    pub fn unsupported(&mut self, rule: &str, site: impl Into<String>) {
        self.rules
            .entry(rule.to_string())
            .or_default()
            .unsupported_sites
            .push(site.into());
    }

    pub fn missing_expected(&mut self, rule: &str, site: impl Into<String>) {
        self.rules
            .entry(rule.to_string())
            .or_default()
            .missing_expected
            .push(site.into());
    }

    pub fn strict_failures(&self) -> Vec<String> {
        let mut failures = Vec::new();
        for (rule, report) in &self.rules {
            for site in &report.unsupported_sites {
                failures.push(format!("{rule}: unsupported site: {site}"));
            }
            for site in &report.missing_expected {
                failures.push(format!("{rule}: expected site missing: {site}"));
            }
        }
        failures
    }
}

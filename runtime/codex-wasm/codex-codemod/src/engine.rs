//! Transform engine: applies transforms to the source tree.

use crate::transform::{Transform, TransformResult};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;
use walkdir::WalkDir;

/// Configuration for the transform engine.
pub struct TransformConfig {
    /// Whether to inject diagnostic console_log traces.
    pub diag_traces: bool,
    /// Whether missing/unsupported semantic matches should fail the run.
    pub strict: bool,
}

/// Stats from a transform run.
#[derive(Default)]
pub struct TransformStats {
    pub files_transformed: usize,
    pub files_stubbed: usize,
    pub transforms_applied: usize,
    pub transforms_already_applied: usize,
    pub transforms_not_matched: Vec<String>,
    /// Diagnostics from the semantic pass.
    pub semantic_warnings: Vec<String>,
}

/// Apply all transforms to the source tree under `codex_rs`.
pub fn apply_transforms(
    codex_rs: &Path,
    transforms: &[Transform],
    config: &TransformConfig,
) -> Result<TransformStats> {
    let mut stats = TransformStats::default();
    let mut skipped_files = HashSet::new();

    // Partition transforms by type for efficient processing
    let mut replace_files: Vec<&Transform> = Vec::new();
    let mut stub_modules: Vec<&Transform> = Vec::new();
    let mut per_file: Vec<&Transform> = Vec::new();

    for t in transforms {
        match t {
            Transform::ReplaceFile { .. } => replace_files.push(t),
            Transform::StubModule { .. } => stub_modules.push(t),
            _ => per_file.push(t),
        }
    }

    for entry in WalkDir::new(codex_rs).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();

        // Only process .rs files
        if path.extension().map(|e| e != "rs").unwrap_or(true) {
            continue;
        }

        // Phase 0: Whole-file replacements (highest priority)
        if let Some(t) = replace_files.iter().find(|t| t.matches_path(path)) {
            let (content, _) = t.apply("");
            std::fs::write(path, content)
                .with_context(|| format!("replacing {}", path.display()))?;
            stats.files_stubbed += 1;
            skipped_files.insert(path.to_path_buf());
            println!("  [replace] {}", path.display());
            continue;
        }

        // Phase 1: Stub modules
        if let Some(t) = stub_modules.iter().find(|t| t.matches_path(path)) {
            let (content, _) = t.apply("");
            std::fs::write(path, content)
                .with_context(|| format!("stubbing {}", path.display()))?;
            stats.files_stubbed += 1;
            skipped_files.insert(path.to_path_buf());
            println!("  [stub] {}", path.display());
            continue;
        }

        // Phase 2: Read file, apply simple per-file transforms
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut modified = content.clone();

        // Apply per-file transforms (in order)
        for t in &per_file {
            if !t.matches_path(path) {
                continue;
            }
            let (new_content, result) = t.apply(&modified);
            match result {
                TransformResult::Applied => {
                    modified = new_content;
                    stats.transforms_applied += 1;
                }
                TransformResult::AlreadyApplied => {
                    stats.transforms_already_applied += 1;
                }
                TransformResult::NotMatched => {
                    let desc = t.description();
                    stats.transforms_not_matched.push(desc.clone());
                    console_log::console_warn!("  [WARN] {desc}");
                }
                TransformResult::PathNotMatched => {} // shouldn't happen after matches_path
            }
        }

        // Write back if changed
        if modified != content {
            std::fs::write(path, &modified)
                .with_context(|| format!("writing {}", path.display()))?;
            stats.files_transformed += 1;
            println!("  [text] {}", path.display());
        }
    }

    // Phase 3: semantic workspace pass.
    let semantic_stats = crate::semantic::apply(
        codex_rs,
        &skipped_files,
        &crate::semantic::rules::RuleConfig {
            diag_traces: config.diag_traces,
            strict: config.strict,
        },
    )?;
    stats.files_transformed += semantic_stats.files_changed;
    for (rule, report) in semantic_stats.diagnostics.rules {
        for site in report.unsupported_sites {
            stats
                .semantic_warnings
                .push(format!("{rule}: unsupported site: {site}"));
        }
        for site in report.missing_expected {
            stats
                .semantic_warnings
                .push(format!("{rule}: expected site missing: {site}"));
        }
    }

    Ok(stats)
}

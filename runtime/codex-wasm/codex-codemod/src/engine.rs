//! Transform engine: applies transforms to the source tree.

use crate::transform::{Transform, TransformResult};
use anyhow::{Context, Result};
use std::path::Path;
use walkdir::WalkDir;

/// Configuration for the transform engine.
pub struct TransformConfig {
    /// Whether to inject diagnostic console_log traces.
    pub diag_traces: bool,
}

/// Stats from a transform run.
#[derive(Default)]
pub struct TransformStats {
    pub files_transformed: usize,
    pub files_stubbed: usize,
    pub transforms_applied: usize,
    pub transforms_already_applied: usize,
    pub transforms_not_matched: Vec<String>,
    /// Warnings from syn-level string_replace transforms that didn't match.
    pub syn_warnings: Vec<String>,
}

/// Apply all transforms to the source tree under `codex_rs`.
pub fn apply_transforms(
    codex_rs: &Path,
    transforms: &[Transform],
    config: &TransformConfig,
) -> Result<TransformStats> {
    let mut stats = TransformStats::default();

    // Configure syn_transforms before running
    crate::syn_transforms::set_diag_traces(config.diag_traces);

    // Partition transforms by type for efficient processing
    let mut replace_files: Vec<&Transform> = Vec::new();
    let mut stub_modules: Vec<&Transform> = Vec::new();
    let mut per_file: Vec<&Transform> = Vec::new();
    let mut globals: Vec<&Transform> = Vec::new();

    for t in transforms {
        match t {
            Transform::ReplaceFile { .. } => replace_files.push(t),
            Transform::StubModule { .. } => stub_modules.push(t),
            Transform::Global { .. } => globals.push(t),
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
            println!("  [replace] {}", path.display());
            continue;
        }

        // Phase 1: Stub modules
        if let Some(t) = stub_modules.iter().find(|t| t.matches_path(path)) {
            let (content, _) = t.apply("");
            std::fs::write(path, content)
                .with_context(|| format!("stubbing {}", path.display()))?;
            stats.files_stubbed += 1;
            println!("  [stub] {}", path.display());
            continue;
        }

        // Phase 2: Read file, apply per-file and global transforms
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

        // Apply global transforms (with file path context)
        for t in &globals {
            let (new_content, result) = t.apply_with_path(&modified, Some(path));
            if result == TransformResult::Applied {
                modified = new_content;
                stats.transforms_applied += 1;
            }
        }

        // Collect syn-level warnings from string_replace calls
        stats
            .syn_warnings
            .extend(crate::syn_transforms::drain_syn_warnings());

        // Write back if changed
        if modified != content {
            std::fs::write(path, &modified)
                .with_context(|| format!("writing {}", path.display()))?;
            stats.files_transformed += 1;
            println!("  [ast] {}", path.display());
        }
    }

    Ok(stats)
}

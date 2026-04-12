//! Semantic workspace pass for codex-codemod.
//!
//! This pass loads the upstream workspace into rust-analyzer's semantic model
//! and applies formatting-preserving source edits directly to files. It owns
//! AST-style rewrites that need syntax awareness and, for some rules, symbol
//! resolution.

pub mod diag_traces;
pub mod diagnostics;
pub mod edits;
pub mod rules;
pub mod workspace;

use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use self::diagnostics::SemanticDiagnostics;
use self::rules::RuleConfig;
use self::workspace::SemanticWorkspace;

/// Stats from the semantic pass.
#[derive(Default)]
pub struct SemanticStats {
    pub files_changed: usize,
    pub diagnostics: SemanticDiagnostics,
}

/// Run the semantic pass over `codex_rs`, skipping files already fully owned by
/// whole-file replacements or stubs.
pub fn apply(
    codex_rs: &Path,
    skipped_files: &HashSet<PathBuf>,
    config: &RuleConfig,
) -> Result<SemanticStats> {
    let workspace = SemanticWorkspace::load(codex_rs)?;
    let diagnostics = rules::apply_all(&workspace, skipped_files, config)?;
    Ok(SemanticStats {
        files_changed: diagnostics.changed_files.len(),
        diagnostics,
    })
}

#[cfg(test)]
mod tests;

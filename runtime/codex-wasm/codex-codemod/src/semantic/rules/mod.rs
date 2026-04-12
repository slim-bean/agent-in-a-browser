//! Semantic rule orchestration.

mod hybrid;
mod resolved_calls;
mod resolved_paths;
mod syntax_only;

use anyhow::{bail, Result};
use ra_ap_ide::{AnalysisHost, FileId};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::semantic::diag_traces;
use crate::semantic::diagnostics::SemanticDiagnostics;
use crate::semantic::edits::{write_if_changed, FileEdits};
use crate::semantic::workspace::SemanticWorkspace;

#[derive(Debug, Clone, Copy)]
pub struct RuleConfig {
    pub diag_traces: bool,
    pub strict: bool,
}

pub fn apply_all(
    workspace: &SemanticWorkspace,
    skipped_files: &HashSet<PathBuf>,
    config: &RuleConfig,
) -> Result<SemanticDiagnostics> {
    let mut diagnostics = SemanticDiagnostics::default();

    for (file_id, path) in workspace.local_rust_files() {
        if skipped_files.contains(&path) {
            continue;
        }

        apply_file(workspace, file_id, &path, &mut diagnostics, config)?;
    }

    if config.strict {
        let failures = diagnostics.strict_failures();
        if !failures.is_empty() {
            bail!(failures.join("\n"));
        }
    }

    Ok(diagnostics)
}

fn apply_file(
    workspace: &SemanticWorkspace,
    file_id: FileId,
    path: &Path,
    diagnostics: &mut SemanticDiagnostics,
    config: &RuleConfig,
) -> Result<()> {
    let source = workspace.file_text(file_id);
    let analysis = AnalysisHost::with_database(workspace.db.clone()).analysis();
    let source_file = analysis.parse(file_id).expect("parse workspace file");
    let mut edits = FileEdits::new(path.to_path_buf(), source.clone());

    syntax_only::apply(&source_file, &mut edits, diagnostics);
    resolved_paths::apply(
        &analysis,
        workspace,
        file_id,
        &source_file,
        &mut edits,
        diagnostics,
    );
    resolved_calls::apply(
        &analysis,
        workspace,
        file_id,
        &source_file,
        &mut edits,
        diagnostics,
    );
    hybrid::apply(
        &analysis,
        workspace,
        file_id,
        &source_file,
        &mut edits,
        diagnostics,
    );
    diag_traces::apply(&mut edits, diagnostics, config.diag_traces);

    let changed = write_if_changed(path, &source, edits.apply()?)?;
    if changed {
        diagnostics.mark_changed(path);
    }

    Ok(())
}

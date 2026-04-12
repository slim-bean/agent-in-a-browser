//! Hybrid rules that need syntax shape plus targeted source rewriting.

use ra_ap_ide::{Analysis, FileId};
use ra_ap_syntax::ast;

use crate::semantic::diagnostics::SemanticDiagnostics;
use crate::semantic::edits::FileEdits;
use crate::semantic::workspace::SemanticWorkspace;

pub fn apply(
    _analysis: &Analysis,
    _workspace: &SemanticWorkspace,
    _file_id: FileId,
    _source_file: &ast::SourceFile,
    _file: &mut FileEdits,
    _diagnostics: &mut SemanticDiagnostics,
) {
    // Intentionally small for the first semantic migration slice.
    // Complex structural rewrites can move here incrementally without
    // coupling them to simple symbol-resolution rules.
}

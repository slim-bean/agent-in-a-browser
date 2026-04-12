//! Nonsemantic diagnostic trace injections.
//!
//! These are intentionally kept out of the semantic rule engine so strict mode
//! and symbol-aware rewrites remain correctness-focused.

use crate::semantic::diagnostics::SemanticDiagnostics;
use crate::semantic::edits::{FileEdits, ReplaceExactResult};

/// Apply any requested diagnostic trace injections.
pub fn apply(file: &mut FileEdits, diagnostics: &mut SemanticDiagnostics, enabled: bool) {
    if !enabled {
        return;
    }

    // Keep this isolated: the semantic migration can move individual trace
    // rewrites here without coupling them to symbol resolution.
    if file.path.ends_with("tui/src/lib.rs") {
        const RULE: &str = "diag_traces.tui_before_find_codex_home";
        match file.replace_first_exact(
            "    let codex_home = match find_codex_home() {",
            "    console_log::console_log!(\"[tui-trace] before find_codex_home\");\n    tokio::time::sleep(std::time::Duration::from_millis(1)).await;\n    let codex_home = match find_codex_home() {",
        ) {
            ReplaceExactResult::Applied => {
                diagnostics.matched(RULE);
                diagnostics.applied(RULE);
            }
            ReplaceExactResult::AlreadyApplied => {
                diagnostics.matched(RULE);
                diagnostics.already_applied(RULE);
            }
            ReplaceExactResult::NotMatched => {
                diagnostics.missing_expected(RULE, file.path.display().to_string());
            }
        }
    }
}

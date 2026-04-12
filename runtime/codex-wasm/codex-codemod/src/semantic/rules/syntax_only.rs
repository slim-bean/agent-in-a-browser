//! Syntax-only rules driven by rust-analyzer syntax trees.

use ra_ap_syntax::{ast, AstNode};

use crate::semantic::diagnostics::SemanticDiagnostics;
use crate::semantic::edits::{extend_to_line_end, extend_to_line_start, FileEdits};

const ATTR_RULE: &str = "syntax_only.attrs";
const EPRINTLN_RULE: &str = "syntax_only.eprintln";

pub fn apply(
    source_file: &ast::SourceFile,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    for attr in source_file
        .syntax()
        .descendants()
        .filter_map(ast::Attr::cast)
    {
        apply_attr_rule(&attr, file, diagnostics);
    }

    for mac in source_file
        .syntax()
        .descendants()
        .filter_map(ast::MacroCall::cast)
    {
        apply_eprintln_rule(&mac, file, diagnostics);
    }
}

fn apply_attr_rule(attr: &ast::Attr, file: &mut FileEdits, diagnostics: &mut SemanticDiagnostics) {
    let text = attr.syntax().text().to_string();
    let start = u32::from(attr.syntax().text_range().start()) as usize;
    let end = u32::from(attr.syntax().text_range().end()) as usize;

    if text.starts_with("#[tokio::main") {
        diagnostics.matched(ATTR_RULE);
        let line_start = extend_to_line_start(&file.source, start);
        let line_end = extend_to_line_end(&file.source, end);
        file.add_raw(line_start, line_end, String::new());
        diagnostics.applied(ATTR_RULE);
        return;
    }

    if text.starts_with("#[tokio::test") {
        diagnostics.matched(ATTR_RULE);
        let line_start = extend_to_line_start(&file.source, start);
        let indent = &file.source[line_start..start];
        let line_end = extend_to_line_end(&file.source, end);
        file.add_raw(line_start, line_end, format!("{indent}#[test]\n"));
        diagnostics.applied(ATTR_RULE);
        return;
    }

    if text.starts_with("#[ts(") || text.starts_with("#[ts_rs::TS(") {
        diagnostics.matched(ATTR_RULE);
        let line_start = extend_to_line_start(&file.source, start);
        let line_end = extend_to_line_end(&file.source, end);
        file.add_raw(line_start, line_end, String::new());
        diagnostics.applied(ATTR_RULE);
        return;
    }

    if text.starts_with("#[derive(") && (text.contains("TS") || text.contains("ts_rs::TS")) {
        diagnostics.matched(ATTR_RULE);
        let inner = text
            .trim_start_matches("#[derive(")
            .trim_end_matches(")]")
            .split(',')
            .map(str::trim)
            .filter(|item| !matches!(*item, "TS" | "ts_rs::TS" | "ts_rs :: TS" | "TS " | " TS"))
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        let replacement = if inner.is_empty() {
            String::new()
        } else {
            format!("#[derive({})]", inner.join(", "))
        };
        if replacement.is_empty() {
            let line_start = extend_to_line_start(&file.source, start);
            let line_end = extend_to_line_end(&file.source, end);
            file.add_raw(line_start, line_end, replacement);
        } else {
            file.add_raw(start, end, replacement);
        }
        diagnostics.applied(ATTR_RULE);
    }
}

fn apply_eprintln_rule(
    mac: &ast::MacroCall,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    if !file.source.contains("use tracing") && !file.source.contains("tracing::") {
        return;
    }
    let Some(path) = mac.path() else {
        return;
    };
    if path.syntax().text().to_string() != "eprintln" {
        return;
    }
    diagnostics.matched(EPRINTLN_RULE);
    file.add_range(path.syntax().text_range(), "tracing::error");
    diagnostics.applied(EPRINTLN_RULE);
}

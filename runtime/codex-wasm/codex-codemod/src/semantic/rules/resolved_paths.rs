//! Rules that depend on resolved paths.

use ra_ap_ide::{Analysis, FileId, FilePosition, GotoDefinitionConfig, RaFixtureConfig};
use ra_ap_syntax::{ast, AstNode};

use crate::semantic::diagnostics::SemanticDiagnostics;
use crate::semantic::edits::FileEdits;
use crate::semantic::workspace::SemanticWorkspace;

const THREAD_RULE: &str = "resolved_paths.thread_api";
const TYPE_RULE: &str = "resolved_paths.type_paths";

pub fn apply(
    analysis: &Analysis,
    workspace: &SemanticWorkspace,
    file_id: FileId,
    source_file: &ast::SourceFile,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    for path in source_file
        .syntax()
        .descendants()
        .filter_map(ast::Path::cast)
    {
        apply_thread_rule(analysis, workspace, file_id, &path, file, diagnostics);
        apply_type_path_rules(analysis, workspace, file_id, &path, file, diagnostics);
    }
}

fn apply_thread_rule(
    analysis: &Analysis,
    workspace: &SemanticWorkspace,
    file_id: FileId,
    path: &ast::Path,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    let Some(target) = goto_path_target(analysis, file_id, workspace, path) else {
        return;
    };
    let replacement = match target.name.as_str() {
        "spawn" if target.definition_path.contains("/thread") => {
            Some(rewrite_thread_prefix(path, "spawn"))
        }
        "sleep" if target.definition_path.contains("/thread") => {
            Some(rewrite_thread_prefix(path, "sleep"))
        }
        "Builder" if target.definition_path.contains("/thread") => {
            Some(rewrite_thread_prefix(path, "Builder"))
        }
        _ => None,
    };
    let Some(replacement) = replacement else {
        return;
    };
    diagnostics.matched(THREAD_RULE);
    file.add_range(path.syntax().text_range(), replacement);
    diagnostics.applied(THREAD_RULE);
}

fn apply_type_path_rules(
    analysis: &Analysis,
    workspace: &SemanticWorkspace,
    file_id: FileId,
    path: &ast::Path,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    let Some(target) = goto_path_target(analysis, file_id, workspace, path) else {
        return;
    };
    let replacement = if target.name == "ExitStatus"
        && target.definition_path.contains("/process")
        && (file.path.ends_with("core/src/exec.rs")
            || file.path.ends_with("core/src/tools/js_repl/mod.rs"))
    {
        Some("tokio::process::ExitStatus")
    } else if target.name == "Output"
        && target.definition_path.contains("/process")
        && (file.path.ends_with("core/src/git_info.rs")
            || file.path.ends_with("git-utils/src/info.rs"))
    {
        Some("tokio::process::Output")
    } else if target.name == "IsolateHandle" && file.path.ends_with("code-mode/src/service.rs") {
        Some("crate::runtime::RuntimeHandle")
    } else {
        None
    };
    let Some(replacement) = replacement else {
        return;
    };

    diagnostics.matched(TYPE_RULE);
    file.add_range(path.syntax().text_range(), replacement);
    diagnostics.applied(TYPE_RULE);
}

struct DefinitionTarget {
    name: String,
    definition_path: String,
}

fn goto_path_target(
    analysis: &Analysis,
    file_id: FileId,
    workspace: &SemanticWorkspace,
    path: &ast::Path,
) -> Option<DefinitionTarget> {
    let segment = path.segment()?;
    let name_ref = segment.name_ref()?;
    let config = GotoDefinitionConfig {
        ra_fixture: RaFixtureConfig::default(),
    };
    let nav = analysis
        .goto_definition(
            FilePosition {
                file_id,
                offset: name_ref.syntax().text_range().start(),
            },
            &config,
        )
        .ok()
        .flatten()?
        .info
        .into_iter()
        .next()?;

    Some(DefinitionTarget {
        name: nav.name.as_str().to_string(),
        definition_path: workspace
            .path_for_file_id(nav.file_id)
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    })
}

fn rewrite_thread_prefix(path: &ast::Path, leaf: &str) -> String {
    let text = path.syntax().text().to_string();
    if text.starts_with("std::thread::") || text.starts_with("thread::") {
        format!("tokio::thread_spawn::{leaf}")
    } else {
        text
    }
}

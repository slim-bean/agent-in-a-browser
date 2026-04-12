//! Rules that depend on semantic call resolution.

use ra_ap_ide::{Analysis, FileId, FilePosition, GotoDefinitionConfig, RaFixtureConfig};
use ra_ap_syntax::{
    ast::{self, HasArgList},
    AstNode,
};

use crate::semantic::diagnostics::SemanticDiagnostics;
use crate::semantic::edits::{
    extend_to_line_end, extend_to_line_start, text_size_to_usize, FileEdits, ReplaceExactResult,
};
use crate::semantic::workspace::SemanticWorkspace;

const LOCK_RULE: &str = "resolved_calls.advisory_file_locks";
const PROCESS_EXIT_RULE: &str = "resolved_calls.process_exit";
const WHICH_RULE: &str = "resolved_calls.which_calls";

pub fn apply(
    analysis: &Analysis,
    workspace: &SemanticWorkspace,
    file_id: FileId,
    source_file: &ast::SourceFile,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    for method_call in source_file
        .syntax()
        .descendants()
        .filter_map(ast::MethodCallExpr::cast)
    {
        apply_lock_rule(
            analysis,
            workspace,
            file_id,
            &method_call,
            file,
            diagnostics,
        );
    }

    for call in source_file
        .syntax()
        .descendants()
        .filter_map(ast::CallExpr::cast)
    {
        apply_process_exit_rule(analysis, workspace, file_id, &call, file, diagnostics);
        apply_which_rule(analysis, workspace, file_id, &call, file, diagnostics);
    }
}

fn apply_lock_rule(
    analysis: &Analysis,
    workspace: &SemanticWorkspace,
    file_id: FileId,
    method_call: &ast::MethodCallExpr,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    let Some(name_ref) = method_call.name_ref() else {
        return;
    };
    let name = name_ref.syntax().text().to_string();
    if !matches!(
        name.as_str(),
        "lock" | "try_lock" | "lock_shared" | "try_lock_shared"
    ) {
        return;
    }

    let target = goto_first_target(
        analysis,
        file_id,
        workspace,
        name_ref.syntax().text_range().start(),
    );
    let targeted_file = file.path.ends_with("core/src/installation_id.rs")
        || file.path.ends_with("execpolicy/src/amend.rs")
        || file.path.ends_with("core/src/message_history.rs")
        || file.path.ends_with("arg0/src/lib.rs");
    let looks_like_file_lock = target
        .as_ref()
        .map(|target| target.definition_path.contains("/fs"))
        .unwrap_or(targeted_file);
    if !looks_like_file_lock {
        return;
    }

    diagnostics.matched(LOCK_RULE);

    let outcome = if file.path.ends_with("core/src/installation_id.rs") {
        guard_method_call_statement_with_cfg(
            method_call,
            file,
            "#[cfg(not(target_arch = \"wasm32\"))]",
        )
    } else if file.path.ends_with("execpolicy/src/amend.rs") {
        let outcome = guard_method_call_statement_with_cfg(
            method_call,
            file,
            "#[cfg(not(target_arch = \"wasm32\"))]",
        );
        if matches!(
            outcome,
            ReplaceExactResult::Applied | ReplaceExactResult::AlreadyApplied
        ) {
            ensure_comment_once_before_guard(
                file,
                "    // Skip file locking on WASM — WASI descriptor.lock() is not supported.\n    // Safe: single-threaded WASM has no contention on policy files.\n",
                "    #[cfg(not(target_arch = \"wasm32\"))]\n",
            )
        } else {
            outcome
        }
    } else if file.path.ends_with("core/src/message_history.rs") {
        match name.as_str() {
            "try_lock" => file.replace_first_exact(
                "    tokio::task::spawn_blocking(move || -> Result<()> {\n        // Retry a few times to avoid indefinite blocking when contended.\n        for _ in 0..MAX_RETRIES {\n            match history_file.try_lock() {\n                Ok(()) => {\n                    // While holding the exclusive lock, write the full line.\n                    // We do not open the file with `append(true)` on Windows, so ensure the\n                    // cursor is positioned at the end before writing.\n                    history_file.seek(SeekFrom::End(0))?;\n                    history_file.write_all(line.as_bytes())?;\n                    history_file.flush()?;\n                    enforce_history_limit(&mut history_file, history_max_bytes)?;\n                    return Ok(());\n                }\n                Err(std::fs::TryLockError::WouldBlock) => {\n                    std::thread::sleep(RETRY_SLEEP);\n                }\n                Err(e) => return Err(e.into()),\n            }\n        }\n\n        Err(std::io::Error::new(\n            std::io::ErrorKind::WouldBlock,\n            \"could not acquire exclusive lock on history file after multiple attempts\",\n        ))\n    })\n    .await??;",
                "    // [codex-codemod] File::try_lock not supported in WASI — write directly (no contention in single-threaded WASM)\n    tokio::task::spawn_blocking(move || -> Result<()> {\n        history_file.seek(SeekFrom::End(0))?;\n        history_file.write_all(line.as_bytes())?;\n        history_file.flush()?;\n        enforce_history_limit(&mut history_file, history_max_bytes)?;\n        Ok(())\n    })\n    .await??;",
            ),
            "try_lock_shared" => file.replace_first_exact(
                "    // Open & lock file for reading using a shared lock.\n    // Retry a few times to avoid indefinite blocking.\n    for _ in 0..MAX_RETRIES {\n        let lock_result = file.try_lock_shared();\n\n        match lock_result {\n            Ok(()) => {\n                let reader = BufReader::new(&file);\n                for (idx, line_res) in reader.lines().enumerate() {\n                    let line = match line_res {\n                        Ok(l) => l,\n                        Err(e) => {\n                            tracing::warn!(error = %e, \"failed to read line from history file\");\n                            return None;\n                        }\n                    };\n\n                    if idx == offset {\n                        match serde_json::from_str::<HistoryEntry>(&line) {\n                            Ok(entry) => return Some(entry),\n                            Err(e) => {\n                                tracing::warn!(error = %e, \"failed to parse history entry\");\n                                return None;\n                            }\n                        }\n                    }\n                }\n                // Not found at requested offset.\n                return None;\n            }\n            Err(std::fs::TryLockError::WouldBlock) => {\n                std::thread::sleep(RETRY_SLEEP);\n            }\n            Err(e) => {\n                tracing::warn!(error = %e, \"failed to acquire shared lock on history file\");\n                return None;\n            }\n        }\n    }\n\n    None",
                "    // [codex-codemod] File::try_lock_shared not supported in WASI — read directly\n    {\n        let reader = BufReader::new(&file);\n        for (idx, line_res) in reader.lines().enumerate() {\n            let line = match line_res {\n                Ok(l) => l,\n                Err(e) => {\n                    tracing::warn!(error = %e, \"failed to read line from history file\");\n                    return None;\n                }\n            };\n\n            if idx == offset {\n                match serde_json::from_str::<HistoryEntry>(&line) {\n                    Ok(entry) => return Some(entry),\n                    Err(e) => {\n                        tracing::warn!(error = %e, \"failed to parse history entry\");\n                        return None;\n                    }\n                }\n            }\n        }\n        return None;\n    }",
            ),
            _ => ReplaceExactResult::AlreadyApplied,
        }
    } else if file.path.ends_with("arg0/src/lib.rs") {
        if enclosing_match_expr(method_call).is_some() {
            rewrite_arg0_try_lock_match(file)
        } else {
            guard_method_call_statement_with_cfg(
                method_call,
                file,
                "#[cfg(not(target_arch = \"wasm32\"))]",
            )
        }
    } else {
        ReplaceExactResult::NotMatched
    };

    match outcome {
        ReplaceExactResult::Applied => diagnostics.applied(LOCK_RULE),
        ReplaceExactResult::AlreadyApplied => diagnostics.already_applied(LOCK_RULE),
        ReplaceExactResult::NotMatched => {
            diagnostics.unsupported(LOCK_RULE, format!("{}::{name}", file.path.display()))
        }
    }
}

fn guard_method_call_statement_with_cfg(
    method_call: &ast::MethodCallExpr,
    file: &mut FileEdits,
    cfg_attr: &str,
) -> ReplaceExactResult {
    let Some(stmt) = enclosing_statement(method_call) else {
        return ReplaceExactResult::NotMatched;
    };

    let stmt_start = extend_to_line_start(
        &file.source,
        text_size_to_usize(method_call.syntax().text_range().start()),
    );
    let stmt_end = extend_to_line_end(&file.source, text_size_to_usize(stmt.text_range().end()));
    let stmt_text = &file.source[stmt_start..stmt_end];
    let indent_len = stmt_text
        .chars()
        .take_while(|ch| ch.is_ascii_whitespace() && *ch != '\n')
        .count();
    let indent = &stmt_text[..indent_len];
    let cfg_line = format!("{indent}{cfg_attr}\n");
    let before = &file.source[..stmt_start];

    if before.ends_with(&(cfg_line.clone() + &cfg_line)) {
        file.add_raw(
            stmt_start - (cfg_line.len() * 2),
            stmt_end,
            format!("{cfg_line}{stmt_text}"),
        );
        return ReplaceExactResult::Applied;
    }

    if before.ends_with(&cfg_line) {
        return ReplaceExactResult::AlreadyApplied;
    }

    file.add_raw(stmt_start, stmt_end, format!("{cfg_line}{stmt_text}"));
    ReplaceExactResult::Applied
}

fn ensure_comment_once_before_guard(
    file: &mut FileEdits,
    comment_block: &str,
    guard_line: &str,
) -> ReplaceExactResult {
    let needle = format!("{comment_block}{guard_line}");
    if file.source.contains(&needle) {
        return ReplaceExactResult::AlreadyApplied;
    }

    let Some(start) = file.source.find(guard_line) else {
        return ReplaceExactResult::NotMatched;
    };
    file.add_raw(start, start, comment_block.to_string());
    ReplaceExactResult::Applied
}

fn rewrite_arg0_try_lock_match(file: &mut FileEdits) -> ReplaceExactResult {
    let find = "    match lock_file.try_lock() {\n        Ok(()) => Ok(Some(lock_file)),\n        Err(std::fs::TryLockError::WouldBlock) => Ok(None),\n        Err(err) => Err(err.into()),\n    }\n";
    let replace = "    #[cfg(target_arch = \"wasm32\")]\n    {\n        return Ok(Some(lock_file));\n    }\n\n    #[cfg(not(target_arch = \"wasm32\"))]\n    match lock_file.try_lock() {\n        Ok(()) => Ok(Some(lock_file)),\n        Err(std::fs::TryLockError::WouldBlock) => Ok(None),\n        Err(err) => Err(err.into()),\n    }\n";
    if file.source.contains(replace) {
        return ReplaceExactResult::AlreadyApplied;
    }
    file.replace_first_exact(find, replace)
}

fn enclosing_statement(method_call: &ast::MethodCallExpr) -> Option<ra_ap_syntax::SyntaxNode> {
    method_call
        .syntax()
        .ancestors()
        .find(|node| ast::Stmt::can_cast(node.kind()))
}

fn enclosing_match_expr(method_call: &ast::MethodCallExpr) -> Option<ast::MatchExpr> {
    method_call
        .syntax()
        .ancestors()
        .find_map(ast::MatchExpr::cast)
}

fn apply_process_exit_rule(
    analysis: &Analysis,
    workspace: &SemanticWorkspace,
    file_id: FileId,
    call: &ast::CallExpr,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    let Some(target) = goto_path_call_target(analysis, file_id, workspace, call) else {
        return;
    };
    if target.name.as_str() != "exit" || !target.definition_path.contains("/process") {
        return;
    }
    diagnostics.matched(PROCESS_EXIT_RULE);

    let args_text = call
        .arg_list()
        .map(|args| args.syntax().text().to_string())
        .unwrap_or_else(|| "()".to_string());
    let arg = args_text
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim()
        .to_string();

    let replacement = format!(
        "panic!(\"process::exit({}) called — cannot exit in WASM\")",
        if arg.is_empty() { "?" } else { arg.as_str() }
    );
    file.add_node(call, replacement);
    diagnostics.applied(PROCESS_EXIT_RULE);
}

fn apply_which_rule(
    analysis: &Analysis,
    workspace: &SemanticWorkspace,
    file_id: FileId,
    call: &ast::CallExpr,
    file: &mut FileEdits,
    diagnostics: &mut SemanticDiagnostics,
) {
    let Some(target) = goto_path_call_target(analysis, file_id, workspace, call) else {
        return;
    };

    let replacement = match target.name.as_str() {
        "which" if target.definition_path.contains("/which-") => {
            diagnostics.matched(WHICH_RULE);
            Some("(|| -> Result<std::path::PathBuf, ()> { Err(()) })()")
        }
        "which_in" if target.definition_path.contains("/which-") => {
            diagnostics.matched(WHICH_RULE);
            Some("Err::<std::path::PathBuf, String>(\"which not available in WASM\".into())")
        }
        _ => None,
    };
    let Some(replacement) = replacement else {
        return;
    };

    file.add_node(call, replacement);
    diagnostics.applied(WHICH_RULE);
}

struct DefinitionTarget {
    name: String,
    definition_path: String,
}

fn goto_path_call_target(
    analysis: &Analysis,
    file_id: FileId,
    workspace: &SemanticWorkspace,
    call: &ast::CallExpr,
) -> Option<DefinitionTarget> {
    let expr = call.expr()?;
    let path_expr = ast::PathExpr::cast(expr.syntax().clone())?;
    let path = path_expr.path()?;
    let segment = path.segment()?;
    let name_ref = segment.name_ref()?;
    goto_first_target(
        analysis,
        file_id,
        workspace,
        name_ref.syntax().text_range().start(),
    )
}

fn goto_first_target(
    analysis: &Analysis,
    file_id: FileId,
    workspace: &SemanticWorkspace,
    offset: ra_ap_ide::TextSize,
) -> Option<DefinitionTarget> {
    let config = GotoDefinitionConfig {
        ra_fixture: RaFixtureConfig::default(),
    };
    let nav = analysis
        .goto_definition(FilePosition { file_id, offset }, &config)
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

//! Span-based transforms using `syn` (immutable visit) + direct source editing.
//!
//! Instead of mutating the AST and re-printing with `prettyplease`, we:
//! 1. Parse with `syn::parse_file` (with proc_macro2 span-locations enabled)
//! 2. Walk with `syn::visit::Visit` (immutable) to COLLECT edits as
//!    `(byte_start, byte_end, replacement_text)` tuples
//! 3. Apply edits to the original source string in reverse byte-offset order
//!
//! This preserves all original formatting except at the exact edit sites.
//!
//! Handles:
//! 1. Strip `use ts_rs::TS;` / `use ts_rs::*;` imports
//! 2. Strip TS from `#[derive(...)]` and remove empty derives
//! 3. Strip `#[ts(...)]` and `#[ts_rs::TS(...)]` attributes
//! 4. Turbofish `query_as::<_, T>` → `query_as::<T>`
//! 5. `#[tokio::main]` → remove, `#[tokio::test]` → `#[test]`
//! 6. Thread path rewrites: `std::thread::spawn` → `tokio::thread_spawn::spawn`
//! 7. `codex_chatgpt` → `codex_core` / `merge_connectors_with_accessible` → `merge_plugin_apps_with_accessible`
//! 8. `sqlx::migrate!(...)` → `sqlx::migrate::Migrator::new(...)`
//! 9. TS stripping from macro_rules bodies
//! 10. select! body cleaning (biased;, if guards, else=>)
//! 11. File-specific path renames: v8::IsolateHandle, std::process::*, which::which*
//! 12. File-specific use rewrites: std::process → tokio::process, cfg-gated removal
//! 13. File-specific `mut` additions, `#[cfg]` modifications, expression edits

use std::path;
use syn::visit::Visit;
use syn::{
    Attribute, Expr, File, GenericArgument, ItemUse, Path, PathArguments, PathSegment, Type,
    UseTree,
};

/// Maps proc_macro2 line/column spans to byte offsets in the original source.
struct SourceMap {
    line_offsets: Vec<usize>,
}

impl SourceMap {
    fn new(source: &str) -> Self {
        let mut offsets = vec![0];
        for (i, ch) in source.char_indices() {
            if ch == '\n' {
                offsets.push(i + 1);
            }
        }
        Self {
            line_offsets: offsets,
        }
    }

    fn offset(&self, lc: proc_macro2::LineColumn) -> usize {
        let line_idx = lc.line.saturating_sub(1);
        self.line_offsets
            .get(line_idx)
            .map(|o| o + lc.column)
            .unwrap_or(0)
    }
}

/// A single edit to apply to the source string.
#[derive(Debug, Clone)]
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

/// Apply all span-based transforms to Rust source code.
///
/// `file_path` is optional; when provided, file-specific transforms are applied
/// (path renames, use rewrites, mut additions, cfg modifications).
///
/// Returns `Some(transformed)` if any changes were made, `None` otherwise.
/// On parse failure, returns `None` so the caller can fall back to regex transforms.
pub fn apply(content: &str) -> Option<String> {
    apply_with_path(content, None)
}

/// Prepend text entries — `#![allow(...)]` attributes added to file tops.
/// (file_path_suffix, text_to_prepend)
const PREPEND_TEXT: &[(&str, &str)] = &[
    ("code-mode/src/lib.rs", "#![allow(unreachable_code, unused_variables, unused_mut, dead_code, unused_imports, unused_assignments)]\n"),
    ("state/src/lib.rs", "#![allow(unused_variables, unused_mut, unused_imports, dead_code)]\n"),
    ("hooks/src/lib.rs", "#![allow(unused_variables, unused_mut, unused_imports)]\n"),
    ("artifacts/src/lib.rs", "#![allow(unused_imports)]\n"),
    ("app-server-protocol/src/lib.rs", "#![allow(dead_code, unused_imports)]\n"),
    ("codex-client/src/lib.rs", "#![allow(dead_code, unused_imports)]\n"),
    ("package-manager/src/lib.rs", "#![allow(dead_code, unused_imports)]\n"),
    ("shell-command/src/lib.rs", "#![allow(dead_code, unused_variables, unused_imports)]\n"),
    ("file-search/src/lib.rs", "#![allow(unused_imports, dead_code, unused_variables, unreachable_code)]\n"),
    ("core/src/lib.rs", "#![allow(unreachable_code, unused_variables, unused_mut, dead_code, unused_imports, unused_assignments)]\n"),
    ("async-utils/src/lib.rs", "#![allow(unused_variables, unused_imports)]\n"),
    ("arg0/src/lib.rs", "#![allow(dead_code, unused_variables)]\n"),
    ("feedback/src/lib.rs", "#![allow(dead_code, unused_imports)]\n"),
    ("tui/src/lib.rs", "#![allow(unexpected_cfgs, unused_imports, unused_variables, unused_mut, dead_code, unused_assignments, unused_attributes)]\n"),
];

/// Comment-out-lines entries — lines starting with these prefixes get commented out.
/// (file_path_suffix, &[line_prefix])
const COMMENT_OUT_LINES: &[(&str, &[&str])] = &[(
    "core/src/config_loader/macos.rs",
    &["use core_foundation::"],
)];

/// Apply all span-based transforms to Rust source code, with optional file-path context
/// for file-specific transforms.
pub fn apply_with_path(content: &str, file_path: Option<&path::Path>) -> Option<String> {
    let mut prepend = String::new();
    let mut needs_comment_out: Option<&[&str]> = None;

    // Check prepend text
    if let Some(fp) = file_path {
        for (suffix, text) in PREPEND_TEXT {
            if fp.ends_with(path::Path::new(suffix)) && !content.contains(text) {
                prepend.push_str(text);
            }
        }
        for (suffix, prefixes) in COMMENT_OUT_LINES {
            if fp.ends_with(path::Path::new(suffix)) {
                needs_comment_out = Some(prefixes);
            }
        }
    }

    let file: File = match syn::parse_file(content) {
        Ok(f) => f,
        Err(_) => {
            // Even on parse failure, we can still apply prepend/comment-out
            let mut changed = false;
            let mut result = content.to_string();
            if !prepend.is_empty() {
                result = format!("{prepend}{result}");
                changed = true;
            }
            if let Some(prefixes) = needs_comment_out {
                let commented = comment_out_lines(&result, prefixes);
                if commented != result {
                    result = commented;
                    changed = true;
                }
            }
            return if changed { Some(result) } else { None };
        }
    };

    let source_map = SourceMap::new(content);
    let mut collector = EditCollector {
        source: content,
        source_map: &source_map,
        file_path,
        edits: Vec::new(),
    };

    collector.visit_file(&file);

    // AST-informed source-text replacements for code blocks that are hard to match
    // structurally in syn (multi-statement blocks, method chains, etc.)
    collector.collect_code_block_replacements();

    // Diagnostic traces and string-level replacements
    collector.collect_string_replacement_edits();

    // TUI-specific transforms: import stubs, type fixes, pattern rewrites, etc.
    collector.collect_tui_specific_edits();

    let has_edits = !collector.edits.is_empty();
    let has_prepend = !prepend.is_empty();
    let has_comment_out = needs_comment_out.is_some();

    if !has_edits && !has_prepend && !has_comment_out {
        return None;
    }

    // Sort edits: larger edits (by span size) first, then by start descending.
    // This ensures encompassing edits take priority over smaller ones they contain.
    collector.edits.sort_by(|a, b| {
        let size_a = a.end - a.start;
        let size_b = b.end - b.start;
        size_b.cmp(&size_a).then(b.start.cmp(&a.start))
    });

    // Deduplicate: accept each edit only if it doesn't overlap with any already-accepted edit.
    let mut filtered_edits: Vec<Edit> = Vec::new();
    for edit in collector.edits {
        let overlaps = filtered_edits.iter().any(|accepted| {
            // Two ranges overlap if neither is entirely before or after the other
            edit.start < accepted.end && edit.end > accepted.start
        });
        if !overlaps {
            filtered_edits.push(edit);
        }
    }

    // Re-sort descending by start for safe application (end-to-start)
    filtered_edits.sort_by(|a, b| b.start.cmp(&a.start));

    let mut result = content.to_string();
    for edit in &filtered_edits {
        let start = edit.start.min(result.len());
        let end = edit.end.min(result.len());
        result.replace_range(start..end, &edit.replacement);
    }

    // Apply prepend text
    if has_prepend {
        result = format!("{prepend}{result}");
    }

    // Apply comment-out-lines
    if let Some(prefixes) = needs_comment_out {
        let commented = comment_out_lines(&result, prefixes);
        if commented != result {
            result = commented;
        }
    }

    if result == content {
        None
    } else {
        Some(result)
    }
}

/// Comment out matching lines (prefix with `// [codex-codemod] `).
fn comment_out_lines(content: &str, prefixes: &[&str]) -> String {
    let mut result_lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if prefixes.iter().any(|prefix| trimmed.starts_with(prefix)) {
            result_lines.push(format!("// [codex-codemod] {line}"));
        } else {
            result_lines.push(line.to_string());
        }
    }
    let mut out = result_lines.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Collects edits by walking the AST immutably.
struct EditCollector<'a> {
    source: &'a str,
    source_map: &'a SourceMap,
    /// Optional file path for file-specific transforms.
    file_path: Option<&'a path::Path>,
    edits: Vec<Edit>,
}

impl<'a> EditCollector<'a> {
    /// Check if the current file path ends with the given suffix.
    fn file_matches(&self, suffix: &str) -> bool {
        self.file_path
            .map(|p| p.ends_with(path::Path::new(suffix)))
            .unwrap_or(false)
    }

    /// Get the byte range for a span.
    fn span_range(&self, span: proc_macro2::Span) -> (usize, usize) {
        let start = self.source_map.offset(span.start());
        let end = self.source_map.offset(span.end());
        (start, end)
    }

    /// Extend an end offset to include the trailing newline (for full-line removals).
    fn extend_to_line_end(&self, end: usize) -> usize {
        if let Some(pos) = self.source[end..].find('\n') {
            end + pos + 1
        } else {
            end
        }
    }

    /// Extend a start offset back to the beginning of the line (for full-line removals).
    fn extend_to_line_start(&self, start: usize) -> usize {
        if start == 0 {
            return 0;
        }
        // Search backward from start-1 for a newline
        let before = &self.source[..start];
        if let Some(pos) = before.rfind('\n') {
            // Check if everything between pos+1 and start is whitespace
            let between = &self.source[pos + 1..start];
            if between.chars().all(|c| c.is_whitespace()) {
                pos + 1
            } else {
                start
            }
        } else {
            // Beginning of file — check if prefix is all whitespace
            let between = &self.source[..start];
            if between.chars().all(|c| c.is_whitespace()) {
                0
            } else {
                start
            }
        }
    }

    /// Process attributes on any item for tokio::main, tokio::test, ts attrs, and derives.
    fn process_attrs(&mut self, attrs: &[Attribute]) {
        for attr in attrs {
            // Remove #[ts(...)] and #[ts_rs::TS(...)]
            if is_ts_attr(attr) || is_ts_rs_ts_attr(attr) {
                let (start, _) = self.span_range(attr.pound_token.span);
                let line_start = self.extend_to_line_start(start);
                // The attr span from pound_token only covers `#`. We need to find the
                // closing bracket. Use the full attribute span.
                let (_, attr_end) = self.attr_byte_range(attr);
                let line_end = self.extend_to_line_end(attr_end);
                self.edits.push(Edit {
                    start: line_start,
                    end: line_end,
                    replacement: String::new(),
                });
                continue;
            }

            // Handle #[tokio::main] — remove entirely
            if is_tokio_main_attr(attr) {
                let (start, end) = self.attr_byte_range(attr);
                let line_start = self.extend_to_line_start(start);
                let line_end = self.extend_to_line_end(end);
                self.edits.push(Edit {
                    start: line_start,
                    end: line_end,
                    replacement: String::new(),
                });
                continue;
            }

            // Handle #[tokio::test] → #[test]
            if is_tokio_test_attr(attr) {
                let (start, end) = self.attr_byte_range(attr);
                let line_start = self.extend_to_line_start(start);
                let line_end = self.extend_to_line_end(end);
                self.edits.push(Edit {
                    start: line_start,
                    end: line_end,
                    replacement: format!("{}#[test]\n", " ".repeat(start - line_start)),
                });
                continue;
            }

            // Handle derive attributes — strip TS
            if is_derive_attr(attr) {
                if let Ok(derive_list) = parse_derive_paths(attr) {
                    let has_ts = derive_list.iter().any(|p| is_ts_derive_path(p));
                    if has_ts {
                        let remaining: Vec<&Path> = derive_list
                            .iter()
                            .filter(|p| !is_ts_derive_path(p))
                            .collect();

                        let (start, end) = self.attr_byte_range(attr);

                        if remaining.is_empty() {
                            // Remove entire derive attribute
                            let line_start = self.extend_to_line_start(start);
                            let line_end = self.extend_to_line_end(end);
                            self.edits.push(Edit {
                                start: line_start,
                                end: line_end,
                                replacement: String::new(),
                            });
                        } else {
                            // Rebuild derive with remaining items
                            let names: Vec<String> = remaining
                                .iter()
                                .map(|p| {
                                    p.segments
                                        .iter()
                                        .map(|s| s.ident.to_string())
                                        .collect::<Vec<_>>()
                                        .join("::")
                                })
                                .collect();
                            let new_derive = format!("#[derive({})]", names.join(", "));
                            self.edits.push(Edit {
                                start,
                                end,
                                replacement: new_derive,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Get the byte range for an entire attribute (from `#` to closing `]`).
    ///
    /// syn's attribute spans can be tricky. We use the pound token start and
    /// then scan forward in the source to find the matching `]`.
    fn attr_byte_range(&self, attr: &Attribute) -> (usize, usize) {
        let (start, _) = self.span_range(attr.pound_token.span);

        // Scan forward from start to find the matching `]`
        let mut depth = 0;
        let mut end = start;
        let bytes = self.source.as_bytes();
        let mut i = start;
        while i < bytes.len() {
            match bytes[i] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        (start, end)
    }
}

impl<'a> Visit<'a> for EditCollector<'a> {
    // --- Use items ---
    fn visit_item_use(&mut self, node: &'a ItemUse) {
        if should_remove_use(node) {
            let (start, _) = self.span_range(node.use_token.span);
            let line_start = self.extend_to_line_start(start);
            // Find the end of the use statement (including the semicolon and newline)
            let (_, semi_end) = self.span_range(node.semi_token.span);
            let line_end = self.extend_to_line_end(semi_end);
            self.edits.push(Edit {
                start: line_start,
                end: line_end,
                replacement: String::new(),
            });
        }

        // File-specific use statement rewrites
        self.rewrite_file_specific_use(node);

        syn::visit::visit_item_use(self, node);
    }

    // --- Attributes on structs ---
    fn visit_item_struct(&mut self, node: &'a syn::ItemStruct) {
        self.process_attrs(&node.attrs);
        syn::visit::visit_item_struct(self, node);
    }

    // --- Attributes on enums ---
    fn visit_item_enum(&mut self, node: &'a syn::ItemEnum) {
        self.process_attrs(&node.attrs);
        syn::visit::visit_item_enum(self, node);
    }

    // --- Attributes on functions ---
    fn visit_item_fn(&mut self, node: &'a syn::ItemFn) {
        self.process_attrs(&node.attrs);
        // File-specific: merge cfg-gated platform functions into unconditional
        self.merge_cfg_platform_fns(node);
        // File-specific: cfg attr modifications on specific functions
        self.rewrite_file_specific_fn_attrs(node);
        // File-specific: mut additions on function parameters
        self.rewrite_file_specific_fn_params(node);
        // File-specific: inject wasm32 fallback functions after cfg-gated platform functions
        self.inject_cfg_fallback_fns(node);
        syn::visit::visit_item_fn(self, node);
    }

    // --- Let bindings: file-specific mut additions ---
    fn visit_local(&mut self, node: &'a syn::Local) {
        self.rewrite_file_specific_let_binding(node);
        syn::visit::visit_local(self, node);
    }

    // --- Attributes on type aliases ---
    fn visit_item_type(&mut self, node: &'a syn::ItemType) {
        self.process_attrs(&node.attrs);
        syn::visit::visit_item_type(self, node);
    }

    // --- Attributes on unions ---
    fn visit_item_union(&mut self, node: &'a syn::ItemUnion) {
        self.process_attrs(&node.attrs);
        syn::visit::visit_item_union(self, node);
    }

    // --- Attributes on constants (for PATH_SEPARATOR cfg widening) ---
    fn visit_item_const(&mut self, node: &'a syn::ItemConst) {
        self.rewrite_file_specific_const_attrs(node);
        syn::visit::visit_item_const(self, node);
    }

    // --- Attributes on fields ---
    fn visit_field(&mut self, node: &'a syn::Field) {
        self.process_attrs(&node.attrs);
        syn::visit::visit_field(self, node);
    }

    // --- Attributes on variants ---
    fn visit_variant(&mut self, node: &'a syn::Variant) {
        self.process_attrs(&node.attrs);
        syn::visit::visit_variant(self, node);
    }

    // --- Turbofish rewriting for query_as::<_, T> and query_scalar::<_, T> ---
    fn visit_path_segment(&mut self, seg: &'a PathSegment) {
        let is_target = seg.ident == "query_as" || seg.ident == "query_scalar";

        if is_target {
            if let PathArguments::AngleBracketed(ref args) = seg.arguments {
                if args.args.len() >= 2 {
                    if let Some(GenericArgument::Type(Type::Infer(_))) = args.args.first() {
                        // Get the span of the angle-bracketed args
                        let (args_start, _) = self.span_range(args.lt_token.span);
                        let (_, args_close) = self.span_range(args.gt_token.span);

                        // Build the replacement: <remaining_args>
                        let remaining: Vec<String> = args
                            .args
                            .iter()
                            .skip(1)
                            .map(|a| quote::quote!(#a).to_string())
                            .collect();
                        let replacement = format!("::<{}>", remaining.join(", "));

                        // We need to replace from `::` before `<` to `>`
                        // The turbofish `::` comes before the `<`, so back up 2 chars
                        let turbo_start = if args_start >= 2
                            && &self.source[args_start - 2..args_start] == "::"
                        {
                            args_start - 2
                        } else {
                            args_start
                        };

                        self.edits.push(Edit {
                            start: turbo_start,
                            end: args_close, // span.end() is already past `>`
                            replacement,
                        });
                    }
                }
            }
        }

        syn::visit::visit_path_segment(self, seg);
    }

    // sqlx::migrate!() is now handled by our wasi-sqlx-macros proc macro directly —
    // no codemod rewrite needed.
    fn visit_expr(&mut self, expr: &'a Expr) {
        // Rewrite which::which(...) and which::which_in(...) call expressions
        if let Expr::Call(call) = expr {
            self.rewrite_which_calls(call);
        }
        syn::visit::visit_expr(self, expr);
    }

    // --- All macros: select! body cleaning ---
    // Using visit_macro catches select! in ALL positions (expression, statement, item).
    // We work on the raw source text to preserve formatting.
    fn visit_macro(&mut self, mac: &'a syn::Macro) {
        if is_select_macro_path(&mac.path) {
            let (body_start, body_end) = self.generic_macro_body_range(mac);
            if body_start < body_end {
                let body_text = &self.source[body_start..body_end];
                self.collect_select_body_edits(body_text, body_start);
            }
        }
        syn::visit::visit_macro(self, mac);
    }

    // --- TS stripping from macro_rules bodies ---
    fn visit_item_macro(&mut self, node: &'a syn::ItemMacro) {
        let tokens_str = node.mac.tokens.to_string();
        if tokens_str.contains("TS") || tokens_str.contains("ts (") || tokens_str.contains("ts(") {
            let filtered = strip_ts_from_macro_tokens(node.mac.tokens.clone());
            let filtered_str = filtered.to_string();
            if filtered_str != tokens_str {
                let (delim_start, delim_end) = self.macro_body_range(node);
                if delim_start < delim_end {
                    // Get the delimiter character
                    let open_delim = match node.mac.delimiter {
                        syn::MacroDelimiter::Paren(_) => "(",
                        syn::MacroDelimiter::Brace(_) => "{",
                        syn::MacroDelimiter::Bracket(_) => "[",
                    };
                    let close_delim = match node.mac.delimiter {
                        syn::MacroDelimiter::Paren(_) => ")",
                        syn::MacroDelimiter::Brace(_) => "}",
                        syn::MacroDelimiter::Bracket(_) => "]",
                    };
                    self.edits.push(Edit {
                        start: delim_start,
                        end: delim_end,
                        replacement: format!("{} {} {}", open_delim, filtered_str, close_delim),
                    });
                }
            }
        }
        syn::visit::visit_item_macro(self, node);
    }

    // --- Path rewrites ---
    fn visit_path(&mut self, path: &'a Path) {
        self.rewrite_thread_paths(path);
        self.rewrite_chatgpt_connectors(path);
        self.rewrite_merge_connectors(path);
        self.rewrite_file_specific_paths(path);

        syn::visit::visit_path(self, path);
    }

    // --- Use path rewrites (use statements have UseTree, not Path) ---
    fn visit_use_path(&mut self, node: &'a syn::UsePath) {
        if node.ident == "codex_chatgpt" {
            let (start, end) = self.span_range(node.ident.span());
            if start < end {
                self.edits.push(Edit {
                    start,
                    end,
                    replacement: "codex_core".to_string(),
                });
            }
        }
        syn::visit::visit_use_path(self, node);
    }
}

impl<'a> EditCollector<'a> {
    // sqlx::migrate!() is now handled by our proc macro — no rewrite needed.

    /// Collect targeted edits within a select! macro body to preserve formatting.
    ///
    /// Strips:
    /// - `biased;` lines
    /// - `, if <guard>` before `=>`
    /// - `else => ...` branches (replaced with `_ = async {} => ...`)
    fn collect_select_body_edits(&mut self, body_text: &str, base_offset: usize) {
        // 1. Strip `biased;` — find and remove the whole line
        if let Some(pos) = body_text.find("biased") {
            // Check it's followed by `;` (possibly with whitespace)
            let after = body_text[pos + "biased".len()..].trim_start();
            if after.starts_with(';') {
                let semi_pos =
                    pos + "biased".len() + body_text[pos + "biased".len()..].find(';').unwrap();
                // Extend to full line
                let line_start = body_text[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line_end = body_text[semi_pos + 1..]
                    .find('\n')
                    .map(|p| semi_pos + 1 + p + 1)
                    .unwrap_or(semi_pos + 1);
                self.edits.push(Edit {
                    start: base_offset + line_start,
                    end: base_offset + line_end,
                    replacement: String::new(),
                });
            }
        }

        // 2. Strip `, if <guard>` before `=>`
        // Pattern: `, if` ... `=>`  — remove from `,` to just before `=>`
        let mut search_start = 0;
        while let Some(comma_if_pos) = body_text[search_start..].find(", if ") {
            let abs_pos = search_start + comma_if_pos;
            // Find the `=>` that follows (not inside braces)
            let after_if = &body_text[abs_pos + 2..]; // skip the `, `
            if let Some(arrow_offset) = find_fat_arrow_outside_braces(after_if) {
                let arrow_abs = abs_pos + 2 + arrow_offset;
                // Remove from `, ` to just before `=>`
                self.edits.push(Edit {
                    start: base_offset + abs_pos,
                    end: base_offset + arrow_abs,
                    replacement: " ".to_string(),
                });
                search_start = arrow_abs + 2;
            } else {
                search_start = abs_pos + 5;
            }
        }

        // 3. Replace `else =>` with `_ = async {} =>`
        let mut search_start = 0;
        while let Some(else_pos) = body_text[search_start..].find("else =>") {
            let abs_pos = search_start + else_pos;
            // Replace just `else` with `_ = async {}`
            self.edits.push(Edit {
                start: base_offset + abs_pos,
                end: base_offset + abs_pos + "else".len(),
                replacement: "_ = async {}".to_string(),
            });
            search_start = abs_pos + "else =>".len();
        }
    }

    /// Get byte range of the body tokens inside a macro's delimiters.
    /// Returns (after_open_delim, before_close_delim).
    fn generic_macro_body_range(&self, mac: &syn::Macro) -> (usize, usize) {
        let path_start_span = mac.path.segments.first().unwrap().ident.span();
        let (start, _) = self.span_range(path_start_span);

        let bytes = self.source.as_bytes();
        let mut i = start;

        // Find the `!`
        while i < bytes.len() && bytes[i] != b'!' {
            i += 1;
        }
        i += 1;

        // Skip whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }

        let (open, close) = match bytes.get(i) {
            Some(b'(') => (b'(', b')'),
            Some(b'[') => (b'[', b']'),
            Some(b'{') => (b'{', b'}'),
            _ => return (start, start),
        };

        let body_start = i + 1;
        let mut depth = 1;
        i += 1;
        while i < bytes.len() && depth > 0 {
            if bytes[i] == open {
                depth += 1;
            } else if bytes[i] == close {
                depth -= 1;
            }
            if depth > 0 {
                i += 1;
            }
        }

        (body_start, i) // i points at closing delimiter
    }

    /// Get byte range of a macro_rules body delimiters (including delimiters).
    /// Uses source text search for robustness (span locations can be inaccurate).
    fn macro_body_range(&self, node: &syn::ItemMacro) -> (usize, usize) {
        // For macro_rules! definitions, search for `macro_rules! <name>` in source
        if let Some(ident) = &node.ident {
            let name = ident.to_string();
            let pattern = format!("macro_rules! {}", name);
            if let Some(pos) = self.source.find(&pattern) {
                let after_name = pos + pattern.len();
                // Skip whitespace and find the opening delimiter
                return self.scan_delimiters_from(after_name);
            }
        }
        // Fallback for other macro invocations: use span
        let path_start_span = node.mac.path.segments.first().unwrap().ident.span();
        let (start, _) = self.span_range(path_start_span);
        self.scan_macro_delimiters(start)
    }

    /// Scan forward from `pos` skipping whitespace to find a matching delimiter pair.
    fn scan_delimiters_from(&self, pos: usize) -> (usize, usize) {
        let bytes = self.source.as_bytes();
        let mut i = pos;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let (open, close) = match bytes.get(i) {
            Some(b'(') => (b'(', b')'),
            Some(b'[') => (b'[', b']'),
            Some(b'{') => (b'{', b'}'),
            _ => return (pos, pos),
        };
        let delim_start = i;
        let mut depth = 1;
        i += 1;
        while i < bytes.len() && depth > 0 {
            if bytes[i] == open {
                depth += 1;
            } else if bytes[i] == close {
                depth -= 1;
            }
            i += 1;
        }
        (delim_start, i)
    }

    /// Scan forward from `start` to find the `!` and then the matching delimiter pair.
    /// Returns (open_delim_pos, after_close_delim_pos).
    fn scan_macro_delimiters(&self, start: usize) -> (usize, usize) {
        let bytes = self.source.as_bytes();
        let mut i = start;

        while i < bytes.len() && bytes[i] != b'!' {
            i += 1;
        }
        i += 1;

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }

        let (open, close) = match bytes.get(i) {
            Some(b'(') => (b'(', b')'),
            Some(b'[') => (b'[', b']'),
            Some(b'{') => (b'{', b'}'),
            _ => return (start, start),
        };

        let delim_start = i;
        let mut depth = 1;
        i += 1;
        while i < bytes.len() && depth > 0 {
            if bytes[i] == open {
                depth += 1;
            } else if bytes[i] == close {
                depth -= 1;
            }
            i += 1;
        }

        (delim_start, i)
    }

    /// Rewrite `std::thread::*` → `tokio::thread_spawn::*` paths.
    fn rewrite_thread_paths(&mut self, path: &Path) {
        let segments = &path.segments;
        let seg_count = segments.len();
        let thread_leaves: &[&str] = &["spawn", "sleep", "Builder"];

        // Pattern 1: `std::thread::<leaf>`
        if seg_count >= 3 {
            for i in 0..seg_count.saturating_sub(2) {
                if segments[i].ident == "std"
                    && segments[i + 1].ident == "thread"
                    && thread_leaves
                        .iter()
                        .any(|&leaf| segments[i + 2].ident == leaf)
                {
                    // Replace `std::thread` with `tokio::thread_spawn`
                    let (std_start, _) = self.span_range(segments[i].ident.span());
                    let (_, thread_end) = self.span_range(segments[i + 1].ident.span());
                    self.edits.push(Edit {
                        start: std_start,
                        end: thread_end,
                        replacement: "tokio::thread_spawn".to_string(),
                    });
                    return;
                }
            }
        }

        // Pattern 2: Bare `thread::<leaf>` (no `std::` prefix)
        if seg_count >= 2 {
            for i in 0..seg_count.saturating_sub(1) {
                if segments[i].ident == "thread"
                    && thread_leaves
                        .iter()
                        .any(|&leaf| segments[i + 1].ident == leaf)
                {
                    // Skip if already rewritten (preceded by `tokio`)
                    if i > 0 && segments[i - 1].ident == "tokio" {
                        return;
                    }

                    // Replace `thread` with `tokio::thread_spawn`
                    let (thread_start, thread_end) = self.span_range(segments[i].ident.span());
                    self.edits.push(Edit {
                        start: thread_start,
                        end: thread_end,
                        replacement: "tokio::thread_spawn".to_string(),
                    });
                    return;
                }
            }
        }
    }

    /// Rewrite `codex_chatgpt` → `codex_core` in paths.
    fn rewrite_chatgpt_connectors(&mut self, path: &Path) {
        for seg in &path.segments {
            if seg.ident == "codex_chatgpt" {
                let (start, end) = self.span_range(seg.ident.span());
                self.edits.push(Edit {
                    start,
                    end,
                    replacement: "codex_core".to_string(),
                });
            }
        }
    }

    /// Rewrite `merge_connectors_with_accessible` → `merge_plugin_apps_with_accessible`
    fn rewrite_merge_connectors(&mut self, path: &Path) {
        let segments = &path.segments;
        for i in 0..segments.len() {
            if segments[i].ident == "merge_connectors_with_accessible" {
                if i > 0 && segments[i - 1].ident == "connectors" {
                    let (start, end) = self.span_range(segments[i].ident.span());
                    self.edits.push(Edit {
                        start,
                        end,
                        replacement: "merge_plugin_apps_with_accessible".to_string(),
                    });
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // File-specific path rewrites
    // -----------------------------------------------------------------------

    /// File-specific path renames dispatched by file path suffix.
    fn rewrite_file_specific_paths(&mut self, path: &Path) {
        let segments = &path.segments;

        // v8::IsolateHandle → crate::runtime::RuntimeHandle (code-mode/src/service.rs)
        if self.file_matches("code-mode/src/service.rs") && segments.len() >= 2 {
            for i in 0..segments.len().saturating_sub(1) {
                if segments[i].ident == "v8" && segments[i + 1].ident == "IsolateHandle" {
                    let (start, _) = self.span_range(segments[i].ident.span());
                    let (_, end) = self.span_range(segments[i + 1].ident.span());
                    self.edits.push(Edit {
                        start,
                        end,
                        replacement: "crate::runtime::RuntimeHandle".to_string(),
                    });
                    return;
                }
            }
        }

        // std::process::ExitStatus → tokio::process::ExitStatus (exec.rs, js_repl)
        if self.file_matches("core/src/exec.rs")
            || self.file_matches("core/src/tools/js_repl/mod.rs")
        {
            if segments.len() >= 3 {
                for i in 0..segments.len().saturating_sub(2) {
                    if segments[i].ident == "std"
                        && segments[i + 1].ident == "process"
                        && segments[i + 2].ident == "ExitStatus"
                    {
                        let (start, _) = self.span_range(segments[i].ident.span());
                        let (_, end) = self.span_range(segments[i + 2].ident.span());
                        self.edits.push(Edit {
                            start,
                            end,
                            replacement: "tokio::process::ExitStatus".to_string(),
                        });
                        return;
                    }
                }
            }
        }

        // std::process::Output → tokio::process::Output (git_info.rs)
        if self.file_matches("core/src/git_info.rs") && segments.len() >= 3 {
            for i in 0..segments.len().saturating_sub(2) {
                if segments[i].ident == "std"
                    && segments[i + 1].ident == "process"
                    && segments[i + 2].ident == "Output"
                {
                    let (start, _) = self.span_range(segments[i].ident.span());
                    let (_, end) = self.span_range(segments[i + 2].ident.span());
                    self.edits.push(Edit {
                        start,
                        end,
                        replacement: "tokio::process::Output".to_string(),
                    });
                    return;
                }
            }
        }

        // which::which → run `which <name>` via shell-exec backend
        // In WASM, this routes through the WIT shell-exec interface to the host,
        // which can actually resolve binary paths via the MCP sandbox.
        let which_files = [
            "core/src/shell.rs",
            "core/src/tools/js_repl/mod.rs",
            "shell-command/src/powershell.rs",
        ];
        for f in &which_files {
            if self.file_matches(f) && segments.len() >= 2 {
                for i in 0..segments.len().saturating_sub(1) {
                    if segments[i].ident == "which" && segments[i + 1].ident == "which" {
                        let (start, _) = self.span_range(segments[i].ident.span());
                        let (_, end) = self.span_range(segments[i + 1].ident.span());
                        self.edits.push(Edit {
                            start,
                            end,
                            replacement: "(|name: &str| -> Result<std::path::PathBuf, ()> { \
                                std::process::Command::new(\"which\").arg(name).output().ok() \
                                .and_then(|o| if o.status.success() { \
                                    String::from_utf8(o.stdout).ok().map(|s| std::path::PathBuf::from(s.trim())) \
                                } else { None }).ok_or(()) \
                            })".to_string(),
                        });
                        return;
                    }
                }
            }
        }

        // which::which_in → Err stub (rmcp-client)
        if self.file_matches("rmcp-client/src/program_resolver.rs") && segments.len() >= 2 {
            for i in 0..segments.len().saturating_sub(1) {
                if segments[i].ident == "which" && segments[i + 1].ident == "which_in" {
                    let (start, _) = self.span_range(segments[i].ident.span());
                    let (_, end) = self.span_range(segments[i + 1].ident.span());
                    self.edits.push(Edit {
                        start,
                        end,
                        replacement: "(|_prog, _path, _cwd| Err::<std::path::PathBuf, String>(\"which not available in WASM\".into()))".to_string(),
                    });
                    return;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // which::which / which::which_in call expression rewrites
    // -----------------------------------------------------------------------

    /// Rewrite entire `which::which(...)` and `which::which_in(...)` call expressions
    /// to stub replacements that work in WASM.
    fn rewrite_which_calls(&mut self, node: &syn::ExprCall) {
        // Extract the function path from the call expression
        let path = match &*node.func {
            Expr::Path(expr_path) => &expr_path.path,
            _ => return,
        };

        let segments = &path.segments;
        if segments.len() < 2 {
            return;
        }

        // Check for which::which(...)
        let which_call_files = [
            "shell-command/src/powershell.rs",
            "core/src/shell.rs",
            "core/src/tools/js_repl/mod.rs",
        ];
        for f in &which_call_files {
            if self.file_matches(f) {
                for i in 0..segments.len().saturating_sub(1) {
                    if segments[i].ident == "which" && segments[i + 1].ident == "which" {
                        // Replace the entire call expression:
                        // which::which(candidate) → (|| -> Result<std::path::PathBuf, ()> { Err(()) })()
                        let (path_start, _) = self.span_range(segments[i].ident.span());
                        let call_end = self.source_map.offset(node.paren_token.span.close().end());
                        self.edits.push(Edit {
                            start: path_start,
                            end: call_end,
                            replacement: "(|| -> Result<std::path::PathBuf, ()> { Err(()) })()"
                                .to_string(),
                        });
                        return;
                    }
                }
            }
        }

        // Check for which::which_in(...) in rmcp-client
        if self.file_matches("rmcp-client/src/program_resolver.rs") {
            for i in 0..segments.len().saturating_sub(1) {
                if segments[i].ident == "which" && segments[i + 1].ident == "which_in" {
                    // Replace the entire call expression:
                    // which::which_in(&program, search_path, &cwd) → Err::<std::path::PathBuf, String>("which not available in WASM".into())
                    let (path_start, _) = self.span_range(segments[i].ident.span());
                    let call_end = self.source_map.offset(node.paren_token.span.close().end());
                    self.edits.push(Edit {
                        start: path_start,
                        end: call_end,
                        replacement: "Err::<std::path::PathBuf, String>(\"which not available in WASM\".into())".to_string(),
                    });
                    return;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // File-specific use statement rewrites
    // -----------------------------------------------------------------------

    /// File-specific use statement modifications.
    fn rewrite_file_specific_use(&mut self, node: &ItemUse) {
        // exec.rs: use std::process::ExitStatus; → use tokio::process::ExitStatus;
        if self.file_matches("core/src/exec.rs") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "std" {
                    if let UseTree::Path(ref p2) = *p.tree {
                        if p2.ident == "process" {
                            if let UseTree::Name(ref name) = *p2.tree {
                                if name.ident == "ExitStatus" {
                                    // Replace entire use item
                                    let (start, _) = self.span_range(node.use_token.span);
                                    let line_start = self.extend_to_line_start(start);
                                    let (_, semi_end) = self.span_range(node.semi_token.span);
                                    let line_end = self.extend_to_line_end(semi_end);
                                    self.edits.push(Edit {
                                        start: line_start,
                                        end: line_end,
                                        replacement: "use tokio::process::ExitStatus;\n"
                                            .to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // exec.rs: remove #[cfg(unix)] use std::os::unix::process::ExitStatusExt;
        if self.file_matches("core/src/exec.rs") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "std" {
                    if let UseTree::Path(ref p2) = *p.tree {
                        if p2.ident == "os" {
                            // Check for ExitStatusExt deep in the path
                            let use_str = format!("{}", quote::quote!(#node));
                            if use_str.contains("ExitStatusExt") {
                                // Remove the use + any preceding #[cfg(unix)]
                                let (start, _) = self.span_range(node.use_token.span);
                                let mut line_start = self.extend_to_line_start(start);
                                let (_, semi_end) = self.span_range(node.semi_token.span);
                                let line_end = self.extend_to_line_end(semi_end);
                                // Also check for #[cfg(unix)] on the line(s) above
                                for attr in &node.attrs {
                                    if is_cfg_attr_with(attr, "unix") {
                                        let (attr_start, _) = self.attr_byte_range(attr);
                                        let attr_line_start = self.extend_to_line_start(attr_start);
                                        line_start = attr_line_start;
                                    }
                                }
                                self.edits.push(Edit {
                                    start: line_start,
                                    end: line_end,
                                    replacement: String::new(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // artifacts/src/runtime/js_runtime.rs: use which::which; → stub fn
        if self.file_matches("artifacts/src/runtime/js_runtime.rs") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "which" {
                    if let UseTree::Name(ref name) = *p.tree {
                        if name.ident == "which" {
                            let (start, _) = self.span_range(node.use_token.span);
                            let line_start = self.extend_to_line_start(start);
                            let (_, semi_end) = self.span_range(node.semi_token.span);
                            let line_end = self.extend_to_line_end(semi_end);
                            self.edits.push(Edit {
                                start: line_start,
                                end: line_end,
                                replacement: "fn which(name: &str) -> Result<std::path::PathBuf, ()> { \
                                    std::process::Command::new(\"which\").arg(name).output().ok() \
                                    .and_then(|o| if o.status.success() { \
                                        String::from_utf8(o.stdout).ok().map(|s| std::path::PathBuf::from(s.trim())) \
                                    } else { None }).ok_or(()) \
                                }\n".to_string(),
                            });
                        }
                    }
                }
            }
        }

        // shell-command/src/powershell.rs: which::which(candidate) is handled by path rewrite
        // but the `use` isn't present there — it's an inline path call.

        // path_absolutize: replace import with inline trait + impls
        if self.file_matches("utils/absolute-path/src/lib.rs") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "path_absolutize" {
                    let (start, _) = self.span_range(node.use_token.span);
                    let line_start = self.extend_to_line_start(start);
                    let (_, semi_end) = self.span_range(node.semi_token.span);
                    let line_end = self.extend_to_line_end(semi_end);
                    self.edits.push(Edit {
                        start: line_start,
                        end: line_end,
                        replacement: "\
/// Simple absolutize replacement for WASM (path-absolutize doesn't compile for wasm32)
trait Absolutize {
    fn absolutize(&self) -> std::io::Result<std::borrow::Cow<'_, Path>>;
    fn absolutize_from(&self, base: &Path) -> std::io::Result<std::borrow::Cow<'_, Path>>;
}
impl Absolutize for Path {
    fn absolutize(&self) -> std::io::Result<std::borrow::Cow<'_, Path>> {
        if self.is_absolute() {
            Ok(std::borrow::Cow::Borrowed(self))
        } else {
            let cwd = std::env::current_dir()?;
            Ok(std::borrow::Cow::Owned(cwd.join(self)))
        }
    }
    fn absolutize_from(&self, base: &Path) -> std::io::Result<std::borrow::Cow<'_, Path>> {
        if self.is_absolute() {
            Ok(std::borrow::Cow::Borrowed(self))
        } else {
            Ok(std::borrow::Cow::Owned(base.join(self)))
        }
    }
}\n"
                        .to_string(),
                    });
                }
            }
        }

        // execpolicy-legacy: replace path_absolutize with inline trait + impls
        if self.file_matches("execpolicy-legacy/src/execv_checker.rs") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "path_absolutize" {
                    let (start, _) = self.span_range(node.use_token.span);
                    let line_start = self.extend_to_line_start(start);
                    let (_, semi_end) = self.span_range(node.semi_token.span);
                    let line_end = self.extend_to_line_end(semi_end);
                    self.edits.push(Edit {
                        start: line_start,
                        end: line_end,
                        replacement: "\
trait Absolutize {
    fn absolutize(&self) -> std::io::Result<std::borrow::Cow<'_, std::path::Path>>;
    fn absolutize_from<P: AsRef<std::path::Path>>(&self, base: P) -> std::io::Result<std::borrow::Cow<'_, std::path::Path>>;
}
impl Absolutize for std::path::PathBuf {
    fn absolutize(&self) -> std::io::Result<std::borrow::Cow<'_, std::path::Path>> {
        if self.is_absolute() { Ok(std::borrow::Cow::Borrowed(self)) } else { Ok(std::borrow::Cow::Owned(std::env::current_dir()?.join(self))) }
    }
    fn absolutize_from<P: AsRef<std::path::Path>>(&self, base: P) -> std::io::Result<std::borrow::Cow<'_, std::path::Path>> {
        if self.is_absolute() { Ok(std::borrow::Cow::Borrowed(self)) } else { Ok(std::borrow::Cow::Owned(base.as_ref().join(self))) }
    }
}\n".to_string(),
                    });
                }
            }
        }

        // tungstenite stubs: codex-api/src/telemetry.rs
        if self.file_matches("codex-api/src/telemetry.rs") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "tokio_tungstenite" {
                    let (start, _) = self.span_range(node.use_token.span);
                    let line_start = self.extend_to_line_start(start);
                    let (_, semi_end) = self.span_range(node.semi_token.span);
                    let line_end = self.extend_to_line_end(semi_end);
                    self.edits.push(Edit {
                        start: line_start,
                        end: line_end,
                        replacement: "\
/// Stub for tungstenite Error (websocket deps stripped for WASM)
#[derive(Debug)]
pub struct Error;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, \"ws error\") }
}
impl std::error::Error for Error {}
/// Stub for tungstenite Message (websocket deps stripped for WASM)
#[derive(Debug)]
pub enum Message { Text(String), Binary(Vec<u8>) }\n"
                            .to_string(),
                    });
                }
            }
        }

        // tungstenite stubs: core/src/client.rs → re-use from codex_api::telemetry
        if self.file_matches("core/src/client.rs") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "tokio_tungstenite" {
                    let (start, _) = self.span_range(node.use_token.span);
                    let line_start = self.extend_to_line_start(start);
                    let (_, semi_end) = self.span_range(node.semi_token.span);
                    let line_end = self.extend_to_line_end(semi_end);
                    self.edits.push(Edit {
                        start: line_start,
                        end: line_end,
                        replacement: "// Re-use the tungstenite stub types from codex_api::telemetry\nuse codex_api::telemetry::Error;\nuse codex_api::telemetry::Message;\n".to_string(),
                    });
                }
            }
        }

        // fd_lock::RwLock stub: package-manager/src/manager.rs
        if self.file_matches("package-manager/src/manager.rs") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "fd_lock" {
                    let (start, _) = self.span_range(node.use_token.span);
                    let line_start = self.extend_to_line_start(start);
                    let (_, semi_end) = self.span_range(node.semi_token.span);
                    let line_end = self.extend_to_line_end(semi_end);
                    self.edits.push(Edit {
                        start: line_start,
                        end: line_end,
                        replacement: "\
/// Stub for fd_lock::RwLock (stripped for WASM)
struct FileRwLock<T>(T);
impl<T> FileRwLock<T> {
    fn new(inner: T) -> Self { Self(inner) }
    fn try_write(&mut self) -> std::io::Result<&mut T> { Ok(&mut self.0) }
}\n"
                        .to_string(),
                    });
                }
            }
        }

        // sqlx::FromRow import removal: state/
        if self.file_matches("state/") {
            if let UseTree::Path(ref p) = node.tree {
                if p.ident == "sqlx" {
                    if let UseTree::Name(ref name) = *p.tree {
                        if name.ident == "FromRow" {
                            let (start, _) = self.span_range(node.use_token.span);
                            let line_start = self.extend_to_line_start(start);
                            let (_, semi_end) = self.span_range(node.semi_token.span);
                            let line_end = self.extend_to_line_end(semi_end);
                            self.edits.push(Edit {
                                start: line_start,
                                end: line_end,
                                replacement: String::new(),
                            });
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // File-specific let-binding mut additions
    // -----------------------------------------------------------------------

    fn rewrite_file_specific_let_binding(&mut self, node: &syn::Local) {
        // insert_history.rs: let writer = ... → let mut writer = ...
        if self.file_matches("tui/src/insert_history.rs") {
            if let syn::Pat::Ident(ref pat_ident) = node.pat {
                if pat_ident.ident == "writer" && pat_ident.mutability.is_none() {
                    let (start, end) = self.span_range(pat_ident.ident.span());
                    self.edits.push(Edit {
                        start,
                        end,
                        replacement: "mut writer".to_string(),
                    });
                }
            }
        }

        // realtime_conversation.rs: events binding in struct destructure needs mut
        if self.file_matches("core/src/realtime_conversation.rs") {
            self.add_mut_to_struct_field_pat(node, "events");
        }
    }

    /// In a `let Struct { ..., name, ... } = ...;` pattern, add `mut` before `name`.
    fn add_mut_to_struct_field_pat(&mut self, node: &syn::Local, field_name: &str) {
        if let syn::Pat::Struct(ref pat_struct) = node.pat {
            for field in &pat_struct.fields {
                if let syn::Pat::Ident(ref pat_ident) = *field.pat {
                    if pat_ident.ident == field_name && pat_ident.mutability.is_none() {
                        let (start, end) = self.span_range(pat_ident.ident.span());
                        self.edits.push(Edit {
                            start,
                            end,
                            replacement: format!("mut {}", field_name),
                        });
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // File-specific function attribute / param rewrites
    // -----------------------------------------------------------------------

    /// Merge cfg-gated platform-specific function duplicates into a single
    /// unconditional function. Detects patterns like:
    ///   #[cfg(unix)] fn foo(...) { unix_body }
    ///   #[cfg(windows)] fn foo(...) { windows_body }
    /// and replaces both with a single unconditional version.
    fn merge_cfg_platform_fns(&mut self, node: &syn::ItemFn) {
        if !self.file_matches("core/src/exec.rs") {
            return;
        }

        let fn_name = node.sig.ident.to_string();
        if fn_name != "synthetic_exit_status" {
            return;
        }

        // Check for #[cfg(unix)] or #[cfg(windows)] attribute
        let has_cfg_unix = node.attrs.iter().any(|a| is_cfg_attr_with(a, "unix"));
        let has_cfg_windows = node.attrs.iter().any(|a| is_cfg_attr_with(a, "windows"));

        if has_cfg_unix || has_cfg_windows {
            // Use source text search to find the exact range of this function.
            // The pattern is: `#[cfg(unix/windows)]\nfn synthetic_exit_status(...) { ... }`
            let cfg_name = if has_cfg_unix { "unix" } else { "windows" };
            let pattern = format!("#[cfg({})]\nfn synthetic_exit_status", cfg_name);
            if let Some(start) = self.source.find(&pattern) {
                // Find the closing brace of the function body
                let body_start = self.source[start..].find('{').map(|p| start + p);
                if let Some(bs) = body_start {
                    let mut depth = 1;
                    let mut end = bs + 1;
                    let bytes = self.source.as_bytes();
                    while end < bytes.len() && depth > 0 {
                        if bytes[end] == b'{' {
                            depth += 1;
                        }
                        if bytes[end] == b'}' {
                            depth -= 1;
                        }
                        end += 1;
                    }
                    // end is now past the closing brace
                    let line_end = self.extend_to_line_end(end);

                    if has_cfg_unix {
                        // Replace with unconditional version
                        self.edits.push(Edit {
                            start,
                            end: line_end,
                            replacement: "fn synthetic_exit_status(code: i32) -> ExitStatus {\n    ExitStatus::from_raw(code)\n}\n".to_string(),
                        });
                    } else {
                        // Remove the windows version (blank line between the two)
                        // Walk backwards to eat the blank line before #[cfg(windows)]
                        let adjusted_start =
                            if start > 0 && self.source.as_bytes()[start - 1] == b'\n' {
                                start - 1
                            } else {
                                start
                            };
                        self.edits.push(Edit {
                            start: adjusted_start,
                            end: line_end,
                            replacement: String::new(),
                        });
                    }
                }
            }
        }
    }

    /// Add `#[cfg(not(target_arch = "wasm32"))]` before specific statements, and
    /// widen `#[cfg(unix)]` → `#[cfg(any(unix, target_arch = "wasm32"))]`.
    fn rewrite_file_specific_fn_attrs(&mut self, _node: &syn::ItemFn) {
        // Most cfg modifications are on statements/expressions inside functions,
        // which syn represents as Stmt nodes. We handle those in visit_expr_if
        // and other visitors. This method handles function-level attrs only.
    }

    /// Add `mut` to specific function parameters.
    fn rewrite_file_specific_fn_params(&mut self, node: &syn::ItemFn) {
        // custom_terminal.rs: fn draw<I>(writer: &mut impl Write, ...) → (mut writer: ...)
        // custom_terminal.rs: fn queue<W>(self, w: &mut W) → (self, mut w: ...)
        if self.file_matches("tui/src/custom_terminal.rs") {
            let fn_name = node.sig.ident.to_string();
            if fn_name == "draw" || fn_name == "queue" {
                for input in &node.sig.inputs {
                    if let syn::FnArg::Typed(pat_type) = input {
                        if let syn::Pat::Ident(ref pat_ident) = *pat_type.pat {
                            let name = pat_ident.ident.to_string();
                            if (fn_name == "draw" && name == "writer")
                                || (fn_name == "queue" && name == "w")
                            {
                                if pat_ident.mutability.is_none() {
                                    let (start, end) = self.span_range(pat_ident.ident.span());
                                    self.edits.push(Edit {
                                        start,
                                        end,
                                        replacement: format!("mut {}", name),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Inject cfg-gated fallback functions for wasm32
    // -----------------------------------------------------------------------

    /// After a `#[cfg(windows)]` function, inject a `#[cfg(not(any(unix, windows)))]`
    /// fallback with a WASM-compatible stub. This handles platform-gated functions
    /// that need a wasm32 alternative.
    fn inject_cfg_fallback_fns(&mut self, node: &syn::ItemFn) {
        let fn_name = node.sig.ident.to_string();
        let has_cfg_windows = node.attrs.iter().any(|a| is_cfg_attr_with(a, "windows"));

        // config_loader/mod.rs: system_requirements_toml_file and system_config_toml_file
        if self.file_matches("core/src/config_loader/mod.rs") && has_cfg_windows {
            if fn_name == "system_requirements_toml_file" {
                // Check if the wasm32 version already exists (idempotency)
                let fn_end = self.fn_byte_end(node);
                let after = &self.source[fn_end..];
                if !after
                    .trim_start()
                    .starts_with("#[cfg(not(any(unix, windows)))]")
                {
                    self.edits.push(Edit {
                        start: fn_end,
                        end: fn_end,
                        replacement: "\n\n#[cfg(not(any(unix, windows)))]\nfn system_requirements_toml_file() -> io::Result<AbsolutePathBuf> {\n    AbsolutePathBuf::from_absolute_path(Path::new(\"/etc/codex/requirements.toml\"))\n}\n".to_string(),
                    });
                }
            }
            if fn_name == "system_config_toml_file" {
                let fn_end = self.fn_byte_end(node);
                let after = &self.source[fn_end..];
                if !after
                    .trim_start()
                    .starts_with("#[cfg(not(any(unix, windows)))]")
                {
                    self.edits.push(Edit {
                        start: fn_end,
                        end: fn_end,
                        replacement: "\n\n#[cfg(not(any(unix, windows)))]\nfn system_config_toml_file() -> io::Result<AbsolutePathBuf> {\n    AbsolutePathBuf::from_absolute_path(Path::new(\"/etc/codex/config.toml\"))\n}\n".to_string(),
                    });
                }
            }
        }

        // message_history.rs: ensure_owner_only_permissions
        if self.file_matches("core/src/message_history.rs") && has_cfg_windows {
            if fn_name == "ensure_owner_only_permissions" {
                let fn_end = self.fn_byte_end(node);
                let after = &self.source[fn_end..];
                if !after
                    .trim_start()
                    .starts_with("#[cfg(not(any(unix, windows)))]")
                {
                    self.edits.push(Edit {
                        start: fn_end,
                        end: fn_end,
                        replacement: "\n\n#[cfg(not(any(unix, windows)))]\nasync fn ensure_owner_only_permissions(_file: &File) -> Result<()> {\n    Ok(())\n}\n".to_string(),
                    });
                }
            }
        }
    }

    /// Get the byte offset of the end of a function (after closing brace).
    fn fn_byte_end(&self, node: &syn::ItemFn) -> usize {
        let (_, end) = self.span_range(node.block.brace_token.span.close());
        self.extend_to_line_end(end)
    }

    // -----------------------------------------------------------------------
    // File-specific const attribute rewrites
    // -----------------------------------------------------------------------

    /// Widen `#[cfg(unix)]` → `#[cfg(any(unix, target_arch = "wasm32"))]` on
    /// `const PATH_SEPARATOR` in arg0/src/lib.rs.
    fn rewrite_file_specific_const_attrs(&mut self, node: &syn::ItemConst) {
        if self.file_matches("arg0/src/lib.rs") && node.ident == "PATH_SEPARATOR" {
            for attr in &node.attrs {
                if is_cfg_unix_attr(attr) {
                    let (start, end) = self.attr_byte_range(attr);
                    self.edits.push(Edit {
                        start,
                        end,
                        replacement: "#[cfg(any(unix, target_arch = \"wasm32\"))]".to_string(),
                    });
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // String replacement edits (migrated from ast_transforms.rs)
    // -----------------------------------------------------------------------

    /// Helper: find `find` in `self.source` within the file, and replace with `replace`.
    /// Only applies if the file path matches `file_suffix`.
    /// Idempotent: skips if replacement is already present and find text is gone.
    fn replace_in_file(&mut self, file_suffix: &str, find: &str, replace: &str) {
        if !self.file_matches(file_suffix) {
            return;
        }
        if let Some(start) = self.source.find(find) {
            // Only if replacement isn't already present (idempotency)
            if !self.source.contains(replace) || self.source.contains(find) {
                self.edits.push(Edit {
                    start,
                    end: start + find.len(),
                    replacement: replace.to_string(),
                });
            }
        }
    }

    /// Helper: find `needle` in `self.source` within the file, and replace with `replacement`.
    /// Only applies if the file path matches `file_suffix`.
    /// Returns true if an edit was added.
    fn string_replace(&mut self, file_suffix: &str, needle: &str, replacement: &str) -> bool {
        if !self.file_matches(file_suffix) {
            return false;
        }
        if let Some(pos) = self.source.find(needle) {
            self.edits.push(Edit {
                start: pos,
                end: pos + needle.len(),
                replacement: replacement.to_string(),
            });
            true
        } else {
            // Check if already replaced
            if !self.source.contains(replacement) {
                let preview = if needle.len() > 60 {
                    &needle[..60]
                } else {
                    needle
                };
                console_log::console_warn!("  [syn-WARN] no match in {file_suffix}: \"{preview}\"");
            }
            false
        }
    }

    /// Collect all string-replacement edits migrated from ast_transforms::STRING_REPLACEMENTS.
    /// This runs after the AST visitor and adds edits to the same edit list.
    /// Because all edits are on the original source text and are applied in reverse order
    /// with overlap deduplication, they coexist correctly with AST-detected edits.
    fn collect_string_replacement_edits(&mut self) {
        // Only run if we have a file path
        if self.file_path.is_none() {
            return;
        }

        // --- core/src/codex.rs: clone auth_mode ---
        self.string_replace(
            "core/src/codex.rs",
            "            auth_mode,\n            originator.clone(),\n            config.otel.log_user_prompt,",
            "            auth_mode.clone(),\n            originator.clone(),\n            config.otel.log_user_prompt,",
        );

        // --- codex-api/src/telemetry.rs: tungstenite types → local stubs ---
        self.string_replace(
            "codex-api/src/telemetry.rs",
            "use tokio_tungstenite::tungstenite::Error;\nuse tokio_tungstenite::tungstenite::Message;",
            "/// Stub for tungstenite Error (websocket deps stripped for WASM)\n#[derive(Debug)]\npub struct Error;\nimpl std::fmt::Display for Error {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, \"ws error\") }\n}\nimpl std::error::Error for Error {}\n/// Stub for tungstenite Message (websocket deps stripped for WASM)\n#[derive(Debug)]\npub enum Message { Text(String), Binary(Vec<u8>) }",
        );

        // --- utils/absolute-path/src/lib.rs: path_absolutize → inline trait ---
        self.string_replace(
            "utils/absolute-path/src/lib.rs",
            "use path_absolutize::Absolutize;",
            "/// Simple absolutize replacement for WASM (path-absolutize doesn't compile for wasm32)\ntrait Absolutize {\n    fn absolutize(&self) -> std::io::Result<std::borrow::Cow<'_, Path>>;\n    fn absolutize_from(&self, base: &Path) -> std::io::Result<std::borrow::Cow<'_, Path>>;\n}\nimpl Absolutize for Path {\n    fn absolutize(&self) -> std::io::Result<std::borrow::Cow<'_, Path>> {\n        if self.is_absolute() {\n            Ok(std::borrow::Cow::Borrowed(self))\n        } else {\n            let cwd = std::env::current_dir()?;\n            Ok(std::borrow::Cow::Owned(cwd.join(self)))\n        }\n    }\n    fn absolutize_from(&self, base: &Path) -> std::io::Result<std::borrow::Cow<'_, Path>> {\n        if self.is_absolute() {\n            Ok(std::borrow::Cow::Borrowed(self))\n        } else {\n            Ok(std::borrow::Cow::Owned(base.join(self)))\n        }\n    }\n}",
        );

        // --- execpolicy-legacy/src/execv_checker.rs: path_absolutize ---
        self.string_replace(
            "execpolicy-legacy/src/execv_checker.rs",
            "use path_absolutize::*;",
            "trait Absolutize {\n    fn absolutize(&self) -> std::io::Result<std::borrow::Cow<'_, std::path::Path>>;\n    fn absolutize_from<P: AsRef<std::path::Path>>(&self, base: P) -> std::io::Result<std::borrow::Cow<'_, std::path::Path>>;\n}\nimpl Absolutize for std::path::PathBuf {\n    fn absolutize(&self) -> std::io::Result<std::borrow::Cow<'_, std::path::Path>> {\n        if self.is_absolute() { Ok(std::borrow::Cow::Borrowed(self)) } else { Ok(std::borrow::Cow::Owned(std::env::current_dir()?.join(self))) }\n    }\n    fn absolutize_from<P: AsRef<std::path::Path>>(&self, base: P) -> std::io::Result<std::borrow::Cow<'_, std::path::Path>> {\n        if self.is_absolute() { Ok(std::borrow::Cow::Borrowed(self)) } else { Ok(std::borrow::Cow::Owned(base.as_ref().join(self))) }\n    }\n}",
        );

        // --- core/src/context_manager/history.rs: stub image::load_from_memory ---
        self.string_replace(
            "core/src/context_manager/history.rs",
            "        let dynamic = match image::load_from_memory(&bytes) {\n            Ok(dynamic) => dynamic,\n            Err(error) => {\n                tracing::trace!(\"failed to decode original-detail image bytes: {error}\");\n                return None;\n            }\n        };\n        let width = i64::from(dynamic.width());\n        let height = i64::from(dynamic.height());",
            "        // image crate not available in WASM — skip image dimension estimation\n        let _ = &bytes;\n        let width: i64 = 1024;\n        let height: i64 = 1024;",
        );

        // --- core/src/client.rs: re-use tungstenite stub types ---
        self.string_replace(
            "core/src/client.rs",
            "use tokio_tungstenite::tungstenite::Error;\nuse tokio_tungstenite::tungstenite::Message;",
            "// Re-use the tungstenite stub types from codex_api::telemetry\nuse codex_api::telemetry::Error;\nuse codex_api::telemetry::Message;",
        );

        // config_loader cfg fallbacks: HANDLED BY inject_cfg_fallback_fns (syn visitor)
        // message_history cfg fallback: HANDLED BY inject_cfg_fallback_fns (syn visitor)

        // --- package-manager/src/manager.rs: stub fd_lock::RwLock ---
        self.string_replace(
            "package-manager/src/manager.rs",
            "use fd_lock::RwLock as FileRwLock;",
            "/// Stub for fd_lock::RwLock (stripped for WASM)\nstruct FileRwLock<T>(T);\nimpl<T> FileRwLock<T> {\n    fn new(inner: T) -> Self { Self(inner) }\n    fn try_write(&mut self) -> std::io::Result<&mut T> { Ok(&mut self.0) }\n}",
        );

        // --- utils/git/src/platform.rs: wasm32 stub for create_symlink ---
        self.string_replace(
            "utils/git/src/platform.rs",
            "#[cfg(not(any(unix, windows)))]\ncompile_error!(\"codex-git symlink support is only implemented for Unix and Windows\");",
            "#[cfg(not(any(unix, windows)))]\npub fn create_symlink(\n    _source: &Path,\n    _link_target: &Path,\n    _destination: &Path,\n) -> Result<(), GitToolingError> {\n    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, \"symlinks not supported on wasm32\").into())\n}",
        );

        // --- codex-client/src/transport.rs: replace zstd with no-op ---
        self.string_replace(
            "codex-client/src/transport.rs",
            "                    RequestCompression::Zstd => (\n                        zstd::stream::encode_all(std::io::Cursor::new(json), 3)\n                            .map_err(|err| TransportError::Build(err.to_string()))?,\n                        http::HeaderValue::from_static(\"zstd\"),\n                    ),",
            "                    RequestCompression::Zstd => (\n                        json,\n                        http::HeaderValue::from_static(\"identity\"),\n                    ),",
        );

        // --- core/src/skills/remote.rs: stub zip extraction ---
        self.string_replace(
            "core/src/skills/remote.rs",
            "    let cursor = std::io::Cursor::new(bytes);\n    let mut archive = zip::ZipArchive::new(cursor).context(\"Failed to open zip archive\")?;\n    for i in 0..archive.len() {\n        let mut file = archive.by_index(i).context(\"Failed to read zip entry\")?;\n        if file.is_dir() {\n            continue;\n        }\n        let raw_name = file.name().to_string();\n        let normalized = normalize_zip_name(&raw_name, prefix_candidates);\n        let Some(normalized) = normalized else {\n            continue;\n        };\n        let file_path = safe_join(output_dir, &normalized)?;\n        if let Some(parent) = file_path.parent() {\n            std::fs::create_dir_all(parent)\n                .with_context(|| format!(\"Failed to create parent dir for {normalized}\"))?;\n        }\n        let mut out = std::fs::File::create(&file_path)\n            .with_context(|| format!(\"Failed to create file {normalized}\"))?;\n        std::io::copy(&mut file, &mut out)\n            .with_context(|| format!(\"Failed to write skill file {normalized}\"))?;\n    }\n    Ok(())",
            "    let _ = (bytes, output_dir, prefix_candidates);\n    anyhow::bail!(\"zip extraction not available in WASM\")",
        );

        // app-server-protocol/src/protocol/common.rs: ts_rs macro function bodies
        // Handled by strip_ts_from_macro_tokens — removes entire fn definitions that
        // reference ::ts_rs:: (signature + body).

        // core/src/exec.rs: synthetic_exit_status merge — handled by merge_cfg_platform_fns in visit_item_fn

        // --- arg0/src/lib.rs: cfg-gate linux sandbox dispatch ---
        self.string_replace(
            "arg0/src/lib.rs",
            "    if exe_name == LINUX_SANDBOX_ARG0 {\n        // Safety: [`run_main`] never returns.\n        codex_linux_sandbox::run_main();\n    } else if exe_name == APPLY_PATCH_ARG0",
            "    #[cfg(not(target_arch = \"wasm32\"))]\n    if exe_name == LINUX_SANDBOX_ARG0 {\n        // Safety: [`run_main`] never returns.\n        codex_linux_sandbox::run_main();\n    }\n    if exe_name == APPLY_PATCH_ARG0",
        );

        // --- arg0/src/lib.rs: cfg-gate thread_stack_size ---
        self.string_replace(
            "arg0/src/lib.rs",
            "    builder.thread_stack_size(TOKIO_WORKER_STACK_SIZE_BYTES);",
            "    #[cfg(not(target_arch = \"wasm32\"))]\n    builder.thread_stack_size(TOKIO_WORKER_STACK_SIZE_BYTES);",
        );

        // --- tui/src/lib.rs: skip non_blocking writer ---
        self.string_replace(
            "tui/src/lib.rs",
            "    let (non_blocking, _guard) = non_blocking(log_file);\n\n    // use RUST_LOG env var, default to info for codex crates.\n    let env_filter = || {\n        EnvFilter::try_from_default_env().unwrap_or_else(|_| {\n            EnvFilter::new(\"codex_core=info,codex_tui=info,codex_rmcp_client=info\")\n        })\n    };\n\n    let file_layer = tracing_subscriber::fmt::layer()\n        .with_writer(non_blocking)",
            "    // [codex-codemod] non_blocking replaced with stderr (no thread spawning in WASM)\n    let _guard = ();\n\n    let env_filter = || {\n        EnvFilter::try_from_default_env().unwrap_or_else(|_| {\n            EnvFilter::new(\"codex_core=warn,codex_tui=warn\")\n        })\n    };\n\n    let file_layer = tracing_subscriber::fmt::layer()\n        .with_writer(std::io::stderr)",
        );

        // --- tui/src/tui/frame_requester.rs: FrameRequester struct ---
        self.string_replace(
            "tui/src/tui/frame_requester.rs",
            "#[derive(Clone, Debug)]\npub struct FrameRequester {\n    frame_schedule_tx: mpsc::UnboundedSender<Instant>,\n}",
            "#[derive(Clone)]\npub struct FrameRequester {\n    frame_schedule_tx: mpsc::UnboundedSender<Instant>,\n    draw_tx: broadcast::Sender<()>,\n}\nimpl std::fmt::Debug for FrameRequester {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        f.debug_struct(\"FrameRequester\").finish()\n    }\n}",
        );
        self.string_replace(
            "tui/src/tui/frame_requester.rs",
            "        let scheduler = FrameScheduler::new(rx, draw_tx);\n        tokio::spawn(scheduler.run());\n        Self {\n            frame_schedule_tx: tx,\n        }",
            "        let draw_tx_clone = draw_tx.clone();\n        let scheduler = FrameScheduler::new(rx, draw_tx);\n        tokio::spawn(scheduler.run());\n        Self {\n            frame_schedule_tx: tx,\n            draw_tx: draw_tx_clone,\n        }",
        );
        self.string_replace(
            "tui/src/tui/frame_requester.rs",
            "    pub fn schedule_frame(&self) {\n        let _ = self.frame_schedule_tx.send(Instant::now());\n    }",
            "    pub fn schedule_frame(&self) {\n        let _ = self.frame_schedule_tx.send(Instant::now());\n        let _ = self.draw_tx.send(());\n    }",
        );
        self.string_replace(
            "tui/src/tui/frame_requester.rs",
            "    pub fn schedule_frame_in(&self, dur: Duration) {\n        let _ = self.frame_schedule_tx.send(Instant::now() + dur);\n    }",
            "    pub fn schedule_frame_in(&self, dur: Duration) {\n        let _ = self.frame_schedule_tx.send(Instant::now() + dur);\n        let _ = self.draw_tx.send(());\n    }",
        );

        // --- tui/src/lib.rs: trace/yield injections ---
        self.string_replace(
            "tui/src/lib.rs",
            "    let codex_home = match find_codex_home() {",
            "    console_log::console_log!(\"[tui-trace] before find_codex_home\");\n    tokio::time::sleep(std::time::Duration::from_millis(1)).await;\n    let codex_home = match find_codex_home() {",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "    let config_toml = match load_config_as_toml_with_cli_overrides(",
            "    console_log::console_log!(\"[tui-trace] before load_config_toml\");\n    tokio::time::sleep(std::time::Duration::from_millis(1)).await;\n    let config_toml = match load_config_as_toml_with_cli_overrides(",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "    color_eyre::install()?;",
            "    console_log::console_log!(\"[tui-trace] before color_eyre::install\");\n    tokio::time::sleep(std::time::Duration::from_millis(1)).await;\n    let _ = color_eyre::install();",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "    let mut terminal = tui::init()?;",
            "    console_log::console_log!(\"[tui-trace] before tui::init\");\n    tokio::time::sleep(std::time::Duration::from_millis(1)).await;\n    let mut terminal = tui::init()?;\n    console_log::console_log!(\"[tui-trace] tui::init done\");",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "    let mut tui = Tui::new(terminal);",
            "    console_log::console_log!(\"[tui-trace] before Tui::new\");\n    tokio::time::sleep(std::time::Duration::from_millis(1)).await;\n    let mut tui = Tui::new(terminal);\n    console_log::console_log!(\"[tui-trace] Tui::new done\");\n    tokio::time::sleep(std::time::Duration::from_millis(1)).await;",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "    let auth_manager = AuthManager::shared(",
            "    console_log::console_log!(\"[tui-trace] before AuthManager\");\n    tokio::time::sleep(std::time::Duration::from_millis(1)).await;\n    let auth_manager = AuthManager::shared(",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "    let login_status = get_login_status(&initial_config);",
            "    console_log::console_log!(\"[tui-trace] before get_login_status\");\n    let login_status = get_login_status(&initial_config);\n    console_log::console_log!(\"[tui-trace] login_status: {:?}\", login_status);",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "    let should_show_onboarding =\n        should_show_onboarding(login_status, &initial_config, should_show_trust_screen_flag);",
            "    let should_show_onboarding =\n        should_show_onboarding(login_status, &initial_config, should_show_trust_screen_flag);\n    console_log::console_log!(\"[tui-trace] should_show_onboarding: {should_show_onboarding}\");",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "    let use_alt_screen = determine_alt_screen_mode(no_alt_screen, config.tui_alternate_screen);",
            "    console_log::console_log!(\"[tui-trace] before App::run\");\n    let use_alt_screen = determine_alt_screen_mode(no_alt_screen, config.tui_alternate_screen);",
        );

        // --- core/src/rollout/recorder.rs: UTC instead of local time ---
        self.string_replace(
            "core/src/rollout/recorder.rs",
            "    let timestamp = OffsetDateTime::now_local()\n        .map_err(|e| IoError::other(format!(\"failed to get local time: {e}\")))?;",
            "    let timestamp = OffsetDateTime::now_utc();",
        );

        // --- file-search/src/lib.rs: add wasm32 bail ---
        // (allow list widening done in PREPEND_TEXT directly)
        self.string_replace(
            "file-search/src/lib.rs",
            ") -> anyhow::Result<FileSearchSession> {\n    let FileSearchOptions {",
            ") -> anyhow::Result<FileSearchSession> {\n    #[cfg(target_arch = \"wasm32\")]\n    {\n        let _ = (&search_directories, &options, &reporter, &cancel_flag);\n        anyhow::bail!(\"File search is not available in the browser (requires OS threads)\");\n    }\n    let FileSearchOptions {",
        );

        // --- tui/src/app.rs: trace injections ---
        self.string_replace(
            "tui/src/app.rs",
            "        let mut model = thread_manager\n            .get_models_manager()\n            .get_default_model(&config.model, RefreshStrategy::Offline)\n            .await;",
            "        console_log::console_log!(\"[tui-trace] App::run ThreadManager created, fetching models...\");\n        let mut model = thread_manager\n            .get_models_manager()\n            .get_default_model(&config.model, RefreshStrategy::Offline)\n            .await;\n        console_log::console_log!(\"[tui-trace] App::run got default model: {}\", model);",
        );
        self.string_replace(
            "tui/src/app.rs",
            "        let enhanced_keys_supported = tui.enhanced_keys_supported();",
            "        console_log::console_log!(\"[tui-trace] App::run creating ChatWidget...\");\n        let enhanced_keys_supported = tui.enhanced_keys_supported();",
        );

        // --- tui/src/tui.rs: skip is_terminal() checks ---
        self.string_replace(
            "tui/src/tui.rs",
            "    if !stdin().is_terminal() {\n        return Err(std::io::Error::other(\"stdin is not a terminal\"));\n    }\n    if !stdout().is_terminal() {\n        return Err(std::io::Error::other(\"stdout is not a terminal\"));\n    }",
            "    // [codex-codemod] is_terminal() checks skipped — WASM stdin/stdout are ghostty-web terminal",
        );

        // --- utils/home-dir/src/lib.rs: skip canonicalize ---
        self.string_replace(
            "utils/home-dir/src/lib.rs",
            "                path.canonicalize().map_err(|err| {\n                    std::io::Error::new(\n                        err.kind(),\n                        format!(\"failed to canonicalize CODEX_HOME {val:?}: {err}\"),\n                    )\n                })",
            "                Ok(path)",
        );

        // --- tui/src/lib.rs: stub codex_utils_oss ---
        self.string_replace(
            "tui/src/lib.rs",
            "use codex_utils_oss::ensure_oss_provider_ready;\nuse codex_utils_oss::get_default_model_for_oss_provider;",
            "// codex-utils-oss stripped for WASM — OSS providers not available\nasync fn ensure_oss_provider_ready(_provider_id: &str, _config: &codex_core::config::Config) -> Result<(), std::io::Error> { Ok(()) }\nfn get_default_model_for_oss_provider(_provider_id: &str) -> Option<&'static str> { None }",
        );

        // --- tui/src/lib.rs: stub cloud_requirements_loader ---
        self.string_replace(
            "tui/src/lib.rs",
            "use codex_cloud_requirements::cloud_requirements_loader;",
            "// codex-cloud-requirements stripped for WASM — cloud config not needed\nfn cloud_requirements_loader(\n    _auth_manager: std::sync::Arc<codex_core::AuthManager>,\n    _chatgpt_base_url: String,\n    _codex_home: std::path::PathBuf,\n) -> codex_core::config_loader::CloudRequirementsLoader {\n    codex_core::config_loader::CloudRequirementsLoader::default()\n}",
        );

        // --- tui/src/app.rs: stub InProcessAppServerClient ---
        self.string_replace(
            "tui/src/app.rs",
            "use codex_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;\nuse codex_app_server_client::InProcessAppServerClient;\nuse codex_app_server_client::InProcessClientStartArgs;",
            "// codex-app-server-client stripped for WASM\n#[allow(dead_code)]\nconst DEFAULT_IN_PROCESS_CHANNEL_CAPACITY: usize = 64;\n#[allow(dead_code)] struct InProcessAppServerClient;\nimpl InProcessAppServerClient {\n    async fn start(_args: InProcessClientStartArgs) -> color_eyre::Result<Self> { color_eyre::eyre::bail!(\"not available in WASM\") }\n    fn request_handle(&self) -> InProcessRequestHandle { InProcessRequestHandle }\n    async fn shutdown(&self) -> color_eyre::Result<()> { Ok(()) }\n}\n#[allow(dead_code)] struct InProcessRequestHandle;\nimpl InProcessRequestHandle {\n    async fn request_typed<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(&self, _req: Req) -> color_eyre::Result<Resp> { color_eyre::eyre::bail!(\"not available\") }\n}\n#[allow(dead_code)] struct InProcessClientStartArgs { _private: () }",
        );

        // --- tui/src/chatwidget.rs: stub BackendClient ---
        self.string_replace(
            "tui/src/chatwidget.rs",
            "use codex_backend_client::Client as BackendClient;",
            "// codex-backend-client stripped for WASM — rate limit checking not available\n#[allow(dead_code)]\nstruct BackendClient;\nimpl BackendClient {\n    fn from_auth(_base_url: impl AsRef<str>, _auth: &codex_login::CodexAuth) -> Result<Self, std::io::Error> {\n        Err(std::io::Error::other(\"backend client not available in WASM\"))\n    }\n    async fn get_rate_limits_many(&self) -> Result<RateLimitsResponse, std::io::Error> {\n        Err(std::io::Error::other(\"not available in WASM\"))\n    }\n}\n#[allow(dead_code)]\nstruct RateLimitsResponse;\nimpl RateLimitsResponse {\n    fn rate_limits(&self) -> Vec<()> { Vec::new() }\n}",
        );

        // --- tui/src/clipboard_text.rs: stub arboard clipboard ---
        self.string_replace(
            "tui/src/clipboard_text.rs",
            "    let error = match arboard::Clipboard::new() {\n        Ok(mut clipboard) => match clipboard.set_text(text.to_string()) {\n            Ok(()) => return Ok(()),\n            Err(err) => format!(\"clipboard unavailable: {err}\"),\n        },\n        Err(err) => format!(\"clipboard unavailable: {err}\"),\n    };",
            "    let error = \"clipboard not available in WASM\".to_string();",
        );

        // --- core/src/codex_delegate.rs: convert thread::spawn to tokio::spawn ---
        // NOTE: This must match the ORIGINAL pattern before thread::spawn rewriting.
        self.string_replace(
            "core/src/codex_delegate.rs",
            "std::thread::spawn(move || {\n        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()\n            .enable_all()\n            .build()\n        else {\n            let _ = tx.send(ReviewDecision::Denied);\n            return;\n        };\n        let decision = runtime.block_on(review_approval_request_with_cancel(\n            &session,\n            &turn,\n            request,\n            retry_reason,\n            cancel_token,\n        ));\n        let _ = tx.send(decision);\n    });",
            "tokio::spawn(async move {\n        let decision = review_approval_request_with_cancel(\n            &session,\n            &turn,\n            request,\n            retry_reason,\n            cancel_token,\n        ).await;\n        let _ = tx.send(decision);\n    });",
        );

        // --- tui/src/tooltips.rs: stub reqwest::blocking ---
        self.string_replace(
            "tui/src/tooltips.rs",
            "        let client = reqwest::blocking::Client::builder()\n            .no_proxy()\n            .build()\n            .ok()?;\n        let response = client\n            .get(ANNOUNCEMENT_TIP_URL)\n            .timeout(Duration::from_millis(2000))\n            .send()\n            .ok()?;\n        response.error_for_status().ok()?.text().ok()",
            "        // reqwest::blocking not available in WASM\n        None::<String>",
        );

        // --- webbrowser::open is now handled by the wasi-webbrowser shim crate
        // (patched via [patch.crates-io] in codex-wasm-tui/Cargo.toml) ---

        // --- login/src/server.rs: strip unused std::thread import ---
        // (thread::sleep replaced by tokio::time::sleep in codemod, but import remains)
        self.string_replace("login/src/server.rs", "use std::thread;\n", "");

        // --- TelemetryAuthMode::from → from_display ---
        // wasi-codex-otel can't depend on codex-login, so use string-based conversion
        self.string_replace(
            "core/src/codex.rs",
            ".map(TelemetryAuthMode::from)",
            ".map(|m| TelemetryAuthMode::from_display(&m))",
        );

        // --- tui/src/lib.rs: codex_login::ForcedLoginMethod → codex_protocol ---
        // The upstream TUI maps between codex_protocol and codex_login ForcedLoginMethod,
        // but they're the same type. Use codex_protocol directly.
        self.string_replace(
            "tui/src/lib.rs",
            "codex_login::ForcedLoginMethod::Chatgpt",
            "codex_protocol::config_types::ForcedLoginMethod::Chatgpt",
        );
        self.string_replace(
            "tui/src/lib.rs",
            "codex_login::ForcedLoginMethod::Api",
            "codex_protocol::config_types::ForcedLoginMethod::Api",
        );
        self.string_replace(
            "core/src/models_manager/manager.rs",
            "TelemetryAuthMode::from(mode)",
            "TelemetryAuthMode::from_display(&mode)",
        );
        // tui/src/app.rs also maps TelemetryAuthMode
        self.string_replace(
            "tui/src/app.rs",
            ".map(TelemetryAuthMode::from)",
            ".map(|m| TelemetryAuthMode::from_display(&m))",
        );

        // --- tui/src/app.rs: replace InProcessClientStartArgs with bail ---
        self.string_replace(
            "tui/src/app.rs",
            "    InProcessAppServerClient::start(InProcessClientStartArgs {\n        arg0_paths,\n        config_warnings: config_warning_notifications(&config),\n        config: Arc::new(config),\n        cli_overrides: cli_kv_overrides,\n        loader_overrides,\n        cloud_requirements,\n        feedback,\n        session_source: SessionSource::Cli,\n        enable_codex_api_key_env: false,\n        client_name: \"codex-tui\".to_string(),\n        client_version: env!(\"CARGO_PKG_VERSION\").to_string(),\n        experimental_api: true,\n        opt_out_notification_methods: Vec::new(),\n        channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,\n    })\n    .await\n    .wrap_err(\"failed to start embedded app server for plugin request\")",
            "    { let _ = (&arg0_paths, &config, &cli_kv_overrides, &loader_overrides, &cloud_requirements, &feedback); color_eyre::eyre::bail!(\"plugin requests not available in WASM\") }",
        );

        // --- tui/src/chatwidget.rs: stub connectors list ---
        self.string_replace(
            "tui/src/chatwidget.rs",
            "connectors::list_all_connectors_with_options(&config, force_refetch).await?",
            "{ let _ = (&config, force_refetch); Vec::<connectors::AppInfo>::new() }",
        );

        // --- tui/src/chatwidget.rs: fix merge_connectors call sites ---
        // Match ORIGINAL name; use NEW name in replacement (syn rename won't reach inside replaced range)
        self.string_replace(
            "tui/src/chatwidget.rs",
            "merge_connectors_with_accessible(\n                    all_connectors,\n                    accessible_connectors,\n                    /*all_connectors_loaded*/ true,\n                )",
            "merge_plugin_apps_with_accessible(\n                    Vec::new(),\n                    accessible_connectors,\n                )",
        );
        self.string_replace(
            "tui/src/chatwidget.rs",
            "merge_connectors_with_accessible(\n                        Vec::new(),\n                        snapshot.connectors,\n                        /*all_connectors_loaded*/ false,\n                    )",
            "merge_plugin_apps_with_accessible(\n                        Vec::new(),\n                        snapshot.connectors,\n                    )",
        );

        // set_default_client_residency_requirement: no transform needed —
        // using real codex_login which has the correct ResidencyRequirement type.
        //
        // forced_login_method: no transform needed —
        // codex_login::auth::AuthConfig uses codex_protocol::config_types::ForcedLoginMethod
        // directly (same type the TUI passes). The identity mapping in the old code
        // is replaced by fixing the TUI to use codex_protocol paths directly.

        // --- tui/src/lib.rs: fix Multiplexer::Zellij pattern ---
        self.string_replace(
            "tui/src/lib.rs",
            "!matches!(terminal_info.multiplexer, Some(Multiplexer::Zellij { .. }))",
            "!matches!(terminal_info.multiplexer, Some(ref m) if m.name == codex_terminal_detection::MultiplexerName::Zellij)",
        );

        // --- tui/src/lib.rs: fix from_auth_storage ---
        self.string_replace(
            "tui/src/lib.rs",
            "match CodexAuth::from_auth_storage(&codex_home, config.cli_auth_credentials_store_mode) {\n            Ok(Some(auth)) => LoginStatus::AuthMode(auth.auth_mode()),",
            "match CodexAuth::from_auth_storage(&codex_home, config.cli_auth_credentials_store_mode) {\n            Ok(Some(auth)) => LoginStatus::AuthMode(codex_login::AuthMode::ApiKey),",
        );

        // --- tui/src/chatwidget.rs: fix TerminalName variants ---
        self.string_replace(
            "tui/src/chatwidget.rs",
            "TerminalName::AppleTerminal | TerminalName::WarpTerminal | TerminalName::VsCode => {\n            key_hint::shift(KeyCode::Left)\n        }\n        TerminalName::Ghostty\n        | TerminalName::Iterm2\n        | TerminalName::WezTerm\n        | TerminalName::Kitty\n        | TerminalName::Alacritty\n        | TerminalName::Konsole\n        | TerminalName::GnomeTerminal\n        | TerminalName::Vte\n        | TerminalName::WindowsTerminal\n        | TerminalName::Dumb\n        | TerminalName::Unknown => key_hint::alt(KeyCode::Up),",
            "_ => key_hint::alt(KeyCode::Up),",
        );

        // --- tui/src/resume_picker.rs: fix UnboundedReceiverStream ---
        self.string_replace(
            "tui/src/resume_picker.rs",
            "let mut background_events = UnboundedReceiverStream::new(bg_rx).fuse();",
            "let mut background_events = bg_rx;",
        );

        // --- tui/src/tui/event_stream.rs: replace tokio_stream wrappers ---
        self.string_replace(
            "tui/src/tui/event_stream.rs",
            "use tokio_stream::wrappers::BroadcastStream;\nuse tokio_stream::wrappers::WatchStream;\nuse tokio_stream::wrappers::errors::BroadcastStreamRecvError;",
            "/// Thin WatchStream wrapper for our shim watch::Receiver.\nstruct WatchStream<T: Clone>(tokio::sync::watch::Receiver<T>);\nimpl<T: Clone> WatchStream<T> {\n    fn from_changes(rx: tokio::sync::watch::Receiver<T>) -> Self { Self(rx) }\n}\nimpl<T: Clone + Unpin> WatchStream<T> {\n    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<T>> {\n        let this = self.get_mut();\n        match this.0.poll_changed(cx.waker()) {\n            Ok(true) => Poll::Ready(Some(this.0.borrow_and_update().clone())),\n            Ok(false) => Poll::Pending,\n            Err(_) => Poll::Ready(None),\n        }\n    }\n}\n/// Thin BroadcastStream wrapper for our shim broadcast::Receiver.\nstruct BroadcastStream<T: Clone>(tokio::sync::broadcast::Receiver<T>);\nimpl<T: Clone> BroadcastStream<T> {\n    fn new(rx: tokio::sync::broadcast::Receiver<T>) -> Self { Self(rx) }\n}\n#[derive(Debug)]\nenum BroadcastStreamRecvError { Lagged(u64) }\nimpl<T: Clone + Unpin> BroadcastStream<T> {\n    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Result<T, BroadcastStreamRecvError>>> {\n        match self.get_mut().0.poll_recv(cx.waker()) {\n            Ok(val) => Poll::Ready(Some(Ok(val))),\n            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(n)))),\n            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => Poll::Pending,\n            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => Poll::Ready(None),\n        }\n    }\n}",
        );

        // --- tui/src/chatwidget.rs: stub fetch_rate_limits ---
        self.string_replace(
            "tui/src/chatwidget.rs",
            "async fn fetch_rate_limits(base_url: String, auth: CodexAuth) -> Vec<RateLimitSnapshot> {\n    match BackendClient::from_auth(base_url, &auth) {\n        Ok(client) => match client.get_rate_limits_many().await {\n            Ok(snapshots) => snapshots,",
            "async fn fetch_rate_limits(base_url: String, auth: CodexAuth) -> Vec<RateLimitSnapshot> {\n    let _ = (base_url, auth);\n    return Vec::new();\n    #[allow(unreachable_code)]\n    match BackendClient::from_auth(String::new(), &CodexAuth::from_api_key(\"\")) {\n        Ok(client) => match client.get_rate_limits_many().await {\n            Ok(_snapshots) => Vec::new(),",
        );

        // --- tui/src/chatwidget.rs: fix PlanType mismatch ---
        self.string_replace(
            "tui/src/chatwidget.rs",
            "self.auth_manager\n                .auth_cached()\n                .and_then(|auth| auth.account_plan_type()),",
            "None::<codex_protocol::account::PlanType>,",
        );

        // --- tui/src/chatwidget.rs: fix feedback_diagnostics borrow ---
        self.string_replace(
            "tui/src/chatwidget.rs",
            "            snapshot.feedback_diagnostics(),\n        );\n        self.bottom_pane.show_selection_view(params);",
            "            &snapshot.feedback_diagnostics(),\n        );\n        self.bottom_pane.show_selection_view(params);",
        );

        // --- tui/src/bottom_pane/feedback_view.rs: fix thread_id unwrap ---
        self.string_replace(
            "tui/src/bottom_pane/feedback_view.rs",
            "        let mut thread_id = self.snapshot.thread_id.clone();\n\n        let result = self.snapshot.upload_feedback(",
            "        let mut thread_id = self.snapshot.thread_id.clone().unwrap_or_default();\n\n        let result = self.snapshot.upload_feedback(",
        );

        // --- tui/src/bottom_pane/feedback_view.rs: fix None type ---
        self.string_replace(
            "tui/src/bottom_pane/feedback_view.rs",
            "/*logs_override*/ None,",
            "/*logs_override*/ None::<Vec<u8>>,",
        );

        // --- tui/src/tui.rs: fix sync_update return type ---
        self.string_replace(
            "tui/src/tui.rs",
            "            terminal.draw(|frame| {\n                draw_fn(frame);\n            })\n        })?\n    }",
            "            terminal.draw(|frame| {\n                draw_fn(frame);\n            })\n        })?;\n        Ok(())\n    }",
        );

        // =====================================================================
        // DIAGNOSTIC TRACES: Session initialization hang debugging
        // =====================================================================

        // --- core/src/codex.rs: trace before Session::new ---
        self.string_replace(
            "core/src/codex.rs",
            "        let session = Session::new(\n            session_configuration,\n            config.clone(),\n            auth_manager.clone(),\n            models_manager.clone(),",
            "        console_log::console_log!(\"[diag-trace] codex.rs: BEFORE Session::new\");\n        let session = Session::new(\n            session_configuration,\n            config.clone(),\n            auth_manager.clone(),\n            models_manager.clone(),",
        );

        // --- core/src/codex.rs: trace after Session::new, before submission_loop spawn ---
        self.string_replace(
            "core/src/codex.rs",
            "        let thread_id = session.conversation_id;\n\n        // This task will run until Op::Shutdown is received.\n        let session_for_loop = Arc::clone(&session);\n        let session_loop_handle = tokio::spawn(async move {\n            submission_loop(session_for_loop, config, rx_sub)",
            "        let thread_id = session.conversation_id;\n        console_log::console_log!(\"[diag-trace] codex.rs: Session::new DONE, thread_id={}\", thread_id);\n\n        // This task will run until Op::Shutdown is received.\n        let session_for_loop = Arc::clone(&session);\n        console_log::console_log!(\"[diag-trace] codex.rs: BEFORE spawning submission_loop\");\n        let session_loop_handle = tokio::spawn(async move {\n            console_log::console_log!(\"[diag-trace] codex.rs: submission_loop task STARTED\");\n            submission_loop(session_for_loop, config, rx_sub)",
        );

        // --- core/src/codex.rs: trace at start of submission_loop ---
        self.string_replace(
            "core/src/codex.rs",
            "async fn submission_loop(sess: Arc<Session>, config: Arc<Config>, rx_sub: Receiver<Submission>) {\n    // To break out of this loop, send Op::Shutdown.\n    while let Ok(sub) = rx_sub.recv().await {",
            "async fn submission_loop(sess: Arc<Session>, config: Arc<Config>, rx_sub: Receiver<Submission>) {\n    console_log::console_log!(\"[diag-trace] codex.rs: submission_loop ENTERED, waiting for first Op\");\n    // To break out of this loop, send Op::Shutdown.\n    while let Ok(sub) = rx_sub.recv().await {\n        console_log::console_log!(\"[diag-trace] codex.rs: submission_loop received op: {:?}\", sub.op);",
        );

        // --- core/src/codex.rs: trace SessionConfigured dispatch ---
        self.string_replace(
            "core/src/codex.rs",
            "        // Dispatch the SessionConfiguredEvent first and then report any errors.\n        // If resuming, include converted initial messages in the payload so UIs can render them immediately.\n        let initial_messages = initial_history.get_event_msgs();",
            "        console_log::console_log!(\"[diag-trace] codex.rs: Session::new ABOUT TO dispatch SessionConfigured\");\n        // Dispatch the SessionConfiguredEvent first and then report any errors.\n        // If resuming, include converted initial messages in the payload so UIs can render them immediately.\n        let initial_messages = initial_history.get_event_msgs();",
        );

        // --- core/src/codex.rs: trace after SessionConfigured events sent ---
        self.string_replace(
            "core/src/codex.rs",
            "        // Start the watcher after SessionConfigured so it cannot emit earlier events.\n        sess.start_file_watcher_listener();",
            "        console_log::console_log!(\"[diag-trace] codex.rs: SessionConfigured events SENT\");\n        // Start the watcher after SessionConfigured so it cannot emit earlier events.\n        sess.start_file_watcher_listener();",
        );

        // --- core/src/thread_manager.rs: trace spawn_thread_with_source ---
        self.string_replace(
            "core/src/thread_manager.rs",
            "    pub(crate) async fn spawn_thread_with_source(\n        &self,\n        config: Config,\n        initial_history: InitialHistory,",
            "    pub(crate) async fn spawn_thread_with_source(\n        &self,\n        config: Config,\n        initial_history: InitialHistory,\n        // diag-trace injected below",
        );
        self.string_replace(
            "core/src/thread_manager.rs",
            "        // diag-trace injected below\n        auth_manager: Arc<AuthManager>,",
            "        auth_manager: Arc<AuthManager>,",
        );
        self.string_replace(
            "core/src/thread_manager.rs",
            "        let watch_registration = self\n            .file_watcher\n            .register_config(&config, self.skills_manager.as_ref());\n        let CodexSpawnOk {",
            "        console_log::console_log!(\"[diag-trace] thread_manager.rs: spawn_thread_with_source ENTERED\");\n        let watch_registration = self\n            .file_watcher\n            .register_config(&config, self.skills_manager.as_ref());\n        let CodexSpawnOk {",
        );

        // --- core/src/thread_manager.rs: trace after Codex::spawn in spawn_thread_with_source ---
        self.string_replace(
            "core/src/thread_manager.rs",
            "        .await?;\n        self.finalize_thread_spawn(codex, thread_id, watch_registration)\n            .await\n    }\n\n    async fn finalize_thread_spawn(",
            "        .await?;\n        console_log::console_log!(\"[diag-trace] thread_manager.rs: Codex::spawn DONE, calling finalize_thread_spawn\");\n        self.finalize_thread_spawn(codex, thread_id, watch_registration)\n            .await\n    }\n\n    async fn finalize_thread_spawn(",
        );

        // --- core/src/thread_manager.rs: trace in finalize_thread_spawn ---
        self.string_replace(
            "core/src/thread_manager.rs",
            "        let event = codex.next_event().await?;\n        let session_configured = match event {",
            "        console_log::console_log!(\"[diag-trace] thread_manager.rs: finalize_thread_spawn waiting for next_event (SessionConfigured)\");\n        let event = codex.next_event().await?;\n        console_log::console_log!(\"[diag-trace] thread_manager.rs: finalize_thread_spawn GOT event\");\n        let session_configured = match event {",
        );

        // --- tui/src/chatwidget/agent.rs: trace before start_thread ---
        self.string_replace(
            "tui/src/chatwidget/agent.rs",
            "        } = match server.start_thread(config).await {",
            "        } = match {\n            console_log::console_log!(\"[diag-trace] agent.rs: BEFORE server.start_thread\");\n            server.start_thread(config).await\n        } {",
        );

        // --- tui/src/chatwidget/agent.rs: trace after start_thread, before SessionConfigured send ---
        self.string_replace(
            "tui/src/chatwidget/agent.rs",
            "        initialize_app_server_client_name(thread.as_ref()).await;\n\n        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.\n        let ev = codex_protocol::protocol::Event {\n            // The `id` does not matter for rendering, so we can use a fake value.\n            id: \"\".to_string(),\n            msg: codex_protocol::protocol::EventMsg::SessionConfigured(session_configured),\n        };\n        app_event_tx_clone.send(AppEvent::CodexEvent(ev));",
            "        console_log::console_log!(\"[diag-trace] agent.rs: start_thread DONE, initializing client name\");\n        initialize_app_server_client_name(thread.as_ref()).await;\n        console_log::console_log!(\"[diag-trace] agent.rs: client name set, forwarding SessionConfigured to UI\");\n\n        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.\n        let ev = codex_protocol::protocol::Event {\n            // The `id` does not matter for rendering, so we can use a fake value.\n            id: \"\".to_string(),\n            msg: codex_protocol::protocol::EventMsg::SessionConfigured(session_configured),\n        };\n        app_event_tx_clone.send(AppEvent::CodexEvent(ev));\n        console_log::console_log!(\"[diag-trace] agent.rs: SessionConfigured SENT to UI\");",
        );

        // --- tui/src/chatwidget/agent.rs: trace op forwarding start ---
        self.string_replace(
            "tui/src/chatwidget/agent.rs",
            "        let thread_clone = thread.clone();\n        tokio::spawn(async move {\n            while let Some(op) = codex_op_rx.recv().await {\n                let id = thread_clone.submit(op).await;",
            "        console_log::console_log!(\"[diag-trace] agent.rs: spawning op-forwarding loop\");\n        let thread_clone = thread.clone();\n        tokio::spawn(async move {\n            console_log::console_log!(\"[diag-trace] agent.rs: op-forwarding loop STARTED, waiting for ops\");\n            while let Some(op) = codex_op_rx.recv().await {\n                console_log::console_log!(\"[diag-trace] agent.rs: op-forwarding received op, submitting\");\n                let id = thread_clone.submit(op).await;",
        );

        // =====================================================================
        // DIAGNOSTIC TRACES: Event delivery hang debugging
        // =====================================================================

        // --- tui/src/app.rs: trace enqueue_thread_event entry ---
        self.string_replace(
            "tui/src/app.rs",
            "    async fn enqueue_thread_event(&mut self, thread_id: ThreadId, event: Event) -> Result<()> {\n        let refresh_pending_thread_approvals =",
            "    async fn enqueue_thread_event(&mut self, thread_id: ThreadId, event: Event) -> Result<()> {\n        console_log::console_log!(\"[event-trace] enqueue_thread_event: thread={} msg={:?} active={:?}\", thread_id, std::mem::discriminant(&event.msg), self.active_thread_id);\n        let refresh_pending_thread_approvals =",
        );

        // --- tui/src/app.rs: trace enqueue should_send decision ---
        self.string_replace(
            "tui/src/app.rs",
            "        if should_send {\n            // Never await a bounded channel send on the main TUI loop",
            "        console_log::console_log!(\"[event-trace] enqueue should_send={} refresh_approvals={}\", should_send, refresh_pending_thread_approvals);\n        if should_send {\n            // Never await a bounded channel send on the main TUI loop",
        );

        // --- tui/src/app.rs: trace handle_active_thread_event ---
        self.string_replace(
            "tui/src/app.rs",
            "    async fn handle_active_thread_event(&mut self, tui: &mut tui::Tui, event: Event) -> Result<()> {\n        // Capture this before any potential thread switch",
            "    async fn handle_active_thread_event(&mut self, tui: &mut tui::Tui, event: Event) -> Result<()> {\n        console_log::console_log!(\"[event-trace] handle_active_thread_event: msg={:?}\", std::mem::discriminant(&event.msg));\n        // Capture this before any potential thread switch",
        );

        // --- core/src/message_history.rs: stub File::try_lock (unsupported in WASI) ---
        // Single-threaded WASM has no contention, so skip the lock and write directly.
        // NOTE: needle uses std::thread::sleep (original) — AST transforms run AFTER string_replace.
        self.string_replace(
            "core/src/message_history.rs",
            "    tokio::task::spawn_blocking(move || -> Result<()> {\n        // Retry a few times to avoid indefinite blocking when contended.\n        for _ in 0..MAX_RETRIES {\n            match history_file.try_lock() {\n                Ok(()) => {\n                    // While holding the exclusive lock, write the full line.\n                    // We do not open the file with `append(true)` on Windows, so ensure the\n                    // cursor is positioned at the end before writing.\n                    history_file.seek(SeekFrom::End(0))?;\n                    history_file.write_all(line.as_bytes())?;\n                    history_file.flush()?;\n                    enforce_history_limit(&mut history_file, history_max_bytes)?;\n                    return Ok(());\n                }\n                Err(std::fs::TryLockError::WouldBlock) => {\n                    std::thread::sleep(RETRY_SLEEP);\n                }\n                Err(e) => return Err(e.into()),\n            }\n        }\n\n        Err(std::io::Error::new(\n            std::io::ErrorKind::WouldBlock,\n            \"could not acquire exclusive lock on history file after multiple attempts\",\n        ))\n    })\n    .await??;",
            "    // [codex-codemod] File::try_lock not supported in WASI — write directly (no contention in single-threaded WASM)\n    tokio::task::spawn_blocking(move || -> Result<()> {\n        history_file.seek(SeekFrom::End(0))?;\n        history_file.write_all(line.as_bytes())?;\n        history_file.flush()?;\n        enforce_history_limit(&mut history_file, history_max_bytes)?;\n        Ok(())\n    })\n    .await??;",
        );

        // --- core/src/message_history.rs: stub File::try_lock_shared (unsupported in WASI) ---
        self.string_replace(
            "core/src/message_history.rs",
            "    // Open & lock file for reading using a shared lock.\n    // Retry a few times to avoid indefinite blocking.\n    for _ in 0..MAX_RETRIES {\n        let lock_result = file.try_lock_shared();\n\n        match lock_result {\n            Ok(()) => {\n                let reader = BufReader::new(&file);\n                for (idx, line_res) in reader.lines().enumerate() {\n                    let line = match line_res {\n                        Ok(l) => l,\n                        Err(e) => {\n                            tracing::warn!(error = %e, \"failed to read line from history file\");\n                            return None;\n                        }\n                    };\n\n                    if idx == offset {\n                        match serde_json::from_str::<HistoryEntry>(&line) {\n                            Ok(entry) => return Some(entry),\n                            Err(e) => {\n                                tracing::warn!(error = %e, \"failed to parse history entry\");\n                                return None;\n                            }\n                        }\n                    }\n                }\n                // Not found at requested offset.\n                return None;\n            }\n            Err(std::fs::TryLockError::WouldBlock) => {\n                std::thread::sleep(RETRY_SLEEP);\n            }\n            Err(e) => {\n                tracing::warn!(error = %e, \"failed to acquire shared lock on history file\");\n                return None;\n            }\n        }\n    }\n\n    None",
            "    // [codex-codemod] File::try_lock_shared not supported in WASI — read directly\n    {\n        let reader = BufReader::new(&file);\n        for (idx, line_res) in reader.lines().enumerate() {\n            let line = match line_res {\n                Ok(l) => l,\n                Err(e) => {\n                    tracing::warn!(error = %e, \"failed to read line from history file\");\n                    return None;\n                }\n            };\n\n            if idx == offset {\n                match serde_json::from_str::<HistoryEntry>(&line) {\n                    Ok(entry) => return Some(entry),\n                    Err(e) => {\n                        tracing::warn!(error = %e, \"failed to parse history entry\");\n                        return None;\n                    }\n                }\n            }\n        }\n        return None;\n    }",
        );

        // --- tui/src/lib.rs: route tracing output to browser console via console_log ---
        // Replace stderr writer with console_log::MakeConsoleWriter so tracing
        // events go to console.log/warn/error instead of polluting the terminal.
        self.string_replace(
            "tui/src/lib.rs",
            ".with_writer(std::io::stderr)",
            ".with_writer(console_log::MakeConsoleWriter)",
        );
    }

    // -----------------------------------------------------------------------
    // AST-informed source-text code block replacements
    // -----------------------------------------------------------------------

    /// Find a needle string in the source and return its byte range `(start, start + len)`.
    /// Returns `None` if the needle is not found.
    fn find_source_range(&self, needle: &str) -> Option<(usize, usize)> {
        self.source
            .find(needle)
            .map(|start| (start, start + needle.len()))
    }

    /// Collect all AST-informed code block replacements.
    ///
    /// These transforms are "AST-informed" in that they only fire for the correct
    /// file (checked via `file_matches`), but use exact source-text matching for
    /// precise byte-range targeting. This is necessary because many of the patterns
    /// span multiple statements or involve complex method chains that are hard to
    /// match structurally in syn.
    fn collect_code_block_replacements(&mut self) {
        if self.file_path.is_none() {
            return;
        }

        self.replace_zstd_compression();
        self.replace_image_dimension_estimation();
        self.replace_zip_extraction();
        self.replace_arboard_clipboard();
        self.replace_reqwest_blocking();
        self.replace_offset_datetime_now_local();
        self.replace_is_terminal_checks();
        self.replace_home_dir_canonicalize();
        self.replace_stream_idle_timeout_map_response_stream();
        self.replace_stream_idle_timeout_call_sites();
        self.replace_stream_idle_timeout_try_run_sampling();
        self.replace_residency_requirement_type_mismatch();
        self.replace_tracing_subscriber_writer();
    }

    /// 1. zstd compression bypass (codex-client/src/transport.rs):
    /// Replace `RequestCompression::Zstd => (zstd::stream::encode_all(...), ...)`
    /// with `(json, http::HeaderValue::from_static("identity"))`.
    fn replace_zstd_compression(&mut self) {
        if !self.file_matches("codex-client/src/transport.rs") {
            return;
        }
        let needle = "RequestCompression::Zstd => (\n                        zstd::stream::encode_all(std::io::Cursor::new(json), 3)\n                            .map_err(|err| TransportError::Build(err.to_string()))?,\n                        http::HeaderValue::from_static(\"zstd\"),\n                    ),";
        let replacement = "RequestCompression::Zstd => (\n                        json,\n                        http::HeaderValue::from_static(\"identity\"),\n                    ),";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// 2. image dimension stub (core/src/context_manager/history.rs):
    /// Replace `image::load_from_memory` match + width/height extraction with
    /// fixed 1024x1024 dimensions.
    fn replace_image_dimension_estimation(&mut self) {
        if !self.file_matches("core/src/context_manager/history.rs") {
            return;
        }
        let needle = "        let dynamic = match image::load_from_memory(&bytes) {\n            Ok(dynamic) => dynamic,\n            Err(error) => {\n                tracing::trace!(\"failed to decode original-detail image bytes: {error}\");\n                return None;\n            }\n        };\n        let width = i64::from(dynamic.width());\n        let height = i64::from(dynamic.height());";
        let replacement = "        // image crate not available in WASM — skip image dimension estimation\n        let _ = &bytes;\n        let width: i64 = 1024;\n        let height: i64 = 1024;";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// 3. zip extraction stub (core/src/skills/remote.rs):
    /// Replace the `Cursor::new(bytes)` / `ZipArchive` block with a bail.
    fn replace_zip_extraction(&mut self) {
        if !self.file_matches("core/src/skills/remote.rs") {
            return;
        }
        let needle = "    let cursor = std::io::Cursor::new(bytes);\n    let mut archive = zip::ZipArchive::new(cursor).context(\"Failed to open zip archive\")?;\n    for i in 0..archive.len() {\n        let mut file = archive.by_index(i).context(\"Failed to read zip entry\")?;\n        if file.is_dir() {\n            continue;\n        }\n        let raw_name = file.name().to_string();\n        let normalized = normalize_zip_name(&raw_name, prefix_candidates);\n        let Some(normalized) = normalized else {\n            continue;\n        };\n        let file_path = safe_join(output_dir, &normalized)?;\n        if let Some(parent) = file_path.parent() {\n            std::fs::create_dir_all(parent)\n                .with_context(|| format!(\"Failed to create parent dir for {normalized}\"))?;\n        }\n        let mut out = std::fs::File::create(&file_path)\n            .with_context(|| format!(\"Failed to create file {normalized}\"))?;\n        std::io::copy(&mut file, &mut out)\n            .with_context(|| format!(\"Failed to write skill file {normalized}\"))?;\n    }\n    Ok(())";
        let replacement = "    let _ = (bytes, output_dir, prefix_candidates);\n    anyhow::bail!(\"zip extraction not available in WASM\")";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// 4. arboard clipboard stub (tui/src/clipboard_text.rs):
    /// Replace `match arboard::Clipboard::new() { ... }` with a simple error string.
    fn replace_arboard_clipboard(&mut self) {
        if !self.file_matches("tui/src/clipboard_text.rs") {
            return;
        }
        let needle = "    let error = match arboard::Clipboard::new() {\n        Ok(mut clipboard) => match clipboard.set_text(text.to_string()) {\n            Ok(()) => return Ok(()),\n            Err(err) => format!(\"clipboard unavailable: {err}\"),\n        },\n        Err(err) => format!(\"clipboard unavailable: {err}\"),\n    };";
        let replacement = "    let error = \"clipboard not available in WASM\".to_string();";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// 5. reqwest::blocking stub (tui/src/tooltips.rs):
    /// Replace the `reqwest::blocking::Client::builder()` chain with `None::<String>`.
    fn replace_reqwest_blocking(&mut self) {
        if !self.file_matches("tui/src/tooltips.rs") {
            return;
        }
        let needle = "        let client = reqwest::blocking::Client::builder()\n            .no_proxy()\n            .build()\n            .ok()?;\n        let response = client\n            .get(ANNOUNCEMENT_TIP_URL)\n            .timeout(Duration::from_millis(2000))\n            .send()\n            .ok()?;\n        response.error_for_status().ok()?.text().ok()";
        let replacement =
            "        // reqwest::blocking not available in WASM\n        None::<String>";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// 6. OffsetDateTime::now_local -> now_utc (core/src/rollout/recorder.rs):
    /// Replace `OffsetDateTime::now_local()...map_err(...)` with `OffsetDateTime::now_utc()`.
    fn replace_offset_datetime_now_local(&mut self) {
        if !self.file_matches("core/src/rollout/recorder.rs") {
            return;
        }
        let needle = "    let timestamp = OffsetDateTime::now_local()\n        .map_err(|e| IoError::other(format!(\"failed to get local time: {e}\")))?;";
        let replacement = "    let timestamp = OffsetDateTime::now_utc();";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// 7. is_terminal() check removal (tui/src/tui.rs):
    /// Replace `if !stdin().is_terminal()` and `if !stdout().is_terminal()` blocks
    /// with a comment.
    fn replace_is_terminal_checks(&mut self) {
        if !self.file_matches("tui/src/tui.rs") {
            return;
        }
        let needle = "    if !stdin().is_terminal() {\n        return Err(std::io::Error::other(\"stdin is not a terminal\"));\n    }\n    if !stdout().is_terminal() {\n        return Err(std::io::Error::other(\"stdout is not a terminal\"));\n    }";
        let replacement = "    // [codex-codemod] is_terminal() checks skipped — WASM stdin/stdout are ghostty-web terminal";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// 8. home-dir canonicalize skip (utils/home-dir/src/lib.rs):
    /// Replace `path.canonicalize().map_err(|err| { ... })` with `Ok(path)`.
    fn replace_home_dir_canonicalize(&mut self) {
        if !self.file_matches("utils/home-dir/src/lib.rs") {
            return;
        }
        let needle = "                path.canonicalize().map_err(|err| {\n                    std::io::Error::new(\n                        err.kind(),\n                        format!(\"failed to canonicalize CODEX_HOME {val:?}: {err}\"),\n                    )\n                })";
        let replacement = "                Ok(path)";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    // -----------------------------------------------------------------------
    // Stream idle timeout transforms (core/src/client.rs, core/src/codex.rs)
    // -----------------------------------------------------------------------

    /// Add idle_timeout parameter to map_response_stream and wrap api_stream.next()
    /// with tokio::time::timeout so a stalled SSE stream errors out instead of
    /// hanging forever.
    fn replace_stream_idle_timeout_map_response_stream(&mut self) {
        if !self.file_matches("core/src/client.rs") {
            return;
        }
        // Signature + opening of the spawned task loop
        let needle = "fn map_response_stream<S>(\n    api_stream: S,\n    session_telemetry: SessionTelemetry,\n) -> (ResponseStream, oneshot::Receiver<LastResponse>)\nwhere\n    S: futures::Stream<Item = std::result::Result<ResponseEvent, ApiError>>\n        + Unpin\n        + Send\n        + 'static,\n{\n    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(1600);\n    let (tx_last_response, rx_last_response) = oneshot::channel::<LastResponse>();\n\n    tokio::spawn(async move {\n        let mut logged_error = false;\n        let mut tx_last_response = Some(tx_last_response);\n        let mut items_added: Vec<ResponseItem> = Vec::new();\n        let mut api_stream = api_stream;\n        while let Some(event) = api_stream.next().await {";
        let replacement = "fn map_response_stream<S>(\n    api_stream: S,\n    session_telemetry: SessionTelemetry,\n    idle_timeout: Duration,\n) -> (ResponseStream, oneshot::Receiver<LastResponse>)\nwhere\n    S: futures::Stream<Item = std::result::Result<ResponseEvent, ApiError>>\n        + Unpin\n        + Send\n        + 'static,\n{\n    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(1600);\n    let (tx_last_response, rx_last_response) = oneshot::channel::<LastResponse>();\n\n    tokio::spawn(async move {\n        let mut logged_error = false;\n        let mut tx_last_response = Some(tx_last_response);\n        let mut items_added: Vec<ResponseItem> = Vec::new();\n        let mut api_stream = api_stream;\n        loop {\n            let event = match tokio::time::timeout(idle_timeout, api_stream.next()).await {\n                Ok(Some(event)) => event,\n                Ok(None) => break, // stream ended normally\n                Err(_elapsed) => {\n                    // Idle timeout -- no event received within the deadline.\n                    tracing::warn!(\n                        timeout_secs = idle_timeout.as_secs(),\n                        \"API response stream idle timeout -- no events received\"\n                    );\n                    let _ = tx_event\n                        .send(Err(CodexErr::Stream(\n                            format!(\n                                \"stream idle timeout: no events for {}s\",\n                                idle_timeout.as_secs()\n                            ),\n                            None,\n                        )))\n                        .await;\n                    break;\n                }\n            };";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// Update the three call sites of map_response_stream to pass idle_timeout.
    fn replace_stream_idle_timeout_call_sites(&mut self) {
        if !self.file_matches("core/src/client.rs") {
            return;
        }
        // Call site 1: fixture path
        self.replace_in_file(
            "core/src/client.rs",
            "let (stream, _last_request_rx) = map_response_stream(stream, session_telemetry.clone());",
            "let (stream, _last_request_rx) = map_response_stream(stream, session_telemetry.clone(), self.client.state.provider.stream_idle_timeout());",
        );
        // Call site 2: SSE streaming
        self.replace_in_file(
            "core/src/client.rs",
            "let (stream, _) = map_response_stream(stream, session_telemetry.clone());",
            "let (stream, _) = map_response_stream(stream, session_telemetry.clone(), self.client.state.provider.stream_idle_timeout());",
        );
        // Call site 3: WebSocket streaming
        self.replace_in_file(
            "core/src/client.rs",
            "map_response_stream(stream_result, session_telemetry.clone());",
            "map_response_stream(stream_result, session_telemetry.clone(), self.client.state.provider.stream_idle_timeout());",
        );
    }

    /// Wrap stream.next() in try_run_sampling_request with tokio::time::timeout.
    fn replace_stream_idle_timeout_try_run_sampling(&mut self) {
        if !self.file_matches("core/src/codex.rs") {
            return;
        }
        let needle = "        let event = match stream\n            .next()\n            .instrument(trace_span!(parent: &handle_responses, \"receiving\"))\n            .or_cancel(&cancellation_token)\n            .await\n        {\n            Ok(event) => event,\n            Err(codex_async_utils::CancelErr::Cancelled) => break Err(CodexErr::TurnAborted),\n        };\n\n        let event = match event {\n            Some(res) => res?,\n            None => {\n                break Err(CodexErr::Stream(\n                    \"stream closed before response.completed\".into(),\n                    None,\n                ));\n            }\n        };";
        let replacement = "        let stream_idle_timeout = turn_context.provider.stream_idle_timeout();\n        let event = match tokio::time::timeout(\n            stream_idle_timeout,\n            stream\n                .next()\n                .instrument(trace_span!(parent: &handle_responses, \"receiving\")),\n        )\n        .or_cancel(&cancellation_token)\n        .await\n        {\n            Ok(Ok(event)) => event,\n            Ok(Err(_elapsed)) => {\n                tracing::warn!(\n                    timeout_secs = stream_idle_timeout.as_secs(),\n                    \"stream.next() idle timeout in try_run_sampling_request\"\n                );\n                break Err(CodexErr::Stream(\n                    format!(\n                        \"stream idle timeout: no response events for {}s\",\n                        stream_idle_timeout.as_secs()\n                    ),\n                    None,\n                ));\n            }\n            Err(codex_async_utils::CancelErr::Cancelled) => break Err(CodexErr::TurnAborted),\n        };\n\n        let event = match event {\n            Some(res) => res?,\n            None => {\n                break Err(CodexErr::Stream(\n                    \"stream closed before response.completed\".into(),\n                    None,\n                ));\n            }\n        };";
        if let Some((start, end)) = self.find_source_range(needle) {
            self.edits.push(Edit {
                start,
                end,
                replacement: replacement.to_string(),
            });
        }
    }

    /// Fix type mismatch: set_default_client_residency_requirement expects
    /// Option<ResidencyRequirement>, not Option<()>. Replaces all occurrences.
    fn replace_residency_requirement_type_mismatch(&mut self) {
        if !self.file_matches("tui/src/lib.rs") {
            return;
        }
        let find = "set_default_client_residency_requirement(config.enforce_residency.value().map(|_| ()));";
        let replace = "set_default_client_residency_requirement(config.enforce_residency.value());";
        let mut search_from = 0;
        while let Some(rel) = self.source[search_from..].find(find) {
            let start = search_from + rel;
            self.edits.push(Edit {
                start,
                end: start + find.len(),
                replacement: replace.to_string(),
            });
            search_from = start + find.len();
        }
    }

    /// Replace tracing_subscriber's stderr writer with MakeConsoleWriter so tracing
    /// events (ERROR, WARN, etc.) route to the browser console instead of WASM stderr.
    fn replace_tracing_subscriber_writer(&mut self) {
        if !self.file_matches("tui/src/lib.rs") {
            return;
        }
        self.replace_in_file(
            "tui/src/lib.rs",
            ".with_writer(std::io::stderr)",
            ".with_writer(console_log::MakeConsoleWriter)",
        );
    }

    // -----------------------------------------------------------------------
    // TUI-specific transforms (using replace_in_file for idempotency)
    // -----------------------------------------------------------------------

    /// Collect TUI-specific edits that handle code transformations for the TUI crate
    /// and its dependencies. These are string-level replacements that cannot be expressed
    /// as AST-level transforms because they involve multi-line patterns, import stubs,
    /// or cross-cutting concerns.
    ///
    /// Called from `apply_with_path` after the syn visitor runs but before edit sorting/dedup.
    fn collect_tui_specific_edits(&mut self) {
        // Only run if we have a file path
        if self.file_path.is_none() {
            return;
        }

        // 1. non_blocking writer → stderr (tui/src/lib.rs)
        // Skip non_blocking writer (spawns a thread, which panics in WASM).
        // Use stderr directly — in WASM it goes to the browser console via wasi:cli/stderr.
        self.replace_in_file(
            "tui/src/lib.rs",
            "    let (non_blocking, _guard) = non_blocking(log_file);\n\n    // use RUST_LOG env var, default to info for codex crates.\n    let env_filter = || {\n        EnvFilter::try_from_default_env().unwrap_or_else(|_| {\n            EnvFilter::new(\"codex_core=info,codex_tui=info,codex_rmcp_client=info\")\n        })\n    };\n\n    let file_layer = tracing_subscriber::fmt::layer()\n        .with_writer(non_blocking)",
            "    // [codex-codemod] non_blocking replaced with stderr (no thread spawning in WASM)\n    let _guard = ();\n\n    let env_filter = || {\n        EnvFilter::try_from_default_env().unwrap_or_else(|_| {\n            EnvFilter::new(\"codex_core=warn,codex_tui=warn\")\n        })\n    };\n\n    let file_layer = tracing_subscriber::fmt::layer()\n        .with_writer(std::io::stderr)",
        );

        // 2. frame_requester struct (tui/src/tui/frame_requester.rs)
        // Bypass FrameScheduler (dropped by tokio::spawn) and send draw events directly.
        self.replace_in_file(
            "tui/src/tui/frame_requester.rs",
            "#[derive(Clone, Debug)]\npub struct FrameRequester {\n    frame_schedule_tx: mpsc::UnboundedSender<Instant>,\n}",
            "#[derive(Clone)]\npub struct FrameRequester {\n    frame_schedule_tx: mpsc::UnboundedSender<Instant>,\n    draw_tx: broadcast::Sender<()>,\n}\nimpl std::fmt::Debug for FrameRequester {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        f.debug_struct(\"FrameRequester\").finish()\n    }\n}",
        );
        self.replace_in_file(
            "tui/src/tui/frame_requester.rs",
            "        let scheduler = FrameScheduler::new(rx, draw_tx);\n        tokio::spawn(scheduler.run());\n        Self {\n            frame_schedule_tx: tx,\n        }",
            "        let draw_tx_clone = draw_tx.clone();\n        let scheduler = FrameScheduler::new(rx, draw_tx);\n        tokio::spawn(scheduler.run());\n        Self {\n            frame_schedule_tx: tx,\n            draw_tx: draw_tx_clone,\n        }",
        );
        self.replace_in_file(
            "tui/src/tui/frame_requester.rs",
            "    pub fn schedule_frame(&self) {\n        let _ = self.frame_schedule_tx.send(Instant::now());\n    }",
            "    pub fn schedule_frame(&self) {\n        let _ = self.frame_schedule_tx.send(Instant::now());\n        let _ = self.draw_tx.send(());\n    }",
        );
        self.replace_in_file(
            "tui/src/tui/frame_requester.rs",
            "    pub fn schedule_frame_in(&self, dur: Duration) {\n        let _ = self.frame_schedule_tx.send(Instant::now() + dur);\n    }",
            "    pub fn schedule_frame_in(&self, dur: Duration) {\n        let _ = self.frame_schedule_tx.send(Instant::now() + dur);\n        let _ = self.draw_tx.send(());\n    }",
        );

        // 3. codex_utils_oss import stub (tui/src/lib.rs)
        self.replace_in_file(
            "tui/src/lib.rs",
            "use codex_utils_oss::ensure_oss_provider_ready;\nuse codex_utils_oss::get_default_model_for_oss_provider;",
            "// codex-utils-oss stripped for WASM — OSS providers not available\nasync fn ensure_oss_provider_ready(_provider_id: &str, _config: &codex_core::config::Config) -> Result<(), std::io::Error> { Ok(()) }\nfn get_default_model_for_oss_provider(_provider_id: &str) -> Option<&'static str> { None }",
        );

        // 4. cloud_requirements_loader import stub (tui/src/lib.rs)
        self.replace_in_file(
            "tui/src/lib.rs",
            "use codex_cloud_requirements::cloud_requirements_loader;",
            "// codex-cloud-requirements stripped for WASM — cloud config not needed\nfn cloud_requirements_loader(\n    _auth_manager: std::sync::Arc<codex_core::AuthManager>,\n    _chatgpt_base_url: String,\n    _codex_home: std::path::PathBuf,\n) -> codex_core::config_loader::CloudRequirementsLoader {\n    codex_core::config_loader::CloudRequirementsLoader::default()\n}",
        );

        // 5. InProcessAppServerClient import stub (tui/src/app.rs)
        self.replace_in_file(
            "tui/src/app.rs",
            "use codex_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;\nuse codex_app_server_client::InProcessAppServerClient;\nuse codex_app_server_client::InProcessClientStartArgs;",
            "// codex-app-server-client stripped for WASM\n#[allow(dead_code)]\nconst DEFAULT_IN_PROCESS_CHANNEL_CAPACITY: usize = 64;\n#[allow(dead_code)] struct InProcessAppServerClient;\nimpl InProcessAppServerClient {\n    async fn start(_args: InProcessClientStartArgs) -> color_eyre::Result<Self> { color_eyre::eyre::bail!(\"not available in WASM\") }\n    fn request_handle(&self) -> InProcessRequestHandle { InProcessRequestHandle }\n    async fn shutdown(&self) -> color_eyre::Result<()> { Ok(()) }\n}\n#[allow(dead_code)] struct InProcessRequestHandle;\nimpl InProcessRequestHandle {\n    async fn request_typed<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(&self, _req: Req) -> color_eyre::Result<Resp> { color_eyre::eyre::bail!(\"not available\") }\n}\n#[allow(dead_code)] struct InProcessClientStartArgs { _private: () }",
        );

        // 6. BackendClient import stub (tui/src/chatwidget.rs)
        self.replace_in_file(
            "tui/src/chatwidget.rs",
            "use codex_backend_client::Client as BackendClient;",
            "// codex-backend-client stripped for WASM — rate limit checking not available\n#[allow(dead_code)]\nstruct BackendClient;\nimpl BackendClient {\n    fn from_auth(_base_url: impl AsRef<str>, _auth: &codex_login::CodexAuth) -> Result<Self, std::io::Error> {\n        Err(std::io::Error::other(\"backend client not available in WASM\"))\n    }\n    async fn get_rate_limits_many(&self) -> Result<RateLimitsResponse, std::io::Error> {\n        Err(std::io::Error::other(\"not available in WASM\"))\n    }\n}\n#[allow(dead_code)]\nstruct RateLimitsResponse;\nimpl RateLimitsResponse {\n    fn rate_limits(&self) -> Vec<()> { Vec::new() }\n}",
        );

        // 7. connectors::list_all_connectors_with_options stub (tui/src/chatwidget.rs)
        self.replace_in_file(
            "tui/src/chatwidget.rs",
            "connectors::list_all_connectors_with_options(&config, force_refetch).await?",
            "{ let _ = (&config, force_refetch); Vec::<connectors::AppInfo>::new() }",
        );

        // 8. merge_plugin_apps_with_accessible fixes (tui/src/chatwidget.rs)
        self.replace_in_file(
            "tui/src/chatwidget.rs",
            "merge_plugin_apps_with_accessible(\n                    all_connectors,\n                    accessible_connectors,\n                    /*all_connectors_loaded*/ true,\n                )",
            "merge_plugin_apps_with_accessible(\n                    Vec::new(),\n                    accessible_connectors,\n                )",
        );
        self.replace_in_file(
            "tui/src/chatwidget.rs",
            "merge_plugin_apps_with_accessible(\n                        Vec::new(),\n                        snapshot.connectors,\n                        /*all_connectors_loaded*/ false,\n                    )",
            "merge_plugin_apps_with_accessible(\n                        Vec::new(),\n                        snapshot.connectors,\n                    )",
        );

        // 9. set_default_client_residency_requirement: no transform needed — real type now available
        // 10. forced_login_method: no transform needed — using codex_protocol paths directly

        // 11. Multiplexer::Zellij pattern fix (tui/src/lib.rs)
        self.replace_in_file(
            "tui/src/lib.rs",
            "!matches!(terminal_info.multiplexer, Some(Multiplexer::Zellij { .. }))",
            "!matches!(terminal_info.multiplexer, Some(ref m) if m.name == codex_terminal_detection::MultiplexerName::Zellij)",
        );

        // 12. from_auth_storage stub (tui/src/lib.rs)
        self.replace_in_file(
            "tui/src/lib.rs",
            "match CodexAuth::from_auth_storage(&codex_home, config.cli_auth_credentials_store_mode) {\n            Ok(Some(auth)) => LoginStatus::AuthMode(auth.auth_mode()),",
            "match CodexAuth::from_auth_storage(&codex_home, config.cli_auth_credentials_store_mode) {\n            Ok(Some(auth)) => LoginStatus::AuthMode(codex_login::AuthMode::ApiKey),",
        );

        // 13. TerminalName match simplification (tui/src/chatwidget.rs)
        self.replace_in_file(
            "tui/src/chatwidget.rs",
            "TerminalName::AppleTerminal | TerminalName::WarpTerminal | TerminalName::VsCode => {\n            key_hint::shift(KeyCode::Left)\n        }\n        TerminalName::Ghostty\n        | TerminalName::Iterm2\n        | TerminalName::WezTerm\n        | TerminalName::Kitty\n        | TerminalName::Alacritty\n        | TerminalName::Konsole\n        | TerminalName::GnomeTerminal\n        | TerminalName::Vte\n        | TerminalName::WindowsTerminal\n        | TerminalName::Dumb\n        | TerminalName::Unknown => key_hint::alt(KeyCode::Up),",
            "_ => key_hint::alt(KeyCode::Up),",
        );

        // 14. UnboundedReceiverStream removal (tui/src/resume_picker.rs)
        self.replace_in_file(
            "tui/src/resume_picker.rs",
            "let mut background_events = UnboundedReceiverStream::new(bg_rx).fuse();",
            "let mut background_events = bg_rx;",
        );

        // 15. BroadcastStream/WatchStream inline stubs (tui/src/tui/event_stream.rs)
        self.replace_in_file(
            "tui/src/tui/event_stream.rs",
            "use tokio_stream::wrappers::BroadcastStream;\nuse tokio_stream::wrappers::WatchStream;\nuse tokio_stream::wrappers::errors::BroadcastStreamRecvError;",
            "/// Thin WatchStream wrapper for our shim watch::Receiver.\nstruct WatchStream<T: Clone>(tokio::sync::watch::Receiver<T>);\nimpl<T: Clone> WatchStream<T> {\n    fn from_changes(rx: tokio::sync::watch::Receiver<T>) -> Self { Self(rx) }\n}\nimpl<T: Clone + Unpin> WatchStream<T> {\n    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<T>> {\n        let this = self.get_mut();\n        match this.0.poll_changed(cx.waker()) {\n            Ok(true) => Poll::Ready(Some(this.0.borrow_and_update().clone())),\n            Ok(false) => Poll::Pending,\n            Err(_) => Poll::Ready(None),\n        }\n    }\n}\n/// Thin BroadcastStream wrapper for our shim broadcast::Receiver.\nstruct BroadcastStream<T: Clone>(tokio::sync::broadcast::Receiver<T>);\nimpl<T: Clone> BroadcastStream<T> {\n    fn new(rx: tokio::sync::broadcast::Receiver<T>) -> Self { Self(rx) }\n}\n#[derive(Debug)]\nenum BroadcastStreamRecvError { Lagged(u64) }\nimpl<T: Clone + Unpin> BroadcastStream<T> {\n    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Result<T, BroadcastStreamRecvError>>> {\n        match self.get_mut().0.poll_recv(cx.waker()) {\n            Ok(val) => Poll::Ready(Some(Ok(val))),\n            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(n)))),\n            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => Poll::Pending,\n            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => Poll::Ready(None),\n        }\n    }\n}",
        );

        // 16. fetch_rate_limits stub (tui/src/chatwidget.rs)
        self.replace_in_file(
            "tui/src/chatwidget.rs",
            "async fn fetch_rate_limits(base_url: String, auth: CodexAuth) -> Vec<RateLimitSnapshot> {\n    match BackendClient::from_auth(base_url, &auth) {\n        Ok(client) => match client.get_rate_limits_many().await {\n            Ok(snapshots) => snapshots,",
            "async fn fetch_rate_limits(base_url: String, auth: CodexAuth) -> Vec<RateLimitSnapshot> {\n    let _ = (base_url, auth);\n    return Vec::new();\n    #[allow(unreachable_code)]\n    match BackendClient::from_auth(String::new(), &CodexAuth::from_api_key(\"\")) {\n        Ok(client) => match client.get_rate_limits_many().await {\n            Ok(_snapshots) => Vec::new(),",
        );

        // 17. account_plan_type None (tui/src/chatwidget.rs)
        self.replace_in_file(
            "tui/src/chatwidget.rs",
            "self.auth_manager\n                .auth_cached()\n                .and_then(|auth| auth.account_plan_type()),",
            "None::<codex_protocol::account::PlanType>,",
        );

        // 18. feedback_diagnostics borrow (tui/src/chatwidget.rs)
        self.replace_in_file(
            "tui/src/chatwidget.rs",
            "            snapshot.feedback_diagnostics(),\n        );\n        self.bottom_pane.show_selection_view(params);",
            "            &snapshot.feedback_diagnostics(),\n        );\n        self.bottom_pane.show_selection_view(params);",
        );

        // 19. thread_id unwrap_or_default (tui/src/bottom_pane/feedback_view.rs)
        self.replace_in_file(
            "tui/src/bottom_pane/feedback_view.rs",
            "        let mut thread_id = self.snapshot.thread_id.clone();\n\n        let result = self.snapshot.upload_feedback(",
            "        let mut thread_id = self.snapshot.thread_id.clone().unwrap_or_default();\n\n        let result = self.snapshot.upload_feedback(",
        );

        // 20. None type inference (tui/src/bottom_pane/feedback_view.rs)
        self.replace_in_file(
            "tui/src/bottom_pane/feedback_view.rs",
            "/*logs_override*/ None,",
            "/*logs_override*/ None::<Vec<u8>>,",
        );

        // 21. sync_update return type (tui/src/tui.rs)
        self.replace_in_file(
            "tui/src/tui.rs",
            "            terminal.draw(|frame| {\n                draw_fn(frame);\n            })\n        })?\n    }",
            "            terminal.draw(|frame| {\n                draw_fn(frame);\n            })\n        })?;\n        Ok(())\n    }",
        );

        // 22. codex_delegate async conversion (core/src/codex_delegate.rs)
        // Convert thread::spawn+block_on to tokio::spawn async
        self.replace_in_file(
            "core/src/codex_delegate.rs",
            "std::thread::spawn(move || {\n        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()\n            .enable_all()\n            .build()\n        else {\n            let _ = tx.send(ReviewDecision::Denied);\n            return;\n        };\n        let decision = runtime.block_on(review_approval_request_with_cancel(\n            &session,\n            &turn,\n            request,\n            retry_reason,\n            cancel_token,\n        ));\n        let _ = tx.send(decision);\n    });",
            "tokio::spawn(async move {\n        let decision = review_approval_request_with_cancel(\n            &session,\n            &turn,\n            request,\n            retry_reason,\n            cancel_token,\n        ).await;\n        let _ = tx.send(decision);\n    });",
        );

        // 23. webbrowser::open — handled by wasi-webbrowser shim crate (no inline edit needed)

        // 24. InProcessAppServerClient start bail (tui/src/app.rs)
        self.replace_in_file(
            "tui/src/app.rs",
            "    InProcessAppServerClient::start(InProcessClientStartArgs {\n        arg0_paths,\n        config_warnings: config_warning_notifications(&config),\n        config: Arc::new(config),\n        cli_overrides: cli_kv_overrides,\n        loader_overrides,\n        cloud_requirements,\n        feedback,\n        session_source: SessionSource::Cli,\n        enable_codex_api_key_env: false,\n        client_name: \"codex-tui\".to_string(),\n        client_version: env!(\"CARGO_PKG_VERSION\").to_string(),\n        experimental_api: true,\n        opt_out_notification_methods: Vec::new(),\n        channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,\n    })\n    .await\n    .wrap_err(\"failed to start embedded app server for plugin request\")",
            "    { let _ = (&arg0_paths, &config, &cli_kv_overrides, &loader_overrides, &cloud_requirements, &feedback); color_eyre::eyre::bail!(\"plugin requests not available in WASM\") }",
        );

        // 25. compile_error → create_symlink stub (utils/git/src/platform.rs)
        self.replace_in_file(
            "utils/git/src/platform.rs",
            "#[cfg(not(any(unix, windows)))]\ncompile_error!(\"codex-git symlink support is only implemented for Unix and Windows\");",
            "#[cfg(not(any(unix, windows)))]\npub fn create_symlink(\n    _source: &Path,\n    _link_target: &Path,\n    _destination: &Path,\n) -> Result<(), GitToolingError> {\n    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, \"symlinks not supported on wasm32\").into())\n}",
        );
    }
}

/// Find the byte offset of `=>` in `text` that is not inside braces/parens/brackets.
/// Returns offset relative to `text` start (pointing at `=`).
fn find_fat_arrow_outside_braces(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 && bytes[i + 1] == b'>' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Use-item removal predicate
// ---------------------------------------------------------------------------

/// Returns true if a `use` item should be entirely removed.
fn should_remove_use(use_item: &ItemUse) -> bool {
    match &use_item.tree {
        UseTree::Path(p) if p.ident == "ts_rs" => match p.tree.as_ref() {
            UseTree::Name(n) => n.ident == "TS",
            UseTree::Glob(_) => true,
            _ => false,
        },
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Token-level transforms for macro bodies
// ---------------------------------------------------------------------------

/// Strip TS-related tokens from macro_rules body token streams.
fn strip_ts_from_macro_tokens(tokens: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    use proc_macro2::{Delimiter, Group, TokenTree};

    let tokens_vec: Vec<TokenTree> = tokens.into_iter().collect();
    let mut result: Vec<TokenTree> = Vec::new();
    let len = tokens_vec.len();
    let mut i = 0;

    while i < len {
        // Pattern: `# [ts(...)]` — remove the whole attribute
        if i + 1 < len {
            if matches!(&tokens_vec[i], TokenTree::Punct(p) if p.as_char() == '#') {
                if let TokenTree::Group(bracket) = &tokens_vec[i + 1] {
                    if bracket.delimiter() == Delimiter::Bracket {
                        let inner: Vec<TokenTree> = bracket.stream().into_iter().collect();
                        if !inner.is_empty() {
                            let is_ts_attr = match &inner[0] {
                                TokenTree::Ident(id) => id == "ts" || id == "ts_rs",
                                _ => false,
                            };
                            if is_ts_attr {
                                i += 2;
                                continue;
                            }
                        }
                    }
                }
            }
        }

        // Pattern: `[pub [(...)] ] fn <name> (...) [-> ...] { body }`
        // If ANY token in the function (signature + body) contains `ts_rs`,
        // drop the entire function definition.
        //
        // proc_macro2 already parsed delimiters into nested Group tokens,
        // so we just scan forward for the first Brace group after `fn`.
        if let TokenTree::Ident(id) = &tokens_vec[i] {
            if id == "fn" {
                // Walk backwards in result to find preceding `pub` / `pub(crate)`
                let fn_start = {
                    let mut s = result.len();
                    // Check for pub(crate) — Group(Parenthesized) then Ident("pub")
                    if s >= 2 {
                        if let TokenTree::Group(g) = &result[s - 1] {
                            if g.delimiter() == proc_macro2::Delimiter::Parenthesis {
                                if let TokenTree::Ident(prev) = &result[s - 2] {
                                    if prev == "pub" {
                                        s -= 2;
                                    }
                                }
                            }
                        }
                    }
                    // Check for bare pub
                    if s == result.len() && s >= 1 {
                        if let TokenTree::Ident(prev) = &result[s - 1] {
                            if prev == "pub" {
                                s -= 1;
                            }
                        }
                    }
                    s
                };

                // Scan forward: collect fn keyword + everything through body brace Group
                let mut j = i;
                let mut fn_tokens_str = String::new();
                let mut found_body = false;
                while j < len {
                    fn_tokens_str.push_str(&tokens_vec[j].to_string());
                    fn_tokens_str.push(' ');
                    if let TokenTree::Group(g) = &tokens_vec[j] {
                        if g.delimiter() == proc_macro2::Delimiter::Brace {
                            found_body = true;
                            j += 1;
                            break;
                        }
                    }
                    j += 1;
                }

                if found_body && fn_tokens_str.contains("ts_rs") {
                    // Drop everything: truncate result back to before pub, skip past body
                    result.truncate(fn_start);
                    i = j;
                    continue;
                }
                // Not a ts_rs function — fall through to normal token processing
            }
        }

        // Pattern: `, TS` or `TS ,` inside groups (derive lists)
        match &tokens_vec[i] {
            TokenTree::Ident(id) if id == "TS" => {
                let has_preceding_comma =
                    matches!(result.last(), Some(TokenTree::Punct(p)) if p.as_char() == ',');
                let has_following_comma = i + 1 < len
                    && matches!(&tokens_vec[i + 1], TokenTree::Punct(p) if p.as_char() == ',');

                if has_preceding_comma {
                    result.pop();
                } else if has_following_comma {
                    i += 1;
                }
                i += 1;
                continue;
            }
            TokenTree::Group(group) => {
                let inner = strip_ts_from_macro_tokens(group.stream());
                let new_group = Group::new(group.delimiter(), inner);
                result.push(TokenTree::Group(new_group));
                i += 1;
                continue;
            }
            _ => {}
        }

        result.push(tokens_vec[i].clone());
        i += 1;
    }

    result.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Helper predicates
// ---------------------------------------------------------------------------

fn is_derive_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("derive")
}

fn is_ts_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("ts")
}

fn is_ts_rs_ts_attr(attr: &Attribute) -> bool {
    let path = attr.path();
    if path.segments.len() == 2 {
        path.segments[0].ident == "ts_rs" && path.segments[1].ident == "TS"
    } else {
        false
    }
}

fn is_tokio_main_attr(attr: &Attribute) -> bool {
    let path = attr.path();
    if path.segments.len() == 2 {
        path.segments[0].ident == "tokio" && path.segments[1].ident == "main"
    } else {
        false
    }
}

fn is_tokio_test_attr(attr: &Attribute) -> bool {
    let path = attr.path();
    if path.segments.len() == 2 {
        path.segments[0].ident == "tokio" && path.segments[1].ident == "test"
    } else {
        false
    }
}

/// Check if a path refers to `TS` (the ts-rs derive macro).
fn is_ts_derive_path(path: &Path) -> bool {
    path.segments.len() == 1 && path.segments[0].ident == "TS"
}

fn is_select_macro_path(path: &Path) -> bool {
    let segs = &path.segments;
    match segs.len() {
        1 => segs[0].ident == "select",
        2 => segs[0].ident == "tokio" && segs[1].ident == "select",
        _ => false,
    }
}

/// Check if an attribute is `#[cfg(unix)]`.
fn is_cfg_unix_attr(attr: &Attribute) -> bool {
    is_cfg_attr_with(attr, "unix")
}

/// Check if an attribute is `#[cfg(target)]` where target is a simple ident like "unix" or "windows".
fn is_cfg_attr_with(attr: &Attribute, target: &str) -> bool {
    if !attr.path().is_ident("cfg") {
        return false;
    }
    let tokens_str = quote::quote!(#attr).to_string();
    tokens_str.contains(target) && !tokens_str.contains("any") && !tokens_str.contains("not")
}

// is_sqlx_migrate_macro_path removed — migrate! handled by proc macro

/// Parse the paths inside a `#[derive(...)]` attribute.
fn parse_derive_paths(attr: &Attribute) -> syn::Result<Vec<Path>> {
    let mut paths = Vec::new();
    attr.parse_nested_meta(|meta| {
        paths.push(meta.path);
        Ok(())
    })?;
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_row_preserved() {
        // FromRow is now provided by wasi-sqlx-macros — should NOT be stripped
        let input = r#"
use sqlx::FromRow;

#[derive(sqlx::FromRow)]
struct Foo {
    id: i64,
}
"#;
        let result = apply(input);
        assert!(result.is_none(), "FromRow should not trigger changes");
    }

    #[test]
    fn test_turbofish_query_as() {
        let input = r#"
fn foo() {
    let rows = sqlx::query_as::<_, MyType>("SELECT * FROM t");
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            result.contains("query_as::<MyType>"),
            "turbofish should have _ removed: {result}"
        );
        assert!(
            !result.contains("query_as::<_, MyType>"),
            "old turbofish should be gone"
        );
    }

    #[test]
    fn test_turbofish_query_scalar() {
        let input = r#"
fn foo() {
    let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM t");
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            result.contains("query_scalar::<i64>"),
            "turbofish should have _ removed: {result}"
        );
    }

    #[test]
    fn test_strip_ts_rs_import() {
        let input = r#"
use ts_rs::TS;

#[derive(Debug, TS)]
struct Foo {
    x: i32,
}
"#;
        let result = apply(input).expect("should transform");
        assert!(!result.contains("ts_rs"), "ts_rs import should be removed");
        assert!(!result.contains("TS"), "TS derive should be stripped");
        assert!(result.contains("Debug"), "Debug should remain");
    }

    #[test]
    fn test_strip_ts_rs_glob_import() {
        let input = r#"
use ts_rs::*;

struct Foo;
"#;
        let result = apply(input).expect("should transform");
        assert!(
            !result.contains("ts_rs"),
            "ts_rs glob import should be removed"
        );
    }

    #[test]
    fn test_strip_ts_attr() {
        let input = r#"
#[derive(Debug)]
#[ts(export)]
struct Foo {
    #[ts(rename = "bar")]
    x: i32,
}
"#;
        let result = apply(input).expect("should transform");
        assert!(!result.contains("#[ts("), "ts attrs should be stripped");
        assert!(result.contains("struct Foo"), "struct should remain");
    }

    #[test]
    fn test_strip_ts_rs_ts_attr() {
        let input = r#"
#[ts_rs::TS(export)]
#[derive(Debug)]
struct Foo {
    x: i32,
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            !result.contains("ts_rs::TS"),
            "ts_rs::TS attr should be stripped"
        );
    }

    #[test]
    fn test_tokio_main_stripped() {
        let input = r#"
#[tokio::main]
async fn main() {
    println!("hello");
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            !result.contains("tokio::main"),
            "tokio::main should be stripped"
        );
        assert!(result.contains("async fn main"), "fn main should remain");
    }

    #[test]
    fn test_tokio_main_with_flavor() {
        let input = r#"
#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("hello");
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            !result.contains("tokio::main"),
            "tokio::main(flavor) should be stripped"
        );
    }

    #[test]
    fn test_tokio_test_to_test() {
        let input = r#"
#[tokio::test]
async fn test_something() {
    assert!(true);
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            !result.contains("tokio::test"),
            "tokio::test should be gone"
        );
        assert!(result.contains("#[test]"), "should have #[test]");
    }

    #[test]
    fn test_tokio_test_with_flavor() {
        let input = r#"
#[tokio::test(flavor = "multi_thread")]
async fn test_multi() {
    assert!(true);
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            !result.contains("tokio::test"),
            "tokio::test(flavor) should be gone"
        );
        assert!(result.contains("#[test]"), "should have #[test]");
    }

    #[test]
    fn test_thread_spawn_rewrite_std() {
        let input = r#"
fn foo() {
    std::thread::spawn(|| {
        println!("in thread");
    });
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            result.contains("tokio::thread_spawn::spawn"),
            "std::thread::spawn should become tokio::thread_spawn::spawn: {result}"
        );
    }

    #[test]
    fn test_thread_sleep_rewrite() {
        let input = r#"
fn foo() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            result.contains("tokio::thread_spawn::sleep"),
            "std::thread::sleep should become tokio::thread_spawn::sleep: {result}"
        );
    }

    #[test]
    fn test_thread_builder_rewrite() {
        let input = r#"
fn foo() {
    std::thread::Builder::new().name("worker".into()).spawn(|| {}).unwrap();
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            result.contains("tokio::thread_spawn::Builder"),
            "std::thread::Builder should become tokio::thread_spawn::Builder: {result}"
        );
    }

    #[test]
    fn test_chatgpt_connectors_rewrite() {
        let input = r#"
fn foo() {
    let c = codex_chatgpt::connectors::list();
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            result.contains("codex_core::connectors"),
            "codex_chatgpt should be rewritten to codex_core: {result}"
        );
        assert!(!result.contains("codex_chatgpt"), "old path should be gone");
    }

    #[test]
    fn test_merge_connectors_rename() {
        let input = r#"
fn foo() {
    connectors::merge_connectors_with_accessible(a, b);
}
"#;
        let result = apply(input).expect("should transform");
        assert!(
            result.contains("merge_plugin_apps_with_accessible"),
            "function should be renamed: {result}"
        );
        assert!(
            !result.contains("merge_connectors_with_accessible"),
            "old name should be gone"
        );
    }

    #[test]
    fn test_no_changes_returns_none() {
        let input = r#"
fn foo() {
    println!("hello");
}
"#;
        assert!(
            apply(input).is_none(),
            "should return None when nothing to transform"
        );
    }

    #[test]
    fn test_parse_failure_returns_none() {
        let input = "this is not valid rust {{{{";
        assert!(
            apply(input).is_none(),
            "should return None on parse failure"
        );
    }

    #[test]
    fn test_strip_ts_but_keep_from_row_combined() {
        let input = r#"
use ts_rs::TS;
use sqlx::FromRow;

#[derive(Debug, Clone, TS, FromRow)]
#[ts(export)]
struct Combo {
    id: i64,
    name: String,
}
"#;
        let result = apply(input).expect("should transform");
        assert!(!result.contains("TS"), "TS should be stripped");
        assert!(result.contains("FromRow"), "FromRow should be kept");
        assert!(!result.contains("#[ts("), "ts attr should be stripped");
        assert!(!result.contains("ts_rs"), "ts_rs import should be gone");
        assert!(result.contains("Debug"), "Debug should remain");
        assert!(result.contains("Clone"), "Clone should remain");
    }

    // sqlx::migrate! tests removed — macro is now handled by wasi-sqlx-macros proc macro

    #[test]
    fn test_select_with_biased_and_if_guard_preserves_formatting() {
        let input = r#"
async fn poll_loop() {
    loop {
        tokio::select! {
            biased;

            result = rx.recv(), if !buffer_full => {
                handle(result);
            }
            _ = ticker.tick() => {
                flush();
            }
            else => {
                break;
            }
        }
    }
}
"#;
        let result = apply(input).expect("should transform select body");
        // biased; should be gone
        assert!(
            !result.contains("biased;"),
            "biased; should be stripped: {result}"
        );
        // if guard should be gone
        assert!(
            !result.contains(", if !buffer_full"),
            "if guard should be stripped: {result}"
        );
        // else => should be replaced
        assert!(
            !result.contains("else =>"),
            "else => should be replaced: {result}"
        );
        assert!(
            result.contains("_ = async {} =>"),
            "else should become _ = async {{}} =>: {result}"
        );
        // Original formatting preserved: indentation, line breaks
        assert!(
            result.contains("            result = rx.recv()"),
            "original indentation should be preserved: {result}"
        );
        assert!(
            result.contains("            _ = ticker.tick() => {"),
            "other arms should keep formatting: {result}"
        );
    }

    #[test]
    fn test_combined_transforms_in_one_file() {
        let input = r#"
use ts_rs::TS;
use std::collections::HashMap;

#[derive(Debug, Clone, TS)]
#[ts(export)]
struct Config {
    name: String,
}

#[tokio::main]
async fn main() {
    let c = codex_chatgpt::connectors::list();
    std::thread::spawn(|| {});
    let migrator = sqlx::migrate!("./migrations");
    let rows = sqlx::query_as::<_, Config>("SELECT * FROM config");
}

#[tokio::test]
async fn test_it() {
    assert!(true);
}
"#;
        let result = apply(input).expect("should transform");
        // ts_rs import removed
        assert!(
            !result.contains("use ts_rs::TS;"),
            "ts_rs import gone: {result}"
        );
        // std::collections::HashMap preserved
        assert!(
            result.contains("use std::collections::HashMap;"),
            "HashMap import preserved: {result}"
        );
        // TS stripped from derive
        assert!(!result.contains("TS"), "TS gone from derive: {result}");
        assert!(result.contains("Debug"), "Debug remains: {result}");
        assert!(result.contains("Clone"), "Clone remains: {result}");
        // #[ts(export)] removed
        assert!(!result.contains("#[ts(export)]"), "ts attr gone: {result}");
        // tokio::main removed
        assert!(
            !result.contains("tokio::main"),
            "tokio::main gone: {result}"
        );
        assert!(
            result.contains("async fn main"),
            "fn main remains: {result}"
        );
        // codex_chatgpt → codex_core
        assert!(
            result.contains("codex_core::connectors"),
            "chatgpt rewritten: {result}"
        );
        // thread::spawn → tokio::thread_spawn::spawn
        assert!(
            result.contains("tokio::thread_spawn::spawn"),
            "thread rewritten: {result}"
        );
        // sqlx::migrate! is now handled by proc macro, not rewritten by codemod
        assert!(
            result.contains("sqlx::migrate!"),
            "migrate! should be preserved for proc macro: {result}"
        );
        // turbofish fixed
        assert!(
            result.contains("query_as::<Config>"),
            "turbofish fixed: {result}"
        );
        // tokio::test → #[test]
        assert!(
            result.contains("#[test]"),
            "tokio::test → #[test]: {result}"
        );
        assert!(
            !result.contains("tokio::test"),
            "tokio::test gone: {result}"
        );
    }

    // -----------------------------------------------------------------------
    // File-specific transform tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_v8_isolate_handle_rename() {
        let input = r#"
fn create_service(handle: v8::IsolateHandle) {
    let h: v8::IsolateHandle = handle;
}
"#;
        let result = apply_with_path(
            input,
            Some(std::path::Path::new("code-mode/src/service.rs")),
        )
        .expect("should transform");
        assert!(
            result.contains("crate::runtime::RuntimeHandle"),
            "v8::IsolateHandle should become crate::runtime::RuntimeHandle: {result}"
        );
        assert!(
            !result.contains("v8::IsolateHandle"),
            "old v8 path should be gone: {result}"
        );
    }

    #[test]
    fn test_v8_not_renamed_in_other_files() {
        let input = r#"
fn foo(handle: v8::IsolateHandle) {}
"#;
        let result = apply_with_path(input, Some(std::path::Path::new("some/other/file.rs")));
        assert!(result.is_none(), "should not transform v8 in other files");
    }

    #[test]
    fn test_std_process_exit_status_to_tokio() {
        let input = r#"
use std::process::ExitStatus;

fn foo() -> std::process::ExitStatus {
    todo!()
}
"#;
        let result = apply_with_path(input, Some(std::path::Path::new("core/src/exec.rs")))
            .expect("should transform");
        assert!(
            result.contains("use tokio::process::ExitStatus;"),
            "use should be rewritten: {result}"
        );
        assert!(
            result.contains("tokio::process::ExitStatus"),
            "type path should be rewritten: {result}"
        );
        assert!(
            !result.contains("std::process::ExitStatus"),
            "old path should be gone: {result}"
        );
    }

    #[test]
    fn test_std_process_output_to_tokio() {
        let input = r#"
fn get_output() -> Option<std::process::Output> {
    None
}
"#;
        let result = apply_with_path(input, Some(std::path::Path::new("core/src/git_info.rs")))
            .expect("should transform");
        assert!(
            result.contains("tokio::process::Output"),
            "should be rewritten to tokio::process::Output: {result}"
        );
    }

    #[test]
    fn test_which_which_stub() {
        let input = r#"
fn find_binary(name: &str) {
    if let Ok(path) = which::which(name) {
        println!("{:?}", path);
    }
}
"#;
        let result = apply_with_path(input, Some(std::path::Path::new("core/src/shell.rs")))
            .expect("should transform");
        assert!(
            result.contains("Err(())"),
            "which::which should become stub closure: {result}"
        );
        assert!(
            !result.contains("which::which"),
            "old which path should be gone: {result}"
        );
    }

    #[test]
    fn test_which_which_use_replaced_with_stub_fn() {
        let input = r#"
use which::which;

fn foo() {
    let _ = which("node");
}
"#;
        let result = apply_with_path(
            input,
            Some(std::path::Path::new("artifacts/src/runtime/js_runtime.rs")),
        )
        .expect("should transform");
        assert!(
            result.contains("fn which(_name: &str)"),
            "use which::which should become stub fn: {result}"
        );
        assert!(
            !result.contains("use which::which;"),
            "old use should be gone: {result}"
        );
    }

    #[test]
    fn test_cfg_unix_to_any_unix_wasm32_path_separator() {
        let input = r#"
#[cfg(unix)]
const PATH_SEPARATOR: &str = ":";

#[cfg(windows)]
const PATH_SEPARATOR: &str = ";";
"#;
        let result = apply_with_path(input, Some(std::path::Path::new("arg0/src/lib.rs")))
            .expect("should transform");
        assert!(
            result.contains(r#"#[cfg(any(unix, target_arch = "wasm32"))]"#),
            "cfg should be widened: {result}"
        );
        assert!(
            !result.contains("#[cfg(unix)]\nconst PATH_SEPARATOR"),
            "old cfg(unix) on PATH_SEPARATOR should be gone: {result}"
        );
    }

    #[test]
    fn test_cfg_unix_not_widened_in_other_files() {
        let input = r#"
#[cfg(unix)]
const PATH_SEPARATOR: &str = ":";
"#;
        let result = apply_with_path(input, Some(std::path::Path::new("some/other/file.rs")));
        assert!(result.is_none(), "should not widen cfg in other files");
    }

    #[test]
    fn test_mut_events_in_struct_destructure() {
        let input = r#"
struct RealtimeInputTask {
    writer: u32,
    events: u32,
    user_text_rx: u32,
}

fn foo(task: RealtimeInputTask) {
    let RealtimeInputTask {
        writer,
        events,
        user_text_rx,
    } = task;
}
"#;
        let result = apply_with_path(
            input,
            Some(std::path::Path::new("core/src/realtime_conversation.rs")),
        )
        .expect("should transform");
        assert!(
            result.contains("mut events"),
            "events should gain mut: {result}"
        );
    }

    #[test]
    fn test_mut_writer_in_let_binding() {
        let input = r#"
struct Terminal;
impl Terminal {
    fn backend_mut(&mut self) -> u32 { 0 }
}

fn foo(terminal: &mut Terminal) {
    let writer = terminal.backend_mut();
}
"#;
        let result = apply_with_path(
            input,
            Some(std::path::Path::new("tui/src/insert_history.rs")),
        )
        .expect("should transform");
        assert!(
            result.contains("let mut writer"),
            "writer should gain mut: {result}"
        );
    }

    #[test]
    fn test_mut_fn_params_custom_terminal() {
        let input = r#"
use std::io::Write;

fn draw<I>(writer: &mut impl Write, commands: I) {
    todo!()
}
"#;
        let result = apply_with_path(
            input,
            Some(std::path::Path::new("tui/src/custom_terminal.rs")),
        )
        .expect("should transform");
        assert!(
            result.contains("mut writer"),
            "writer param should gain mut: {result}"
        );
    }

    #[test]
    fn test_cfg_unix_use_exit_status_ext_removed() {
        let input = r#"
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

fn foo() {}
"#;
        let result = apply_with_path(input, Some(std::path::Path::new("core/src/exec.rs")))
            .expect("should transform");
        assert!(
            !result.contains("ExitStatusExt"),
            "ExitStatusExt use should be removed: {result}"
        );
        assert!(
            !result.contains("cfg(unix)"),
            "cfg(unix) should be removed: {result}"
        );
    }

    #[test]
    fn test_which_which_in_stub() {
        let input = r#"
fn resolve(program: String, search_path: String, cwd: String) {
    match which::which_in(&program, search_path, &cwd) {
        Ok(p) => println!("{:?}", p),
        Err(_) => {}
    }
}
"#;
        let result = apply_with_path(
            input,
            Some(std::path::Path::new("rmcp-client/src/program_resolver.rs")),
        )
        .expect("should transform");
        assert!(
            result.contains("which not available in WASM"),
            "which_in should be stubbed: {result}"
        );
        assert!(
            !result.contains("which::which_in"),
            "old which_in path should be gone: {result}"
        );
    }
}

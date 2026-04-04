//! tree-sitter-wasm codemod — inlines workspace-inherited fields so the
//! `tree-sitter/lib` crate can be used as a standalone path dependency
//! from another Cargo workspace.
//!
//! Tree-sitter's lib/Cargo.toml uses `field.workspace = true` for package
//! metadata and some build-dependencies. When consumed via `{ path = "..." }`
//! from a different workspace, Cargo can't resolve those references. This
//! codemod reads the workspace root Cargo.toml, extracts the concrete values,
//! and replaces every `*.workspace = true` reference in target crate manifests.
//!
//! Usage:
//!   cargo run -p tree-sitter-wasm-codemod -- <tree-sitter-root>
//!
//! The tool modifies Cargo.toml files in-place. It is idempotent.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Value};

/// Dependency redirects applied to grammar crates (tree-sitter-bash etc.)
/// to ensure they use the same tree-sitter-language as the local tree-sitter lib.
/// Format: (dep_name, replacement_value_toml).
const GRAMMAR_DEP_REDIRECTS: &[(&str, &str)] = &[
    // Point tree-sitter-language to the local copy inside tree-sitter/lib/language
    // to avoid diamond dependency (local v0.1.5 vs crates.io v0.1.7).
    ("tree-sitter-language", "../tree-sitter/lib/language"),
];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        bail!("Usage: tree-sitter-wasm-codemod <tree-sitter-wasm-root>\n\nExpects the tree-sitter-wasm/ directory containing tree-sitter/ and tree-sitter-bash/ submodules.");
    }
    let wasm_root = PathBuf::from(&args[1]);

    // Phase 1: Inline workspace fields in tree-sitter
    let ts_root = wasm_root.join("tree-sitter");
    let workspace_toml_path = ts_root.join("Cargo.toml");
    if !workspace_toml_path.exists() {
        bail!("tree-sitter workspace Cargo.toml not found at {}", workspace_toml_path.display());
    }

    println!("Phase 1: Inline workspace fields in tree-sitter/");

    let ws_content = std::fs::read_to_string(&workspace_toml_path)
        .context("reading workspace Cargo.toml")?;
    let ws_doc = ws_content.parse::<DocumentMut>()
        .context("parsing workspace Cargo.toml")?;

    let ws_pkg = ws_doc
        .get("workspace")
        .and_then(|w| w.get("package"))
        .context("[workspace.package] not found")?;

    let ws_deps = ws_doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .context("[workspace.dependencies] not found")?;

    let ws_lints = ws_doc
        .get("workspace")
        .and_then(|w| w.get("lints"));

    let members = find_member_tomls(&ts_root)?;
    for member_path in &members {
        patch_member_toml(member_path, ws_pkg, ws_deps, ws_lints)?;
    }

    // Phase 2: Redirect dependencies in grammar crates (tree-sitter-bash etc.)
    println!("\nPhase 2: Redirect dependencies in grammar crates");

    let grammar_dirs = ["tree-sitter-bash"];
    for grammar_dir in &grammar_dirs {
        let grammar_toml = wasm_root.join(grammar_dir).join("Cargo.toml");
        if grammar_toml.exists() {
            redirect_grammar_deps(&grammar_toml)?;
        }
    }

    println!("\ntree-sitter-wasm-codemod: done");
    Ok(())
}

/// Redirect dependencies in a grammar crate's Cargo.toml to use local paths.
fn redirect_grammar_deps(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut doc = content.parse::<DocumentMut>()?;
    let mut changed = false;

    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(deps) = doc.get_mut(section).and_then(|d| d.as_table_like_mut()) {
            for (dep_name, relative_path) in GRAMMAR_DEP_REDIRECTS {
                if deps.contains_key(dep_name) {
                    let mut table = toml_edit::InlineTable::new();
                    table.insert("path", Value::from(*relative_path));
                    deps.insert(dep_name, Item::Value(Value::InlineTable(table)));
                    changed = true;
                    println!("  [{section}] redirect {dep_name} → {relative_path}");
                }
            }
        }
    }

    if changed {
        std::fs::write(path, doc.to_string())?;
        println!("  ✓ {}", path.display());
    }

    Ok(())
}

/// Find all Cargo.toml files in workspace members (excluding the root).
fn find_member_tomls(root: &Path) -> Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            // Skip hidden dirs, target, node_modules
            !name.starts_with('.') && name != "target" && name != "node_modules"
        })
    {
        let entry = entry?;
        if entry.file_name() == "Cargo.toml" && entry.path() != root.join("Cargo.toml") {
            results.push(entry.path().to_path_buf());
        }
    }
    Ok(results)
}

/// Patch a single member Cargo.toml: inline all `*.workspace = true` references.
fn patch_member_toml(
    path: &Path,
    ws_pkg: &Item,
    ws_deps: &Item,
    ws_lints: Option<&Item>,
) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut doc = content.parse::<DocumentMut>()?;
    let mut changed = false;

    // 1. Inline [package] fields that reference workspace
    if let Some(pkg) = doc.get_mut("package").and_then(|p| p.as_table_like_mut()) {
        let pkg_fields: Vec<String> = pkg
            .iter()
            .filter_map(|(k, v)| {
                if is_workspace_true(v) {
                    Some(k.to_string())
                } else {
                    None
                }
            })
            .collect();

        for field in &pkg_fields {
            if let Some(ws_value) = ws_pkg.get(field) {
                pkg.insert(field, ws_value.clone());
                changed = true;
            }
        }
    }

    // 2. Inline dependency sections
    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(deps) = doc.get_mut(section).and_then(|d| d.as_table_like_mut()) {
            changed |= inline_workspace_deps(deps, ws_deps);
        }
    }

    // 3. Inline [lints] workspace = true
    if let Some(lints) = doc.get_mut("lints") {
        if is_workspace_true(lints) {
            if let Some(ws_lints_val) = ws_lints {
                *lints = ws_lints_val.clone();
                changed = true;
            } else {
                // No workspace lints defined — remove the section
                doc.remove("lints");
                changed = true;
            }
        }
    }

    if changed {
        std::fs::write(path, doc.to_string())?;
        println!("  ✓ {}", path.display());
    }

    Ok(())
}

/// Check if a TOML value is `{ workspace = true }` (inline table) or `workspace = true` (dotted key).
fn is_workspace_true(item: &Item) -> bool {
    // Check inline table: { workspace = true }
    if let Some(table) = item.as_inline_table() {
        if let Some(ws) = table.get("workspace") {
            return ws.as_bool() == Some(true);
        }
    }
    // Check regular table
    if let Some(table) = item.as_table_like() {
        if let Some(ws) = table.get("workspace") {
            if let Some(val) = ws.as_bool() {
                return val;
            }
        }
    }
    // Check if it's a simple bool value (for dotted keys like `workspace = true`)
    if let Some(val) = item.as_bool() {
        // This would be the case for `[lints]\nworkspace = true`
        return val;
    }
    false
}

/// Inline workspace dependency references.
/// Handles both `dep.workspace = true` and `dep = { workspace = true, features = [...] }`.
fn inline_workspace_deps(deps: &mut dyn toml_edit::TableLike, ws_deps: &Item) -> bool {
    let mut changed = false;

    let dep_names: Vec<String> = deps
        .iter()
        .map(|(k, _)| k.to_string())
        .collect();

    for dep_name in &dep_names {
        let dep = deps.get(dep_name).unwrap();

        if is_workspace_true(dep) {
            // Simple case: `dep.workspace = true` or `dep = { workspace = true }`
            if let Some(ws_val) = ws_deps.get(dep_name) {
                // Merge: if the local dep has extra fields (features, optional), keep them
                let local_extras = extract_non_workspace_fields(dep);
                let mut resolved = ws_val.clone();

                if !local_extras.is_empty() {
                    merge_dep_fields(&mut resolved, &local_extras);
                }

                deps.insert(dep_name, resolved);
                changed = true;
            }
        } else if let Some(table) = dep.as_table_like() {
            // Check if table has workspace = true alongside other fields
            if table.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
                if let Some(ws_val) = ws_deps.get(dep_name) {
                    let local_extras = extract_non_workspace_fields(dep);
                    let mut resolved = ws_val.clone();

                    if !local_extras.is_empty() {
                        merge_dep_fields(&mut resolved, &local_extras);
                    }

                    deps.insert(dep_name, resolved);
                    changed = true;
                }
            }
        }
    }

    changed
}

/// Extract fields from a dependency entry that aren't `workspace`.
fn extract_non_workspace_fields(item: &Item) -> Vec<(String, Item)> {
    let mut extras = Vec::new();
    if let Some(table) = item.as_table_like() {
        for (k, v) in table.iter() {
            if k != "workspace" {
                extras.push((k.to_string(), v.clone()));
            }
        }
    }
    extras
}

/// Merge extra fields (features, optional, etc.) into a resolved workspace dependency.
fn merge_dep_fields(resolved: &mut Item, extras: &[(String, Item)]) {
    // If resolved is a simple string version, convert to inline table
    if let Some(version) = resolved.as_str().map(String::from) {
        let mut table = toml_edit::InlineTable::new();
        table.insert("version", Value::from(version.as_str()));
        for (k, v) in extras {
            if let Some(val) = v.as_value() {
                table.insert(k, val.clone());
            }
        }
        *resolved = Item::Value(Value::InlineTable(table));
        return;
    }

    // If resolved is already a table, just add the extras
    if let Some(table) = resolved.as_inline_table_mut() {
        for (k, v) in extras {
            if let Some(val) = v.as_value() {
                table.insert(k, val.clone());
            }
        }
    } else if let Some(table) = resolved.as_table_like_mut() {
        for (k, v) in extras {
            table.insert(k, v.clone());
        }
    }
}

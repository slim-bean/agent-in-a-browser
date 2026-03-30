//! codex-codemod: Transforms Codex upstream code for wasip2 compatibility.
//!
//! This tool is designed to be idempotent — running it multiple times on
//! the same source tree produces the same result. This is critical for
//! the upstream sync workflow:
//!
//! 1. cd runtime/codex-upstream
//! 2. git fetch upstream && git checkout upstream/main
//! 3. cd ../.. && cargo run -p codex-codemod -- runtime/codex-upstream/
//! 4. cargo component check --manifest-path runtime/codex-wasm/codex-wasm-tui/Cargo.toml --target wasm32-wasip2
//! 5. cd runtime/codex-upstream && git add -A && git commit -m "feat: apply wasip2 codemod"
//! 6. git push origin HEAD:wasm32-wasip2 --force-with-lease
//! 7. cd ../.. && git add runtime/codex-upstream && git commit -m "sync: update codex fork"

mod cargo_toml;
pub mod engine;
pub mod syn_transforms;
pub mod transform;
mod transforms;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "codex-codemod", about = "Transform Codex for wasip2")]
struct Args {
    /// Path to the codex-upstream directory
    #[arg(default_value = "runtime/codex-upstream")]
    upstream_dir: PathBuf,

    /// Dry run — show what would be changed without writing
    #[arg(long)]
    dry_run: bool,

    /// Show diff of changes
    #[arg(long)]
    diff: bool,

    /// Only run Cargo.toml transforms (skip AST transforms)
    #[arg(long)]
    cargo_only: bool,

    /// Only run AST transforms (skip Cargo.toml transforms)
    #[arg(long)]
    ast_only: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let codex_rs = args.upstream_dir.join("codex-rs");
    if !codex_rs.exists() {
        anyhow::bail!(
            "codex-rs/ not found in {}. Is this the right upstream directory?",
            args.upstream_dir.display()
        );
    }

    println!("codex-codemod: transforming {}", codex_rs.display());

    // Phase 1: Cargo.toml transforms
    if !args.ast_only {
        println!("\n=== Phase 1: Cargo.toml transforms ===");
        cargo_toml::transform_workspace(&codex_rs, args.dry_run)?;
    }

    // Phase 2: Source transforms via engine + syn
    if !args.cargo_only {
        println!("\n=== Phase 2: Source transforms ===");
        let all_transforms = transforms::all_transforms();
        let stats = engine::apply_transforms(&codex_rs, &all_transforms)?;
        println!(
            "  {} files transformed, {} stubbed, {} transforms applied ({} already applied)",
            stats.files_transformed,
            stats.files_stubbed,
            stats.transforms_applied,
            stats.transforms_already_applied
        );
        if !stats.transforms_not_matched.is_empty() {
            for desc in &stats.transforms_not_matched {
                console_log::console_warn!("  [WARN] not matched: {desc}");
            }
        }
    }

    println!("\ncodex-codemod: done.");
    Ok(())
}

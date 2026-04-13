//! codex-codemod: Transforms Codex upstream code for wasip2 compatibility.
//!
//! This tool is designed to be idempotent — running it multiple times on
//! the same source tree produces the same result. This is critical for
//! the upstream sync workflow:
//!
//! 1. cd runtime/codex-upstream
//! 2. git fetch upstream origin && git checkout upstream/main
//! 3. cd ../.. && cargo run -p codex-codemod -- runtime/codex-upstream/
//! 4. cargo component check --manifest-path runtime/codex-wasm/codex-wasm-tui/Cargo.toml --target wasm32-wasip2
//! 5. cd runtime/codex-upstream && git add -A && git commit -m "codemod: apply wasip2 transforms"
//! 6. git rev-list --count upstream/main..HEAD   # must print 1
//! 7. cd ../.. && ./scripts/publish-codex-upstream.sh
//! 8. cd ../.. && git add runtime/codex-upstream && git commit -m "sync: update codex fork"

mod cargo_toml;
pub mod engine;
mod git_policy;
pub mod semantic;
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

    /// Exit with error if any transform doesn't match its target pattern.
    /// Use in CI to catch upstream changes that break transforms.
    #[arg(long)]
    strict: bool,

    /// Inject diagnostic console_log traces into session initialization code.
    /// Off by default — only enable for debugging startup hangs.
    #[arg(long)]
    diag_traces: bool,
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

    if let Some(status) = git_policy::verify_single_carry_commit(&args.upstream_dir)? {
        let mode = if status.ahead == 0 {
            "ready to regenerate carrying commit"
        } else {
            "single carrying commit preserved"
        };
        let head_kind = if status.detached {
            "detached"
        } else {
            "branch"
        };
        println!(
            "codex-codemod: git policy OK ({mode}; {head_kind} HEAD at {})",
            status.head_short
        );
    } else {
        println!(
            "codex-codemod: git policy skipped ({} is not a git worktree)",
            args.upstream_dir.display()
        );
    }

    println!("codex-codemod: transforming {}", codex_rs.display());

    // Phase 1: Cargo.toml transforms
    if !args.ast_only {
        println!("\n=== Phase 1: Cargo.toml transforms ===");
        cargo_toml::transform_workspace(&codex_rs, args.dry_run)?;
    }

    // Phase 2+: Source transforms via engine + semantic workspace pass
    if !args.cargo_only {
        println!("\n=== Phase 2: Source transforms ===");
        let all_transforms = transforms::all_transforms();
        let config = engine::TransformConfig {
            diag_traces: args.diag_traces,
            strict: args.strict,
        };
        let stats = engine::apply_transforms(&codex_rs, &all_transforms, &config)?;
        println!(
            "  {} files transformed, {} stubbed, {} transforms applied ({} already applied)",
            stats.files_transformed,
            stats.files_stubbed,
            stats.transforms_applied,
            stats.transforms_already_applied
        );
        if !stats.transforms_not_matched.is_empty() {
            let count = stats.transforms_not_matched.len();
            for desc in &stats.transforms_not_matched {
                console_log::console_warn!("  [WARN] not matched: {desc}");
            }
            if args.strict {
                anyhow::bail!(
                    "{count} transform(s) did not match — upstream may have changed. \
                     Review warnings above and update the codemod."
                );
            }
        }
        if !stats.semantic_warnings.is_empty() {
            let count = stats.semantic_warnings.len();
            for warn in &stats.semantic_warnings {
                console_log::console_warn!("  [semantic-WARN] {warn}");
            }
            if args.strict {
                anyhow::bail!(
                    "{count} semantic transform(s) reported unsupported or missing sites — upstream may have changed. \
                     Review warnings above and update the codemod."
                );
            }
        }
    }

    println!("\ncodex-codemod: done.");
    Ok(())
}

//! Git workflow policy checks for the codex-upstream carrying commit.

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AheadBehindCounts {
    behind: usize,
    ahead: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarryCommitStatus {
    pub ahead: usize,
    pub detached: bool,
    pub head_short: String,
}

pub fn verify_single_carry_commit(upstream_dir: &Path) -> Result<Option<CarryCommitStatus>> {
    if !is_git_repo(upstream_dir)? {
        return Ok(None);
    }

    ensure_ref_exists(upstream_dir, "upstream/main")?;
    let counts = parse_left_right_counts(&git_stdout(
        upstream_dir,
        [
            "rev-list",
            "--left-right",
            "--count",
            "upstream/main...HEAD",
        ],
    )?)?;
    validate_counts(counts)?;

    Ok(Some(CarryCommitStatus {
        ahead: counts.ahead,
        detached: current_branch(upstream_dir)?.is_none(),
        head_short: git_stdout(upstream_dir, ["rev-parse", "--short", "HEAD"])?,
    }))
}

fn validate_counts(counts: AheadBehindCounts) -> Result<()> {
    if counts.behind > 0 {
        bail!(
            "codex-upstream must fast-forward from upstream/main before applying the codemod \
             (HEAD is {} commit(s) behind upstream/main)",
            counts.behind
        );
    }

    if counts.ahead > 1 {
        bail!(
            "codex-upstream must have at most one carrying commit above upstream/main; \
             found {} commit(s) ahead. Reset to upstream/main and regenerate a single codemod commit.",
            counts.ahead
        );
    }

    Ok(())
}

fn parse_left_right_counts(raw: &str) -> Result<AheadBehindCounts> {
    let mut parts = raw.split_whitespace();
    let behind = parts
        .next()
        .ok_or_else(|| anyhow!("missing behind count"))?
        .parse::<usize>()
        .context("parsing behind count")?;
    let ahead = parts
        .next()
        .ok_or_else(|| anyhow!("missing ahead count"))?
        .parse::<usize>()
        .context("parsing ahead count")?;

    if parts.next().is_some() {
        bail!("unexpected extra output in rev-list count: {raw}");
    }

    Ok(AheadBehindCounts { behind, ahead })
}

fn is_git_repo(dir: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .with_context(|| format!("running git rev-parse in {}", dir.display()))?;

    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn ensure_ref_exists(dir: &Path, reference: &str) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .with_context(|| format!("verifying git ref {reference} in {}", dir.display()))?;

    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "missing git ref {reference} in {}. Run `git -C {} fetch upstream` before codemodding.",
            dir.display(),
            dir.display()
        )
    }
}

fn current_branch(dir: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["symbolic-ref", "--short", "-q", "HEAD"])
        .output()
        .with_context(|| format!("reading current branch in {}", dir.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let branch = String::from_utf8(output.stdout)
        .context("decoding current branch output")?
        .trim()
        .to_string();
    if branch.is_empty() {
        Ok(None)
    } else {
        Ok(Some(branch))
    }
}

fn git_stdout<const N: usize>(dir: &Path, args: [&str; N]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("running git {:?} in {}", args, dir.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git {:?} failed in {}: {}",
            args,
            dir.display(),
            stderr.trim()
        );
    }

    Ok(String::from_utf8(output.stdout)
        .context("decoding git output")?
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_left_right_counts, validate_counts, AheadBehindCounts};

    #[test]
    fn parses_tab_separated_counts() {
        let counts = parse_left_right_counts("0\t1\n").expect("counts should parse");
        assert_eq!(
            counts,
            AheadBehindCounts {
                behind: 0,
                ahead: 1
            }
        );
    }

    #[test]
    fn accepts_zero_or_one_carry_commit() {
        validate_counts(AheadBehindCounts {
            behind: 0,
            ahead: 0,
        })
        .expect("upstream/main baseline should be allowed");
        validate_counts(AheadBehindCounts {
            behind: 0,
            ahead: 1,
        })
        .expect("single carrying commit should be allowed");
    }

    #[test]
    fn rejects_multiple_carry_commits() {
        let err = validate_counts(AheadBehindCounts {
            behind: 0,
            ahead: 2,
        })
        .expect_err("multiple carrying commits must fail");
        assert!(err
            .to_string()
            .contains("at most one carrying commit above upstream/main"));
    }

    #[test]
    fn rejects_divergence_from_upstream() {
        let err = validate_counts(AheadBehindCounts {
            behind: 3,
            ahead: 1,
        })
        .expect_err("behind upstream/main must fail");
        assert!(err.to_string().contains("fast-forward from upstream/main"));
    }
}

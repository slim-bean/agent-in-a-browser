//! Stub -- Windows sandbox read grants for wasip2.
#![allow(dead_code, unused_variables)]

use codex_protocol::protocol::SandboxPolicy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn grant_read_root_non_elevated(
    _policy: &SandboxPolicy,
    _policy_cwd: &Path,
    _command_cwd: &Path,
    _env_map: &HashMap<String, String>,
    _codex_home: &Path,
    _read_root: &Path,
) -> anyhow::Result<PathBuf> {
    anyhow::bail!("Windows sandbox not available in WASM")
}

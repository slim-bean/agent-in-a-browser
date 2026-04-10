//! Stub -- Linux landlock sandbox for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]

use codex_network_proxy::NetworkProxy;
use codex_protocol::protocol::SandboxPolicy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::process::Child;

use crate::spawn::StdioPolicy;

pub fn create_linux_sandbox_command_args_for_policies<A, B, C, D, E, F, G, H>(
    _a: A, _b: B, _c: C, _d: D, _e: E, _f: F, _g: G, _h: H,
) -> Vec<String> {
    vec![]
}

pub fn allow_network_for_proxy(_enforce: bool) -> bool {
    false
}

#[allow(clippy::too_many_arguments)]
pub async fn spawn_command_under_linux_sandbox<P>(
    _codex_linux_sandbox_exe: P,
    _command: Vec<String>,
    _command_cwd: PathBuf,
    _sandbox_policy: &SandboxPolicy,
    _sandbox_policy_cwd: &Path,
    _use_legacy_landlock: bool,
    _stdio_policy: StdioPolicy,
    _network: Option<&NetworkProxy>,
    _env: HashMap<String, String>,
) -> std::io::Result<Child> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "landlock not available in WASM",
    ))
}

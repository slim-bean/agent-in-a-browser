//! Stub — Windows sandbox for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]
use crate::config::Config;
use crate::config::ConfigToml;
use crate::config::profile::ConfigProfile;
use crate::config::types::WindowsSandboxModeToml;
use codex_protocol::config_types::WindowsSandboxLevel;

pub const ELEVATED_SANDBOX_NUX_ENABLED: bool = false;

pub trait WindowsSandboxLevelExt {
    fn from_config(config: &Config) -> WindowsSandboxLevel;
    fn from_features(features: &codex_features::Features) -> WindowsSandboxLevel;
}

impl WindowsSandboxLevelExt for WindowsSandboxLevel {
    fn from_config(_config: &Config) -> WindowsSandboxLevel {
        WindowsSandboxLevel::Disabled
    }
    fn from_features(_features: &codex_features::Features) -> WindowsSandboxLevel {
        WindowsSandboxLevel::Disabled
    }
}

pub fn windows_sandbox_level_from_config(_config: &Config) -> WindowsSandboxLevel {
    WindowsSandboxLevel::Disabled
}

pub fn windows_sandbox_level_from_features(_features: &codex_features::Features) -> WindowsSandboxLevel {
    WindowsSandboxLevel::Disabled
}

pub fn resolve_windows_sandbox_mode(
    _cfg: &ConfigToml,
    _profile: &ConfigProfile,
) -> Option<WindowsSandboxModeToml> {
    None
}

pub fn resolve_windows_sandbox_private_desktop(
    _cfg: &ConfigToml,
    _profile: &ConfigProfile,
) -> bool {
    false
}

/// Windows sandbox setup mode — stub for wasip2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsSandboxSetupMode {
    Elevated,
    Unelevated,
}

/// Windows sandbox setup request — stub for wasip2.
pub struct WindowsSandboxSetupRequest {
    pub mode: WindowsSandboxSetupMode,
    pub policy: codex_protocol::protocol::SandboxPolicy,
    pub policy_cwd: std::path::PathBuf,
    pub command_cwd: std::path::PathBuf,
    pub env_map: std::collections::HashMap<String, String>,
    pub codex_home: std::path::PathBuf,
    pub active_profile: Option<String>,
}

/// Run Windows sandbox setup — not available in WASM.
pub async fn run_windows_sandbox_setup(
    _request: WindowsSandboxSetupRequest,
) -> anyhow::Result<()> {
    anyhow::bail!("Windows sandbox setup not available in WASM")
}

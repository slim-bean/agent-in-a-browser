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

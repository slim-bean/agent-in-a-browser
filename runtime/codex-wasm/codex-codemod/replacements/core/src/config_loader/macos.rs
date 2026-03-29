//! Stub — macOS managed admin config for wasip2.
//! CoreFoundation/MDM APIs are not available in WASM.
#![allow(dead_code, unused_variables)]

pub const MANAGED_PREFERENCES_APPLICATION_ID: &str = "com.openai.codex";

#[derive(Debug, Clone)]
pub struct ManagedAdminConfigLayer {
    pub config: toml::Value,
    pub raw_toml: String,
}

pub async fn load_managed_admin_config_layer(
    _override_base64: Option<&str>,
) -> std::io::Result<Option<ManagedAdminConfigLayer>> {
    Ok(None)
}

pub async fn load_managed_admin_requirements_toml(
    _target: &mut codex_config::ConfigRequirementsWithSources,
    _override_base64: Option<&str>,
) -> std::io::Result<()> {
    Ok(())
}

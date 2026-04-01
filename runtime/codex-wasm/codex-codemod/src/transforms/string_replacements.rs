//! String replacement transforms — remaining entries not yet in syn.
//!
//! These are targeted to be migrated to syn_transforms.rs.
//! Once all are migrated, this file can be deleted.

use crate::transform::Transform;

pub fn transforms() -> Vec<Transform> {
    vec![
        // Add missing connector functions that were previously in codex_chatgpt::connectors.
        // After the codemod rewrites codex_chatgpt → codex_core, the app-server expects
        // these functions in codex_core::connectors. They are stubs since the chatgpt
        // auth layer is not available in WASM.
        Transform::ReplaceFirst {
            path_suffix: "core/src/connectors.rs",
            find: "\n#[cfg(test)]",
            replace: r#"
/// List all connectors with options — stub for WASM (no ChatGPT auth).
/// Previously in codex_chatgpt::connectors, moved to core after codemod.
pub async fn list_all_connectors_with_options(
    _config: &crate::config::Config,
    _force_refetch: bool,
) -> anyhow::Result<Vec<AppInfo>> {
    Ok(Vec::new())
}

/// List cached connectors — stub for WASM (no ChatGPT auth).
/// Previously in codex_chatgpt::connectors, moved to core after codemod.
pub async fn list_cached_all_connectors(
    _config: &crate::config::Config,
) -> Option<Vec<AppInfo>> {
    Some(Vec::new())
}

/// Filter connectors for plugin apps — stub for WASM.
/// Previously in codex_chatgpt::connectors, moved to core after codemod.
pub fn connectors_for_plugin_apps(
    connectors: Vec<AppInfo>,
    plugin_apps: &[crate::plugins::AppConnectorId],
) -> Vec<AppInfo> {
    let plugin_app_ids: std::collections::HashSet<&str> = plugin_apps
        .iter()
        .map(|connector_id| connector_id.0.as_str())
        .collect();
    filter_disallowed_connectors(merge_plugin_apps(connectors, plugin_apps.to_vec()))
        .into_iter()
        .filter(|connector| plugin_app_ids.contains(connector.id.as_str()))
        .collect()
}

/// Merge connectors with accessible connectors — 3-arg version for app-server.
/// The app-server code calls this as connectors::merge_connectors_with_accessible(3 args).
/// The 2-arg merge_plugin_apps_with_accessible is the upstream version used by codex-core.
pub fn merge_connectors_with_accessible(
    connectors: Vec<AppInfo>,
    accessible_connectors: Vec<AppInfo>,
    _all_connectors_loaded: bool,
) -> Vec<AppInfo> {
    let merged = merge_connectors(connectors, accessible_connectors);
    filter_disallowed_connectors(merged)
}

#[cfg(test)]"#,
        },
    ]
}

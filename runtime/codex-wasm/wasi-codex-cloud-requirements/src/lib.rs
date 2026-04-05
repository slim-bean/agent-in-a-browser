//! wasip2-compatible shim for codex-cloud-requirements.
//!
//! Cloud requirements fetching requires the backend API which is not
//! available in the browser. Returns default (empty) loaders.

use std::path::PathBuf;
use std::sync::Arc;

pub use codex_config::CloudRequirementsLoader;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthManager;

/// Create a cloud requirements loader.
/// Returns a default (no-op) loader in WASM — no backend API access.
pub fn cloud_requirements_loader(
    _auth_manager: Arc<AuthManager>,
    _chatgpt_base_url: String,
    _codex_home: PathBuf,
) -> CloudRequirementsLoader {
    CloudRequirementsLoader::default()
}

/// Create a cloud requirements loader for storage initialization.
/// Returns a default (no-op) loader in WASM.
pub fn cloud_requirements_loader_for_storage(
    _codex_home: PathBuf,
    _enable_codex_api_key_env: bool,
    _credentials_store_mode: AuthCredentialsStoreMode,
) -> CloudRequirementsLoader {
    CloudRequirementsLoader::default()
}

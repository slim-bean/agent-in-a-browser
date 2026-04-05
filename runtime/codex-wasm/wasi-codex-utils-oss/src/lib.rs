//! wasip2-compatible shim for codex-utils-oss.
//!
//! OSS providers (LMStudio, Ollama) are not available in the browser.
//! These functions are no-ops that allow the TUI code to compile.

/// Returns the default model for a given OSS provider.
/// Always returns `None` in WASM — no local OSS providers available.
pub fn get_default_model_for_oss_provider(_provider_id: &str) -> Option<&'static str> {
    None
}

/// Ensures the specified OSS provider is ready.
/// Always succeeds in WASM — no local OSS providers to check.
pub async fn ensure_oss_provider_ready(
    _provider_id: &str,
    _config: &codex_core::config::Config,
) -> Result<(), std::io::Error> {
    Ok(())
}

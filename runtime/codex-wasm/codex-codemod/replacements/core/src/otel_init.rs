//! Stub — OTel init for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]
use crate::config::Config;
use codex_otel::OtelProvider;

pub fn build_provider(
    _config: &Config,
    _service_version: &str,
    _service_name_override: Option<&str>,
    _default_analytics_enabled: bool,
) -> Result<Option<OtelProvider>, Box<dyn std::error::Error>> {
    Ok(None)
}

pub fn codex_export_filter(_meta: &tracing::Metadata<'_>) -> bool {
    false
}

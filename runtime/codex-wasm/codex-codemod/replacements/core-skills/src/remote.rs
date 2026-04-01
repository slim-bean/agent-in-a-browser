//! Stub — remote skill download not available in WASM.
#![allow(dead_code, unused_variables)]

use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSkillScope {
    WorkspaceShared,
    AllShared,
    Personal,
    Example,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSkillProductSurface {
    Chatgpt,
    Codex,
    Api,
    Atlas,
}

#[derive(Debug, Clone)]
pub struct RemoteSkillSummary {
    pub name: String,
    pub description: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug)]
pub struct RemoteSkillDownloadResult {
    pub path: PathBuf,
}

pub async fn list_remote_skills(
    _auth: &codex_login::CodexAuth,
    _chatgpt_base_url: &str,
    _scope: RemoteSkillScope,
    _product_surface: RemoteSkillProductSurface,
) -> Result<Vec<RemoteSkillSummary>> {
    anyhow::bail!("Remote skills not available in WASM")
}

pub async fn export_remote_skill(
    _auth: &codex_login::CodexAuth,
    _chatgpt_base_url: &str,
    _skill_id: &str,
    _version: &str,
    _target_dir: &std::path::Path,
) -> Result<RemoteSkillDownloadResult> {
    anyhow::bail!("Remote skill download not available in WASM")
}

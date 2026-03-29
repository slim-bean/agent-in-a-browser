#![allow(unused_variables, unused_mut, unused_imports, dead_code, clippy::all)]

//! Stub replacement for codex-state that removes the sqlx dependency.
//! All database operations are no-ops that return empty/default values.

pub mod log_db;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::{AskForApproval, RolloutItem, SandboxPolicy, SessionSource};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Environment variable for overriding the SQLite state database home directory.
pub const SQLITE_HOME_ENV: &str = "CODEX_SQLITE_HOME";

pub const LOGS_DB_FILENAME: &str = "logs";
pub const LOGS_DB_VERSION: u32 = 1;
pub const STATE_DB_FILENAME: &str = "state";
pub const STATE_DB_VERSION: u32 = 5;

/// Errors encountered during DB operations. Tags: [stage]
pub const DB_ERROR_METRIC: &str = "codex.db.error";
/// Metrics on backfill process. Tags: [status]
pub const DB_METRIC_BACKFILL: &str = "codex.db.backfill";
/// Metrics on backfill duration. Tags: [status]
pub const DB_METRIC_BACKFILL_DURATION_MS: &str = "codex.db.backfill.duration_ms";

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

pub fn state_db_filename() -> String {
    format!("{}_{}.sqlite", STATE_DB_FILENAME, STATE_DB_VERSION)
}

pub fn state_db_path(codex_home: &Path) -> PathBuf {
    codex_home.join(state_db_filename())
}

pub fn logs_db_filename() -> String {
    format!("{}_{}.sqlite", LOGS_DB_FILENAME, LOGS_DB_VERSION)
}

pub fn logs_db_path(codex_home: &Path) -> PathBuf {
    codex_home.join(logs_db_filename())
}

/// Apply a rollout item to the metadata structure (stub: no-op).
pub fn apply_rollout_item(
    metadata: &mut ThreadMetadata,
    item: &RolloutItem,
    default_provider: &str,
) {
    // no-op stub
}

/// Return whether this rollout item can mutate thread metadata stored in SQLite.
pub fn rollout_item_affects_thread_metadata(item: &RolloutItem) -> bool {
    match item {
        RolloutItem::SessionMeta(_) | RolloutItem::TurnContext(_) => true,
        _ => false,
    }
}

fn enum_to_string<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// SortKey
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    CreatedAt,
    UpdatedAt,
}

// ---------------------------------------------------------------------------
// Anchor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub ts: DateTime<Utc>,
    pub id: Uuid,
}

// ---------------------------------------------------------------------------
// ThreadsPage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadsPage {
    pub items: Vec<ThreadMetadata>,
    pub next_anchor: Option<Anchor>,
    pub num_scanned_rows: usize,
}

// ---------------------------------------------------------------------------
// ExtractionOutcome
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionOutcome {
    pub metadata: ThreadMetadata,
    pub memory_mode: Option<String>,
    pub parse_errors: usize,
}

// ---------------------------------------------------------------------------
// ThreadMetadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMetadata {
    pub id: ThreadId,
    pub rollout_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source: String,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
    pub agent_path: Option<String>,
    pub model_provider: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub cwd: PathBuf,
    pub cli_version: String,
    pub title: String,
    pub sandbox_policy: String,
    pub approval_mode: String,
    pub tokens_used: i64,
    pub first_user_message: Option<String>,
    pub archived_at: Option<DateTime<Utc>>,
    pub git_sha: Option<String>,
    pub git_branch: Option<String>,
    pub git_origin_url: Option<String>,
}

impl ThreadMetadata {
    pub fn prefer_existing_git_info(&mut self, existing: &Self) {
        if existing.git_sha.is_some() {
            self.git_sha = existing.git_sha.clone();
        }
        if existing.git_branch.is_some() {
            self.git_branch = existing.git_branch.clone();
        }
        if existing.git_origin_url.is_some() {
            self.git_origin_url = existing.git_origin_url.clone();
        }
    }

    pub fn diff_fields(&self, other: &Self) -> Vec<&'static str> {
        let mut diffs = Vec::new();
        if self.id != other.id { diffs.push("id"); }
        if self.rollout_path != other.rollout_path { diffs.push("rollout_path"); }
        if self.created_at != other.created_at { diffs.push("created_at"); }
        if self.updated_at != other.updated_at { diffs.push("updated_at"); }
        if self.source != other.source { diffs.push("source"); }
        if self.agent_nickname != other.agent_nickname { diffs.push("agent_nickname"); }
        if self.agent_role != other.agent_role { diffs.push("agent_role"); }
        if self.agent_path != other.agent_path { diffs.push("agent_path"); }
        if self.model_provider != other.model_provider { diffs.push("model_provider"); }
        if self.model != other.model { diffs.push("model"); }
        if self.reasoning_effort != other.reasoning_effort { diffs.push("reasoning_effort"); }
        if self.cwd != other.cwd { diffs.push("cwd"); }
        if self.cli_version != other.cli_version { diffs.push("cli_version"); }
        if self.title != other.title { diffs.push("title"); }
        if self.sandbox_policy != other.sandbox_policy { diffs.push("sandbox_policy"); }
        if self.approval_mode != other.approval_mode { diffs.push("approval_mode"); }
        if self.tokens_used != other.tokens_used { diffs.push("tokens_used"); }
        if self.first_user_message != other.first_user_message { diffs.push("first_user_message"); }
        if self.archived_at != other.archived_at { diffs.push("archived_at"); }
        if self.git_sha != other.git_sha { diffs.push("git_sha"); }
        if self.git_branch != other.git_branch { diffs.push("git_branch"); }
        if self.git_origin_url != other.git_origin_url { diffs.push("git_origin_url"); }
        diffs
    }
}

// ---------------------------------------------------------------------------
// ThreadMetadataBuilder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMetadataBuilder {
    pub id: ThreadId,
    pub rollout_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub source: SessionSource,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
    pub agent_path: Option<String>,
    pub model_provider: Option<String>,
    pub cwd: PathBuf,
    pub cli_version: Option<String>,
    pub sandbox_policy: SandboxPolicy,
    pub approval_mode: AskForApproval,
    pub archived_at: Option<DateTime<Utc>>,
    pub git_sha: Option<String>,
    pub git_branch: Option<String>,
    pub git_origin_url: Option<String>,
}

impl ThreadMetadataBuilder {
    pub fn new(
        id: ThreadId,
        rollout_path: PathBuf,
        created_at: DateTime<Utc>,
        source: SessionSource,
    ) -> Self {
        Self {
            id,
            rollout_path,
            created_at,
            updated_at: None,
            source,
            agent_nickname: None,
            agent_role: None,
            agent_path: None,
            model_provider: None,
            cwd: PathBuf::new(),
            cli_version: None,
            sandbox_policy: SandboxPolicy::new_read_only_policy(),
            approval_mode: AskForApproval::OnRequest,
            archived_at: None,
            git_sha: None,
            git_branch: None,
            git_origin_url: None,
        }
    }

    pub fn build(&self, default_provider: &str) -> ThreadMetadata {
        let source = enum_to_string(&self.source);
        let sandbox_policy = enum_to_string(&self.sandbox_policy);
        let approval_mode = enum_to_string(&self.approval_mode);
        let created_at = self.created_at;
        let updated_at = self.updated_at.unwrap_or(created_at);
        ThreadMetadata {
            id: self.id,
            rollout_path: self.rollout_path.clone(),
            created_at,
            updated_at,
            source,
            agent_nickname: self.agent_nickname.clone(),
            agent_role: self.agent_role.clone(),
            agent_path: self
                .agent_path
                .clone()
                .or_else(|| self.source.get_agent_path().map(Into::into)),
            model_provider: self
                .model_provider
                .clone()
                .unwrap_or_else(|| default_provider.to_string()),
            model: None,
            reasoning_effort: None,
            cwd: self.cwd.clone(),
            cli_version: self.cli_version.clone().unwrap_or_default(),
            title: String::new(),
            sandbox_policy,
            approval_mode,
            tokens_used: 0,
            first_user_message: None,
            archived_at: self.archived_at,
            git_sha: self.git_sha.clone(),
            git_branch: self.git_branch.clone(),
            git_origin_url: self.git_origin_url.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// BackfillState / BackfillStatus / BackfillStats
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackfillState {
    pub status: BackfillStatus,
    pub last_watermark: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
}

impl Default for BackfillState {
    fn default() -> Self {
        Self {
            status: BackfillStatus::Pending,
            last_watermark: None,
            last_success_at: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillStatus {
    Pending,
    Running,
    Complete,
}

impl BackfillStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            BackfillStatus::Pending => "pending",
            BackfillStatus::Running => "running",
            BackfillStatus::Complete => "complete",
        }
    }

    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "complete" => Ok(Self::Complete),
            _ => Err(anyhow::anyhow!("invalid backfill status: {value}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackfillStats {
    pub scanned: usize,
    pub upserted: usize,
    pub failed: usize,
}

// ---------------------------------------------------------------------------
// DirectionalThreadSpawnEdgeStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectionalThreadSpawnEdgeStatus {
    Open,
    Closed,
}

impl DirectionalThreadSpawnEdgeStatus {
    pub fn as_ref(&self) -> &str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

impl std::fmt::Display for DirectionalThreadSpawnEdgeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl std::str::FromStr for DirectionalThreadSpawnEdgeStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "closed" => Ok(Self::Closed),
            _ => Err(anyhow::anyhow!("invalid edge status: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// AgentJob types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl AgentJobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            AgentJobStatus::Pending => "pending",
            AgentJobStatus::Running => "running",
            AgentJobStatus::Completed => "completed",
            AgentJobStatus::Failed => "failed",
            AgentJobStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(anyhow::anyhow!("invalid agent job status: {value}")),
        }
    }

    pub fn is_final(self) -> bool {
        matches!(
            self,
            AgentJobStatus::Completed | AgentJobStatus::Failed | AgentJobStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentJobItemStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl AgentJobItemStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            AgentJobItemStatus::Pending => "pending",
            AgentJobItemStatus::Running => "running",
            AgentJobItemStatus::Completed => "completed",
            AgentJobItemStatus::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow::anyhow!("invalid agent job item status: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentJob {
    pub id: String,
    pub name: String,
    pub status: AgentJobStatus,
    pub instruction: String,
    pub auto_export: bool,
    pub max_runtime_seconds: Option<u64>,
    pub output_schema_json: Option<Value>,
    pub input_headers: Vec<String>,
    pub input_csv_path: String,
    pub output_csv_path: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentJobItem {
    pub job_id: String,
    pub item_id: String,
    pub row_index: i64,
    pub source_id: Option<String>,
    pub row_json: Value,
    pub status: AgentJobItemStatus,
    pub assigned_thread_id: Option<String>,
    pub attempt_count: i64,
    pub result_json: Option<Value>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub reported_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentJobProgress {
    pub total_items: usize,
    pub pending_items: usize,
    pub running_items: usize,
    pub completed_items: usize,
    pub failed_items: usize,
}

#[derive(Debug, Clone)]
pub struct AgentJobCreateParams {
    pub id: String,
    pub name: String,
    pub instruction: String,
    pub auto_export: bool,
    pub max_runtime_seconds: Option<u64>,
    pub output_schema_json: Option<Value>,
    pub input_headers: Vec<String>,
    pub input_csv_path: String,
    pub output_csv_path: String,
}

#[derive(Debug, Clone)]
pub struct AgentJobItemCreateParams {
    pub item_id: String,
    pub row_index: i64,
    pub source_id: Option<String>,
    pub row_json: Value,
}

// ---------------------------------------------------------------------------
// Log types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub ts: i64,
    pub ts_nanos: i64,
    pub level: String,
    pub target: String,
    pub message: Option<String>,
    pub feedback_log_body: Option<String>,
    pub thread_id: Option<String>,
    pub process_uuid: Option<String>,
    pub module_path: Option<String>,
    pub file: Option<String>,
    pub line: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct LogRow {
    pub id: i64,
    pub ts: i64,
    pub ts_nanos: i64,
    pub level: String,
    pub target: String,
    pub message: Option<String>,
    pub thread_id: Option<String>,
    pub process_uuid: Option<String>,
    pub file: Option<String>,
    pub line: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct LogQuery {
    pub level_upper: Option<String>,
    pub from_ts: Option<i64>,
    pub to_ts: Option<i64>,
    pub module_like: Vec<String>,
    pub file_like: Vec<String>,
    pub thread_ids: Vec<String>,
    pub search: Option<String>,
    pub include_threadless: bool,
    pub after_id: Option<i64>,
    pub limit: Option<usize>,
    pub descending: bool,
}

// ---------------------------------------------------------------------------
// Memories types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage1Output {
    pub thread_id: ThreadId,
    pub rollout_path: PathBuf,
    pub source_updated_at: DateTime<Utc>,
    pub raw_memory: String,
    pub rollout_summary: String,
    pub rollout_slug: Option<String>,
    pub cwd: PathBuf,
    pub git_branch: Option<String>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage1OutputRef {
    pub thread_id: ThreadId,
    pub source_updated_at: DateTime<Utc>,
    pub rollout_slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Phase2InputSelection {
    pub selected: Vec<Stage1Output>,
    pub previous_selected: Vec<Stage1Output>,
    pub retained_thread_ids: Vec<ThreadId>,
    pub removed: Vec<Stage1OutputRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage1JobClaimOutcome {
    Claimed { ownership_token: String },
    SkippedUpToDate,
    SkippedRunning,
    SkippedRetryBackoff,
    SkippedRetryExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage1JobClaim {
    pub thread: ThreadMetadata,
    pub ownership_token: String,
}

#[derive(Debug, Clone, Copy)]
pub struct Stage1StartupClaimParams<'a> {
    pub scan_limit: usize,
    pub max_claimed: usize,
    pub max_age_days: i64,
    pub min_rollout_idle_hours: i64,
    pub allowed_sources: &'a [String],
    pub lease_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase2JobClaimOutcome {
    Claimed {
        ownership_token: String,
        input_watermark: i64,
    },
    SkippedNotDirty,
    SkippedRunning,
}

// ---------------------------------------------------------------------------
// StateRuntime — stub
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct StateRuntime {
    codex_home: PathBuf,
    default_provider: String,
}

impl StateRuntime {
    pub async fn init(codex_home: PathBuf, default_provider: String) -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            codex_home,
            default_provider,
        }))
    }

    pub fn codex_home(&self) -> &Path {
        self.codex_home.as_path()
    }

    // -- threads --

    pub async fn get_thread(&self, id: ThreadId) -> anyhow::Result<Option<ThreadMetadata>> {
        Ok(None)
    }

    pub async fn get_thread_memory_mode(&self, id: ThreadId) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    pub async fn get_dynamic_tools(
        &self,
        thread_id: ThreadId,
    ) -> anyhow::Result<Option<Vec<DynamicToolSpec>>> {
        Ok(None)
    }

    pub async fn upsert_thread_spawn_edge(
        &self,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
        status: DirectionalThreadSpawnEdgeStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn set_thread_spawn_edge_status(
        &self,
        child_thread_id: ThreadId,
        status: DirectionalThreadSpawnEdgeStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn list_thread_spawn_children_with_status(
        &self,
        parent_thread_id: ThreadId,
        status: DirectionalThreadSpawnEdgeStatus,
    ) -> anyhow::Result<Vec<ThreadId>> {
        Ok(Vec::new())
    }

    pub async fn list_thread_spawn_descendants_with_status(
        &self,
        root_thread_id: ThreadId,
        status: DirectionalThreadSpawnEdgeStatus,
    ) -> anyhow::Result<Vec<ThreadId>> {
        Ok(Vec::new())
    }

    pub async fn find_thread_spawn_child_by_path(
        &self,
        parent_thread_id: ThreadId,
        agent_path: &str,
    ) -> anyhow::Result<Option<ThreadId>> {
        Ok(None)
    }

    pub async fn find_thread_spawn_descendant_by_path(
        &self,
        root_thread_id: ThreadId,
        agent_path: &str,
    ) -> anyhow::Result<Option<ThreadId>> {
        Ok(None)
    }

    pub async fn find_rollout_path_by_id(
        &self,
        thread_id: ThreadId,
        _archived_only: Option<bool>,
    ) -> anyhow::Result<Option<PathBuf>> {
        Ok(None)
    }

    pub async fn list_threads(
        &self,
        page_size: usize,
        anchor: Option<&Anchor>,
        sort_key: SortKey,
        allowed_sources: &[String],
        model_providers: Option<&[String]>,
        archived_only: bool,
        search_term: Option<&str>,
    ) -> anyhow::Result<ThreadsPage> {
        Ok(ThreadsPage {
            items: Vec::new(),
            next_anchor: None,
            num_scanned_rows: 0,
        })
    }

    pub async fn list_thread_ids(
        &self,
        limit: usize,
        anchor: Option<&Anchor>,
        sort_key: SortKey,
        allowed_sources: &[String],
        model_providers: Option<&[String]>,
        archived_only: bool,
    ) -> anyhow::Result<Vec<ThreadId>> {
        Ok(Vec::new())
    }

    pub async fn upsert_thread(&self, metadata: &ThreadMetadata) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn insert_thread_if_absent(&self, metadata: &ThreadMetadata) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn set_thread_memory_mode(
        &self,
        thread_id: ThreadId,
        memory_mode: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn touch_thread_updated_at(
        &self,
        thread_id: ThreadId,
        updated_at: DateTime<Utc>,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn update_thread_git_info(
        &self,
        thread_id: ThreadId,
        git_sha: Option<Option<&str>>,
        git_branch: Option<Option<&str>>,
        git_origin_url: Option<Option<&str>>,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn persist_dynamic_tools(
        &self,
        thread_id: ThreadId,
        tools: Option<&[DynamicToolSpec]>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn apply_rollout_items(
        &self,
        builder: &ThreadMetadataBuilder,
        items: &[RolloutItem],
        new_thread_memory_mode: Option<&str>,
        updated_at_override: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn mark_archived(
        &self,
        thread_id: ThreadId,
        rollout_path: &Path,
        archived_at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn mark_unarchived(
        &self,
        thread_id: ThreadId,
        rollout_path: &Path,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn delete_thread(&self, thread_id: ThreadId) -> anyhow::Result<u64> {
        Ok(0)
    }

    // -- backfill --

    pub async fn get_backfill_state(&self) -> anyhow::Result<BackfillState> {
        Ok(BackfillState::default())
    }

    pub async fn try_claim_backfill(&self, lease_seconds: i64) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_backfill_running(&self) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn checkpoint_backfill(&self, watermark: &str) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn mark_backfill_complete(&self, last_watermark: Option<&str>) -> anyhow::Result<()> {
        Ok(())
    }

    // -- logs --

    pub async fn insert_log(&self, entry: &LogEntry) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn insert_logs(&self, entries: &[LogEntry]) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn query_logs(&self, query: &LogQuery) -> anyhow::Result<Vec<LogRow>> {
        Ok(Vec::new())
    }

    pub async fn query_feedback_logs(&self, thread_id: &str) -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }

    pub async fn max_log_id(&self, query: &LogQuery) -> anyhow::Result<i64> {
        Ok(0)
    }

    // -- agent jobs --

    pub async fn create_agent_job(
        &self,
        params: &AgentJobCreateParams,
        items: &[AgentJobItemCreateParams],
    ) -> anyhow::Result<AgentJob> {
        let now = Utc::now();
        Ok(AgentJob {
            id: params.id.clone(),
            name: params.name.clone(),
            status: AgentJobStatus::Pending,
            instruction: params.instruction.clone(),
            auto_export: params.auto_export,
            max_runtime_seconds: params.max_runtime_seconds,
            output_schema_json: params.output_schema_json.clone(),
            input_headers: params.input_headers.clone(),
            input_csv_path: params.input_csv_path.clone(),
            output_csv_path: params.output_csv_path.clone(),
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
            last_error: None,
        })
    }

    pub async fn get_agent_job(&self, job_id: &str) -> anyhow::Result<Option<AgentJob>> {
        Ok(None)
    }

    pub async fn list_agent_job_items(
        &self,
        job_id: &str,
        status: Option<AgentJobItemStatus>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<AgentJobItem>> {
        Ok(Vec::new())
    }

    pub async fn get_agent_job_item(
        &self,
        job_id: &str,
        item_id: &str,
    ) -> anyhow::Result<Option<AgentJobItem>> {
        Ok(None)
    }

    pub async fn mark_agent_job_running(&self, job_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn mark_agent_job_completed(&self, job_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn mark_agent_job_failed(
        &self,
        job_id: &str,
        error_message: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn mark_agent_job_cancelled(
        &self,
        job_id: &str,
        reason: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn is_agent_job_cancelled(&self, job_id: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_agent_job_item_running(
        &self,
        job_id: &str,
        item_id: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_agent_job_item_running_with_thread(
        &self,
        job_id: &str,
        item_id: &str,
        thread_id: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_agent_job_item_pending(
        &self,
        job_id: &str,
        item_id: &str,
        error_message: Option<&str>,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn set_agent_job_item_thread(
        &self,
        job_id: &str,
        item_id: &str,
        thread_id: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn report_agent_job_item_result(
        &self,
        job_id: &str,
        item_id: &str,
        reporting_thread_id: &str,
        result_json: &Value,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_agent_job_item_completed(
        &self,
        job_id: &str,
        item_id: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_agent_job_item_failed(
        &self,
        job_id: &str,
        item_id: &str,
        error_message: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn get_agent_job_progress(&self, job_id: &str) -> anyhow::Result<AgentJobProgress> {
        Ok(AgentJobProgress {
            total_items: 0,
            pending_items: 0,
            running_items: 0,
            completed_items: 0,
            failed_items: 0,
        })
    }

    // -- memories --

    pub async fn clear_memory_data(&self) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn reset_memory_data_for_fresh_start(&self) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn record_stage1_output_usage(
        &self,
        thread_ids: &[ThreadId],
    ) -> anyhow::Result<usize> {
        Ok(0)
    }

    pub async fn claim_stage1_jobs_for_startup(
        &self,
        current_thread_id: ThreadId,
        params: Stage1StartupClaimParams<'_>,
    ) -> anyhow::Result<Vec<Stage1JobClaim>> {
        Ok(Vec::new())
    }

    pub async fn list_stage1_outputs_for_global(
        &self,
        n: usize,
    ) -> anyhow::Result<Vec<Stage1Output>> {
        Ok(Vec::new())
    }

    pub async fn prune_stage1_outputs_for_retention(
        &self,
        max_unused_days: i64,
        limit: usize,
    ) -> anyhow::Result<usize> {
        Ok(0)
    }

    pub async fn get_phase2_input_selection(
        &self,
        n: usize,
        max_unused_days: i64,
    ) -> anyhow::Result<Phase2InputSelection> {
        Ok(Phase2InputSelection::default())
    }

    pub async fn mark_thread_memory_mode_polluted(
        &self,
        thread_id: ThreadId,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn try_claim_stage1_job(
        &self,
        thread_id: ThreadId,
        worker_id: ThreadId,
        source_updated_at: i64,
        lease_seconds: i64,
        max_running_jobs: usize,
    ) -> anyhow::Result<Stage1JobClaimOutcome> {
        Ok(Stage1JobClaimOutcome::SkippedRunning)
    }

    pub async fn mark_stage1_job_succeeded(
        &self,
        thread_id: ThreadId,
        ownership_token: &str,
        source_updated_at: i64,
        raw_memory: &str,
        rollout_summary: &str,
        rollout_slug: Option<&str>,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_stage1_job_succeeded_no_output(
        &self,
        thread_id: ThreadId,
        ownership_token: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_stage1_job_failed(
        &self,
        thread_id: ThreadId,
        ownership_token: &str,
        failure_reason: &str,
        retry_delay_seconds: i64,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn enqueue_global_consolidation(&self, input_watermark: i64) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn try_claim_global_phase2_job(
        &self,
        worker_id: ThreadId,
        lease_seconds: i64,
    ) -> anyhow::Result<Phase2JobClaimOutcome> {
        Ok(Phase2JobClaimOutcome::SkippedNotDirty)
    }

    pub async fn heartbeat_global_phase2_job(
        &self,
        ownership_token: &str,
        lease_seconds: i64,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_global_phase2_job_succeeded(
        &self,
        ownership_token: &str,
        completed_watermark: i64,
        selected_outputs: &[Stage1Output],
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_global_phase2_job_failed(
        &self,
        ownership_token: &str,
        failure_reason: &str,
        retry_delay_seconds: i64,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub async fn mark_global_phase2_job_failed_if_unowned(
        &self,
        ownership_token: &str,
        failure_reason: &str,
        retry_delay_seconds: i64,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }
}

#![allow(unused_variables, unused_mut, unused_imports, dead_code, clippy::all)]

//! codex-state for WASM — backed by wasi-sqlx (rusqlite) instead of async sqlx.
//! All database operations use real SQLite via OPFS.

pub mod log_db;
pub mod model;
mod extract;
mod migrations;
mod paths;

// Runtime sub-modules implement StateRuntime methods via `impl StateRuntime` blocks.
mod runtime {
    use super::*;
    use sqlx::{QueryBuilder, Row, Sqlite};

    mod agent_jobs;
    mod backfill;
    mod logs;
    mod memories;
    #[cfg(test)]
    mod test_support;
    mod threads;
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::{AskForApproval, RolloutItem, SandboxPolicy, SessionSource};
use serde::Serialize;
use serde_json::Value;
use sqlx::sqlite::{
    SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions,
    SqliteSynchronous,
};
use sqlx::ConnectOptions;
use sqlx::SqlitePool;
use uuid::Uuid;

pub use model::*;

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
// StateRuntime — real implementation backed by wasi-sqlx
// ---------------------------------------------------------------------------

use migrations::{LOGS_MIGRATOR, STATE_MIGRATOR};

#[derive(Clone)]
pub struct StateRuntime {
    codex_home: PathBuf,
    default_provider: String,
    pool: Arc<SqlitePool>,
    logs_pool: Arc<SqlitePool>,
}

impl StateRuntime {
    /// Initialize the state runtime with real SQLite databases.
    pub async fn init(codex_home: PathBuf, default_provider: String) -> anyhow::Result<Arc<Self>> {
        tokio::fs::create_dir_all(&codex_home).await.ok();

        let state_path = state_db_path(&codex_home);
        let logs_path = logs_db_path(&codex_home);

        let pool = Arc::new(open_sqlite(&state_path, &STATE_MIGRATOR).await?);
        let logs_pool = Arc::new(open_sqlite(&logs_path, &LOGS_MIGRATOR).await?);

        Ok(Arc::new(Self {
            pool,
            logs_pool,
            codex_home,
            default_provider,
        }))
    }

    /// Return the configured Codex home directory for this runtime.
    pub fn codex_home(&self) -> &Path {
        self.codex_home.as_path()
    }
}

/// Open a SQLite database with WASM-compatible settings and run migrations.
async fn open_sqlite(
    path: &Path,
    migrator: &'static sqlx::migrate::Migrator,
) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        // Use MEMORY journal mode — OPFS doesn't support WAL's multi-file handles
        .journal_mode(SqliteJournalMode::Memory)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .auto_vacuum(SqliteAutoVacuum::Incremental)
        .log_statements(log::LevelFilter::Off);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    migrator.run(&pool).await?;

    Ok(pool)
}

// All type definitions (ThreadMetadata, AgentJob, LogEntry, etc.) and
// StateRuntime method implementations (get_thread, upsert_thread, etc.)
// are provided by `pub use model::*` and the `runtime/` sub-modules.

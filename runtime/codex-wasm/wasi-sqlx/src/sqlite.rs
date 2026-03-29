//! SQLite backend module — matches sqlx::sqlite types.

use crate::{Error, Query, QueryParam, SqliteQueryResult};
use std::sync::{Arc, Mutex};

// Re-export SqliteRow so it's accessible as sqlx::sqlite::SqliteRow
pub use crate::SqliteRow;


/// Marker type for the SQLite database backend. Matches sqlx::Sqlite.
pub struct Sqlite;

/// Connection pool — wraps a single rusqlite::Connection behind a Mutex.
/// In single-threaded WASM, this never contends.
#[derive(Clone)]
pub struct SqlitePool {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl SqlitePool {
    pub fn new(conn: rusqlite::Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn with_conn<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<R, Error>,
    {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&conn)
    }

    pub(crate) fn execute_query(&self, query: Query) -> Result<SqliteQueryResult, Error> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&query.sql)?;
            bind_params(&mut stmt, &query.params)?;
            let rows_affected: u64 = stmt.raw_execute()? as u64;
            Ok(SqliteQueryResult { rows_affected })
        })
    }

    pub(crate) fn fetch_optional(&self, query: Query) -> Result<Option<SqliteRow>, Error> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&query.sql)?;
            let col_count = stmt.column_count();
            let col_names: Vec<String> = stmt
                .column_names()
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            bind_params(&mut stmt, &query.params)?;
            let mut rows = stmt.raw_query();
            match rows.next()? {
                Some(row) => Ok(Some(SqliteRow::from_rusqlite_row(
                    row, col_count, &col_names,
                )?)),
                None => Ok(None),
            }
        })
    }

    pub(crate) fn fetch_one(&self, query: Query) -> Result<SqliteRow, Error> {
        self.fetch_optional(query)?
            .ok_or(Error::RowNotFound)
    }

    pub(crate) fn fetch_all(&self, query: Query) -> Result<Vec<SqliteRow>, Error> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&query.sql)?;
            let col_count = stmt.column_count();
            let col_names: Vec<String> = stmt
                .column_names()
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            bind_params(&mut stmt, &query.params)?;
            let mut raw_rows = stmt.raw_query();
            let mut result = Vec::new();
            while let Some(row) = raw_rows.next()? {
                result.push(SqliteRow::from_rusqlite_row(row, col_count, &col_names)?);
            }
            Ok(result)
        })
    }
}

/// Bind parameters to a prepared statement.
fn bind_params(stmt: &mut rusqlite::Statement<'_>, params: &[QueryParam]) -> Result<(), Error> {
    for (i, param) in params.iter().enumerate() {
        let idx = i + 1; // rusqlite uses 1-based indices
        match param {
            QueryParam::Null => stmt.raw_bind_parameter(idx, rusqlite::types::Null)?,
            QueryParam::Integer(v) => stmt.raw_bind_parameter(idx, v)?,
            QueryParam::Real(v) => stmt.raw_bind_parameter(idx, v)?,
            QueryParam::Text(v) => stmt.raw_bind_parameter(idx, v.as_str())?,
            QueryParam::Blob(v) => stmt.raw_bind_parameter(idx, v.as_slice())?,
            QueryParam::Bool(v) => stmt.raw_bind_parameter(idx, *v as i64)?,
        }
    }
    Ok(())
}

/// Connection options — matches sqlx::sqlite::SqliteConnectOptions.
pub struct SqliteConnectOptions {
    pub(crate) path: String,
    pub(crate) create_if_missing: bool,
}

impl SqliteConnectOptions {
    pub fn new() -> Self {
        Self {
            path: ":memory:".to_string(),
            create_if_missing: true,
        }
    }

    pub fn filename(mut self, path: impl AsRef<str>) -> Self {
        self.path = path.as_ref().to_string();
        self
    }

    pub fn create_if_missing(mut self, create: bool) -> Self {
        self.create_if_missing = create;
        self
    }

    pub fn journal_mode(self, _mode: SqliteJournalMode) -> Self {
        self // Journal mode set at runtime via PRAGMA
    }

    pub fn synchronous(self, _sync: SqliteSynchronous) -> Self {
        self
    }

    pub fn busy_timeout(self, _timeout: std::time::Duration) -> Self {
        self
    }
}

impl crate::ConnectOptions for SqliteConnectOptions {
    fn log_statements(self, _level: log::LevelFilter) -> Self {
        self
    }
    fn log_slow_statements(self, _level: log::LevelFilter, _duration: std::time::Duration) -> Self {
        self
    }
}

impl std::str::FromStr for SqliteConnectOptions {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            path: s.replace("sqlite://", "").replace("sqlite:", ""),
            create_if_missing: true,
        })
    }
}

/// Pool options — matches sqlx::sqlite::SqlitePoolOptions.
pub struct SqlitePoolOptions {
    _max_connections: u32,
}

impl SqlitePoolOptions {
    pub fn new() -> Self {
        Self {
            _max_connections: 1,
        }
    }

    pub fn max_connections(mut self, max: u32) -> Self {
        self._max_connections = max;
        self
    }

    pub async fn connect_with(self, options: SqliteConnectOptions) -> Result<SqlitePool, Error> {
        let conn = rusqlite::Connection::open(&options.path)?;
        // Match sqlite-module settings for WASM/OPFS compatibility
        conn.execute_batch("PRAGMA journal_mode = MEMORY;")?;
        Ok(SqlitePool::new(conn))
    }
}

/// Journal mode enum — matches sqlx::sqlite::SqliteJournalMode.
pub enum SqliteJournalMode {
    Delete,
    Truncate,
    Persist,
    Memory,
    Wal,
    Off,
}

/// Synchronous mode — matches sqlx::sqlite::SqliteSynchronous.
pub enum SqliteSynchronous {
    Off,
    Normal,
    Full,
    Extra,
}

/// Connection type alias.
pub type SqliteConnection = SqlitePool;

/// Transaction wrapper — in single-threaded WASM, this is just
/// a reference to the pool with BEGIN/COMMIT semantics.
pub struct Transaction {
    pool: SqlitePool,
    committed: bool,
}

impl Transaction {
    pub async fn commit(mut self) -> Result<(), Error> {
        self.pool.with_conn(|conn| {
            conn.execute_batch("COMMIT;")?;
            Ok(())
        })?;
        self.committed = true;
        Ok(())
    }

    pub async fn rollback(mut self) -> Result<(), Error> {
        self.pool.with_conn(|conn| {
            conn.execute_batch("ROLLBACK;")?;
            Ok(())
        })?;
        self.committed = true; // prevent double rollback in drop
        Ok(())
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.pool.with_conn(|conn| {
                conn.execute_batch("ROLLBACK;").ok();
                Ok(())
            });
        }
    }
}

impl std::ops::Deref for Transaction {
    type Target = SqlitePool;
    fn deref(&self) -> &SqlitePool {
        &self.pool
    }
}

impl SqlitePool {
    pub async fn begin(&self) -> Result<Transaction, Error> {
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN;")?;
            Ok(())
        })?;
        Ok(Transaction {
            pool: self.clone(),
            committed: false,
        })
    }
}

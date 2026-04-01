#![allow(unused_variables, unused_imports, dead_code, clippy::all)]
//! wasi-sqlx: A sqlx-compatible API shim backed by rusqlite for WASM.
//!
//! Provides the subset of sqlx's API that codex-state uses, backed by
//! rusqlite (bundled SQLite) instead of async sqlx with compile-time
//! checked queries. All async methods are synchronous under the hood
//! since WASM is single-threaded.

use std::sync::{Arc, Mutex};

pub mod sqlite;

// Re-export sqlx_macros via `extern crate` so its proc macros (migrate!, FromRow)
// are accessible in the macro namespace as `sqlx::migrate!()` and `#[derive(sqlx::FromRow)]`.
// This coexists with `pub mod migrate` because macros and modules are in different namespaces.
#[doc(hidden)]
pub extern crate sqlx_macros;
pub use sqlx_macros::FromRow;

// The migrate module provides Migrator and EmbeddedMigration types.
// `sqlx::migrate::Migrator` (type namespace) coexists with `sqlx::migrate!()` (macro namespace).
pub mod migrate {
    //! Migration types used by the `migrate!` proc macro.
    mod inner {
        include!("_migrate_types.rs");
    }
    pub use inner::*;
}

// Re-exports matching sqlx's public API
pub use sqlite::Sqlite;
pub use sqlite::SqlitePool;
pub use sqlite::SqliteConnection;

/// Trait for types that can execute queries against a SqlitePool.
pub trait QueryExecutor {}
impl QueryExecutor for &SqlitePool {}

/// Marker trait matching sqlx::Executor. Any Executor can be used where
/// QueryExecutor is needed (for .execute()/.fetch_*() calls).
pub trait Executor<'c>: QueryExecutor {
    type Database;
    fn as_pool(&self) -> &SqlitePool;
}
impl<'c> Executor<'c> for &'c SqlitePool {
    type Database = Sqlite;
    fn as_pool(&self) -> &SqlitePool {
        self
    }
}
impl<'c> Executor<'c> for &'c mut SqlitePool {
    type Database = Sqlite;
    fn as_pool(&self) -> &SqlitePool {
        self
    }
}
impl<'c, 't> Executor<'c> for &'c sqlite::Transaction<'t, Sqlite> {
    type Database = Sqlite;
    fn as_pool(&self) -> &SqlitePool {
        self // Deref to SqlitePool
    }
}
impl<'c, 't> Executor<'c> for &'c mut sqlite::Transaction<'t, Sqlite> {
    type Database = Sqlite;
    fn as_pool(&self) -> &SqlitePool {
        self // Deref to SqlitePool
    }
}
impl QueryExecutor for &mut SqlitePool {}
impl<'t> QueryExecutor for &sqlite::Transaction<'t, Sqlite> {}
impl<'t> QueryExecutor for &mut sqlite::Transaction<'t, Sqlite> {}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    RowNotFound,
    Database(String),
    ColumnNotFound(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::RowNotFound => write!(f, "row not found"),
            Error::Database(e) => write!(f, "database error: {e}"),
            Error::ColumnNotFound(c) => write!(f, "column not found: {c}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => Error::RowNotFound,
            other => Error::Database(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Row
// ---------------------------------------------------------------------------

/// A row from a SQLite query result. Wraps column name→value pairs.
#[derive(Debug, Clone)]
pub struct SqliteRow {
    columns: Vec<(String, SqliteValue)>,
}

#[derive(Debug, Clone)]
pub enum SqliteValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl SqliteRow {
    pub(crate) fn from_rusqlite_row(
        row: &rusqlite::Row<'_>,
        col_count: usize,
        col_names: &[String],
    ) -> Result<Self, rusqlite::Error> {
        let mut columns = Vec::with_capacity(col_count);
        for (i, name) in col_names.iter().enumerate() {
            let val = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => SqliteValue::Null,
                rusqlite::types::ValueRef::Integer(v) => SqliteValue::Integer(v),
                rusqlite::types::ValueRef::Real(v) => SqliteValue::Real(v),
                rusqlite::types::ValueRef::Text(v) => {
                    SqliteValue::Text(String::from_utf8_lossy(v).to_string())
                }
                rusqlite::types::ValueRef::Blob(v) => SqliteValue::Blob(v.to_vec()),
            };
            columns.push((name.clone(), val));
        }
        Ok(SqliteRow { columns })
    }

    fn find_column(&self, name: &str) -> Option<&SqliteValue> {
        self.columns
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    fn find_column_by_index(&self, idx: usize) -> Option<&SqliteValue> {
        self.columns.get(idx).map(|(_, v)| v)
    }
}

/// Trait for extracting typed values from rows.
/// Matches sqlx::Row which uses `try_get::<T, I>(index)` with two type params.
/// The second type param (index type) is ignored in our shim.
pub trait Row {
    fn try_get<'r, T: FromSqliteValue, I: ColumnIndex>(&'r self, col: I) -> Result<T, Error>;
    fn get<'r, T: FromSqliteValue, I: ColumnIndex>(&'r self, col: I) -> T {
        self.try_get(col).expect("column should exist")
    }
}

/// Trait for column index types (by name or position).
pub trait ColumnIndex {
    fn resolve(&self, row: &SqliteRow) -> Option<usize>;
}

impl ColumnIndex for &str {
    fn resolve(&self, row: &SqliteRow) -> Option<usize> {
        row.columns.iter().position(|(n, _)| n == *self)
    }
}

impl ColumnIndex for usize {
    fn resolve(&self, row: &SqliteRow) -> Option<usize> {
        if *self < row.columns.len() {
            Some(*self)
        } else {
            None
        }
    }
}

impl Row for SqliteRow {
    fn try_get<'r, T: FromSqliteValue, I: ColumnIndex>(&'r self, col: I) -> Result<T, Error> {
        let idx = col
            .resolve(self)
            .ok_or_else(|| Error::ColumnNotFound("column".to_string()))?;
        let val = &self.columns[idx].1;
        T::from_sqlite_value(val).ok_or_else(|| Error::ColumnNotFound("column".to_string()))
    }
}

/// Conversion from SQLite values to Rust types.
pub trait FromSqliteValue: Sized {
    fn from_sqlite_value(val: &SqliteValue) -> Option<Self>;
}

impl FromSqliteValue for String {
    fn from_sqlite_value(val: &SqliteValue) -> Option<Self> {
        match val {
            SqliteValue::Text(s) => Some(s.clone()),
            SqliteValue::Null => None,
            SqliteValue::Integer(i) => Some(i.to_string()),
            _ => None,
        }
    }
}

impl FromSqliteValue for i64 {
    fn from_sqlite_value(val: &SqliteValue) -> Option<Self> {
        match val {
            SqliteValue::Integer(i) => Some(*i),
            SqliteValue::Null => Some(0),
            _ => None,
        }
    }
}

impl FromSqliteValue for i32 {
    fn from_sqlite_value(val: &SqliteValue) -> Option<Self> {
        match val {
            SqliteValue::Integer(i) => Some(*i as i32),
            SqliteValue::Null => Some(0),
            _ => None,
        }
    }
}

impl FromSqliteValue for bool {
    fn from_sqlite_value(val: &SqliteValue) -> Option<Self> {
        match val {
            SqliteValue::Integer(i) => Some(*i != 0),
            SqliteValue::Null => Some(false),
            _ => None,
        }
    }
}

impl FromSqliteValue for f64 {
    fn from_sqlite_value(val: &SqliteValue) -> Option<Self> {
        match val {
            SqliteValue::Real(f) => Some(*f),
            SqliteValue::Integer(i) => Some(*i as f64),
            SqliteValue::Null => Some(0.0),
            _ => None,
        }
    }
}

impl<T: FromSqliteValue> FromSqliteValue for Option<T> {
    fn from_sqlite_value(val: &SqliteValue) -> Option<Self> {
        match val {
            SqliteValue::Null => Some(None),
            _ => T::from_sqlite_value(val).map(Some),
        }
    }
}

// ---------------------------------------------------------------------------
// Query + Bind
// ---------------------------------------------------------------------------

/// A query with bound parameters.
pub struct Query {
    sql: String,
    params: Vec<QueryParam>,
}

#[derive(Debug, Clone)]
pub enum QueryParam {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Bool(bool),
}

impl Query {
    pub fn bind<T: IntoQueryParam>(mut self, value: T) -> Self {
        self.params.push(value.into_query_param());
        self
    }

    pub async fn fetch_optional<'e, E>(self, executor: E) -> Result<Option<SqliteRow>, Error>
    where
        E: Executor<'e>,
    {
        executor.as_pool().fetch_optional(self)
    }

    pub async fn fetch_one<'e, E>(self, executor: E) -> Result<SqliteRow, Error>
    where
        E: Executor<'e>,
    {
        executor.as_pool().fetch_one(self)
    }

    pub async fn fetch_all<'e, E>(self, executor: E) -> Result<Vec<SqliteRow>, Error>
    where
        E: Executor<'e>,
    {
        executor.as_pool().fetch_all(self)
    }

    pub async fn execute<'e, E>(self, executor: E) -> Result<SqliteQueryResult, Error>
    where
        E: Executor<'e>,
    {
        executor.as_pool().execute_query(self)
    }
}

/// Result of an execute() call.
pub struct SqliteQueryResult {
    pub rows_affected: u64,
}

impl SqliteQueryResult {
    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
}

/// Trait for binding values to query parameters.
pub trait IntoQueryParam {
    fn into_query_param(self) -> QueryParam;
}

impl IntoQueryParam for String {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Text(self)
    }
}

impl IntoQueryParam for &str {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Text(self.to_string())
    }
}

impl IntoQueryParam for &String {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Text(self.clone())
    }
}

impl IntoQueryParam for &&str {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Text((*self).to_string())
    }
}

impl<T: IntoQueryParam + Clone> IntoQueryParam for &Option<T> {
    fn into_query_param(self) -> QueryParam {
        match self {
            Some(v) => v.clone().into_query_param(),
            None => QueryParam::Null,
        }
    }
}

impl IntoQueryParam for &i64 {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Integer(*self)
    }
}

impl IntoQueryParam for &bool {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Bool(*self)
    }
}

impl IntoQueryParam for i64 {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Integer(self)
    }
}

impl IntoQueryParam for i32 {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Integer(self as i64)
    }
}

impl IntoQueryParam for f64 {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Real(self)
    }
}

impl IntoQueryParam for bool {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Bool(self)
    }
}

impl IntoQueryParam for u64 {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Integer(self as i64)
    }
}

impl IntoQueryParam for u32 {
    fn into_query_param(self) -> QueryParam {
        QueryParam::Integer(self as i64)
    }
}

impl<T: IntoQueryParam> IntoQueryParam for Option<T> {
    fn into_query_param(self) -> QueryParam {
        match self {
            Some(v) => v.into_query_param(),
            None => QueryParam::Null,
        }
    }
}

/// Create a query. Matches `sqlx::query(sql)`.
pub fn query(sql: &str) -> Query {
    Query {
        sql: sql.to_string(),
        params: Vec::new(),
    }
}

/// Create a query that returns a single scalar value.
pub fn query_scalar<T>(sql: &str) -> QueryScalar<T> {
    QueryScalar {
        inner: Query {
            sql: sql.to_string(),
            params: Vec::new(),
        },
        _phantom: std::marker::PhantomData,
    }
}

/// Create a query that maps rows via FromRow.
/// Accepts one type param (the row type) to match sqlx's turbofish: query_as::<T>(sql)
pub fn query_as<T>(sql: &str) -> QueryAs<T> {
    QueryAs {
        inner: Query {
            sql: sql.to_string(),
            params: Vec::new(),
        },
        _phantom: std::marker::PhantomData,
    }
}

// ---------------------------------------------------------------------------
// QueryScalar
// ---------------------------------------------------------------------------

pub struct QueryScalar<T> {
    inner: Query,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: FromSqliteValue> QueryScalar<T> {
    pub fn bind<V: IntoQueryParam>(mut self, value: V) -> Self {
        self.inner.params.push(value.into_query_param());
        self
    }

    pub async fn fetch_one<'e, E: Executor<'e>>(self, executor: E) -> Result<T, Error> {
        let row = executor.as_pool().fetch_one(self.inner)?;
        let val = row
            .find_column_by_index(0)
            .ok_or(Error::ColumnNotFound("0".into()))?;
        T::from_sqlite_value(val).ok_or(Error::ColumnNotFound("scalar".into()))
    }

    pub async fn fetch_optional<'e, E: Executor<'e>>(self, executor: E) -> Result<Option<T>, Error> {
        let row = executor.as_pool().fetch_optional(self.inner)?;
        match row {
            Some(row) => {
                let val = row
                    .find_column_by_index(0)
                    .ok_or(Error::ColumnNotFound("0".into()))?;
                Ok(T::from_sqlite_value(val))
            }
            None => Ok(None),
        }
    }
}

// ---------------------------------------------------------------------------
// QueryAs
// ---------------------------------------------------------------------------

pub struct QueryAs<T> {
    inner: Query,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: FromRow> QueryAs<T> {
    pub fn bind<V: IntoQueryParam>(mut self, value: V) -> Self {
        self.inner.params.push(value.into_query_param());
        self
    }

    pub async fn fetch_all<'e, E: Executor<'e>>(self, executor: E) -> Result<Vec<T>, Error> {
        let rows = executor.as_pool().fetch_all(self.inner)?;
        rows.into_iter().map(|r| T::from_row(&r)).collect()
    }

    pub async fn fetch_optional<'e, E: Executor<'e>>(self, executor: E) -> Result<Option<T>, Error> {
        let row = executor.as_pool().fetch_optional(self.inner)?;
        row.map(|r| T::from_row(&r)).transpose()
    }

    pub async fn fetch_one<'e, E: Executor<'e>>(self, executor: E) -> Result<T, Error> {
        let row = executor.as_pool().fetch_one(self.inner)?;
        T::from_row(&row)
    }
}

/// Trait for mapping rows to structs. Matches sqlx::FromRow.
pub trait FromRow: Sized {
    fn from_row(row: &SqliteRow) -> Result<Self, Error>;
}

// ---------------------------------------------------------------------------
// QueryBuilder
// ---------------------------------------------------------------------------

pub struct QueryBuilder<'q, DB = Sqlite> {
    sql: String,
    params: Vec<QueryParam>,
    _phantom: std::marker::PhantomData<(&'q (), DB)>,
}

impl<'q, DB> QueryBuilder<'q, DB> {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn push(&mut self, sql: impl AsRef<str>) -> &mut Self {
        self.sql.push_str(sql.as_ref());
        self
    }

    pub fn push_bind<T: IntoQueryParam>(&mut self, value: T) -> &mut Self {
        self.sql.push('?');
        self.params.push(value.into_query_param());
        self
    }

    /// Separator helper for building IN clauses.
    pub fn separated(&mut self, sep: &str) -> Separated<'_, 'q, DB> {
        Separated {
            builder: self,
            sep: sep.to_string(),
            first: true,
        }
    }

    pub fn build(self) -> Query {
        Query {
            sql: self.sql,
            params: self.params,
        }
    }

    pub fn build_query_as<T: FromRow>(self) -> QueryAs<T> {
        QueryAs {
            inner: self.build(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Push multiple rows of values for INSERT statements.
    pub fn push_values<I, F>(&mut self, rows: I, mut push_row: F) -> &mut Self
    where
        I: IntoIterator,
        F: FnMut(Separated<'_, 'q, DB>, I::Item),
    {
        let mut first_row = true;
        for row in rows {
            if !first_row {
                self.sql.push_str(", ");
            }
            first_row = false;
            self.sql.push('(');
            let sep = Separated {
                builder: self,
                sep: ", ".to_string(),
                first: true,
            };
            push_row(sep, row);
            self.sql.push(')');
        }
        self
    }

    pub fn into_sql(self) -> String {
        self.sql
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }
}

pub struct Separated<'a, 'q, DB = Sqlite> {
    builder: &'a mut QueryBuilder<'q, DB>,
    sep: String,
    first: bool,
}

impl<'a, 'q, DB> Separated<'a, 'q, DB> {
    pub fn push(&mut self, sql: impl AsRef<str>) -> &mut Self {
        if !self.first {
            self.builder.sql.push_str(&self.sep);
        }
        self.first = false;
        self.builder.sql.push_str(sql.as_ref());
        self
    }

    pub fn push_bind<T: IntoQueryParam>(&mut self, value: T) -> &mut Self {
        if !self.first {
            self.builder.sql.push_str(&self.sep);
        }
        self.first = false;
        self.builder.sql.push('?');
        self.builder.params.push(value.into_query_param());
        self
    }

    pub fn push_unseparated(&mut self, sql: impl AsRef<str>) -> &mut Self {
        self.builder.sql.push_str(sql.as_ref());
        self
    }

    pub fn push_bind_unseparated<T: IntoQueryParam>(&mut self, value: T) -> &mut Self {
        self.builder.sql.push('?');
        self.builder.params.push(value.into_query_param());
        self
    }
}

// ---------------------------------------------------------------------------
// ConnectOptions (stub)
// ---------------------------------------------------------------------------

pub trait ConnectOptions: Sized {
    fn log_statements(self, level: log::LevelFilter) -> Self {
        self
    }
    fn log_slow_statements(
        self,
        level: log::LevelFilter,
        duration: std::time::Duration,
    ) -> Self {
        self
    }
}

// ---------------------------------------------------------------------------
// sqlx::migrate! macro replacement
// ---------------------------------------------------------------------------

/// Replacement for sqlx::migrate!("path") compile-time macro.
/// Reads .sql files from a directory at runtime.
#[macro_export]
macro_rules! migrate {
    ($dir:expr) => {{
        $crate::migrate::Migrator::new($dir)
    }};
}

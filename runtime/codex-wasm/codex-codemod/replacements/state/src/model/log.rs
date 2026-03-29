use serde::Serialize;

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

impl sqlx::FromRow for LogRow {
    fn from_row(row: &sqlx::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id")?,
            ts: row.try_get("ts")?,
            ts_nanos: row.try_get("ts_nanos")?,
            level: row.try_get("level")?,
            target: row.try_get("target")?,
            message: row.try_get("message")?,
            thread_id: row.try_get("thread_id")?,
            process_uuid: row.try_get("process_uuid")?,
            file: row.try_get("file")?,
            line: row.try_get("line")?,
        })
    }
}

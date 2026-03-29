//! SQL commands: sqlite3
//!
//! Provides SQL database functionality using rusqlite (bundled sqlite3).
//! Matches the standard sqlite3 CLI interface.

use futures_lite::io::AsyncWriteExt;
use runtime_macros::shell_commands;

use rusqlite::{Connection, OpenFlags};

use super::super::ShellEnv;
use super::parse_common;

/// SQL commands using rusqlite.
pub struct SqlCommands;

#[shell_commands]
impl SqlCommands {
    /// sqlite3 - execute SQL queries using SQLite-compatible database
    #[shell_command(
        name = "sqlite3",
        usage = "sqlite3 [DATABASE] [SQL]",
        description = "SQLite database CLI. DATABASE defaults to :memory: if not specified."
    )]
    fn cmd_sqlite3(
        args: Vec<String>,
        _env: &ShellEnv,
        stdin: piper::Reader,
        mut stdout: piper::Writer,
        mut stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        Box::pin(async move {
            let (_, remaining) = parse_common(&args);
            // Parse positional arguments: [DATABASE] [SQL]
            let (db_path, sql_arg): (String, Option<String>) = match remaining.len() {
                0 => (":memory:".to_string(), None),
                1 => {
                    let arg = &remaining[0];
                    if arg == ":memory:"
                        || arg.ends_with(".db")
                        || arg.ends_with(".sqlite")
                        || arg.ends_with(".sqlite3")
                        || arg.contains('/')
                    {
                        (arg.clone(), None)
                    } else {
                        (":memory:".to_string(), Some(arg.clone()))
                    }
                }
                _ => {
                    let db = remaining[0].clone();
                    let sql = remaining[1..].join(" ");
                    (db, Some(sql))
                }
            };

            // Get SQL from arg or stdin
            let sql = if let Some(s) = sql_arg {
                s
            } else {
                use futures_lite::io::AsyncReadExt;
                let mut buf = Vec::new();
                let mut reader = stdin;
                let _ = reader.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).to_string()
            };

            if sql.trim().is_empty() {
                let _ = stderr.write_all(b"Error: no SQL provided\n").await;
                return 1;
            }

            // Run all SQLite operations synchronously (rusqlite types are !Send),
            // then write collected output asynchronously.
            let result = execute_sql(&db_path, &sql);

            match result {
                Ok(output) => {
                    for line in &output {
                        let _ = stdout.write_all(line.as_bytes()).await;
                        let _ = stdout.write_all(b"\n").await;
                    }
                    0
                }
                Err(msg) => {
                    let _ = stderr.write_all(msg.as_bytes()).await;
                    1
                }
            }
        })
    }
}

/// Execute SQL synchronously, returning output lines or an error message.
/// Separated from the async shell command so rusqlite's !Send types
/// don't cross await boundaries.
fn execute_sql(db_path: &str, sql: &str) -> Result<Vec<String>, String> {
    // Open database
    let conn = if db_path == ":memory:" {
        Connection::open_in_memory().map_err(|e| format!("Error: {}\n", e))?
    } else {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let c = Connection::open_with_flags_and_vfs(db_path, flags, "unix-none")
            .map_err(|e| format!("Error: unable to open database \"{}\": {}\n", db_path, e))?;
        // MEMORY journal: OPFS doesn't support POSIX unlink-while-open semantics
        let _ = c.pragma_update(None, "journal_mode", "MEMORY");
        c
    };

    let statements: Vec<&str> = sql
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // For file-backed databases, wrap in an explicit EXCLUSIVE transaction
    // to prevent re-validation failures under WASI's stream I/O model.
    let has_user_txn = statements.iter().any(|s| {
        let upper = s.to_uppercase();
        upper.starts_with("BEGIN") || upper.starts_with("COMMIT") || upper.starts_with("ROLLBACK")
    });
    let use_explicit_txn = db_path != ":memory:" && !has_user_txn;
    if use_explicit_txn {
        conn.execute_batch("BEGIN EXCLUSIVE")
            .map_err(|e| format!("Error: {}\n", e))?;
    }

    let mut output = Vec::new();

    for stmt_sql in statements {
        let mut stmt = match conn.prepare(stmt_sql) {
            Ok(s) => s,
            Err(_) => {
                conn.execute_batch(stmt_sql)
                    .map_err(|e| format!("Error: {}\n", e))?;
                continue;
            }
        };

        let col_count = stmt.column_count();
        if col_count == 0 {
            stmt.execute([]).map_err(|e| format!("Error: {}\n", e))?;
            continue;
        }

        let mut rows = stmt.query([]).map_err(|e| format!("Error: {}\n", e))?;

        loop {
            match rows.next() {
                Ok(Some(row)) => {
                    let mut values: Vec<String> = Vec::with_capacity(col_count);
                    for i in 0..col_count {
                        let val = match row.get_ref(i) {
                            Ok(rusqlite::types::ValueRef::Null) => String::new(),
                            Ok(rusqlite::types::ValueRef::Integer(n)) => n.to_string(),
                            Ok(rusqlite::types::ValueRef::Real(f)) => f.to_string(),
                            Ok(rusqlite::types::ValueRef::Text(s)) => {
                                String::from_utf8_lossy(s).to_string()
                            }
                            Ok(rusqlite::types::ValueRef::Blob(b)) => {
                                format!("<blob:{} bytes>", b.len())
                            }
                            Err(_) => String::new(),
                        };
                        values.push(val);
                    }
                    output.push(values.join("|"));
                }
                Ok(None) => break,
                Err(e) => return Err(format!("Error: {}\n", e)),
            }
        }
    }

    if use_explicit_txn {
        conn.execute_batch("COMMIT")
            .map_err(|e| format!("Error: COMMIT: {}\n", e))?;
    }

    if let Err((_, e)) = conn.close() {
        return Err(format!("Warning: close error: {}\n", e));
    }

    Ok(output)
}

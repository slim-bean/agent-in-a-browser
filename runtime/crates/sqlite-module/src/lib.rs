//! sqlite-module
//!
//! Provides sqlite3 command via the unix-command WIT interface.
//! Uses rusqlite (bundled sqlite3.c) for full SQLite3 compatibility.

#[allow(warnings)]
mod bindings;

use bindings::exports::shell::unix::command::{ExecEnv, Guest};
use bindings::wasi::io::streams::{InputStream, OutputStream};
use rusqlite::{Connection, OpenFlags};

struct SqliteModule;

impl Guest for SqliteModule {
    fn run(
        name: String,
        args: Vec<String>,
        env: ExecEnv,
        stdin: InputStream,
        stdout: OutputStream,
        stderr: OutputStream,
    ) -> i32 {
        match name.as_str() {
            "sqlite3" => run_sqlite3(args, env, stdin, stdout, stderr),
            _ => {
                write_to_stream(&stderr, format!("Unknown command: {}\n", name).as_bytes());
                127
            }
        }
    }

    fn list_commands() -> Vec<String> {
        vec!["sqlite3".to_string()]
    }
}

/// Execute SQLite commands
fn run_sqlite3(
    args: Vec<String>,
    env: ExecEnv,
    stdin: InputStream,
    stdout: OutputStream,
    stderr: OutputStream,
) -> i32 {
    // Parse arguments: [DATABASE] [SQL]
    let (db_path, sql_arg): (String, Option<String>) = match args.len() {
        0 => (":memory:".to_string(), None),
        1 => {
            let arg = &args[0];
            if arg == ":memory:"
                || arg.ends_with(".db")
                || arg.ends_with(".sqlite")
                || arg.contains('/')
            {
                (arg.clone(), None)
            } else {
                (":memory:".to_string(), Some(arg.clone()))
            }
        }
        _ => {
            let db = args[0].clone();
            let sql = args[1..].join(" ");
            (db, Some(sql))
        }
    };

    // Get SQL from arg or stdin
    let sql = if let Some(s) = sql_arg {
        s
    } else {
        match read_all_from_stream(&stdin) {
            Ok(data) => String::from_utf8_lossy(&data).to_string(),
            Err(e) => {
                write_to_stream(
                    &stderr,
                    format!("sqlite3: failed to read stdin: {}\n", e).as_bytes(),
                );
                return 1;
            }
        }
    };

    if sql.trim().is_empty() {
        write_to_stream(&stderr, b"Error: no SQL provided\n");
        return 1;
    }

    // Resolve database path relative to cwd
    let resolved_path = if db_path == ":memory:" {
        db_path.clone()
    } else if db_path.starts_with('/') {
        db_path.clone()
    } else {
        format!("{}/{}", env.cwd, db_path)
    };

    // Open database
    let conn = if resolved_path == ":memory:" {
        match Connection::open_in_memory() {
            Ok(c) => c,
            Err(e) => {
                write_to_stream(&stderr, format!("Error: {}\n", e).as_bytes());
                return 1;
            }
        }
    } else {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        match Connection::open_with_flags_and_vfs(&resolved_path, flags, "unix-none") {
            Ok(c) => {
                // MEMORY journal: OPFS doesn't support POSIX unlink-while-open
                // semantics that SQLite's default journal mode requires.
                let _ = c.pragma_update(None, "journal_mode", "MEMORY");
                c
            }
            Err(e) => {
                write_to_stream(
                    &stderr,
                    format!("Error: unable to open database \"{}\": {}\n", db_path, e).as_bytes(),
                );
                return 1;
            }
        }
    };

    // Execute SQL statements
    let statements: Vec<&str> = sql
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // For file-backed databases, wrap in an explicit EXCLUSIVE transaction.
    // This prevents SQLite from re-validating the database between statements
    // (which fails under the WASI preview1→preview2 adapter's stream I/O model),
    // and ensures COMMIT writes ALL dirty pages (including the header with updated
    // page count) to disk — unlike PRAGMA locking_mode=EXCLUSIVE which defers
    // header writes and discards them on close.
    let has_user_txn = statements.iter().any(|s| {
        let upper = s.to_uppercase();
        upper.starts_with("BEGIN") || upper.starts_with("COMMIT") || upper.starts_with("ROLLBACK")
    });
    let use_explicit_txn = resolved_path != ":memory:" && !has_user_txn;
    if use_explicit_txn {
        if let Err(e) = conn.execute_batch("BEGIN EXCLUSIVE") {
            write_to_stream(&stderr, format!("Error: {}\n", e).as_bytes());
            return 1;
        }
    }

    for stmt_sql in statements {
        // Use prepare to get column info and iterate rows
        let mut stmt = match conn.prepare(stmt_sql) {
            Ok(s) => s,
            Err(_e) => {
                // Might be a non-query statement (CREATE, INSERT, etc.)
                if let Err(exec_err) = conn.execute_batch(stmt_sql) {
                    write_to_stream(&stderr, format!("Error: {}\n", exec_err).as_bytes());
                    return 1;
                }
                continue;
            }
        };

        let col_count = stmt.column_count();
        if col_count == 0 {
            // Non-query statement
            if let Err(e) = stmt.execute([]) {
                write_to_stream(&stderr, format!("Error: {}\n", e).as_bytes());
                return 1;
            }
            continue;
        }

        let mut rows = match stmt.query([]) {
            Ok(r) => r,
            Err(e) => {
                write_to_stream(&stderr, format!("Error: {}\n", e).as_bytes());
                return 1;
            }
        };

        loop {
            match rows.next() {
                Ok(Some(row)) => {
                    let mut values: Vec<String> = Vec::with_capacity(col_count);
                    for i in 0..col_count {
                        let val: String = match row.get_ref(i) {
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
                    write_to_stream(&stdout, values.join("|").as_bytes());
                    write_to_stream(&stdout, b"\n");
                }
                Ok(None) => break,
                Err(e) => {
                    write_to_stream(&stderr, format!("Error: {}\n", e).as_bytes());
                    return 1;
                }
            }
        }
    }

    // Commit the explicit transaction to flush all dirty pages to disk.
    if use_explicit_txn {
        if let Err(e) = conn.execute_batch("COMMIT") {
            write_to_stream(&stderr, format!("Error: COMMIT: {}\n", e).as_bytes());
            return 1;
        }
    }

    // Close the connection, releasing WASI file descriptors → OPFS SyncAccessHandles
    // so the file can be reopened by future invocations.
    if let Err((_, e)) = conn.close() {
        write_to_stream(&stderr, format!("Warning: close error: {}\n", e).as_bytes());
    }

    0
}

/// Helper to write data to an output stream
fn write_to_stream(stream: &OutputStream, data: &[u8]) {
    let _ = stream.blocking_write_and_flush(data);
}

/// Helper to read all data from an input stream
fn read_all_from_stream(stream: &InputStream) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    while let Ok(chunk) = stream.blocking_read(4096) {
        if chunk.is_empty() {
            break;
        }
        result.extend_from_slice(&chunk);
    }
    Ok(result)
}

bindings::export!(SqliteModule with_types_in bindings);

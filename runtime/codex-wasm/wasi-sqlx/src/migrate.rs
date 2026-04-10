//! Migration support — matches sqlx::migrate.

use crate::sqlite::SqlitePool;
use crate::Error;

/// A migrator that runs SQL migration files.
/// Replaces sqlx::migrate::Migrator which uses compile-time embedding.
pub struct Migrator {
    dir: &'static str,
}

impl Migrator {
    pub const fn new(dir: &'static str) -> Self {
        Self { dir }
    }

    /// Returns the migration directory path.
    pub fn dir(&self) -> &'static str {
        self.dir
    }

    /// Run all pending migrations against the pool.
    pub async fn run(&self, pool: &SqlitePool) -> Result<(), Error> {
        pool.with_conn(|conn| {
            // Create migrations tracking table
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
                    version INTEGER PRIMARY KEY,
                    description TEXT NOT NULL,
                    installed_on TEXT NOT NULL DEFAULT (datetime('now')),
                    success INTEGER NOT NULL DEFAULT 1
                );"
            )?;

            // Read and run migration files from the filesystem
            // In WASM/OPFS, these files are embedded by the codemod build
            // For now, we run all migrations that haven't been applied
            let applied: Vec<i64> = {
                let mut stmt = conn.prepare("SELECT version FROM _sqlx_migrations WHERE success = 1")?;
                let rows = stmt.query_map([], |row| row.get(0))?;
                rows.filter_map(|r| r.ok()).collect()
            };

            // Read migration files from the directory
            let mut migrations = Vec::new();
            if let Ok(entries) = std::fs::read_dir(self.dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "sql").unwrap_or(false) {
                        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                            // Extract version number from filename (e.g., "0001_threads" → 1)
                            let version: i64 = name
                                .split('_')
                                .next()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                            if version > 0 && !applied.contains(&version) {
                                if let Ok(sql) = std::fs::read_to_string(&path) {
                                    migrations.push((version, name.to_string(), sql));
                                }
                            }
                        }
                    }
                }
            }

            // Sort by version and apply
            migrations.sort_by_key(|(v, _, _)| *v);
            for (version, description, sql) in &migrations {
                conn.execute_batch(sql)?;
                conn.execute(
                    "INSERT INTO _sqlx_migrations (version, description) VALUES (?1, ?2)",
                    rusqlite::params![version, description],
                )?;
            }

            Ok(())
        })
    }
}

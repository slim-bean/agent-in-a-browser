// Migration types — used by the sqlx::migrate! proc macro and Migrator::new.

use crate::sqlite::SqlitePool;
use crate::Error;

/// A single migration embedded at compile time by the `migrate!` macro.
pub struct EmbeddedMigration {
    pub version: i64,
    pub description: &'static str,
    pub sql: &'static str,
}

/// A migrator that runs SQL migrations against a SQLite pool.
pub struct Migrator {
    embedded: Option<&'static [EmbeddedMigration]>,
    dir: Option<&'static str>,
}

impl Migrator {
    /// Create a migrator that reads SQL files from the filesystem at runtime.
    /// Used as fallback when the `migrate!` macro isn't available.
    pub const fn new(dir: &'static str) -> Self {
        Self {
            embedded: None,
            dir: Some(dir),
        }
    }

    /// Create a migrator from compile-time embedded migrations.
    /// Called by the `migrate!` proc macro.
    pub const fn from_embedded(migrations: &'static [EmbeddedMigration]) -> Self {
        Self {
            embedded: Some(migrations),
            dir: None,
        }
    }

    /// Clone a static Migrator. Used by the state crate's `runtime_migrator`
    /// which upstream constructs by copying fields from a static Migrator.
    pub fn clone_static(&'static self) -> Self {
        Self {
            embedded: self.embedded,
            dir: self.dir,
        }
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
                );",
            )?;

            let applied: Vec<i64> = {
                let mut stmt =
                    conn.prepare("SELECT version FROM _sqlx_migrations WHERE success = 1")?;
                let rows = stmt.query_map([], |row| row.get(0))?;
                rows.filter_map(|r| r.ok()).collect()
            };

            // Collect migrations from either embedded or filesystem source
            let migrations: Vec<(i64, String, String)> = if let Some(embedded) = self.embedded {
                // Compile-time embedded migrations
                embedded
                    .iter()
                    .filter(|m| !applied.contains(&m.version))
                    .map(|m| (m.version, m.description.to_string(), m.sql.to_string()))
                    .collect()
            } else if let Some(dir) = self.dir {
                // Runtime filesystem migrations
                Self::read_migration_files(dir, &applied)
            } else {
                Vec::new()
            };

            // Apply in order
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

    fn read_migration_files(dir: &str, applied: &[i64]) -> Vec<(i64, String, String)> {
        let mut migrations = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "sql").unwrap_or(false) {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
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
        migrations.sort_by_key(|(v, _, _)| *v);
        migrations
    }
}

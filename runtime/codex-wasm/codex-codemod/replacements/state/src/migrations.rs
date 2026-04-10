use sqlx::migrate::Migrator;

pub(crate) static STATE_MIGRATOR: Migrator = sqlx::migrate!("./migrations");
pub(crate) static LOGS_MIGRATOR: Migrator = sqlx::migrate!("./logs_migrations");

/// In the wasi-sqlx shim, Migrator doesn't have fields like `ignore_missing`,
/// `locking`, `no_tx`, etc. We simply return a copy of the base migrator.
fn runtime_migrator(base: &'static Migrator) -> Migrator {
    base.clone_static()
}

pub(crate) fn runtime_state_migrator() -> Migrator {
    runtime_migrator(&STATE_MIGRATOR)
}

pub(crate) fn runtime_logs_migrator() -> Migrator {
    runtime_migrator(&LOGS_MIGRATOR)
}

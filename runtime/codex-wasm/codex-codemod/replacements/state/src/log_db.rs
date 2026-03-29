#![allow(unused_variables, unused_mut, unused_imports, dead_code, clippy::all)]

//! Stub replacement for log_db that removes the tracing-subscriber Layer
//! and sqlx dependency. The `start` function returns a no-op layer and
//! `flush` is a no-op.

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use tracing::span::{Attributes, Id, Record};
use tracing::Event;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::StateRuntime;

/// No-op tracing layer returned by [`start`].
pub struct LogDbLayer {
    _phantom: (),
}

impl Clone for LogDbLayer {
    fn clone(&self) -> Self {
        Self { _phantom: () }
    }
}

/// Create a no-op log database layer.
pub fn start(state_db: Arc<StateRuntime>) -> LogDbLayer {
    LogDbLayer { _phantom: () }
}

impl LogDbLayer {
    /// No-op flush.
    pub async fn flush(&self) {}
}

impl<S> Layer<S> for LogDbLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, _attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {}

    fn on_record(&self, _id: &Id, _values: &Record<'_>, _ctx: Context<'_, S>) {}

    fn on_event(&self, _event: &Event<'_>, _ctx: Context<'_, S>) {}
}

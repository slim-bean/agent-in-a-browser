#![allow(dead_code, unused_variables, unused_imports)]
//! Stub for codex-feedback in wasip2 — no-op feedback collection.

use std::path::PathBuf;

/// Main feedback collector — no-op in WASM.
#[derive(Clone, Debug, Default)]
pub struct CodexFeedback;

impl CodexFeedback {
    pub fn new() -> Self {
        Self
    }

    pub fn logger_layer(&self) -> Option<NoopLayer> {
        None
    }

    pub fn metadata_layer(&self) -> Option<NoopLayer> {
        None
    }

    pub fn snapshot<T>(&self, _thread: Option<T>) -> FeedbackSnapshot {
        FeedbackSnapshot::default()
    }

    pub fn snapshot_with_diagnostics(
        &self,
        _diagnostics: &feedback_diagnostics::FeedbackDiagnostics,
    ) -> FeedbackSnapshot {
        FeedbackSnapshot::default()
    }
}

/// NoopLayer for tracing subscriber compatibility.
pub struct NoopLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for NoopLayer {}

/// Feedback snapshot — no-op in WASM.
#[derive(Clone, Debug, Default)]
pub struct FeedbackSnapshot {
    pub id: String,
    pub path: Option<PathBuf>,
    pub thread_id: Option<String>,
}

impl FeedbackSnapshot {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn upload_feedback<A, B, C, D, E, F>(&self, _a: A, _b: B, _c: C, _d: D, _e: E, _f: F) -> Result<(), std::io::Error> {
        Err(std::io::Error::other("feedback upload not available in WASM"))
    }

    pub fn feedback_diagnostics(&self) -> feedback_diagnostics::FeedbackDiagnostics {
        feedback_diagnostics::FeedbackDiagnostics::default()
    }
}

/// Feedback diagnostics module.
pub mod feedback_diagnostics {
    pub const FEEDBACK_DIAGNOSTICS_ATTACHMENT_FILENAME: &str = "diagnostics.json";

    #[derive(Clone, Debug, Default)]
    pub struct FeedbackDiagnostics {
        pub diagnostics: Vec<FeedbackDiagnostic>,
    }

    impl FeedbackDiagnostics {
        pub fn new(diagnostics: Vec<FeedbackDiagnostic>) -> Self {
            Self { diagnostics }
        }

        pub fn is_empty(&self) -> bool {
            self.diagnostics.is_empty()
        }

        pub fn len(&self) -> usize {
            self.diagnostics.len()
        }

        pub fn diagnostics(&self) -> &[FeedbackDiagnostic] {
            &self.diagnostics
        }
    }

    #[derive(Clone, Debug, Default)]
    pub struct FeedbackDiagnostic {
        pub key: String,
        pub value: String,
        pub headline: String,
        pub details: Vec<String>,
    }
}

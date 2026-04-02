//! Thin wrapper re-exporting rquickjs and futures-lite for the code-mode runtime.
//!
//! This crate exists so that `code-mode` can depend on rquickjs via a simple
//! path dependency in INJECT_DEPS, avoiding the need for git dependency
//! injection in the codemod's cargo_toml.rs.

pub use futures_lite;
pub use rquickjs;

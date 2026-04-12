//! Legacy placeholder for global transforms.
//!
//! Workspace-aware AST edits have moved to the semantic subsystem and are
//! invoked directly by the transform engine.

use crate::transform::Transform;

pub fn transforms() -> Vec<Transform> {
    Vec::new()
}

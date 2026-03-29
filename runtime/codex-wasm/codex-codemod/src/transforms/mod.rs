//! All codemod transforms, organized by category.
//!
//! This module replaces the monolithic `ast_transforms.rs` with a structured
//! system that routes through `engine.rs` + `transform.rs`.

mod globals;
mod stubs;
mod string_replacements;

use crate::transform::Transform;

/// Build the complete list of transforms to apply.
pub fn all_transforms() -> Vec<Transform> {
    let mut t = Vec::new();
    t.extend(stubs::transforms());
    t.extend(string_replacements::transforms());
    t.extend(globals::transforms());
    t
}

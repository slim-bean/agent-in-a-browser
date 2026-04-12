//! All nonsemantic codemod transforms, organized by category.
//!
//! Workspace-aware AST edits now run through the semantic subsystem directly
//! from the transform engine rather than pretending to be ordinary per-file
//! `Transform::Global` entries.

mod string_replacements;
mod stubs;
mod text_patches;

use crate::transform::Transform;

/// Build the complete list of transforms to apply.
pub fn all_transforms() -> Vec<Transform> {
    let mut t = Vec::new();
    t.extend(stubs::transforms());
    t.extend(text_patches::transforms());
    t.extend(string_replacements::transforms());
    t
}

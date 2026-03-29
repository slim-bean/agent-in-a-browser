//! Global transforms applied to every .rs file.
//!
//! These wrap syn_transforms which provides AST-aware span-based editing.

use crate::transform::Transform;

pub fn transforms() -> Vec<Transform> {
    vec![Transform::Global {
        name: "syn_transforms",
        apply: |content, path| {
            crate::syn_transforms::apply_with_path(content, path)
                .unwrap_or_else(|| content.to_string())
        },
    }]
}

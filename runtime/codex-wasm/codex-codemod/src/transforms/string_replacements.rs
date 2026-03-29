//! String replacement transforms — remaining entries not yet in syn.
//!
//! These are targeted to be migrated to syn_transforms.rs.
//! Once all are migrated, this file can be deleted.

use crate::transform::Transform;

pub fn transforms() -> Vec<Transform> {
    // All string replacements have been migrated to syn_transforms.rs
    // which runs as a Transform::Global in globals.rs.
    Vec::new()
}

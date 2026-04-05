#![allow(mismatched_lifetime_syntaxes)]
//! wasip2-compatible shim for path-absolutize.
//!
//! Provides the `Absolutize` trait for `Path` and `PathBuf` without
//! relying on `std::fs::canonicalize` (which may not be available or
//! desirable in WASM sandboxed environments).

use std::borrow::Cow;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Trait for converting paths to absolute form.
pub trait Absolutize {
    /// Make the path absolute relative to the current working directory.
    fn absolutize(&self) -> io::Result<Cow<Path>>;

    /// Make the path absolute relative to the given base directory.
    fn absolutize_from(&self, base: &Path) -> io::Result<Cow<Path>>;
}

impl Absolutize for Path {
    fn absolutize(&self) -> io::Result<Cow<Path>> {
        if self.is_absolute() {
            Ok(Cow::Owned(normalize(self)))
        } else {
            let cwd = std::env::current_dir()?;
            Ok(Cow::Owned(normalize(&cwd.join(self))))
        }
    }

    fn absolutize_from(&self, base: &Path) -> io::Result<Cow<Path>> {
        if self.is_absolute() {
            Ok(Cow::Owned(normalize(self)))
        } else {
            Ok(Cow::Owned(normalize(&base.join(self))))
        }
    }
}

impl Absolutize for PathBuf {
    fn absolutize(&self) -> io::Result<Cow<Path>> {
        self.as_path().absolutize()
    }

    fn absolutize_from(&self, base: &Path) -> io::Result<Cow<Path>> {
        self.as_path().absolutize_from(base)
    }
}

/// Normalize a path by resolving `.` and `..` components without touching the
/// filesystem. This is a pure lexical operation (no symlink resolution).
fn normalize(path: &Path) -> PathBuf {
    let mut stack: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = stack.last() {
                    stack.pop();
                } else {
                    stack.push(component);
                }
            }
            _ => stack.push(component),
        }
    }
    stack.iter().collect()
}

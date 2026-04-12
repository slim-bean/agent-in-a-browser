use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::rules::{self, RuleConfig};
use super::workspace::SemanticWorkspace;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, contents).expect("write file");
}

fn apply_workspace(root: &Path) {
    let workspace = SemanticWorkspace::load(root).expect("load semantic workspace");
    let diagnostics = rules::apply_all(
        &workspace,
        &HashSet::<PathBuf>::new(),
        &RuleConfig {
            diag_traces: false,
            strict: false,
        },
    )
    .expect("apply semantic rules");
    assert!(
        !diagnostics.rules.is_empty(),
        "expected at least one semantic rule to run"
    );
}

#[test]
fn semantic_pass_rewrites_thread_spawn() {
    let dir = TempDir::new().expect("tempdir");
    write(
        &dir.path().join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
    );
    write(
        &dir.path().join("src/lib.rs"),
        r#"pub fn demo() {
    std::thread::spawn(|| {
        println!("hello");
    });
}
"#,
    );

    apply_workspace(dir.path());

    let updated = fs::read_to_string(dir.path().join("src/lib.rs")).expect("read updated file");
    assert!(
        updated.contains("tokio::thread_spawn::spawn"),
        "thread spawn should be rewritten: {updated}"
    );
}

#[test]
fn semantic_pass_rewrites_process_exit() {
    let dir = TempDir::new().expect("tempdir");
    write(
        &dir.path().join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
    );
    write(
        &dir.path().join("src/lib.rs"),
        r#"pub fn demo() {
    std::process::exit(7);
}
"#,
    );

    apply_workspace(dir.path());

    let updated = fs::read_to_string(dir.path().join("src/lib.rs")).expect("read updated file");
    assert!(
        updated.contains("cannot exit in WASM"),
        "process::exit should be rewritten: {updated}"
    );
}

#[test]
fn semantic_pass_rewrites_installation_id_lock_site() {
    let dir = TempDir::new().expect("tempdir");
    write(
        &dir.path().join("Cargo.toml"),
        r#"[workspace]
members = ["core"]
"#,
    );
    write(
        &dir.path().join("core/Cargo.toml"),
        r#"[package]
name = "core"
version = "0.1.0"
edition = "2021"
"#,
    );
    write(
        &dir.path().join("core/src/lib.rs"),
        "mod installation_id;\n",
    );
    write(
        &dir.path().join("core/src/installation_id.rs"),
        r#"use std::fs::OpenOptions;
use std::path::Path;

pub fn resolve_installation_id(codex_home: &Path) -> std::io::Result<String> {
    let path = codex_home.join("installation_id");
    let mut file = OpenOptions::new().read(true).write(true).create(true).open(&path)?;
    file.lock()?;
    Ok(String::new())
}
"#,
    );

    apply_workspace(dir.path());

    let updated = fs::read_to_string(dir.path().join("core/src/installation_id.rs"))
        .expect("read updated file");
    assert!(
        updated.contains("#[cfg(not(target_arch = \"wasm32\"))]"),
        "installation_id lock should be cfg-gated: {updated}"
    );
}

#[test]
fn semantic_pass_rewrites_arg0_lock_sites_idempotently() {
    let dir = TempDir::new().expect("tempdir");
    write(
        &dir.path().join("Cargo.toml"),
        r#"[workspace]
members = ["arg0"]
"#,
    );
    write(
        &dir.path().join("arg0/Cargo.toml"),
        r#"[package]
name = "arg0"
version = "0.1.0"
edition = "2021"
"#,
    );
    write(
        &dir.path().join("arg0/src/lib.rs"),
        r#"use std::fs::File;
use std::path::Path;

fn direct(lock_file: &File) -> std::io::Result<()> {
    lock_file.try_lock()?;
    Ok(())
}

fn try_lock_dir(dir: &Path) -> std::io::Result<Option<File>> {
    let lock_file = File::options().read(true).write(true).open(dir.join("x"))?;

    match lock_file.try_lock() {
        Ok(()) => Ok(Some(lock_file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(err) => Err(err.into()),
    }
}
"#,
    );

    apply_workspace(dir.path());
    apply_workspace(dir.path());

    let updated = fs::read_to_string(dir.path().join("arg0/src/lib.rs")).expect("read file");
    assert_eq!(
        updated
            .matches("#[cfg(not(target_arch = \"wasm32\"))]")
            .count(),
        2,
        "expected one guard for the direct call and one for the match path: {updated}"
    );
    assert!(
        updated.contains("#[cfg(target_arch = \"wasm32\")]"),
        "arg0 match path should have a wasm fast path: {updated}"
    );
}

#[test]
fn semantic_pass_rewrites_execpolicy_lock_idempotently() {
    let dir = TempDir::new().expect("tempdir");
    write(
        &dir.path().join("Cargo.toml"),
        r#"[workspace]
members = ["execpolicy"]
"#,
    );
    write(
        &dir.path().join("execpolicy/Cargo.toml"),
        r#"[package]
name = "execpolicy"
version = "0.1.0"
edition = "2021"
"#,
    );
    write(&dir.path().join("execpolicy/src/lib.rs"), "mod amend;\n");
    write(
        &dir.path().join("execpolicy/src/amend.rs"),
        r#"use std::fs::OpenOptions;
use std::path::Path;

pub fn append_locked_line(policy_path: &Path) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).read(true).append(true).open(policy_path)?;
    file.lock().map_err(std::io::Error::from)?;
    Ok(())
}
"#,
    );

    apply_workspace(dir.path());
    apply_workspace(dir.path());

    let updated =
        fs::read_to_string(dir.path().join("execpolicy/src/amend.rs")).expect("read file");
    assert_eq!(
        updated
            .matches("#[cfg(not(target_arch = \"wasm32\"))]")
            .count(),
        1,
        "execpolicy cfg guard should not duplicate: {updated}"
    );
}

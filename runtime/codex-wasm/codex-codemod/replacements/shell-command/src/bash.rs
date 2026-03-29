#![allow(unused)]
//! Stub replacement for `shell-command/src/bash.rs` that removes the
//! tree-sitter dependency. All parsing functions return `None` so the WASM
//! build compiles without a C toolchain.

use std::path::PathBuf;

use crate::shell_detect::ShellType;
use crate::shell_detect::detect_shell_type;

/// Opaque stand-in for `tree_sitter::Tree` (not used in this stub).
pub struct Tree;

/// Stub: always returns `None`.
pub fn try_parse_shell(_shell_lc_arg: &str) -> Option<Tree> {
    None
}

/// Stub: always returns `None`.
pub fn try_parse_word_only_commands_sequence(_tree: &Tree, _src: &str) -> Option<Vec<Vec<String>>> {
    None
}

/// Extract the shell name and script from a `bash -lc "..."` style command.
/// This function does not depend on tree-sitter and is kept intact.
pub fn extract_bash_command(command: &[String]) -> Option<(&str, &str)> {
    let [shell, flag, script] = command else {
        return None;
    };
    if !matches!(flag.as_str(), "-lc" | "-c")
        || !matches!(
            detect_shell_type(&PathBuf::from(shell)),
            Some(ShellType::Zsh) | Some(ShellType::Bash) | Some(ShellType::Sh)
        )
    {
        return None;
    }
    Some((shell, script))
}

/// Stub: always returns `None`.
pub fn parse_shell_lc_plain_commands(_command: &[String]) -> Option<Vec<Vec<String>>> {
    None
}

/// Stub: always returns `None`.
pub fn parse_shell_lc_single_command_prefix(_command: &[String]) -> Option<Vec<String>> {
    None
}

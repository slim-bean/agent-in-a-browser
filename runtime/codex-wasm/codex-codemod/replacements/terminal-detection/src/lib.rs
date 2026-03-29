//! Stub — terminal-detection for wasip2.
#![allow(dead_code, unused_variables)]

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalInfo {
    pub name: TerminalName,
    pub term_program: Option<String>,
    pub version: Option<String>,
    pub term: Option<String>,
    pub multiplexer: Option<Multiplexer>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalName {
    AppleTerminal, Ghostty, ITerm2, Iterm2, Kitty, Tmux, Alacritty, WezTerm,
    WindowsTerminal, Vscode, VsCode, Cursor, Windsurf, WarpTerminal,
    Konsole, GnomeTerminal, Vte, Dumb, Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Multiplexer {
    pub name: MultiplexerName,
    pub version: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MultiplexerName { Tmux, Screen, Zellij, Unknown }

pub fn user_agent() -> String { "codex-wasm/0.0.0".to_string() }

pub fn terminal_info() -> TerminalInfo {
    TerminalInfo {
        name: TerminalName::Unknown,
        term_program: None,
        version: None,
        term: None,
        multiplexer: None,
    }
}

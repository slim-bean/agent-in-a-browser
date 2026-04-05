//! Terminal event handling matching crossterm::event.

use std::time::Duration;

/// Key event matching crossterm::event::KeyEvent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
    pub state: KeyEventState,
}

impl KeyEvent {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }
}

/// Key code matching crossterm::event::KeyCode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    Null,
    Esc,
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    Menu,
    KeypadBegin,
    Char(char),
    F(u8),
}

impl std::fmt::Display for KeyCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyCode::Backspace => write!(f, "backspace"),
            KeyCode::Enter => write!(f, "enter"),
            KeyCode::Left => write!(f, "left"),
            KeyCode::Right => write!(f, "right"),
            KeyCode::Up => write!(f, "up"),
            KeyCode::Down => write!(f, "down"),
            KeyCode::Home => write!(f, "home"),
            KeyCode::End => write!(f, "end"),
            KeyCode::PageUp => write!(f, "pageup"),
            KeyCode::PageDown => write!(f, "pagedown"),
            KeyCode::Tab => write!(f, "tab"),
            KeyCode::BackTab => write!(f, "backtab"),
            KeyCode::Delete => write!(f, "delete"),
            KeyCode::Insert => write!(f, "insert"),
            KeyCode::Null => write!(f, "null"),
            KeyCode::Esc => write!(f, "esc"),
            KeyCode::CapsLock => write!(f, "capslock"),
            KeyCode::ScrollLock => write!(f, "scrolllock"),
            KeyCode::NumLock => write!(f, "numlock"),
            KeyCode::PrintScreen => write!(f, "printscreen"),
            KeyCode::Pause => write!(f, "pause"),
            KeyCode::Menu => write!(f, "menu"),
            KeyCode::KeypadBegin => write!(f, "keypadbegin"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::F(n) => write!(f, "F{n}"),
        }
    }
}

/// Key modifiers matching crossterm::event::KeyModifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub const ALT: Self = Self(4);
    pub const SUPER: Self = Self(8);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl std::ops::BitOr for KeyModifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Key event kind matching crossterm::event::KeyEventKind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

/// Key event state matching crossterm::event::KeyEventState.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEventState(u8);

impl KeyEventState {
    pub const fn empty() -> Self {
        Self(0)
    }
}

/// Mouse event matching crossterm::event::MouseEvent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Terminal event matching crossterm::event::Event.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize(u16, u16),
    FocusGained,
    FocusLost,
}

/// Poll for a terminal event by checking if stdin has available data.
/// In WASM, stdin is backed by the ghostty-cli-shim which buffers
/// terminal input from ghostty-web's onData callback.
pub fn poll(_timeout: Duration) -> std::io::Result<bool> {
    // In WASM, we can't do a true non-blocking poll of stdin.
    // The ghostty-cli-shim delivers data via blocking_read.
    // Return true to indicate we should try reading — the read()
    // call will block (via JSPI) until data is actually available.
    Ok(true)
}

/// Bracketed paste start/end markers.
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// Read the next terminal event (blocking).
/// Reads raw bytes from WASI stdin and parses ANSI escape sequences
/// into crossterm Event values. The ghostty-cli-shim delivers
/// keystrokes from ghostty-web as raw terminal bytes.
///
/// Also checks for terminal size changes via the registered host
/// size query, generating Event::Resize when the size differs from
/// the cached value.
pub fn read() -> std::io::Result<Event> {
    use std::io::Read;

    // Check for terminal size changes before reading stdin.
    // The host updates its cached size when the browser window resizes;
    // we detect the change here without stdin-injected escape sequences.
    if let Some((host_cols, host_rows)) = crate::terminal::query_host_size() {
        let (cached_cols, cached_rows) = (
            crate::terminal::cached_cols(),
            crate::terminal::cached_rows(),
        );
        if host_cols != cached_cols || host_rows != cached_rows {
            crate::terminal::update_size(host_cols, host_rows);
            return Ok(Event::Resize(host_cols, host_rows));
        }
    }

    let mut buf = [0u8; 4096];
    let n = std::io::stdin().read(&mut buf)?;
    if n == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "no data available",
        ));
    }

    let bytes = &buf[..n];

    // Check for bracketed paste start: ESC[200~
    if bytes.starts_with(PASTE_START) {
        return read_bracketed_paste(&bytes[PASTE_START.len()..]);
    }

    Ok(parse_ansi_event(bytes))
}

/// Read a bracketed paste sequence. `initial` contains bytes after ESC[200~.
/// Keeps reading until ESC[201~ is found, then returns Event::Paste.
fn read_bracketed_paste(initial: &[u8]) -> std::io::Result<Event> {
    use std::io::Read;

    let mut paste_buf = Vec::with_capacity(256);

    // Check if the end marker is already in the initial data
    if let Some(pos) = find_subsequence(initial, PASTE_END) {
        paste_buf.extend_from_slice(&initial[..pos]);
    } else {
        paste_buf.extend_from_slice(initial);

        // Keep reading until we find ESC[201~
        let mut read_buf = [0u8; 4096];
        loop {
            let n = std::io::stdin().read(&mut read_buf)?;
            if n == 0 {
                break; // EOF — return what we have
            }
            let chunk = &read_buf[..n];
            if let Some(pos) = find_subsequence(chunk, PASTE_END) {
                paste_buf.extend_from_slice(&chunk[..pos]);
                break;
            }
            paste_buf.extend_from_slice(chunk);
        }
    }

    let text = String::from_utf8_lossy(&paste_buf).into_owned();
    Ok(Event::Paste(text))
}

/// Find the position of a subsequence in a byte slice.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Parse raw terminal bytes into a crossterm Event.
/// Handles common ANSI escape sequences from xterm-compatible terminals.
fn parse_ansi_event(bytes: &[u8]) -> Event {
    match bytes {
        // === Escape sequences (CSI) ===
        [0x1b, b'[', b'A', ..] => Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        [0x1b, b'[', b'B', ..] => Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        [0x1b, b'[', b'C', ..] => Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        [0x1b, b'[', b'D', ..] => Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
        [0x1b, b'[', b'H', ..] => Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
        [0x1b, b'[', b'F', ..] => Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),

        // Arrow keys with shift
        [0x1b, b'[', b'1', b';', b'2', b'A', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT))
        }
        [0x1b, b'[', b'1', b';', b'2', b'B', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
        }
        [0x1b, b'[', b'1', b';', b'2', b'C', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
        }
        [0x1b, b'[', b'1', b';', b'2', b'D', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT))
        }

        // Arrow keys with alt
        [0x1b, b'[', b'1', b';', b'3', b'A', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT))
        }
        [0x1b, b'[', b'1', b';', b'3', b'B', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT))
        }
        [0x1b, b'[', b'1', b';', b'3', b'C', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT))
        }
        [0x1b, b'[', b'1', b';', b'3', b'D', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
        }

        // Arrow keys with ctrl
        [0x1b, b'[', b'1', b';', b'5', b'A', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL))
        }
        [0x1b, b'[', b'1', b';', b'5', b'B', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL))
        }
        [0x1b, b'[', b'1', b';', b'5', b'C', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL))
        }
        [0x1b, b'[', b'1', b';', b'5', b'D', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL))
        }

        // Insert/Delete/PageUp/PageDown
        [0x1b, b'[', b'2', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Insert, KeyModifiers::NONE))
        }
        [0x1b, b'[', b'3', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        }
        [0x1b, b'[', b'5', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
        }
        [0x1b, b'[', b'6', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
        }

        // Function keys F1-F4
        [0x1b, b'O', b'P', ..] => Event::Key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)),
        [0x1b, b'O', b'Q', ..] => Event::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)),
        [0x1b, b'O', b'R', ..] => Event::Key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE)),
        [0x1b, b'O', b'S', ..] => Event::Key(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)),

        // Function keys F5-F12
        [0x1b, b'[', b'1', b'5', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE))
        }
        [0x1b, b'[', b'1', b'7', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE))
        }
        [0x1b, b'[', b'1', b'8', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE))
        }
        [0x1b, b'[', b'1', b'9', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE))
        }
        [0x1b, b'[', b'2', b'0', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE))
        }
        [0x1b, b'[', b'2', b'1', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE))
        }
        [0x1b, b'[', b'2', b'3', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE))
        }
        [0x1b, b'[', b'2', b'4', b'~', ..] => {
            Event::Key(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE))
        }

        // Resize: CSI 8 ; rows ; cols t (sent by ghostty-cli-shim setTerminalSize)
        [0x1b, b'[', b'8', b';', rest @ ..] => parse_resize_sequence(rest),

        // Bracketed paste start — handled in read() before reaching parse_ansi_event.
        // If we get here, it means partial data; treat as no-op.
        [0x1b, b'[', b'2', b'0', b'0', b'~', ..] => Event::FocusGained,

        // Focus events
        [0x1b, b'[', b'I', ..] => Event::FocusGained,
        [0x1b, b'[', b'O', ..] => Event::FocusLost,

        // Alt+key (ESC followed by a char)
        [0x1b, c, ..] if c.is_ascii_graphic() || *c == b' ' => {
            Event::Key(KeyEvent::new(KeyCode::Char(*c as char), KeyModifiers::ALT))
        }

        // Bare escape
        [0x1b] => Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),

        // === Control characters ===
        [0x00, ..] => Event::Key(KeyEvent::new(KeyCode::Char('@'), KeyModifiers::CONTROL)), // Ctrl+@/Space
        [0x01, ..] => Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        [0x02, ..] => Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
        [0x03, ..] => Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        [0x04, ..] => Event::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        [0x05, ..] => Event::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL)),
        [0x06, ..] => Event::Key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
        [0x07, ..] => Event::Key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)),
        [0x08, ..] => Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)), // Ctrl+H = Backspace
        [0x09, ..] => Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)), // Ctrl+I = Tab
        [0x0a, ..] | [0x0d, ..] => Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), // LF/CR
        [0x0b, ..] => Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
        [0x0c, ..] => Event::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
        [0x0e, ..] => Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
        [0x0f, ..] => Event::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
        [0x10, ..] => Event::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
        [0x11, ..] => Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)),
        [0x12, ..] => Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        [0x13, ..] => Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        [0x14, ..] => Event::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
        [0x15, ..] => Event::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
        [0x16, ..] => Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)),
        [0x17, ..] => Event::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        [0x18, ..] => Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
        [0x19, ..] => Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)),
        [0x1a, ..] => Event::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)),
        [0x7f, ..] => Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)), // DEL

        // === Regular UTF-8 characters ===
        _ => {
            // Try to decode as UTF-8
            if let Ok(s) = std::str::from_utf8(bytes) {
                if let Some(c) = s.chars().next() {
                    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                } else {
                    Event::Key(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE))
                }
            } else {
                Event::Key(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE))
            }
        }
    }
}

/// Parse CSI 8 ; rows ; cols t resize sequence.
/// Input is the bytes after `\x1b[8;` (i.e., `rows;colst`).
fn parse_resize_sequence(rest: &[u8]) -> Event {
    // Find the 't' terminator and parse rows;cols
    if let Ok(s) = std::str::from_utf8(rest) {
        if let Some(t_pos) = s.find('t') {
            let params = &s[..t_pos];
            let parts: Vec<&str> = params.split(';').collect();
            if parts.len() == 2 {
                if let (Ok(rows), Ok(cols)) = (parts[0].parse::<u16>(), parts[1].parse::<u16>()) {
                    // Update the cached terminal size
                    crate::terminal::update_size(cols, rows);
                    return Event::Resize(cols, rows);
                }
            }
        }
    }
    // Fallback: unknown escape sequence
    Event::Key(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE))
}

/// Enable mouse capture.
#[derive(Debug)]
pub struct EnableMouseCapture;

impl super::Command for EnableMouseCapture {
    fn write_ansi(&self, _f: &mut impl std::fmt::Write) -> std::fmt::Result {
        Ok(())
    }
}

/// Disable mouse capture.
#[derive(Debug)]
pub struct DisableMouseCapture;

impl super::Command for DisableMouseCapture {
    fn write_ansi(&self, _f: &mut impl std::fmt::Write) -> std::fmt::Result {
        Ok(())
    }
}

/// Enable bracketed paste.
#[derive(Debug)]
pub struct EnableBracketedPaste;

impl super::Command for EnableBracketedPaste {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?2004h")
    }
}

/// Disable bracketed paste.
#[derive(Debug)]
pub struct DisableBracketedPaste;

impl super::Command for DisableBracketedPaste {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?2004l")
    }
}

/// Enable focus change.
#[derive(Debug)]
pub struct EnableFocusChange;

impl super::Command for EnableFocusChange {
    fn write_ansi(&self, _f: &mut impl std::fmt::Write) -> std::fmt::Result {
        Ok(())
    }
}

/// Disable focus change.
#[derive(Debug)]
pub struct DisableFocusChange;

impl super::Command for DisableFocusChange {
    fn write_ansi(&self, _f: &mut impl std::fmt::Write) -> std::fmt::Result {
        Ok(())
    }
}

/// Keyboard enhancement flags — not available in WASM/browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardEnhancementFlags(u32);

impl KeyboardEnhancementFlags {
    pub const DISAMBIGUATE_ESCAPE_CODES: Self = Self(0b0000_0001);
    pub const REPORT_EVENT_TYPES: Self = Self(0b0000_0010);
    pub const REPORT_ALTERNATE_KEYS: Self = Self(0b0000_0100);
    pub const REPORT_ALL_KEYS_AS_ESCAPE_CODES: Self = Self(0b0000_1000);
}

impl std::ops::BitOr for KeyboardEnhancementFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Push keyboard enhancement flags — no-op in WASM.
#[derive(Debug)]
pub struct PushKeyboardEnhancementFlags(pub KeyboardEnhancementFlags);

impl super::Command for PushKeyboardEnhancementFlags {
    fn write_ansi(&self, _f: &mut impl std::fmt::Write) -> std::fmt::Result {
        Ok(())
    }
}

/// Pop keyboard enhancement flags — no-op in WASM.
#[derive(Debug)]
pub struct PopKeyboardEnhancementFlags;

impl super::Command for PopKeyboardEnhancementFlags {
    fn write_ansi(&self, _f: &mut impl std::fmt::Write) -> std::fmt::Result {
        Ok(())
    }
}

/// Event stream (for event-stream feature).
/// Wraps blocking stdin reads as an async Stream of crossterm Events.
pub struct EventStream;

impl EventStream {
    pub fn new() -> Self {
        Self
    }
}

impl futures_core::Stream for EventStream {
    type Item = std::io::Result<Event>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // In WASM with JSPI, std::io::stdin().read() will suspend the WASM
        // instance via JSPI until data is available from ghostty-web.
        // We call read() which does a blocking stdin read and parses the result.
        match read() {
            Ok(event) => std::task::Poll::Ready(Some(Ok(event))),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::task::Poll::Pending,
            Err(e) => std::task::Poll::Ready(Some(Err(e))),
        }
    }
}

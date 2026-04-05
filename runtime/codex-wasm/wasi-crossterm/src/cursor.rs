//! Cursor control matching crossterm::cursor.

use std::io;
use std::sync::OnceLock;

/// Optional host-side cursor position query function. When registered,
/// `position()` calls this to get the actual cursor coordinates from the
/// terminal emulator instead of returning (0, 0).
static POSITION_QUERY: OnceLock<Box<dyn Fn() -> (u16, u16) + Send + Sync>> = OnceLock::new();

/// Register a function that queries the host for the current cursor position.
/// Called once at startup by the component entry point if the host supports it.
pub fn set_position_query(f: impl Fn() -> (u16, u16) + Send + Sync + 'static) {
    let _ = POSITION_QUERY.set(Box::new(f));
}

#[derive(Debug)]
pub struct MoveTo(pub u16, pub u16);

impl super::Command for MoveTo {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{};{}H", self.1 + 1, self.0 + 1)
    }
}

#[derive(Debug)]
pub struct MoveToColumn(pub u16);

impl super::Command for MoveToColumn {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{}G", self.0 + 1)
    }
}

#[derive(Debug)]
pub struct MoveToRow(pub u16);

impl super::Command for MoveToRow {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{}d", self.0 + 1)
    }
}

#[derive(Debug)]
pub struct MoveUp(pub u16);

impl super::Command for MoveUp {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{}A", self.0)
    }
}

#[derive(Debug)]
pub struct MoveDown(pub u16);

impl super::Command for MoveDown {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{}B", self.0)
    }
}

#[derive(Debug)]
pub struct MoveLeft(pub u16);

impl super::Command for MoveLeft {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{}D", self.0)
    }
}

#[derive(Debug)]
pub struct MoveRight(pub u16);

impl super::Command for MoveRight {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{}C", self.0)
    }
}

#[derive(Debug)]
pub struct Show;

impl super::Command for Show {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?25h")
    }
}

#[derive(Debug)]
pub struct Hide;

impl super::Command for Hide {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?25l")
    }
}

#[derive(Debug)]
pub struct SavePosition;

impl super::Command for SavePosition {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b7")
    }
}

#[derive(Debug)]
pub struct RestorePosition;

impl super::Command for RestorePosition {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b8")
    }
}

/// Set cursor style.
#[derive(Debug, Clone, Copy)]
pub enum SetCursorStyle {
    DefaultUserShape,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderScore,
    SteadyUnderScore,
    BlinkingBar,
    SteadyBar,
}

impl super::Command for SetCursorStyle {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        let n = match self {
            Self::DefaultUserShape => 0,
            Self::BlinkingBlock => 1,
            Self::SteadyBlock => 2,
            Self::BlinkingUnderScore => 3,
            Self::SteadyUnderScore => 4,
            Self::BlinkingBar => 5,
            Self::SteadyBar => 6,
        };
        write!(f, "\x1b[{n} q")
    }
}

/// Get cursor position. Queries the host via the registered position query
/// function if available, otherwise returns (0, 0) as a fallback.
pub fn position() -> io::Result<(u16, u16)> {
    if let Some(query) = POSITION_QUERY.get() {
        Ok(query())
    } else {
        Ok((0, 0))
    }
}

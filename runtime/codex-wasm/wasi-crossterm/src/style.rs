//! Style/color matching crossterm::style.

use std::fmt;

// Import Command trait so its methods are callable within this module (e.g. SetColors).
#[allow(unused_imports)]
use super::Command as _;

/// Color matching crossterm::style::Color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Reset,
    Black,
    DarkGrey,
    Red,
    DarkRed,
    Green,
    DarkGreen,
    Yellow,
    DarkYellow,
    Blue,
    DarkBlue,
    Magenta,
    DarkMagenta,
    Cyan,
    DarkCyan,
    White,
    Grey,
    Rgb { r: u8, g: u8, b: u8 },
    AnsiValue(u8),
}

/// Attribute matching crossterm::style::Attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Attribute {
    Reset,
    Bold,
    Dim,
    Italic,
    Underlined,
    DoubleUnderlined,
    Undercurled,
    Underdotted,
    Underdashed,
    SlowBlink,
    RapidBlink,
    Reverse,
    Hidden,
    CrossedOut,
    NormalIntensity,
    NoBold,
    NoItalic,
    NoUnderline,
    NoBlink,
    NoReverse,
    NoHidden,
    NotCrossedOut,
}

/// SGR parameter for each attribute.
impl Attribute {
    pub fn sgr(self) -> &'static str {
        match self {
            Attribute::Reset => "0",
            Attribute::Bold => "1",
            Attribute::Dim => "2",
            Attribute::Italic => "3",
            Attribute::Underlined => "4",
            Attribute::DoubleUnderlined => "4:2",
            Attribute::Undercurled => "4:3",
            Attribute::Underdotted => "4:4",
            Attribute::Underdashed => "4:5",
            Attribute::SlowBlink => "5",
            Attribute::RapidBlink => "6",
            Attribute::Reverse => "7",
            Attribute::Hidden => "8",
            Attribute::CrossedOut => "9",
            Attribute::NormalIntensity => "22",
            Attribute::NoBold => "22",
            Attribute::NoItalic => "23",
            Attribute::NoUnderline => "24",
            Attribute::NoBlink => "25",
            Attribute::NoReverse => "27",
            Attribute::NoHidden => "28",
            Attribute::NotCrossedOut => "29",
        }
    }
}

/// A set of `Attribute`s — mirrors crossterm's bitflag-based `Attributes`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Attributes(pub u32);

impl Attributes {
    pub const fn new() -> Self {
        Attributes(0)
    }

    pub fn set(&mut self, attr: Attribute) {
        self.0 |= 1 << (attr as u32);
    }

    pub fn unset(&mut self, attr: Attribute) {
        self.0 &= !(1 << (attr as u32));
    }

    pub fn has(self, attr: Attribute) -> bool {
        self.0 & (1 << (attr as u32)) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl From<Attribute> for Attributes {
    fn from(attr: Attribute) -> Self {
        let mut a = Attributes::new();
        a.set(attr);
        a
    }
}

impl std::ops::BitOr<Attribute> for Attributes {
    type Output = Self;
    fn bitor(mut self, rhs: Attribute) -> Self {
        self.set(rhs);
        self
    }
}

impl std::ops::BitOrAssign<Attribute> for Attributes {
    fn bitor_assign(&mut self, rhs: Attribute) {
        self.set(rhs);
    }
}

/// Foreground + background color pair.
#[derive(Debug, Clone, Copy, Default)]
pub struct Colors {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
}

impl Colors {
    pub fn new(foreground: Color, background: Color) -> Self {
        Self {
            foreground: Some(foreground),
            background: Some(background),
        }
    }
}

/// Styled content.
#[derive(Debug)]
pub struct StyledContent<D: std::fmt::Display> {
    style: ContentStyle,
    content: D,
}

impl<D: std::fmt::Display> std::fmt::Display for StyledContent<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Emit attribute SGR codes
        for attr in [
            Attribute::Reset,
            Attribute::Bold,
            Attribute::Dim,
            Attribute::Italic,
            Attribute::Underlined,
            Attribute::DoubleUnderlined,
            Attribute::Undercurled,
            Attribute::Underdotted,
            Attribute::Underdashed,
            Attribute::SlowBlink,
            Attribute::RapidBlink,
            Attribute::Reverse,
            Attribute::Hidden,
            Attribute::CrossedOut,
        ] {
            if self.style.attributes.has(attr) {
                write!(f, "\x1b[{}m", attr.sgr())?;
            }
        }
        // Emit foreground color
        if let Some(fg) = self.style.foreground_color {
            SetForegroundColor(fg).write_ansi(f)?;
        }
        // Emit background color
        if let Some(bg) = self.style.background_color {
            SetBackgroundColor(bg).write_ansi(f)?;
        }
        // Emit underline color
        if let Some(ul) = self.style.underline_color {
            SetUnderlineColor(ul).write_ansi(f)?;
        }
        // Content
        write!(f, "{}", self.content)?;
        // Reset all
        write!(f, "\x1b[0m")
    }
}

/// Content style.
#[derive(Debug, Default, Clone)]
pub struct ContentStyle {
    pub foreground_color: Option<Color>,
    pub background_color: Option<Color>,
    pub underline_color: Option<Color>,
    pub attributes: Attributes,
}

/// Stylize trait for easy styling.
pub trait Stylize {
    fn stylize(self) -> StyledContent<Self>
    where
        Self: std::fmt::Display + Sized,
    {
        StyledContent {
            style: ContentStyle::default(),
            content: self,
        }
    }
}

impl Stylize for &str {}
impl Stylize for String {}

/// Print styled content.
#[derive(Debug)]
pub struct Print<D: std::fmt::Display>(pub D);

impl<D: std::fmt::Display> super::Command for Print<D> {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Set foreground color.
#[derive(Debug)]
pub struct SetForegroundColor(pub Color);

impl super::Command for SetForegroundColor {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        match self.0 {
            Color::Rgb { r, g, b } => write!(f, "\x1b[38;2;{r};{g};{b}m"),
            Color::AnsiValue(v) => write!(f, "\x1b[38;5;{v}m"),
            Color::Reset => write!(f, "\x1b[39m"),
            Color::Black => write!(f, "\x1b[30m"),
            Color::DarkGrey => write!(f, "\x1b[90m"),
            Color::Red => write!(f, "\x1b[31m"),
            Color::DarkRed => write!(f, "\x1b[91m"),
            Color::Green => write!(f, "\x1b[32m"),
            Color::DarkGreen => write!(f, "\x1b[92m"),
            Color::Yellow => write!(f, "\x1b[33m"),
            Color::DarkYellow => write!(f, "\x1b[93m"),
            Color::Blue => write!(f, "\x1b[34m"),
            Color::DarkBlue => write!(f, "\x1b[94m"),
            Color::Magenta => write!(f, "\x1b[35m"),
            Color::DarkMagenta => write!(f, "\x1b[95m"),
            Color::Cyan => write!(f, "\x1b[36m"),
            Color::DarkCyan => write!(f, "\x1b[96m"),
            Color::White => write!(f, "\x1b[37m"),
            Color::Grey => write!(f, "\x1b[97m"),
        }
    }
}

/// Set background color.
#[derive(Debug)]
pub struct SetBackgroundColor(pub Color);

impl super::Command for SetBackgroundColor {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        match self.0 {
            Color::Rgb { r, g, b } => write!(f, "\x1b[48;2;{r};{g};{b}m"),
            Color::AnsiValue(v) => write!(f, "\x1b[48;5;{v}m"),
            Color::Reset => write!(f, "\x1b[49m"),
            Color::Black => write!(f, "\x1b[40m"),
            Color::DarkGrey => write!(f, "\x1b[100m"),
            Color::Red => write!(f, "\x1b[41m"),
            Color::DarkRed => write!(f, "\x1b[101m"),
            Color::Green => write!(f, "\x1b[42m"),
            Color::DarkGreen => write!(f, "\x1b[102m"),
            Color::Yellow => write!(f, "\x1b[43m"),
            Color::DarkYellow => write!(f, "\x1b[103m"),
            Color::Blue => write!(f, "\x1b[44m"),
            Color::DarkBlue => write!(f, "\x1b[104m"),
            Color::Magenta => write!(f, "\x1b[45m"),
            Color::DarkMagenta => write!(f, "\x1b[105m"),
            Color::Cyan => write!(f, "\x1b[46m"),
            Color::DarkCyan => write!(f, "\x1b[106m"),
            Color::White => write!(f, "\x1b[47m"),
            Color::Grey => write!(f, "\x1b[107m"),
        }
    }
}

/// Reset color.
#[derive(Debug)]
pub struct ResetColor;

impl super::Command for ResetColor {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[0m")
    }
}

/// Set underline color.
#[derive(Debug)]
pub struct SetUnderlineColor(pub Color);

impl super::Command for SetUnderlineColor {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        match self.0 {
            // Underline color uses SGR 58 with 256-color index (5;n) since there
            // are no dedicated named-color codes for underline like fg/bg have.
            Color::Rgb { r, g, b } => write!(f, "\x1b[58;2;{r};{g};{b}m"),
            Color::AnsiValue(v) => write!(f, "\x1b[58;5;{v}m"),
            Color::Reset => write!(f, "\x1b[59m"),
            Color::Black => write!(f, "\x1b[58;5;0m"),
            Color::DarkGrey => write!(f, "\x1b[58;5;8m"),
            Color::Red => write!(f, "\x1b[58;5;1m"),
            Color::DarkRed => write!(f, "\x1b[58;5;9m"),
            Color::Green => write!(f, "\x1b[58;5;2m"),
            Color::DarkGreen => write!(f, "\x1b[58;5;10m"),
            Color::Yellow => write!(f, "\x1b[58;5;3m"),
            Color::DarkYellow => write!(f, "\x1b[58;5;11m"),
            Color::Blue => write!(f, "\x1b[58;5;4m"),
            Color::DarkBlue => write!(f, "\x1b[58;5;12m"),
            Color::Magenta => write!(f, "\x1b[58;5;5m"),
            Color::DarkMagenta => write!(f, "\x1b[58;5;13m"),
            Color::Cyan => write!(f, "\x1b[58;5;6m"),
            Color::DarkCyan => write!(f, "\x1b[58;5;14m"),
            Color::White => write!(f, "\x1b[58;5;7m"),
            Color::Grey => write!(f, "\x1b[58;5;15m"),
        }
    }
}

/// Set both foreground and background colors at once.
#[derive(Debug)]
pub struct SetColors(pub Colors);

impl super::Command for SetColors {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        if let Some(fg) = self.0.foreground {
            SetForegroundColor(fg).write_ansi(f)?;
        }
        if let Some(bg) = self.0.background {
            SetBackgroundColor(bg).write_ansi(f)?;
        }
        Ok(())
    }
}

/// Set attribute.
#[derive(Debug)]
pub struct SetAttribute(pub Attribute);

impl super::Command for SetAttribute {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{}m", self.0.sgr())
    }
}

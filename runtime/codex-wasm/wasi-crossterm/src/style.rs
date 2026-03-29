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
        // TODO: Write ANSI codes for style, then content, then reset
        write!(f, "{}", self.content)
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
            _ => Ok(()), // TODO: map named colors to ANSI codes
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
            _ => Ok(()),
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
            Color::Rgb { r, g, b } => write!(f, "\x1b[58;2;{r};{g};{b}m"),
            Color::AnsiValue(v) => write!(f, "\x1b[58;5;{v}m"),
            Color::Reset => write!(f, "\x1b[59m"),
            _ => Ok(()),
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

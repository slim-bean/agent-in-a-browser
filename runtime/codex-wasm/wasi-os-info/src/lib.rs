//! WASM shim for the `os_info` crate.
//!
//! Returns wasm32/wasip2 platform info for User-Agent strings.

use std::fmt;

pub struct Info;

#[derive(Debug, Clone, Copy)]
pub enum Type {
    Unknown,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WASI")
    }
}

#[derive(Debug, Clone)]
pub struct Version(String);

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn get() -> Info {
    Info
}

impl Info {
    pub fn os_type(&self) -> Type {
        Type::Unknown
    }

    pub fn version(&self) -> Version {
        Version("preview2".to_string())
    }

    pub fn architecture(&self) -> Option<&str> {
        Some("wasm32")
    }
}

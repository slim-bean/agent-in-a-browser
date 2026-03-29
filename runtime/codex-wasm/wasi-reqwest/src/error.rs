//! Error type matching reqwest::Error.

#[derive(Debug)]
pub struct Error {
    message: String,
}

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn is_timeout(&self) -> bool {
        false
    }

    pub fn is_connect(&self) -> bool {
        false
    }

    pub fn is_request(&self) -> bool {
        false
    }

    pub fn url(&self) -> Option<&url::Url> {
        None
    }

    pub fn url_mut(&mut self) -> Option<&mut url::Url> {
        None
    }

    pub fn is_status(&self) -> bool {
        self.message.starts_with("HTTP ")
    }

    pub fn status(&self) -> Option<http::StatusCode> {
        if self.message.starts_with("HTTP ") {
            self.message[5..]
                .parse::<u16>()
                .ok()
                .and_then(|code| http::StatusCode::from_u16(code).ok())
        } else {
            None
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

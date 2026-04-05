#![allow(dead_code, unused_variables)]
//! wasi-reqwest: A reqwest-compatible API shim for wasip2 environments.
//!
//! Provides the subset of reqwest's API that Codex uses, backed by
//! wasi:http/outgoing-handler instead of hyper/rustls.

use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

pub use bytes::Bytes;
pub use futures_core::Stream;
pub use http::header;
pub use http::StatusCode;
pub use http::{Method, Version};
pub use url::Url;

mod error;
pub use error::Error;

pub mod backend;
pub use backend::set_backend;

pub type Result<T> = std::result::Result<T, Error>;

// Re-export Duration so `reqwest::Duration` works (some codex code uses it)
pub use std::time::Duration;

/// Certificate type (stub for WASM — TLS handled by host).
#[derive(Debug, Clone)]
pub struct Certificate;

impl Certificate {
    pub fn from_pem(_pem: &[u8]) -> Result<Self> {
        Ok(Certificate)
    }
    pub fn from_der(_der: &[u8]) -> Result<Self> {
        Ok(Certificate)
    }
}

/// Request body type.
#[derive(Debug)]
pub struct Body(Vec<u8>);

impl From<Vec<u8>> for Body {
    fn from(v: Vec<u8>) -> Self {
        Body(v)
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        Body(s.into_bytes())
    }
}

impl From<&str> for Body {
    fn from(s: &str) -> Self {
        Body(s.as_bytes().to_vec())
    }
}

impl From<Bytes> for Body {
    fn from(b: Bytes) -> Self {
        Body(b.to_vec())
    }
}

/// HTTP client matching reqwest::Client.
#[derive(Clone, Debug)]
pub struct Client {
    default_headers: HashMap<String, String>,
    timeout: Option<std::time::Duration>,
    redirect_policy: redirect::Policy,
}

impl Client {
    pub fn new() -> Self {
        Self {
            default_headers: HashMap::new(),
            timeout: None,
            redirect_policy: redirect::Policy::default(),
        }
    }

    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    pub fn get(&self, url: impl IntoUrl) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    pub fn post(&self, url: impl IntoUrl) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    pub fn put(&self, url: impl IntoUrl) -> RequestBuilder {
        self.request(Method::PUT, url)
    }

    pub fn delete(&self, url: impl IntoUrl) -> RequestBuilder {
        self.request(Method::DELETE, url)
    }

    pub fn patch(&self, url: impl IntoUrl) -> RequestBuilder {
        self.request(Method::PATCH, url)
    }

    pub fn head(&self, url: impl IntoUrl) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }

    pub fn request(&self, method: Method, url: impl IntoUrl) -> RequestBuilder {
        RequestBuilder {
            method,
            url: url.into_url().ok(),
            headers: self.default_headers.clone(),
            body: None,
            timeout: self.timeout,
            redirect_policy: self.redirect_policy.clone(),
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

/// Client builder matching reqwest::ClientBuilder.
pub struct ClientBuilder {
    default_headers: HashMap<String, String>,
    timeout: Option<std::time::Duration>,
    redirect_policy: redirect::Policy,
}

impl ClientBuilder {
    pub fn new() -> Self {
        Self {
            default_headers: HashMap::new(),
            timeout: None,
            redirect_policy: redirect::Policy::default(),
        }
    }

    pub fn default_headers(mut self, headers: header::HeaderMap) -> Self {
        for (key, value) in headers.iter() {
            if let Ok(v) = value.to_str() {
                self.default_headers.insert(key.to_string(), v.to_string());
            }
        }
        self
    }

    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn connect_timeout(self, _timeout: std::time::Duration) -> Self {
        self
    }

    pub fn pool_idle_timeout(self, _timeout: std::time::Duration) -> Self {
        self
    }

    pub fn pool_max_idle_per_host(self, _max: usize) -> Self {
        self
    }

    pub fn danger_accept_invalid_certs(self, _accept: bool) -> Self {
        self
    }

    pub fn no_proxy(self) -> Self {
        self
    }

    pub fn add_root_certificate(self, _cert: Certificate) -> Self {
        self // TLS handled by host
    }

    pub fn redirect(mut self, policy: redirect::Policy) -> Self {
        self.redirect_policy = policy;
        self
    }

    pub fn user_agent(mut self, value: impl AsRef<str>) -> Self {
        self.default_headers
            .insert("user-agent".to_string(), value.as_ref().to_string());
        self
    }

    pub fn build(self) -> Result<Client> {
        Ok(Client {
            default_headers: self.default_headers,
            timeout: self.timeout,
            redirect_policy: self.redirect_policy,
        })
    }
}

/// Request builder matching reqwest::RequestBuilder.
#[derive(Debug)]
pub struct RequestBuilder {
    method: Method,
    url: Option<Url>,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    timeout: Option<std::time::Duration>,
    redirect_policy: redirect::Policy,
}

impl RequestBuilder {
    pub fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        header::HeaderName: TryFrom<K>,
        header::HeaderValue: TryFrom<V>,
    {
        if let (Ok(name), Ok(val)) = (
            header::HeaderName::try_from(key),
            header::HeaderValue::try_from(value),
        ) {
            self.headers
                .insert(name.to_string(), val.to_str().unwrap_or("").to_string());
        }
        self
    }

    pub fn headers(mut self, headers: header::HeaderMap) -> Self {
        for (key, value) in headers.iter() {
            if let Ok(v) = value.to_str() {
                self.headers.insert(key.to_string(), v.to_string());
            }
        }
        self
    }

    pub fn bearer_auth(self, token: impl std::fmt::Display) -> Self {
        self.header("authorization", format!("Bearer {token}"))
    }

    pub fn basic_auth(
        self,
        username: impl std::fmt::Display,
        password: Option<impl std::fmt::Display>,
    ) -> Self {
        let value = match password {
            Some(p) => format!("{username}:{p}"),
            None => format!("{username}:"),
        };
        let encoded = base64_encode(value.as_bytes());
        self.header("authorization", format!("Basic {encoded}"))
    }

    pub fn body(mut self, body: impl Into<Body>) -> Self {
        let b: Body = body.into();
        self.body = Some(b.0);
        self
    }

    pub fn json<T: serde::Serialize + ?Sized>(mut self, json: &T) -> Self {
        match serde_json::to_vec(json) {
            Ok(body) => {
                self.headers
                    .insert("content-type".to_string(), "application/json".to_string());
                self.body = Some(body);
            }
            Err(_) => {}
        }
        self
    }

    pub fn query<T: serde::Serialize>(mut self, query: &T) -> Self {
        // Serialize to a sequence of key-value pairs and append to the URL.
        // reqwest supports both maps (&[("k","v")]) and structs; serde_urlencoded
        // handles both via Serialize.
        if let Some(ref mut url) = self.url {
            if let Ok(extra) = serde_urlencoded::to_string(query) {
                if !extra.is_empty() {
                    // Append to existing query string if present
                    let existing = url.query().unwrap_or("").to_string();
                    let combined = if existing.is_empty() {
                        extra
                    } else {
                        format!("{existing}&{extra}")
                    };
                    url.set_query(Some(&combined));
                }
            }
        }
        self
    }

    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub async fn send(self) -> Result<Response> {
        let url = self.url.ok_or_else(|| Error::new("missing URL"))?;
        let timeout_duration = self.timeout;

        let redirect = match self.redirect_policy.kind {
            redirect::PolicyKind::None => backend::RedirectMode::Manual,
            _ => backend::RedirectMode::Follow,
        };

        let raw_request = backend::RawRequest {
            method: self.method.to_string(),
            url: url.to_string(),
            headers: self.headers.into_iter().collect(),
            body: self.body,
            timeout_ms: timeout_duration.map(|d| d.as_millis() as u64),
            redirect,
        };

        // Execute the request, optionally wrapped in a timeout.
        // `execute_streaming_request` is synchronous but JSPI-suspends in
        // wasm32-wasip2, so wrapping in `async { ... }` lets
        // `tokio::time::timeout` enforce the deadline via wasi:clocks.
        let raw_response = if let Some(duration) = timeout_duration {
            match tokio::time::timeout(duration, async {
                backend::execute_streaming_request(raw_request)
            })
            .await
            {
                Ok(result) => result?,
                Err(_elapsed) => return Err(Error::new("request timed out")),
            }
        } else {
            backend::execute_streaming_request(raw_request)?
        };

        let status =
            StatusCode::from_u16(raw_response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let mut headers = header::HeaderMap::new();
        for (key, value) in &raw_response.headers {
            if let (Ok(name), Ok(val)) = (
                header::HeaderName::from_bytes(key.as_bytes()),
                header::HeaderValue::from_str(value),
            ) {
                headers.insert(name, val);
            }
        }

        Ok(Response {
            status,
            headers,
            body_reader: Some(raw_response.body_reader),
            body_buffered: None,
            url,
            version: Version::HTTP_11,
        })
    }
}

/// Response matching reqwest::Response.
/// Supports both streaming (body_reader) and buffered (body_buffered) modes.
pub struct Response {
    status: StatusCode,
    headers: header::HeaderMap,
    /// Streaming body reader — reads chunks on demand via WASI blocking-read.
    body_reader: Option<Box<dyn backend::BodyChunkReader>>,
    /// Buffered body — populated lazily when text()/bytes()/json() is called.
    body_buffered: Option<Vec<u8>>,
    url: Url,
    version: Version,
}

impl Response {
    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn headers(&self) -> &header::HeaderMap {
        &self.headers
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn content_length(&self) -> Option<u64> {
        // Unknown for streaming responses
        self.body_buffered.as_ref().map(|b| b.len() as u64)
    }

    /// Read the entire body, consuming the stream if necessary.
    fn read_full_body(&mut self) -> Result<Vec<u8>> {
        if let Some(body) = self.body_buffered.take() {
            return Ok(body);
        }
        if let Some(reader) = self.body_reader.take() {
            let mut buf = Vec::new();
            loop {
                let chunk = reader.read_chunk(65536).map_err(Error::new)?;
                if chunk.is_empty() {
                    break;
                }
                buf.extend(chunk);
            }
            Ok(buf)
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn text(mut self) -> Result<String> {
        let body = self.read_full_body()?;
        String::from_utf8(body).map_err(|_| Error::new("invalid utf-8 in response body"))
    }

    pub async fn bytes(mut self) -> Result<Bytes> {
        let body = self.read_full_body()?;
        Ok(Bytes::from(body))
    }

    /// Returns the body as a stream of byte chunks.
    /// Reads chunks incrementally from the WASI input-stream via blocking-read,
    /// which JSPI-suspends between chunks to yield to the JS event loop.
    pub fn bytes_stream(mut self) -> BytesStream {
        if let Some(reader) = self.body_reader.take() {
            BytesStream::Streaming {
                reader,
                done: false,
            }
        } else {
            // Already buffered — yield as a single chunk
            BytesStream::Buffered {
                data: self.body_buffered.take(),
            }
        }
    }

    pub async fn json<T: serde::de::DeserializeOwned>(mut self) -> Result<T> {
        let body = self.read_full_body()?;
        serde_json::from_slice(&body).map_err(|e| Error::new(format!("json decode: {e}")))
    }

    pub fn error_for_status(self) -> Result<Self> {
        if self.status.is_client_error() || self.status.is_server_error() {
            Err(Error::new(format!("HTTP {}", self.status)))
        } else {
            Ok(self)
        }
    }

    pub fn error_for_status_ref(&self) -> Result<&Self> {
        if self.status.is_client_error() || self.status.is_server_error() {
            Err(Error::new(format!("HTTP {}", self.status)))
        } else {
            Ok(self)
        }
    }
}

/// Stream of byte chunks from a response body.
pub enum BytesStream {
    /// Reads chunks on demand from the backend's body reader.
    /// Each poll calls blocking_read which JSPI-suspends until data arrives.
    Streaming {
        reader: Box<dyn backend::BodyChunkReader>,
        done: bool,
    },
    /// Pre-buffered body, yields as a single chunk.
    Buffered { data: Option<Vec<u8>> },
}

impl Stream for BytesStream {
    type Item = Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this {
            BytesStream::Streaming { reader, done } => {
                if *done {
                    return Poll::Ready(None);
                }
                // Blocking read: JSPI-suspends until data arrives or EOF.
                // Always returns Ready — no Pending. Matches real reqwest's
                // WASM backend which awaits JS promises for each chunk.
                match reader.read_chunk(65536) {
                    Ok(chunk) if chunk.is_empty() => {
                        *done = true;
                        Poll::Ready(None) // EOF
                    }
                    Ok(chunk) => Poll::Ready(Some(Ok(Bytes::from(chunk)))),
                    Err(e) => {
                        *done = true;
                        Poll::Ready(Some(Err(Error::new(e))))
                    }
                }
            }
            BytesStream::Buffered { data } => match data.take() {
                Some(d) if d.is_empty() => Poll::Ready(None),
                Some(d) => Poll::Ready(Some(Ok(Bytes::from(d)))),
                None => Poll::Ready(None),
            },
        }
    }
}

/// Trait for types convertible to a URL (matches reqwest::IntoUrl).
pub trait IntoUrl: IntoUrlSealed {}

/// Sealed helper so we can add `as_str()` to the trait without breaking things.
pub trait IntoUrlSealed {
    fn into_url(self) -> Result<Url>;
    /// Convenience for callers that need the string form before consuming.
    fn as_str(&self) -> &str;
}

impl IntoUrl for &str {}
impl IntoUrlSealed for &str {
    fn into_url(self) -> Result<Url> {
        Url::parse(self).map_err(|e| Error::new(format!("invalid url: {e}")))
    }
    fn as_str(&self) -> &str {
        self
    }
}

impl IntoUrl for String {}
impl IntoUrlSealed for String {
    fn into_url(self) -> Result<Url> {
        Url::parse(&self).map_err(|e| Error::new(format!("invalid url: {e}")))
    }
    fn as_str(&self) -> &str {
        self.as_str()
    }
}

impl IntoUrl for Url {}
impl IntoUrlSealed for Url {
    fn into_url(self) -> Result<Url> {
        Ok(self)
    }
    fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl IntoUrl for &String {}
impl IntoUrlSealed for &String {
    fn into_url(self) -> Result<Url> {
        Url::parse(self).map_err(|e| Error::new(format!("invalid url: {e}")))
    }
    fn as_str(&self) -> &str {
        String::as_str(self)
    }
}

/// Redirect policy module.
pub mod redirect {
    #[derive(Debug, Clone)]
    pub struct Policy {
        pub(crate) kind: PolicyKind,
    }

    #[derive(Debug, Clone)]
    pub(crate) enum PolicyKind {
        /// Follow redirects (default browser behavior).
        Follow,
        /// Do not follow redirects.
        None,
        /// Follow up to `max` redirects.
        Limited(usize),
    }

    impl Policy {
        /// Never follow redirects.
        pub fn none() -> Self {
            Policy {
                kind: PolicyKind::None,
            }
        }

        /// Follow redirects up to a maximum count.
        pub fn limited(max: usize) -> Self {
            Policy {
                kind: PolicyKind::Limited(max),
            }
        }
    }

    impl Default for Policy {
        fn default() -> Self {
            Policy {
                kind: PolicyKind::Follow,
            }
        }
    }
}

/// Simple base64 encoding (avoid pulling in the base64 crate).
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

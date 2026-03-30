//! WASI HTTP backend for wasi-reqwest.
//!
//! Routes reqwest HTTP requests through wasi:http/outgoing-handler,
//! which JCO transpiles to browser `fetch()` or sync XMLHttpRequest.
//!
//! Supports both buffered (execute) and streaming (execute_streaming)
//! response modes. Streaming is critical for SSE endpoints where the
//! server sends events incrementally over a long-lived connection.

use crate::bindings::wasi::http::{
    outgoing_handler,
    types::{Fields, OutgoingRequest, Scheme},
};
use reqwest::backend::{
    BodyChunkReader, HttpBackend, RawRequest, RawResponse, RawStreamingResponse,
};

pub struct WasiHttpBackend;

/// Send a request through wasi:http/outgoing-handler and wait for the response
/// (headers). Returns the response object without consuming the body.
fn send_request(
    request: &RawRequest,
) -> Result<crate::bindings::wasi::http::types::IncomingResponse, String> {
    console_log::console_log!("[wasi-http] {} {}", request.method, request.url);
    let (scheme, authority, path) = parse_url(&request.url)?;

    let header_fields = Fields::new();
    for (key, value) in &request.headers {
        let _ = header_fields.append(key.as_ref(), value.as_bytes());
    }

    let outgoing_request = OutgoingRequest::new(header_fields);
    outgoing_request
        .set_method(&parse_method(&request.method))
        .map_err(|_| "Failed to set method")?;
    outgoing_request
        .set_scheme(Some(&scheme))
        .map_err(|_| "Failed to set scheme")?;
    outgoing_request
        .set_authority(Some(&authority))
        .map_err(|_| "Failed to set authority")?;
    outgoing_request
        .set_path_with_query(Some(&path))
        .map_err(|_| "Failed to set path")?;

    // Write body if provided
    if let Some(body_bytes) = &request.body {
        let outgoing_body = outgoing_request
            .body()
            .map_err(|_| "Failed to get outgoing body")?;
        let stream = outgoing_body
            .write()
            .map_err(|_| "Failed to get write stream")?;

        let mut offset = 0;
        while offset < body_bytes.len() {
            let chunk_size = std::cmp::min(65536, body_bytes.len() - offset);
            let chunk = &body_bytes[offset..offset + chunk_size];

            let pollable = stream.subscribe();
            pollable.block();

            stream
                .write(chunk)
                .map_err(|e| format!("Failed to write body chunk: {e:?}"))?;
            offset += chunk_size;
        }

        drop(stream);
        crate::bindings::wasi::http::types::OutgoingBody::finish(outgoing_body, None)
            .map_err(|_| "Failed to finish outgoing body")?;
    }

    // Send request and wait for response headers
    console_log::console_log!("[wasi-http] waiting for response headers...");
    let future_response = outgoing_handler::handle(outgoing_request, None)
        .map_err(|e| format!("HTTP request failed: {e:?}"))?;

    let mut poll_count = 0u32;
    loop {
        let pollable = future_response.subscribe();
        pollable.block();
        poll_count += 1;

        if let Some(result) = future_response.get() {
            let res = result
                .map_err(|_| "Response error".to_string())?
                .map_err(|e| format!("HTTP error: {e:?}"))?;
            let status = res.status();
            console_log::console_log!("[wasi-http] got response: {} (polls={})", status, poll_count);
            return Ok(res);
        }

        if poll_count % 100 == 0 {
            console_log::console_warn!("[wasi-http] still waiting for headers, polls={}", poll_count);
        }
    }
}

/// Extract status and headers from a WASI IncomingResponse.
fn extract_headers(
    response: &crate::bindings::wasi::http::types::IncomingResponse,
) -> (u16, Vec<(String, String)>) {
    let status = response.status();
    let response_headers = response.headers();
    let header_entries: Vec<(String, String)> = response_headers
        .entries()
        .into_iter()
        .map(|(k, v)| (k, String::from_utf8_lossy(&v).to_string()))
        .collect();
    (status, header_entries)
}

impl HttpBackend for WasiHttpBackend {
    fn execute(&self, request: RawRequest) -> Result<RawResponse, String> {
        let url = request.url.clone();
        let response = send_request(&request)?;
        let (status, headers) = extract_headers(&response);

        // Read full body (blocking — suitable for non-streaming endpoints)
        console_log::console_log!("[wasi-http] reading buffered body for {}", url);
        let body_handle = response
            .consume()
            .map_err(|_| "Failed to consume response body")?;
        let body = read_body_bytes(body_handle)?;
        console_log::console_log!("[wasi-http] buffered body complete: {} bytes", body.len());

        Ok(RawResponse {
            status,
            headers,
            body,
        })
    }

    fn execute_streaming(
        &self,
        request: RawRequest,
    ) -> Result<RawStreamingResponse, String> {
        console_log::console_log!("[wasi-http] streaming request: {} {}", request.method, request.url);
        let response = send_request(&request)?;
        let (status, headers) = extract_headers(&response);
        console_log::console_log!("[wasi-http] streaming response: status={}", status);

        // Get body stream but don't read it — return a reader that yields
        // chunks on demand. Each read_chunk call JSPI-suspends until data
        // arrives, allowing the cooperative scheduler to yield to JS.
        let body_handle = response
            .consume()
            .map_err(|_| "Failed to consume response body")?;
        let stream = body_handle
            .stream()
            .map_err(|_| "Failed to get body stream")?;

        // body_handle must stay alive — dropping IncomingBody closes InputStream
        Ok(RawStreamingResponse {
            status,
            headers,
            body_reader: Box::new(WasiBodyReader { stream, _body: body_handle }),
        })
    }
}

/// Reads body chunks from a WASI InputStream on demand.
/// Each `read_chunk` call invokes `blocking-read` which JSPI-suspends
/// until data arrives from the network.
///
/// IMPORTANT: Must keep `_body` alive — per WASI spec, dropping IncomingBody
/// closes its child InputStream.
struct WasiBodyReader {
    stream: crate::bindings::wasi::io::streams::InputStream,
    _body: crate::bindings::wasi::http::types::IncomingBody,
}

// SAFETY: Single-threaded WASM — no concurrent access.
unsafe impl Send for WasiBodyReader {}

impl BodyChunkReader for WasiBodyReader {
    fn read_chunk(&self, max_len: usize) -> Result<Vec<u8>, String> {
        // Non-blocking read. Per WASI spec:
        // - Ok(data) with data.len() > 0 → chunk available
        // - Ok(empty) → no data available yet (would-block)
        // - Err(StreamError::Closed) → stream done (EOF)
        match self.stream.read(max_len as u64) {
            Ok(chunk) if chunk.is_empty() => Err("would-block".to_string()),
            Ok(chunk) => {
                console_log::console_log!("[wasi-http] stream chunk: {} bytes", chunk.len());
                Ok(chunk)
            }
            Err(crate::bindings::wasi::io::streams::StreamError::Closed) => {
                console_log::console_log!("[wasi-http] stream closed (EOF)");
                Ok(Vec::new())
            }
            Err(e) => {
                console_log::console_error!("[wasi-http] stream error: {e:?}");
                Err(format!("Stream read error: {e:?}"))
            }
        }
    }
}

/// Read the entire body from an IncomingBody stream.
fn read_body_bytes(
    body: crate::bindings::wasi::http::types::IncomingBody,
) -> Result<Vec<u8>, String> {
    let stream = body.stream().map_err(|_| "Failed to get body stream")?;

    let mut bytes = Vec::new();
    loop {
        match stream.blocking_read(65536) {
            Ok(chunk) => {
                if chunk.is_empty() {
                    break;
                }
                bytes.extend(chunk);
            }
            Err(_) => break,
        }
    }

    drop(stream);
    Ok(bytes)
}

/// Parse URL into (scheme, authority, path_with_query).
fn parse_url(url: &str) -> Result<(Scheme, String, String), String> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (Scheme::Http, rest)
    } else {
        return Err(format!("Unsupported URL scheme: {url}"));
    };

    let split_idx = rest
        .find(|ch: char| ['/', '?', '#'].contains(&ch))
        .unwrap_or(rest.len());
    let authority = rest[..split_idx].to_string();

    let mut path = if split_idx >= rest.len() {
        "/".to_string()
    } else {
        let tail = &rest[split_idx..];
        if tail.starts_with('/') {
            tail.to_string()
        } else {
            format!("/{tail}")
        }
    };

    if let Some(frag) = path.find('#') {
        path.truncate(frag);
    }
    if path.is_empty() {
        path = "/".to_string();
    }
    if authority.is_empty() {
        return Err("URL has no authority (host)".to_string());
    }

    Ok((scheme, authority, path))
}

/// Map method string to WASI HTTP Method enum.
fn parse_method(method: &str) -> crate::bindings::wasi::http::types::Method {
    use crate::bindings::wasi::http::types::Method;
    match method.to_uppercase().as_str() {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "OPTIONS" => Method::Options,
        "TRACE" => Method::Trace,
        "PATCH" => Method::Patch,
        _ => Method::Other(method.to_string()),
    }
}

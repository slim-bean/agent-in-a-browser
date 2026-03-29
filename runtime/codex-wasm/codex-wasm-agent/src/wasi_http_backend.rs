//! WASI HTTP backend for wasi-reqwest.
//!
//! Routes reqwest HTTP requests through wasi:http/outgoing-handler,
//! which JCO transpiles to browser `fetch()` or sync XMLHttpRequest.

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

    let future_response = outgoing_handler::handle(outgoing_request, None)
        .map_err(|e| format!("HTTP request failed: {e:?}"))?;

    loop {
        let pollable = future_response.subscribe();
        pollable.block();

        if let Some(result) = future_response.get() {
            return result
                .map_err(|_| "Response error".to_string())?
                .map_err(|e| format!("HTTP error: {e:?}"));
        }
    }
}

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
        let response = send_request(&request)?;
        let (status, headers) = extract_headers(&response);

        let body_handle = response
            .consume()
            .map_err(|_| "Failed to consume response body")?;
        let body = read_body_bytes(body_handle)?;

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
        let response = send_request(&request)?;
        let (status, headers) = extract_headers(&response);

        let body_handle = response
            .consume()
            .map_err(|_| "Failed to consume response body")?;
        let stream = body_handle
            .stream()
            .map_err(|_| "Failed to get body stream")?;

        Ok(RawStreamingResponse {
            status,
            headers,
            body_reader: Box::new(WasiBodyReader { stream, _body: body_handle }),
        })
    }
}

struct WasiBodyReader {
    stream: crate::bindings::wasi::io::streams::InputStream,
    _body: crate::bindings::wasi::http::types::IncomingBody,
}

unsafe impl Send for WasiBodyReader {}

impl BodyChunkReader for WasiBodyReader {
    fn read_chunk(&self, max_len: usize) -> Result<Vec<u8>, String> {
        match self.stream.blocking_read(max_len as u64) {
            Ok(chunk) => Ok(chunk),
            Err(crate::bindings::wasi::io::streams::StreamError::Closed) => Ok(Vec::new()),
            Err(e) => Err(format!("Stream read error: {e:?}")),
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

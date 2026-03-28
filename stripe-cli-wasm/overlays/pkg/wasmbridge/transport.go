//go:build wasip1

// Package wasmbridge provides an http.RoundTripper implementation that routes
// HTTP requests through host-provided WASM imports, bridging to the browser's
// fetch() API via the JS shim layer.
//
// This replaces Go's net/http.DefaultTransport which cannot function in WASI
// (no network stack). The host functions are implemented in TypeScript as part
// of the wasi-shims package.
package wasmbridge

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"unsafe"
)

// Host-imported functions for HTTP communication.
// These are implemented by the JS runtime (packages/wasi-shims/src/http-bridge-impl.ts)
// and linked at WASM instantiation time.
//
// Functions with high-level return types (string, list<u8>) use the canonical ABI
// retptr pattern: an extra pointer param where the host writes (ptr, len).

//go:wasmimport stripe:bridge/http-bridge@0.1.0 request
func hostHTTPRequest(
	methodPtr, methodLen uint32,
	urlPtr, urlLen uint32,
	headersPtr, headersLen uint32,
	bodyPtr, bodyLen uint32,
) uint32

//go:wasmimport stripe:bridge/http-bridge@0.1.0 response-status
func hostResponseStatus(handle uint32) uint32

// response-headers returns a string via retptr: the host writes (ptr: u32, len: u32) at retptr.
//
//go:wasmimport stripe:bridge/http-bridge@0.1.0 response-headers
func hostResponseHeadersRaw(handle uint32, retptr unsafe.Pointer)

// response-body-read returns list<u8> via retptr: the host writes (ptr: u32, len: u32) at retptr.
//
//go:wasmimport stripe:bridge/http-bridge@0.1.0 response-body-read
func hostResponseBodyReadRaw(handle uint32, maxBytes uint32, retptr unsafe.Pointer)

//go:wasmimport stripe:bridge/http-bridge@0.1.0 response-close
func hostResponseClose(handle uint32)

// hostResponseHeaders calls the raw import and returns the header JSON string.
func hostResponseHeaders(handle uint32) string {
	var ret [2]uint32 // [ptr, len]
	hostResponseHeadersRaw(handle, unsafe.Pointer(&ret[0]))
	if ret[1] == 0 {
		return "{}"
	}
	return unsafe.String((*byte)(unsafe.Pointer(uintptr(ret[0]))), int(ret[1]))
}

// hostResponseBodyRead calls the raw import and returns the data slice.
func hostResponseBodyRead(handle uint32, maxBytes uint32) []byte {
	var ret [2]uint32 // [ptr, len]
	hostResponseBodyReadRaw(handle, maxBytes, unsafe.Pointer(&ret[0]))
	if ret[1] == 0 {
		return nil
	}
	return unsafe.Slice((*byte)(unsafe.Pointer(uintptr(ret[0]))), int(ret[1]))
}

// Transport implements http.RoundTripper using WASM host imports.
type Transport struct{}

// RoundTrip executes a single HTTP transaction via the host bridge.
func (t *Transport) RoundTrip(req *http.Request) (*http.Response, error) {
	// Marshal method
	method := req.Method
	methodBytes := []byte(method)

	// Marshal URL
	urlStr := req.URL.String()
	urlBytes := []byte(urlStr)

	// Marshal headers as JSON
	headerMap := make(map[string]string)
	for key, values := range req.Header {
		if len(values) > 0 {
			headerMap[key] = values[0]
		}
	}
	headersJSON, err := json.Marshal(headerMap)
	if err != nil {
		return nil, fmt.Errorf("wasmbridge: failed to marshal headers: %w", err)
	}

	// Read request body
	var bodyBytes []byte
	if req.Body != nil {
		bodyBytes, err = io.ReadAll(req.Body)
		if err != nil {
			return nil, fmt.Errorf("wasmbridge: failed to read request body: %w", err)
		}
		req.Body.Close()
	}

	// Call host function
	methodPtr, methodLen := ptrAndLen(methodBytes)
	urlPtr, urlLen := ptrAndLen(urlBytes)
	headersPtr, headersLen := ptrAndLen(headersJSON)
	bodyPtr, bodyLen := ptrAndLen(bodyBytes)
	handle := hostHTTPRequest(
		methodPtr, methodLen,
		urlPtr, urlLen,
		headersPtr, headersLen,
		bodyPtr, bodyLen,
	)

	// Get response status
	status := hostResponseStatus(handle)

	// Get response headers (host returns a JSON string via canonical ABI)
	headersStr := hostResponseHeaders(handle)
	var respHeaders map[string]string
	if headersStr != "" && headersStr != "{}" {
		if err := json.Unmarshal([]byte(headersStr), &respHeaders); err != nil {
			respHeaders = make(map[string]string)
		}
	}

	// Build http.Response
	resp := &http.Response{
		StatusCode: int(status),
		Status:     fmt.Sprintf("%d %s", status, http.StatusText(int(status))),
		Header:     make(http.Header),
		Body:       &responseBody{handle: handle},
		Request:    req,
	}

	for key, value := range respHeaders {
		resp.Header.Set(key, value)
	}

	return resp, nil
}

// responseBody implements io.ReadCloser for streaming response data from the host.
type responseBody struct {
	handle uint32
	buf    bytes.Buffer
	closed bool
}

func (r *responseBody) Read(p []byte) (int, error) {
	if r.closed {
		return 0, io.EOF
	}

	// If we have buffered data, return it first
	if r.buf.Len() > 0 {
		return r.buf.Read(p)
	}

	// Read from host (returns a new byte slice via canonical ABI)
	data := hostResponseBodyRead(r.handle, uint32(len(p)))
	if len(data) == 0 {
		return 0, io.EOF
	}
	// Copy into caller's buffer
	n := copy(p, data)
	// Buffer any excess
	if n < len(data) {
		r.buf.Write(data[n:])
	}
	return n, nil
}

func (r *responseBody) Close() error {
	if !r.closed {
		r.closed = true
		hostResponseClose(r.handle)
	}
	return nil
}

// ptrAndLen returns the pointer and length of a byte slice as uint32 values
// suitable for passing to WASM host imports.
func ptrAndLen(b []byte) (uint32, uint32) {
	if len(b) == 0 {
		return 0, 0
	}
	return uint32(uintptr(unsafe.Pointer(&b[0]))), uint32(len(b))
}

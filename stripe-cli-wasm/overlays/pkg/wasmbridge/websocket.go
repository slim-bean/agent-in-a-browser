//go:build wasip1

// Package wasmbridge provides a WebSocket client that routes connections through
// host-provided WASM imports, bridging to the browser's WebSocket API.
//
// This replaces gorilla/websocket which cannot function in WASI (no network stack).
// Used by `stripe listen` for real-time webhook event streaming.
package wasmbridge

import (
	"encoding/binary"
	"fmt"
	"io"
	"sync"
	"unsafe"
)

// Host-imported functions for WebSocket communication.
// Implemented by packages/wasi-shims/src/ws-bridge-impl.ts
//
// Functions with high-level types use the canonical ABI. Go uses retptr-based
// imports for functions returning string/list<u8>.

// connect: func(url: string) -> u32
// canonical ABI: (i32, i32) -> i32 (url_ptr, url_len) -> handle
//
//go:wasmimport stripe:bridge/ws-bridge@0.1.0 connect
func hostWSConnect(urlPtr, urlLen uint32) uint32

// read: func(handle: u32, max-bytes: u32) -> list<u8>
// canonical ABI: (i32, i32, i32) -> void (handle, max_bytes, retptr)
//
//go:wasmimport stripe:bridge/ws-bridge@0.1.0 read
func hostWSReadRaw(handle uint32, maxBytes uint32, retptr unsafe.Pointer)

// write: func(handle: u32, data: list<u8>) -> u32
// canonical ABI: (i32, i32, i32) -> i32 (handle, data_ptr, data_len) -> written
//
//go:wasmimport stripe:bridge/ws-bridge@0.1.0 write
func hostWSWrite(handle uint32, dataPtr, dataLen uint32) uint32

//go:wasmimport stripe:bridge/ws-bridge@0.1.0 close
func hostWSClose(handle uint32)

// hostWSRead wraps the raw retptr import.
func hostWSRead(handle uint32, maxBytes uint32) []byte {
	var ret [2]uint32 // [ptr, len]
	hostWSReadRaw(handle, maxBytes, unsafe.Pointer(&ret[0]))
	if ret[1] == 0 {
		return nil
	}
	return unsafe.Slice((*byte)(unsafe.Pointer(uintptr(ret[0]))), int(ret[1]))
}

// MessageType represents a WebSocket message type.
type MessageType int

const (
	TextMessage   MessageType = 1
	BinaryMessage MessageType = 2
	CloseMessage  MessageType = 8
	PingMessage   MessageType = 9
	PongMessage   MessageType = 10
)

// Conn represents a WebSocket connection via the host bridge.
type Conn struct {
	handle uint32
	mu     sync.Mutex
	closed bool
}

// Dial opens a WebSocket connection through the host bridge.
func Dial(url string) (*Conn, error) {
	urlBytes := []byte(url)
	handle := hostWSConnect(
		uint32(uintptr(unsafe.Pointer(&urlBytes[0]))),
		uint32(len(urlBytes)),
	)
	if handle == 0 {
		return nil, fmt.Errorf("wasmbridge: failed to connect WebSocket to %s", url)
	}
	return &Conn{handle: handle}, nil
}

// ReadMessage reads the next message from the WebSocket connection.
// Returns the message type and payload.
func (c *Conn) ReadMessage() (MessageType, []byte, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.closed {
		return 0, nil, io.EOF
	}

	// Read via canonical ABI — host returns data as list<u8>
	data := hostWSRead(c.handle, 64*1024)
	if len(data) == 0 {
		return 0, nil, io.EOF
	}

	if len(data) < 4 {
		return 0, nil, fmt.Errorf("wasmbridge: WebSocket message too short (%d bytes)", len(data))
	}

	// First 4 bytes: message type (uint32 LE), rest: payload
	msgType := MessageType(binary.LittleEndian.Uint32(data[:4]))
	payload := make([]byte, len(data)-4)
	copy(payload, data[4:])

	return msgType, payload, nil
}

// WriteMessage sends a message on the WebSocket connection.
func (c *Conn) WriteMessage(msgType MessageType, data []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.closed {
		return fmt.Errorf("wasmbridge: WebSocket connection closed")
	}

	// Prefix data with message type (4 bytes LE)
	frame := make([]byte, 4+len(data))
	binary.LittleEndian.PutUint32(frame[:4], uint32(msgType))
	copy(frame[4:], data)

	written := hostWSWrite(c.handle, uint32(uintptr(unsafe.Pointer(&frame[0]))), uint32(len(frame)))
	if written == 0 {
		return fmt.Errorf("wasmbridge: failed to write WebSocket message")
	}
	return nil
}

// Close closes the WebSocket connection.
func (c *Conn) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if !c.closed {
		c.closed = true
		hostWSClose(c.handle)
	}
	return nil
}

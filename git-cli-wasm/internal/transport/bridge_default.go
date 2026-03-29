//go:build !wasip1

// Package transport provides platform-specific HTTP transport for go-git.
// On non-WASM platforms, go-git uses its default HTTP transport.
package transport

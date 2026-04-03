//go:build wasip1

// WASM overlay for pkg/open — routes URL opening through a host-provided
// WASM import that calls window.open() in the browser via BroadcastChannel.
//
// Replaces the native implementation that uses exec.Command (xdg-open/open/rundll32)
// which cannot function in WASI (no subprocess support).
package open

import "unsafe"

// Host-imported function for opening URLs in a browser tab.
// Implemented by go-wasip1-loader.ts which routes to browser-impl.ts.
//
// open-url: func(url: string)
// canonical ABI: (url_ptr, url_len) -> void
//
//go:wasmimport host:browser/actions@0.1.0 open-url
func hostOpenUrl(urlPtr, urlLen uint32)

// Browser opens a URL using the host browser's window.open().
func Browser(url string) error {
	urlBytes := []byte(url)
	hostOpenUrl(
		uint32(uintptr(unsafe.Pointer(&urlBytes[0]))),
		uint32(len(urlBytes)),
	)
	return nil
}

// CanOpenBrowser always returns true in WASM since we have the host bridge.
func CanOpenBrowser() bool {
	return true
}

//go:build wasip1

package fixtures

import "fmt"

// Edit is not supported in WASM — external editors are not available.
// Fixture execution still works; only interactive editing is disabled.
var Edit = func(path string, filedata []byte) ([]byte, error) {
	return nil, fmt.Errorf("interactive fixture editing is not supported in WASM mode")
}

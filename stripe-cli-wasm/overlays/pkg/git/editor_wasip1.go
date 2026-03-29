//go:build wasip1

package git

import "fmt"

// Editor can be used to allow the user to directly edit content using
// their default IDE. Not supported in WASM mode.
type Editor struct {
	Program           string
	File              string
	Launch            func() error
	usesTemporaryFile bool
}

// NewTemporaryFileEditor is not supported in WASM mode.
func NewTemporaryFileEditor(filename string, content []byte) (editor *Editor, err error) {
	return nil, fmt.Errorf("editor is not supported in WASM mode")
}

// NewEditor is not supported in WASM mode.
func NewEditor(file string) (editor *Editor, err error) {
	return nil, fmt.Errorf("editor is not supported in WASM mode")
}

// EditContent is not supported in WASM mode.
func (e *Editor) EditContent() ([]byte, error) {
	return nil, fmt.Errorf("editor is not supported in WASM mode")
}

//go:build wasip1

package copy

import "os"

// pcopy is a no-op on WASM (named pipes not supported).
func pcopy(dest string, info os.FileInfo) error {
	return &os.PathError{Op: "mkfifo", Path: dest, Err: os.ErrPermission}
}

//go:build wasip1

package copy

// preserveLtimes is a no-op on WASM (no lutimes syscall available).
func preserveLtimes(src, dest string) error {
	return nil
}

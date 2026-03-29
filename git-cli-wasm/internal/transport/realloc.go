//go:build wasip1

package transport

import "unsafe"

// cabi_realloc is the canonical ABI realloc function required by the component
// model for imports that return string or list<u8>. The host calls this to
// allocate memory in the module's linear memory for writing return values.
//
//go:wasmexport cabi_realloc
func cabiRealloc(oldPtr, oldSize, align, newSize uint32) uint32 {
	if newSize == 0 {
		return 0
	}
	buf := make([]byte, newSize)
	if oldSize > 0 && oldPtr != 0 {
		old := unsafe.Slice((*byte)(unsafe.Pointer(uintptr(oldPtr))), oldSize)
		copy(buf, old)
	}
	return uint32(uintptr(unsafe.Pointer(&buf[0])))
}

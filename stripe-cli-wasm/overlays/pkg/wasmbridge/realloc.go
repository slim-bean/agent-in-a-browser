//go:build wasip1

package wasmbridge

import "unsafe"

// cabi_realloc is the canonical ABI realloc function required by the component
// model for imports that return string or list<u8>. The host calls this to
// allocate memory in the module's linear memory for writing return values.
//
// This is a simple bump allocator. Since Go manages its own heap via the GC,
// we allocate from Go's heap and return the pointer. The memory will be
// managed by Go's GC (the caller is expected to copy the data out promptly).
//
//go:wasmexport cabi_realloc
func cabiRealloc(oldPtr, oldSize, align, newSize uint32) uint32 {
	if newSize == 0 {
		return 0
	}
	buf := make([]byte, newSize)
	if oldSize > 0 && oldPtr != 0 {
		// Copy old data to new allocation
		old := unsafe.Slice((*byte)(unsafe.Pointer(uintptr(oldPtr))), oldSize)
		copy(buf, old)
	}
	return uint32(uintptr(unsafe.Pointer(&buf[0])))
}

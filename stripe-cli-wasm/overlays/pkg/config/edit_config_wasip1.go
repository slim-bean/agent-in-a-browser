//go:build wasip1

package config

import "fmt"

// EditConfig is not supported in WASM - external editors are not available.
func (c *Config) EditConfig() error {
	return fmt.Errorf("editing config is not supported in WASM mode")
}

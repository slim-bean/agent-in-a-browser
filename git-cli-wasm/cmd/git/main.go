// git CLI — a Go implementation using go-git, compiled to WASM for browser execution.
package main

import (
	"fmt"
	"os"

	"github.com/tjfontaine/git-cli-wasm/internal/commands"
	// Import transport to register the WASM HTTP bridge (wasip1 build)
	// or use default transport (native build).
	_ "github.com/tjfontaine/git-cli-wasm/internal/transport"
)

func main() {
	args := os.Args[1:]
	if len(args) == 0 {
		commands.PrintUsage(os.Stdout)
		os.Exit(0)
	}

	subcommand := args[0]
	subargs := args[1:]

	code, err := commands.Run(subcommand, subargs)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		if code == 0 {
			code = 1
		}
	}
	os.Exit(code)
}

package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/mod/modfile"
)

// applyGoModPatches adds replace directives to go.mod.
func applyGoModPatches(ctx *Context) error {
	gomodPath := filepath.Join(ctx.TargetDir, "go.mod")
	data, err := os.ReadFile(gomodPath)
	if err != nil {
		return fmt.Errorf("read go.mod: %w", err)
	}

	f, err := modfile.Parse("go.mod", data, nil)
	if err != nil {
		return fmt.Errorf("parse go.mod: %w", err)
	}

	modified := false

	for _, repl := range Manifest.GoModReplaces {
		// Find the version of the module in the require block
		version := findRequireVersion(f, repl.Module)
		if version == "" {
			fmt.Printf("  [warn] %s not found in go.mod require block\n", repl.Module)
			continue
		}

		// Check if replace already exists
		if hasReplace(f, repl.Module) {
			fmt.Printf("  [skip] replace %s (already exists)\n", repl.Module)
			continue
		}

		if ctx.DryRun {
			fmt.Printf("  [dry-run] add replace %s %s => %s\n", repl.Module, version, repl.Replacement)
			continue
		}

		// Add the replace directive
		if err := f.AddReplace(repl.Module, version, repl.Replacement, ""); err != nil {
			return fmt.Errorf("add replace %s: %w", repl.Module, err)
		}

		// Add a comment (modfile doesn't support comments directly, so we'll
		// write it manually after formatting)
		modified = true
		fmt.Printf("  added replace %s %s => %s\n", repl.Module, version, repl.Replacement)
	}

	if !modified || ctx.DryRun {
		return nil
	}

	// Format and write
	formatted, err := f.Format()
	if err != nil {
		return fmt.Errorf("format go.mod: %w", err)
	}

	// Add comments before replace directives. The modfile library doesn't
	// support adding comments, so we do a post-format text insertion.
	output := string(formatted)
	for _, repl := range Manifest.GoModReplaces {
		if repl.Comment == "" {
			continue
		}
		version := findRequireVersion(f, repl.Module)
		replaceLine := fmt.Sprintf("replace %s %s => %s", repl.Module, version, repl.Replacement)
		commentLine := fmt.Sprintf("// %s\n%s", repl.Comment, replaceLine)
		output = strings.Replace(output, replaceLine, commentLine, 1)
	}

	return os.WriteFile(gomodPath, []byte(output), 0o644)
}

func findRequireVersion(f *modfile.File, module string) string {
	for _, req := range f.Require {
		if req.Mod.Path == module {
			return req.Mod.Version
		}
	}
	return ""
}

func hasReplace(f *modfile.File, module string) bool {
	for _, repl := range f.Replace {
		if repl.Old.Path == module {
			return true
		}
	}
	return false
}

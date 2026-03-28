package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// applyInlineReplacements performs targeted text replacements and then
// removes imports that are no longer needed.
func applyInlineReplacements(ctx *Context) error {
	for _, repl := range Manifest.InlineReplacements {
		if err := applyInlineReplacement(ctx, repl); err != nil {
			return err
		}
	}

	// Now handle import removals (grouped by file for efficiency)
	byFile := make(map[string][]string)
	for _, rem := range Manifest.ImportRemovals {
		byFile[rem.File] = append(byFile[rem.File], rem.ImportPath)
	}

	for file, imports := range byFile {
		if err := applyImportRemovals(ctx, file, imports); err != nil {
			return err
		}
	}

	return nil
}

func applyInlineReplacement(ctx *Context, repl InlineReplacement) error {
	path := filepath.Join(ctx.TargetDir, repl.File)
	if !fileExists(path) {
		fmt.Printf("  [skip] %s (not found)\n", repl.File)
		return nil
	}

	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	src := string(content)

	if !strings.Contains(src, repl.From) {
		// Check if the replacement has already been applied
		if strings.Contains(src, repl.To) {
			fmt.Printf("  [skip] %s (replacement already applied)\n", repl.File)
			return nil
		}
		fmt.Printf("  [warn] %s: pattern not found (upstream may have changed)\n", repl.File)
		return nil
	}

	if ctx.DryRun {
		fmt.Printf("  [dry-run] replace in %s\n", repl.File)
		return nil
	}

	newSrc := strings.Replace(src, repl.From, repl.To, 1)
	if err := os.WriteFile(path, []byte(newSrc), 0o644); err != nil {
		return err
	}

	fmt.Printf("  replaced inline code in %s\n", repl.File)
	return nil
}

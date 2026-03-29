package main

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/printer"
	"go/token"
	"os"
	"path/filepath"
	"strings"
)

// applyBuildTags handles both exclusions (prepend //go:build !wasip1) and
// amendments (add !wasip1 to existing constraints).
func applyBuildTags(ctx *Context) error {
	for _, excl := range Manifest.BuildTagExclusions {
		path := filepath.Join(ctx.TargetDir, excl.File)
		if !fileExists(path) {
			fmt.Printf("  [skip] %s (not found)\n", excl.File)
			continue
		}
		if err := prependBuildTag(ctx, excl.File); err != nil {
			return err
		}
	}

	for _, removal := range Manifest.BuildTagRemovals {
		path := filepath.Join(ctx.TargetDir, removal.File)
		if !fileExists(path) {
			fmt.Printf("  [skip] %s (not found)\n", removal.File)
			continue
		}
		if err := removeBuildTag(ctx, removal.File); err != nil {
			return err
		}
	}

	for _, amend := range Manifest.BuildTagAmendments {
		path := filepath.Join(ctx.TargetDir, amend.File)
		if !fileExists(path) {
			fmt.Printf("  [skip] %s (not found)\n", amend.File)
			continue
		}
		if err := amendBuildTag(ctx, amend.File); err != nil {
			return err
		}
	}

	return nil
}

// prependBuildTag adds `//go:build !wasip1` before the package clause.
// If the file already has a //go:build line containing "wasip1", it's skipped.
func prependBuildTag(ctx *Context, relPath string) error {
	path := filepath.Join(ctx.TargetDir, relPath)

	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	src := string(content)

	// Check if already has a build tag with wasip1
	if containsBuildTagWithWasip1(src) {
		fmt.Printf("  [skip] %s (already has wasip1 constraint)\n", relPath)
		return nil
	}

	if ctx.DryRun {
		fmt.Printf("  [dry-run] prepend //go:build !wasip1 to %s\n", relPath)
		return nil
	}

	// Parse to find the package clause position
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, path, content, parser.ParseComments)
	if err != nil {
		return fmt.Errorf("parse %s: %w", relPath, err)
	}

	// If the file already has a //go:build directive, we need to AND with !wasip1
	if existingTag := findGoBuildComment(f); existingTag != nil {
		return amendExistingTag(ctx, path, relPath, content, existingTag)
	}

	// No existing build tag — prepend before everything
	newContent := "//go:build !wasip1\n\n" + src

	if err := os.WriteFile(path, []byte(newContent), 0o644); err != nil {
		return err
	}

	fmt.Printf("  prepended //go:build !wasip1 to %s\n", relPath)
	return nil
}

// amendBuildTag adds `&& !wasip1` to an existing //go:build constraint.
func amendBuildTag(ctx *Context, relPath string) error {
	path := filepath.Join(ctx.TargetDir, relPath)

	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	src := string(content)

	if strings.Contains(src, "wasip1") {
		fmt.Printf("  [skip] %s (already has wasip1)\n", relPath)
		return nil
	}

	if ctx.DryRun {
		fmt.Printf("  [dry-run] amend build tag in %s\n", relPath)
		return nil
	}

	lines := strings.Split(src, "\n")
	modified := false

	for i, line := range lines {
		trimmed := strings.TrimSpace(line)

		// Handle //go:build lines
		if strings.HasPrefix(trimmed, "//go:build ") {
			constraint := strings.TrimPrefix(trimmed, "//go:build ")
			lines[i] = "//go:build " + constraint + " && !wasip1"
			modified = true
			continue
		}

		// Handle legacy // +build lines
		if strings.HasPrefix(trimmed, "// +build ") {
			constraint := strings.TrimPrefix(trimmed, "// +build ")
			// Legacy format uses comma for AND within a tag, space for OR
			lines[i] = "// +build " + constraint + ",!wasip1"
			modified = true
			continue
		}

		// Stop after package line
		if strings.HasPrefix(trimmed, "package ") {
			break
		}
	}

	if !modified {
		fmt.Printf("  [skip] %s (no build tag found to amend)\n", relPath)
		return nil
	}

	if err := os.WriteFile(path, []byte(strings.Join(lines, "\n")), 0o644); err != nil {
		return err
	}

	fmt.Printf("  amended build tag in %s\n", relPath)
	return nil
}

// removeBuildTag removes the `//go:build !wasip1` line from a file.
// Used when a file was previously excluded from wasip1 builds but is now supported.
func removeBuildTag(ctx *Context, relPath string) error {
	path := filepath.Join(ctx.TargetDir, relPath)

	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	src := string(content)

	// Check if it has a standalone !wasip1 tag
	if !strings.Contains(src, "//go:build !wasip1") {
		fmt.Printf("  [skip] %s (no !wasip1 tag to remove)\n", relPath)
		return nil
	}

	if ctx.DryRun {
		fmt.Printf("  [dry-run] remove //go:build !wasip1 from %s\n", relPath)
		return nil
	}

	// Remove the //go:build !wasip1 line (and the blank line after it)
	newSrc := strings.Replace(src, "//go:build !wasip1\n\n", "", 1)
	if newSrc == src {
		// Try without double newline
		newSrc = strings.Replace(src, "//go:build !wasip1\n", "", 1)
	}

	if err := os.WriteFile(path, []byte(newSrc), 0o644); err != nil {
		return err
	}

	fmt.Printf("  removed //go:build !wasip1 from %s\n", relPath)
	return nil
}

// containsBuildTagWithWasip1 checks if the source already has a //go:build line
// mentioning wasip1.
func containsBuildTagWithWasip1(src string) bool {
	for _, line := range strings.Split(src, "\n") {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "//go:build ") && strings.Contains(trimmed, "wasip1") {
			return true
		}
		if strings.HasPrefix(trimmed, "package ") {
			break
		}
	}
	return false
}

// findGoBuildComment finds the //go:build comment in a parsed file.
func findGoBuildComment(f *ast.File) *ast.Comment {
	for _, cg := range f.Comments {
		for _, c := range cg.List {
			if strings.HasPrefix(c.Text, "//go:build ") {
				return c
			}
		}
	}
	return nil
}

// amendExistingTag handles the case where prependBuildTag finds a file that
// already has a //go:build directive (but without wasip1).
func amendExistingTag(ctx *Context, path, relPath string, content []byte, tag *ast.Comment) error {
	_ = ctx
	src := string(content)
	oldTag := tag.Text
	constraint := strings.TrimPrefix(oldTag, "//go:build ")
	newTag := "//go:build " + constraint + " && !wasip1"
	newSrc := strings.Replace(src, oldTag, newTag, 1)

	// Also handle legacy // +build if present
	lines := strings.Split(newSrc, "\n")
	for i, line := range lines {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "// +build ") && !strings.Contains(trimmed, "wasip1") {
			legacyConstraint := strings.TrimPrefix(trimmed, "// +build ")
			lines[i] = "// +build " + legacyConstraint + ",!wasip1"
		}
		if strings.HasPrefix(trimmed, "package ") {
			break
		}
	}

	if err := os.WriteFile(path, []byte(strings.Join(lines, "\n")), 0o644); err != nil {
		return err
	}

	fmt.Printf("  amended existing build tag in %s\n", relPath)
	return nil
}

// Unused but kept for reference — the printer-based approach for when we need
// full AST round-tripping.
var _ = printer.Fprint
var _ = token.NoPos

package main

import (
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
)

// applyOverlays copies all files from the overlays directory into the target.
func applyOverlays(ctx *Context) error {
	return filepath.WalkDir(ctx.OverlaysDir, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}

		rel, err := filepath.Rel(ctx.OverlaysDir, path)
		if err != nil {
			return err
		}

		dst := filepath.Join(ctx.TargetDir, rel)

		if ctx.DryRun {
			fmt.Printf("  [dry-run] copy %s\n", rel)
			return nil
		}

		if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
			return fmt.Errorf("mkdir %s: %w", filepath.Dir(dst), err)
		}

		if err := copyFile(path, dst); err != nil {
			return fmt.Errorf("copy %s: %w", rel, err)
		}

		fmt.Printf("  copied %s\n", rel)
		return nil
	})
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()

	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()

	_, err = io.Copy(out, in)
	return err
}

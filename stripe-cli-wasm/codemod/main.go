package main

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

func main() {
	target := flag.String("target", "", "Path to stripe-cli checkout (default: auto-detect ../stripe-cli)")
	verify := flag.Bool("verify", false, "After applying, run GOOS=wasip1 GOARCH=wasm go build")
	dryRun := flag.Bool("dry-run", false, "Report what would change without modifying files")
	flag.Parse()

	// Resolve target directory
	targetDir := *target
	if targetDir == "" {
		// Find the codemod directory (where this binary lives or where we're run from)
		exe, err := os.Executable()
		if err == nil {
			targetDir = filepath.Join(filepath.Dir(exe), "..", "stripe-cli")
		}
		// Fallback: relative to cwd
		if targetDir == "" || !dirExists(targetDir) {
			targetDir = filepath.Join("..", "stripe-cli")
		}
		// Also try sibling (when run via `go run ./codemod` from stripe-cli-wasm/)
		if !dirExists(targetDir) {
			targetDir = "stripe-cli"
		}
	}

	targetDir, err := filepath.Abs(targetDir)
	if err != nil {
		fatalf("failed to resolve target path: %v", err)
	}

	// Resolve overlays directory
	overlaysDir := resolveOverlaysDir()

	// Validate
	if !fileExists(filepath.Join(targetDir, "go.mod")) {
		fatalf("target %s does not contain go.mod — is the submodule initialized?", targetDir)
	}
	if !dirExists(overlaysDir) {
		fatalf("overlays directory not found at %s", overlaysDir)
	}

	fmt.Printf("stripe-cli-wasm codemod\n")
	fmt.Printf("  target:   %s\n", targetDir)
	fmt.Printf("  overlays: %s\n", overlaysDir)
	fmt.Printf("  dry-run:  %v\n\n", *dryRun)

	ctx := &Context{
		TargetDir:  targetDir,
		OverlaysDir: overlaysDir,
		DryRun:     *dryRun,
	}

	// Phase 1: Copy overlay files
	fmt.Println("=== Phase 1: Copy overlay files ===")
	if err := applyOverlays(ctx); err != nil {
		fatalf("overlay copy failed: %v", err)
	}

	// Phase 2: Inject build tags
	fmt.Println("\n=== Phase 2: Inject build tags ===")
	if err := applyBuildTags(ctx); err != nil {
		fatalf("build tag injection failed: %v", err)
	}

	// Phase 3: Extract functions and clean imports
	fmt.Println("\n=== Phase 3: Extract functions ===")
	if err := applyFunctionExtractions(ctx); err != nil {
		fatalf("function extraction failed: %v", err)
	}

	// Phase 4: Inline replacements
	fmt.Println("\n=== Phase 4: Inline replacements ===")
	if err := applyInlineReplacements(ctx); err != nil {
		fatalf("inline replacement failed: %v", err)
	}

	// Phase 5: Patch go.mod
	fmt.Println("\n=== Phase 5: Patch go.mod ===")
	if err := applyGoModPatches(ctx); err != nil {
		fatalf("go.mod patching failed: %v", err)
	}

	fmt.Println("\n=== Done ===")

	// Optional verification
	if *verify {
		fmt.Println("\n=== Verify: GOOS=wasip1 GOARCH=wasm go build ===")
		cmd := exec.Command("go", "build", "./cmd/stripe")
		cmd.Dir = targetDir
		cmd.Env = append(os.Environ(), "GOOS=wasip1", "GOARCH=wasm")
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		if err := cmd.Run(); err != nil {
			fatalf("verification build failed: %v", err)
		}
		fmt.Println("Build succeeded!")
	}
}

// Context holds shared state for all transforms.
type Context struct {
	TargetDir   string
	OverlaysDir string
	DryRun      bool
}

func resolveOverlaysDir() string {
	// Try relative to the Go source (for `go run ./codemod`)
	candidates := []string{
		filepath.Join("codemod", "overlays"),  // from stripe-cli-wasm/
		"overlays",                            // from codemod/
	}
	for _, c := range candidates {
		if abs, err := filepath.Abs(c); err == nil && dirExists(abs) {
			return abs
		}
	}
	// Fallback: relative to executable
	if exe, err := os.Executable(); err == nil {
		d := filepath.Join(filepath.Dir(exe), "overlays")
		if dirExists(d) {
			return d
		}
	}
	return "overlays"
}

func dirExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}

func fileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}

func fatalf(format string, args ...interface{}) {
	fmt.Fprintf(os.Stderr, "FATAL: "+format+"\n", args...)
	os.Exit(1)
}

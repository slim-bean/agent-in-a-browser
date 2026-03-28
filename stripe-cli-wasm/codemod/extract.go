package main

import (
	"bytes"
	"fmt"
	"go/ast"
	"go/parser"
	"go/printer"
	"go/token"
	"os"
	"path/filepath"

	"golang.org/x/tools/go/ast/astutil"
)

// applyFunctionExtractions removes functions from source files.
// The functions have already been placed in overlay files; this prevents
// duplicate symbol errors.
func applyFunctionExtractions(ctx *Context) error {
	for _, ext := range Manifest.FuncExtractions {
		if err := extractFunction(ctx, ext); err != nil {
			return err
		}
	}
	return nil
}

func extractFunction(ctx *Context, ext FuncExtraction) error {
	path := filepath.Join(ctx.TargetDir, ext.File)
	if !fileExists(path) {
		fmt.Printf("  [skip] %s (not found)\n", ext.File)
		return nil
	}

	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, path, nil, parser.ParseComments)
	if err != nil {
		return fmt.Errorf("parse %s: %w", ext.File, err)
	}

	// Find and remove the function declaration
	found := false
	var newDecls []ast.Decl
	for _, decl := range f.Decls {
		fn, ok := decl.(*ast.FuncDecl)
		if !ok {
			newDecls = append(newDecls, decl)
			continue
		}

		if fn.Name.Name != ext.FuncName {
			newDecls = append(newDecls, decl)
			continue
		}

		// Check receiver matches
		if ext.Receiver != "" {
			if fn.Recv == nil || len(fn.Recv.List) == 0 {
				newDecls = append(newDecls, decl)
				continue
			}
			recvType := receiverTypeString(fn.Recv.List[0].Type)
			if recvType != ext.Receiver {
				newDecls = append(newDecls, decl)
				continue
			}
		} else if fn.Recv != nil {
			// ext.Receiver is empty but function has a receiver — not our target
			newDecls = append(newDecls, decl)
			continue
		}

		found = true
		fmt.Printf("  removed func %s from %s\n", formatFuncName(ext), ext.File)

		// Remove the doc comment associated with this function from the file's comment list
		if fn.Doc != nil {
			removeCommentGroup(f, fn.Doc)
		}
	}

	if !found {
		fmt.Printf("  [skip] %s: func %s not found (already extracted?)\n", ext.File, formatFuncName(ext))
		return nil
	}

	if ctx.DryRun {
		return nil
	}

	f.Decls = newDecls

	// Write back
	var buf bytes.Buffer
	cfg := &printer.Config{Mode: printer.UseSpaces | printer.TabIndent, Tabwidth: 8}
	if err := cfg.Fprint(&buf, fset, f); err != nil {
		return fmt.Errorf("print %s: %w", ext.File, err)
	}

	return os.WriteFile(path, buf.Bytes(), 0o644)
}

// applyImportRemovals removes unused imports left behind after function extraction.
// Uses astutil.DeleteImport which correctly handles import groups.
func applyImportRemovals(ctx *Context, file string, imports []string) error {
	path := filepath.Join(ctx.TargetDir, file)
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, path, nil, parser.ParseComments)
	if err != nil {
		return fmt.Errorf("parse %s: %w", file, err)
	}

	for _, imp := range imports {
		if astutil.DeleteImport(fset, f, imp) {
			fmt.Printf("  removed import %q from %s\n", imp, file)
		}
	}

	ast.SortImports(fset, f)

	var buf bytes.Buffer
	cfg := &printer.Config{Mode: printer.UseSpaces | printer.TabIndent, Tabwidth: 8}
	if err := cfg.Fprint(&buf, fset, f); err != nil {
		return fmt.Errorf("print %s: %w", file, err)
	}

	if ctx.DryRun {
		return nil
	}

	return os.WriteFile(path, buf.Bytes(), 0o644)
}

func receiverTypeString(expr ast.Expr) string {
	switch t := expr.(type) {
	case *ast.StarExpr:
		if ident, ok := t.X.(*ast.Ident); ok {
			return "*" + ident.Name
		}
	case *ast.Ident:
		return t.Name
	}
	return ""
}

func formatFuncName(ext FuncExtraction) string {
	if ext.Receiver != "" {
		return "(" + ext.Receiver + ") " + ext.FuncName
	}
	return ext.FuncName
}

func removeCommentGroup(f *ast.File, target *ast.CommentGroup) {
	for i, cg := range f.Comments {
		if cg == target {
			f.Comments = append(f.Comments[:i], f.Comments[i+1:]...)
			return
		}
	}
}

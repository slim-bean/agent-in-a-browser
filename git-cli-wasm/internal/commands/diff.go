package commands

import (
	"fmt"

	"github.com/go-git/go-git/v5"
)

func cmdDiff(args []string) (int, error) {
	cached := false
	for _, a := range args {
		if a == "--cached" || a == "--staged" {
			cached = true
		}
	}

	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	if cached {
		return diffCached(repo)
	}

	return diffWorktree(repo)
}

func diffWorktree(repo *git.Repository) (int, error) {
	w, err := repo.Worktree()
	if err != nil {
		return 1, err
	}

	status, err := w.Status()
	if err != nil {
		return 1, err
	}

	if status.IsClean() {
		return 0, nil
	}

	// Get HEAD tree
	head, err := repo.Head()
	if err != nil {
		// No commits yet — nothing to diff against
		return 0, nil
	}

	commit, err := repo.CommitObject(head.Hash())
	if err != nil {
		return 1, err
	}

	headTree, err := commit.Tree()
	if err != nil {
		return 1, err
	}

	// Get worktree as a pseudo-tree by using the index
	// go-git doesn't have a direct worktree-to-tree diff, so we diff HEAD vs index
	idx, err := repo.Storer.Index()
	if err != nil {
		return 1, err
	}

	// Show changes for each modified file in status
	for file, s := range status {
		if s.Worktree == git.Modified || s.Worktree == git.Deleted {
			// Get the file from HEAD
			headFile, err := headTree.File(file)
			if err != nil {
				continue
			}
			headContent, err := headFile.Contents()
			if err != nil {
				continue
			}

			if s.Worktree == git.Deleted {
				fmt.Printf("diff --git a/%s b/%s\n", file, file)
				fmt.Println("deleted file mode 100644")
				fmt.Printf("--- a/%s\n", file)
				fmt.Println("+++ /dev/null")
				for _, line := range splitLines(headContent) {
					fmt.Printf("-%s\n", line)
				}
				continue
			}

			// Read worktree version
			wt, err := repo.Worktree()
			if err != nil {
				continue
			}
			f, err := wt.Filesystem.Open(file)
			if err != nil {
				continue
			}
			buf := make([]byte, 1024*1024)
			n, _ := f.Read(buf)
			f.Close()
			workContent := string(buf[:n])

			fmt.Printf("diff --git a/%s b/%s\n", file, file)
			fmt.Printf("--- a/%s\n", file)
			fmt.Printf("+++ b/%s\n", file)
			printSimpleDiff(headContent, workContent)
		}
	}

	_ = idx // suppress unused
	return 0, nil
}

func diffCached(repo *git.Repository) (int, error) {
	head, err := repo.Head()
	if err != nil {
		// No commits: all staged files are new
		return diffAgainstEmpty(repo)
	}

	commit, err := repo.CommitObject(head.Hash())
	if err != nil {
		return 1, err
	}

	headTree, err := commit.Tree()
	if err != nil {
		return 1, err
	}

	// Get the index tree
	w, err := repo.Worktree()
	if err != nil {
		return 1, err
	}

	status, err := w.Status()
	if err != nil {
		return 1, err
	}

	for file, s := range status {
		if s.Staging == git.Added || s.Staging == git.Modified || s.Staging == git.Deleted {
			fmt.Printf("diff --git a/%s b/%s\n", file, file)
			if s.Staging == git.Added {
				fmt.Println("new file mode 100644")
				fmt.Println("--- /dev/null")
				fmt.Printf("+++ b/%s\n", file)
			} else if s.Staging == git.Deleted {
				fmt.Println("deleted file mode 100644")
				fmt.Printf("--- a/%s\n", file)
				fmt.Println("+++ /dev/null")
			} else {
				fmt.Printf("--- a/%s\n", file)
				fmt.Printf("+++ b/%s\n", file)
			}

			// Show content diff
			var oldContent string
			if s.Staging != git.Added {
				hf, err := headTree.File(file)
				if err == nil {
					oldContent, _ = hf.Contents()
				}
			}

			var newContent string
			if s.Staging != git.Deleted {
				f, err := w.Filesystem.Open(file)
				if err == nil {
					buf := make([]byte, 1024*1024)
					n, _ := f.Read(buf)
					f.Close()
					newContent = string(buf[:n])
				}
			}

			printSimpleDiff(oldContent, newContent)
		}
	}

	return 0, nil
}

func diffAgainstEmpty(repo *git.Repository) (int, error) {
	w, err := repo.Worktree()
	if err != nil {
		return 1, err
	}

	status, err := w.Status()
	if err != nil {
		return 1, err
	}

	for file, s := range status {
		if s.Staging == git.Added {
			fmt.Printf("diff --git a/%s b/%s\n", file, file)
			fmt.Println("new file mode 100644")
			fmt.Println("--- /dev/null")
			fmt.Printf("+++ b/%s\n", file)

			f, err := w.Filesystem.Open(file)
			if err == nil {
				buf := make([]byte, 1024*1024)
				n, _ := f.Read(buf)
				f.Close()
				for _, line := range splitLines(string(buf[:n])) {
					fmt.Printf("+%s\n", line)
				}
			}
		}
	}

	return 0, nil
}

func splitLines(s string) []string {
	if s == "" {
		return nil
	}
	var lines []string
	start := 0
	for i := 0; i < len(s); i++ {
		if s[i] == '\n' {
			lines = append(lines, s[start:i])
			start = i + 1
		}
	}
	if start < len(s) {
		lines = append(lines, s[start:])
	}
	return lines
}

func printSimpleDiff(old, new string) {
	// Simple line-by-line diff (not a proper unified diff, but functional)
	oldLines := splitLines(old)
	newLines := splitLines(new)

	// Use go-diff for proper output if available, otherwise simple comparison
	_ = oldLines
	_ = newLines

	// For now, use go-git's built-in patch generation where possible
	// Fall through to a basic display
	if old == "" {
		for _, line := range newLines {
			fmt.Printf("+%s\n", line)
		}
		return
	}
	if new == "" {
		for _, line := range oldLines {
			fmt.Printf("-%s\n", line)
		}
		return
	}

	// Simple: show removed then added
	fmt.Printf("@@ -1,%d +1,%d @@\n", len(oldLines), len(newLines))
	for _, line := range oldLines {
		fmt.Printf("-%s\n", line)
	}
	for _, line := range newLines {
		fmt.Printf("+%s\n", line)
	}
}


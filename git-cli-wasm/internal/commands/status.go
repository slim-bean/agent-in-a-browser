package commands

import (
	"fmt"
	"sort"

	"github.com/go-git/go-git/v5"
)

func cmdStatus(args []string) (int, error) {
	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	w, err := repo.Worktree()
	if err != nil {
		return 1, err
	}

	status, err := w.Status()
	if err != nil {
		return 1, err
	}

	head, err := repo.Head()
	if err == nil {
		fmt.Printf("On branch %s\n", head.Name().Short())
	}

	if status.IsClean() {
		fmt.Println("nothing to commit, working tree clean")
		return 0, nil
	}

	// Collect staged and unstaged changes
	var staged, unstaged, untracked []string
	for file, s := range status {
		if s.Staging == git.Added || s.Staging == git.Modified || s.Staging == git.Deleted || s.Staging == git.Renamed || s.Staging == git.Copied {
			staged = append(staged, formatStatus(s.Staging, file))
		}
		if s.Worktree == git.Modified || s.Worktree == git.Deleted {
			unstaged = append(unstaged, formatStatus(s.Worktree, file))
		}
		if s.Worktree == git.Untracked && s.Staging == git.Untracked {
			untracked = append(untracked, file)
		}
	}

	sort.Strings(staged)
	sort.Strings(unstaged)
	sort.Strings(untracked)

	if len(staged) > 0 {
		fmt.Println("\nChanges to be committed:")
		fmt.Println("  (use \"git restore --staged <file>...\" to unstage)")
		for _, s := range staged {
			fmt.Printf("\t%s\n", s)
		}
	}

	if len(unstaged) > 0 {
		fmt.Println("\nChanges not staged for commit:")
		fmt.Println("  (use \"git add <file>...\" to update what will be committed)")
		for _, s := range unstaged {
			fmt.Printf("\t%s\n", s)
		}
	}

	if len(untracked) > 0 {
		fmt.Println("\nUntracked files:")
		fmt.Println("  (use \"git add <file>...\" to include in what will be committed)")
		for _, f := range untracked {
			fmt.Printf("\t%s\n", f)
		}
	}

	return 0, nil
}

func formatStatus(code git.StatusCode, file string) string {
	switch code {
	case git.Added:
		return "new file:   " + file
	case git.Modified:
		return "modified:   " + file
	case git.Deleted:
		return "deleted:    " + file
	case git.Renamed:
		return "renamed:    " + file
	case git.Copied:
		return "copied:     " + file
	default:
		return file
	}
}

func openRepo() (*git.Repository, error) {
	repo, err := git.PlainOpenWithOptions(".", &git.PlainOpenOptions{
		DetectDotGit: true,
	})
	if err != nil {
		return nil, fmt.Errorf("not a git repository (or any of the parent directories): .git")
	}
	return repo, nil
}

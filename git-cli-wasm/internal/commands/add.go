package commands

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/go-git/go-git/v5"
)

func cmdAdd(args []string) (int, error) {
	if len(args) == 0 {
		return 1, fmt.Errorf("Nothing specified, nothing added.\nMaybe you wanted to say 'git add .'?")
	}

	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	w, err := repo.Worktree()
	if err != nil {
		return 1, err
	}

	for _, pattern := range args {
		if pattern == "." || pattern == "-A" || pattern == "--all" {
			// Add all changes
			err = w.AddWithOptions(&git.AddOptions{All: true})
			if err != nil {
				return 1, err
			}
			continue
		}

		if strings.ContainsAny(pattern, "*?[") {
			// Glob pattern
			matches, err := filepath.Glob(pattern)
			if err != nil {
				return 1, fmt.Errorf("pathspec '%s': %w", pattern, err)
			}
			for _, m := range matches {
				_, err = w.Add(m)
				if err != nil {
					return 1, fmt.Errorf("error adding '%s': %w", m, err)
				}
			}
		} else {
			_, err = w.Add(pattern)
			if err != nil {
				return 1, fmt.Errorf("pathspec '%s' did not match any files", pattern)
			}
		}
	}

	return 0, nil
}

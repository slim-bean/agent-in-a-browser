package commands

import (
	"fmt"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
)

func cmdCheckout(args []string) (int, error) {
	if len(args) == 0 {
		return 1, fmt.Errorf("usage: git checkout [-b] <branch>")
	}

	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	createBranch := false
	var target string

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "-b":
			createBranch = true
		default:
			if args[i][0] != '-' {
				target = args[i]
			}
		}
	}

	if target == "" {
		return 1, fmt.Errorf("usage: git checkout [-b] <branch>")
	}

	w, err := repo.Worktree()
	if err != nil {
		return 1, err
	}

	if createBranch {
		err = w.Checkout(&git.CheckoutOptions{
			Branch: plumbing.NewBranchReferenceName(target),
			Create: true,
		})
	} else {
		// Try as branch first
		err = w.Checkout(&git.CheckoutOptions{
			Branch: plumbing.NewBranchReferenceName(target),
		})
		if err != nil {
			// Try as tag
			err = w.Checkout(&git.CheckoutOptions{
				Branch: plumbing.NewTagReferenceName(target),
			})
		}
		if err != nil {
			// Try as raw hash
			hash := plumbing.NewHash(target)
			if !hash.IsZero() {
				err = w.Checkout(&git.CheckoutOptions{
					Hash: hash,
				})
			}
		}
	}

	if err != nil {
		return 1, fmt.Errorf("error: pathspec '%s' did not match any file(s) known to git", target)
	}

	if createBranch {
		fmt.Printf("Switched to a new branch '%s'\n", target)
	} else {
		fmt.Printf("Switched to branch '%s'\n", target)
	}

	return 0, nil
}

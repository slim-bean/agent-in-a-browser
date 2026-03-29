package commands

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
)

func cmdInit(args []string) (int, error) {
	dir, _ := os.Getwd()
	bare := false
	branch := ""

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--bare":
			bare = true
		case "-b", "--initial-branch":
			if i+1 < len(args) {
				i++
				branch = args[i]
			}
		default:
			if args[i][0] != '-' {
				dir = args[i]
			}
		}
	}

	opts := &git.PlainInitOptions{Bare: bare}
	if branch != "" {
		opts.DefaultBranch = plumbing.NewBranchReferenceName(branch)
	}

	_, err := git.PlainInitWithOptions(dir, opts)
	if err != nil {
		return 1, err
	}

	absDir, _ := filepath.Abs(dir)
	if bare {
		fmt.Printf("Initialized empty Git repository in %s\n", absDir)
	} else {
		fmt.Printf("Initialized empty Git repository in %s/.git/\n", absDir)
	}
	return 0, nil
}

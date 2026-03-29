package commands

import (
	"fmt"
	"os"
	"path"
	"strings"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
)

func cmdClone(args []string) (int, error) {
	var url, dir, branch string
	depth := 0
	singleBranch := false

	for i := 0; i < len(args); i++ {
		switch {
		case args[i] == "--depth" && i+1 < len(args):
			i++
			fmt.Sscanf(args[i], "%d", &depth)
		case strings.HasPrefix(args[i], "--depth="):
			fmt.Sscanf(args[i][8:], "%d", &depth)
		case args[i] == "--single-branch":
			singleBranch = true
		case args[i] == "-b" || args[i] == "--branch":
			if i+1 < len(args) {
				i++
				branch = args[i]
			}
		case args[i][0] != '-' && url == "":
			url = args[i]
		case args[i][0] != '-' && dir == "":
			dir = args[i]
		}
	}

	if url == "" {
		return 1, fmt.Errorf("usage: git clone [--depth <depth>] [--single-branch] [-b <branch>] <url> [<dir>]")
	}

	if dir == "" {
		// Derive directory name from URL
		dir = path.Base(strings.TrimSuffix(url, ".git"))
	}

	opts := &git.CloneOptions{
		URL:      url,
		Progress: os.Stderr,
	}

	if depth > 0 {
		opts.Depth = depth
	}
	if singleBranch {
		opts.SingleBranch = true
	}
	if branch != "" {
		opts.ReferenceName = plumbing.NewBranchReferenceName(branch)
		opts.SingleBranch = true
	}

	fmt.Fprintf(os.Stderr, "Cloning into '%s'...\n", dir)
	_, err := git.PlainClone(dir, false, opts)
	if err != nil {
		return 1, err
	}

	return 0, nil
}

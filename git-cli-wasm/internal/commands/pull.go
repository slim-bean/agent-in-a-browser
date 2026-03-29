package commands

import (
	"fmt"
	"os"

	"github.com/go-git/go-git/v5"
)

func cmdPull(args []string) (int, error) {
	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	w, err := repo.Worktree()
	if err != nil {
		return 1, err
	}

	remoteName := "origin"
	for i := 0; i < len(args); i++ {
		if args[i][0] != '-' {
			remoteName = args[i]
			break
		}
	}

	err = w.Pull(&git.PullOptions{
		RemoteName: remoteName,
		Progress:   os.Stderr,
	})
	if err == git.NoErrAlreadyUpToDate {
		fmt.Println("Already up to date.")
		return 0, nil
	}
	if err != nil {
		return 1, err
	}

	return 0, nil
}

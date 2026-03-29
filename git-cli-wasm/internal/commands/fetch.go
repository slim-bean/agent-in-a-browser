package commands

import (
	"fmt"
	"os"

	"github.com/go-git/go-git/v5"
)

func cmdFetch(args []string) (int, error) {
	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	remoteName := "origin"
	all := false

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--all":
			all = true
		default:
			if args[i][0] != '-' {
				remoteName = args[i]
			}
		}
	}

	if all {
		remotes, err := repo.Remotes()
		if err != nil {
			return 1, err
		}
		for _, r := range remotes {
			fmt.Fprintf(os.Stderr, "Fetching %s\n", r.Config().Name)
			err = r.Fetch(&git.FetchOptions{
				Progress: os.Stderr,
			})
			if err != nil && err != git.NoErrAlreadyUpToDate {
				fmt.Fprintf(os.Stderr, "error fetching %s: %v\n", r.Config().Name, err)
			}
		}
		return 0, nil
	}

	err = repo.Fetch(&git.FetchOptions{
		RemoteName: remoteName,
		Progress:   os.Stderr,
	})
	if err == git.NoErrAlreadyUpToDate {
		return 0, nil
	}
	if err != nil {
		return 1, err
	}

	return 0, nil
}

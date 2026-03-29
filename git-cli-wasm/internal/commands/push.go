package commands

import (
	"fmt"
	"os"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
)

func cmdPush(args []string) (int, error) {
	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	remoteName := "origin"
	setUpstream := false
	var refspec string

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "-u", "--set-upstream":
			setUpstream = true
		case "--tags":
			refspec = "refs/tags/*:refs/tags/*"
		default:
			if args[i][0] != '-' {
				if remoteName == "origin" && i == 0 {
					remoteName = args[i]
				} else if refspec == "" {
					refspec = args[i]
				}
			}
		}
	}

	opts := &git.PushOptions{
		RemoteName: remoteName,
		Progress:   os.Stderr,
	}

	if refspec != "" {
		opts.RefSpecs = []config.RefSpec{config.RefSpec(refspec)}
	}

	err = repo.Push(opts)
	if err == git.NoErrAlreadyUpToDate {
		fmt.Println("Everything up-to-date")
		return 0, nil
	}
	if err != nil {
		return 1, err
	}

	if setUpstream {
		// Set upstream tracking (best effort)
		head, err := repo.Head()
		if err == nil && head.Name().IsBranch() {
			cfg, err := repo.Config()
			if err == nil {
				branchName := head.Name().Short()
				cfg.Branches[branchName] = &config.Branch{
					Name:   branchName,
					Remote: remoteName,
					Merge:  head.Name(),
				}
				_ = repo.SetConfig(cfg)
			}
		}
	}

	return 0, nil
}

package commands

import (
	"fmt"
	"os"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
)

func cmdCommit(args []string) (int, error) {
	message := ""
	all := false
	allowEmpty := false

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "-m", "--message":
			if i+1 < len(args) {
				i++
				message = args[i]
			}
		case "-a", "--all":
			all = true
		case "--allow-empty":
			allowEmpty = true
		}
	}

	if message == "" {
		return 1, fmt.Errorf("Aborting commit due to empty commit message.")
	}

	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	w, err := repo.Worktree()
	if err != nil {
		return 1, err
	}

	if all {
		// Stage all modified/deleted tracked files
		err = w.AddWithOptions(&git.AddOptions{All: true})
		if err != nil {
			return 1, err
		}
	}

	// Determine author from env or defaults
	authorName := os.Getenv("GIT_AUTHOR_NAME")
	if authorName == "" {
		authorName = os.Getenv("USER")
		if authorName == "" {
			authorName = "Anonymous"
		}
	}
	authorEmail := os.Getenv("GIT_AUTHOR_EMAIL")
	if authorEmail == "" {
		authorEmail = authorName + "@localhost"
	}

	opts := &git.CommitOptions{
		Author: &object.Signature{
			Name:  authorName,
			Email: authorEmail,
			When:  time.Now(),
		},
		AllowEmptyCommits: allowEmpty,
	}

	hash, err := w.Commit(message, opts)
	if err != nil {
		return 1, err
	}

	// Show summary
	head, _ := repo.Head()
	branchName := "HEAD"
	if head != nil {
		branchName = head.Name().Short()
	}
	fmt.Printf("[%s %s] %s\n", branchName, hash.String()[:7], message)

	return 0, nil
}

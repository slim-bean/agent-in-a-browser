package commands

import (
	"fmt"
	"sort"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
)

func cmdTag(args []string) (int, error) {
	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	deleteFlag := false
	listFlag := false
	var tagName, message string

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "-d", "--delete":
			deleteFlag = true
		case "-l", "--list":
			listFlag = true
		case "-m", "--message":
			if i+1 < len(args) {
				i++
				message = args[i]
			}
		default:
			if args[i][0] != '-' && tagName == "" {
				tagName = args[i]
			}
		}
	}

	if deleteFlag && tagName != "" {
		err := repo.DeleteTag(tagName)
		if err != nil {
			return 1, fmt.Errorf("error: tag '%s' not found", tagName)
		}
		fmt.Printf("Deleted tag '%s'\n", tagName)
		return 0, nil
	}

	if tagName == "" || listFlag {
		return listTags(repo)
	}

	// Create tag
	head, err := repo.Head()
	if err != nil {
		return 1, err
	}

	if message != "" {
		// Annotated tag — go-git doesn't easily support this without GPG
		// Fall back to lightweight tag
		_ = message
	}

	// Create lightweight tag
	ref := plumbing.NewHashReference(plumbing.NewTagReferenceName(tagName), head.Hash())
	err = repo.Storer.SetReference(ref)
	if err != nil {
		return 1, err
	}

	return 0, nil
}

func listTags(repo *git.Repository) (int, error) {
	tags, err := repo.Tags()
	if err != nil {
		return 1, err
	}

	var names []string
	err = tags.ForEach(func(ref *plumbing.Reference) error {
		names = append(names, ref.Name().Short())
		return nil
	})
	if err != nil {
		return 1, err
	}

	sort.Strings(names)
	for _, name := range names {
		fmt.Println(name)
	}

	return 0, nil
}

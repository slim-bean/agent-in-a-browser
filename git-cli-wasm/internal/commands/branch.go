package commands

import (
	"fmt"
	"sort"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
)

func cmdBranch(args []string) (int, error) {
	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	deleteFlag := false
	forceDelete := false
	var branchName string

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "-d", "--delete":
			deleteFlag = true
		case "-D":
			forceDelete = true
			deleteFlag = true
		default:
			if args[i][0] != '-' {
				branchName = args[i]
			}
		}
	}

	if deleteFlag && branchName != "" {
		return deleteBranch(repo, branchName, forceDelete)
	}

	if branchName != "" {
		return createBranch(repo, branchName)
	}

	return listBranches(repo)
}

func listBranches(repo *git.Repository) (int, error) {
	head, _ := repo.Head()
	currentBranch := ""
	if head != nil && head.Name().IsBranch() {
		currentBranch = head.Name().Short()
	}

	branches, err := repo.Branches()
	if err != nil {
		return 1, err
	}

	var names []string
	err = branches.ForEach(func(ref *plumbing.Reference) error {
		names = append(names, ref.Name().Short())
		return nil
	})
	if err != nil {
		return 1, err
	}

	sort.Strings(names)
	for _, name := range names {
		if name == currentBranch {
			fmt.Printf("* %s\n", name)
		} else {
			fmt.Printf("  %s\n", name)
		}
	}

	return 0, nil
}

func createBranch(repo *git.Repository, name string) (int, error) {
	head, err := repo.Head()
	if err != nil {
		return 1, err
	}

	ref := plumbing.NewHashReference(plumbing.NewBranchReferenceName(name), head.Hash())
	err = repo.Storer.SetReference(ref)
	if err != nil {
		return 1, err
	}

	return 0, nil
}

func deleteBranch(repo *git.Repository, name string, force bool) (int, error) {
	head, _ := repo.Head()
	if head != nil && head.Name().Short() == name {
		return 1, fmt.Errorf("Cannot delete branch '%s' checked out", name)
	}

	err := repo.DeleteBranch(name)
	if err != nil && !force {
		return 1, err
	}

	// Also remove the reference
	refName := plumbing.NewBranchReferenceName(name)
	err = repo.Storer.RemoveReference(refName)
	if err != nil {
		return 1, err
	}

	fmt.Printf("Deleted branch %s\n", name)
	return 0, nil
}


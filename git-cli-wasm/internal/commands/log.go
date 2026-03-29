package commands

import (
	"fmt"
	"strings"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
)

func cmdLog(args []string) (int, error) {
	maxCount := 0
	oneline := false

	for i := 0; i < len(args); i++ {
		switch {
		case args[i] == "--oneline":
			oneline = true
		case args[i] == "-n" && i+1 < len(args):
			i++
			fmt.Sscanf(args[i], "%d", &maxCount)
		case strings.HasPrefix(args[i], "-") && len(args[i]) > 1:
			// Check for -N shorthand
			var n int
			if _, err := fmt.Sscanf(args[i], "-%d", &n); err == nil {
				maxCount = n
			}
		}
	}

	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	opts := &git.LogOptions{
		Order: git.LogOrderCommitterTime,
	}

	iter, err := repo.Log(opts)
	if err != nil {
		return 1, err
	}

	count := 0
	err = iter.ForEach(func(c *object.Commit) error {
		if maxCount > 0 && count >= maxCount {
			return fmt.Errorf("stop")
		}
		count++

		if oneline {
			msg := strings.Split(c.Message, "\n")[0]
			fmt.Printf("%s %s\n", c.Hash.String()[:7], msg)
		} else {
			fmt.Printf("commit %s\n", c.Hash.String())
			fmt.Printf("Author: %s <%s>\n", c.Author.Name, c.Author.Email)
			fmt.Printf("Date:   %s\n", c.Author.When.Format("Mon Jan 2 15:04:05 2006 -0700"))
			fmt.Println()
			for _, line := range strings.Split(strings.TrimSpace(c.Message), "\n") {
				fmt.Printf("    %s\n", line)
			}
			fmt.Println()
		}

		return nil
	})

	// "stop" is our sentinel, not a real error
	if err != nil && err.Error() != "stop" {
		return 1, err
	}

	return 0, nil
}

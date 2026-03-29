package commands

import (
	"fmt"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
)

func cmdRemote(args []string) (int, error) {
	repo, err := openRepo()
	if err != nil {
		return 1, err
	}

	if len(args) == 0 {
		return remoteList(repo, false)
	}

	switch args[0] {
	case "-v", "--verbose":
		return remoteList(repo, true)
	case "add":
		if len(args) < 3 {
			return 1, fmt.Errorf("usage: git remote add <name> <url>")
		}
		return remoteAdd(repo, args[1], args[2])
	case "remove", "rm":
		if len(args) < 2 {
			return 1, fmt.Errorf("usage: git remote remove <name>")
		}
		return remoteRemove(repo, args[1])
	case "get-url":
		if len(args) < 2 {
			return 1, fmt.Errorf("usage: git remote get-url <name>")
		}
		return remoteGetURL(repo, args[1])
	case "set-url":
		if len(args) < 3 {
			return 1, fmt.Errorf("usage: git remote set-url <name> <newurl>")
		}
		return remoteSetURL(repo, args[1], args[2])
	default:
		return 1, fmt.Errorf("Unknown subcommand: git remote %s", args[0])
	}
}

func remoteList(repo *git.Repository, verbose bool) (int, error) {
	remotes, err := repo.Remotes()
	if err != nil {
		return 1, err
	}

	for _, r := range remotes {
		cfg := r.Config()
		if verbose {
			for _, url := range cfg.URLs {
				fmt.Printf("%s\t%s (fetch)\n", cfg.Name, url)
				fmt.Printf("%s\t%s (push)\n", cfg.Name, url)
			}
		} else {
			fmt.Println(cfg.Name)
		}
	}

	return 0, nil
}

func remoteAdd(repo *git.Repository, name, url string) (int, error) {
	_, err := repo.CreateRemote(&config.RemoteConfig{
		Name: name,
		URLs: []string{url},
	})
	if err != nil {
		return 1, err
	}
	return 0, nil
}

func remoteRemove(repo *git.Repository, name string) (int, error) {
	err := repo.DeleteRemote(name)
	if err != nil {
		return 1, fmt.Errorf("fatal: No such remote: '%s'", name)
	}
	return 0, nil
}

func remoteGetURL(repo *git.Repository, name string) (int, error) {
	remote, err := repo.Remote(name)
	if err != nil {
		return 1, fmt.Errorf("fatal: No such remote '%s'", name)
	}
	cfg := remote.Config()
	if len(cfg.URLs) > 0 {
		fmt.Println(cfg.URLs[0])
	}
	return 0, nil
}

func remoteSetURL(repo *git.Repository, name, newURL string) (int, error) {
	cfg, err := repo.Config()
	if err != nil {
		return 1, err
	}

	remoteCfg, ok := cfg.Remotes[name]
	if !ok {
		return 1, fmt.Errorf("fatal: No such remote '%s'", name)
	}

	if len(remoteCfg.URLs) > 0 {
		remoteCfg.URLs[0] = newURL
	} else {
		remoteCfg.URLs = []string{newURL}
	}

	err = repo.SetConfig(cfg)
	if err != nil {
		return 1, err
	}

	return 0, nil
}

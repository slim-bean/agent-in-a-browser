package commands

import (
	"fmt"
	"io"
)

func Run(subcommand string, args []string) (int, error) {
	switch subcommand {
	case "init":
		return cmdInit(args)
	case "clone":
		return cmdClone(args)
	case "status":
		return cmdStatus(args)
	case "add":
		return cmdAdd(args)
	case "commit":
		return cmdCommit(args)
	case "log":
		return cmdLog(args)
	case "branch":
		return cmdBranch(args)
	case "checkout":
		return cmdCheckout(args)
	case "diff":
		return cmdDiff(args)
	case "remote":
		return cmdRemote(args)
	case "tag":
		return cmdTag(args)
	case "fetch":
		return cmdFetch(args)
	case "pull":
		return cmdPull(args)
	case "push":
		return cmdPush(args)
	case "version":
		fmt.Println("git version 2.47.0 (go-git)")
		return 0, nil
	case "help", "--help", "-h":
		PrintUsage(nil)
		return 0, nil
	default:
		return 1, fmt.Errorf("git: '%s' is not a git command. See 'git help'.", subcommand)
	}
}

func PrintUsage(w io.Writer) {
	if w == nil {
		w = io.Discard
	}
	fmt.Println(`usage: git <command> [<args>]

These are common Git commands:

start a working area
   clone      Clone a repository into a new directory
   init       Create an empty Git repository

work on the current change
   add        Add file contents to the index

examine the history and state
   diff       Show changes between commits, commit and working tree, etc
   log        Show commit logs
   status     Show the working tree status

grow, mark and tweak your common history
   branch     List, create, or delete branches
   checkout   Switch branches or restore working tree files
   commit     Record changes to the repository
   tag        Create, list, or delete tags

collaborate
   fetch      Download objects and refs from another repository
   pull       Fetch from and integrate with another repository
   push       Update remote refs along with associated objects
   remote     Manage set of tracked repositories`)
}

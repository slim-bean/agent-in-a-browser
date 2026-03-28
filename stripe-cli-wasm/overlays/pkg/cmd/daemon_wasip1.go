//go:build wasip1

package cmd

import (
	"github.com/spf13/cobra"

	"github.com/stripe/stripe-cli/pkg/config"
)

type daemonCmd struct {
	cmd *cobra.Command
}

func newDaemonCmd(cfg *config.Config) *daemonCmd {
	return &daemonCmd{
		cmd: &cobra.Command{
			Use:    "daemon",
			Short:  "Run as a daemon (not supported in WASM)",
			Hidden: true,
			RunE: func(cmd *cobra.Command, args []string) error {
				cmd.Println("daemon mode is not supported in WASM")
				return nil
			},
		},
	}
}

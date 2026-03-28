//go:build wasip1

package samples

import (
	"github.com/spf13/cobra"

	"github.com/stripe/stripe-cli/pkg/config"
)

type CreateCmd struct {
	cfg *config.Config
	Cmd *cobra.Command
}

func NewCreateCmd(config *config.Config) *CreateCmd {
	return &CreateCmd{
		cfg: config,
		Cmd: &cobra.Command{
			Use:   "create",
			Short: "Setup and bootstrap a Stripe Sample (not supported in WASM)",
			RunE: func(cmd *cobra.Command, args []string) error {
				cmd.Println("samples create is not supported in WASM mode")
				return nil
			},
		},
	}
}

type ListCmd struct {
	Cmd *cobra.Command
}

func NewListCmd() *ListCmd {
	return &ListCmd{
		Cmd: &cobra.Command{
			Use:   "list",
			Short: "List Stripe Samples (not supported in WASM)",
			RunE: func(cmd *cobra.Command, args []string) error {
				cmd.Println("samples list is not supported in WASM mode")
				return nil
			},
		},
	}
}

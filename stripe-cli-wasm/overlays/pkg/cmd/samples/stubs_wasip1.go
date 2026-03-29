//go:build wasip1

// Package samples provides CLI commands for managing Stripe samples.
// WASM version: uses flag-based selection instead of interactive promptui.
package samples

import (
	"fmt"
	"os"
	"sort"

	"github.com/spf13/cobra"

	"github.com/stripe/stripe-cli/pkg/ansi"
	"github.com/stripe/stripe-cli/pkg/config"
	"github.com/stripe/stripe-cli/pkg/samples"
	"github.com/stripe/stripe-cli/pkg/validators"
)

// CreateCmd wraps the `create` command for samples which generates a new project.
// WASM version: uses flags instead of interactive prompts.
type CreateCmd struct {
	cfg *config.Config
	Cmd *cobra.Command

	forceRefresh bool
	integration  string
	client       string
	server       string
}

// NewCreateCmd creates and returns a create command for samples
func NewCreateCmd(config *config.Config) *CreateCmd {
	createCmd := &CreateCmd{
		cfg:          config,
		forceRefresh: false,
	}
	createCmd.Cmd = &cobra.Command{
		Use:   "create <sample> [destination]",
		Args:  validators.MaximumNArgs(2),
		Short: "Setup and bootstrap a Stripe Sample",
		Long: `The create command will locally clone a sample, let you select which integration,
client, and server you want to run. It then automatically bootstraps the
local configuration to let you get started faster.

In WASM mode, use --integration, --client, and --server flags instead of
interactive prompts.`,
		Example: `stripe samples create accept-a-payment
  stripe samples create accept-a-payment my-dir --integration using-webhooks --server node`,
		RunE: createCmd.runCreateCmd,
	}

	createCmd.Cmd.Flags().BoolVar(&createCmd.forceRefresh, "force-refresh", false, "Forcefully refresh the local samples cache")
	createCmd.Cmd.Flags().StringVar(&createCmd.integration, "integration", "", "Integration to use (required if sample has multiple)")
	createCmd.Cmd.Flags().StringVar(&createCmd.client, "client", "", "Client language")
	createCmd.Cmd.Flags().StringVar(&createCmd.server, "server", "", "Server language")

	return createCmd
}

func (cc *CreateCmd) runCreateCmd(cmd *cobra.Command, args []string) error {
	if len(args) == 0 {
		cmd.Help()
		return nil
	}

	selectedSample := args[0]
	destination := selectedSample
	if len(args) > 1 {
		destination = args[1]
	}

	color := ansi.Color(os.Stdout)
	fmt.Printf("Downloading %s...\n", selectedSample)

	sampleManager, err := samples.NewSampleManager(cc.cfg)
	if err != nil {
		return err
	}

	sampleConfig, err := sampleManager.GetSampleConfig(selectedSample, cc.forceRefresh)
	if err != nil {
		return err
	}
	fmt.Printf("%s %s\n", color.Green("✔"), ansi.Faint("Finished downloading"))

	// Select config via flags (no interactive prompts in WASM)
	selectedConfig, err := cc.selectConfig(sampleConfig)
	if err != nil {
		return err
	}

	resultChan := make(chan samples.CreationResult)

	go sampleManager.Create(
		cmd.Context(),
		selectedSample,
		selectedConfig,
		destination,
		cc.forceRefresh,
		resultChan,
	)

	for res := range resultChan {
		if res.Err != nil {
			return res.Err
		}

		switch res.State {
		case samples.WillInitialize:
		case samples.DidInitialize:
		case samples.WillCopy:
			fmt.Printf("Copying files over... %s\n", destination)
		case samples.DidCopy:
			fmt.Printf("%s %s\n", color.Green("✔"), ansi.Faint("Files copied"))
		case samples.WillConfigure:
			fmt.Printf("Configuring your code... %s\n", selectedSample)
		case samples.DidConfigure:
			fmt.Printf("%s %s\n", color.Green("✔"), ansi.Faint("Project configured"))
		case samples.DidConfigureWithoutTestPubKey:
			fmt.Printf("%s %s\n", color.Green("⚠️"), ansi.Faint("Project configured without testmode publishable key"))
		case samples.Done:
			fmt.Println("You're all set. To get started: cd", destination)
			if res.PostInstall != "" {
				fmt.Println(res.PostInstall)
			}
		}
	}

	return nil
}

func (cc *CreateCmd) selectConfig(sampleConfig *samples.SampleConfig) (*samples.SelectedConfig, error) {
	var selectedConfig samples.SelectedConfig

	if sampleConfig.HasIntegrations() {
		if cc.integration == "" {
			names := sampleConfig.IntegrationNames()
			return nil, fmt.Errorf("this sample has multiple integrations. Use --integration flag.\nAvailable: %v", names)
		}
		for i, integration := range sampleConfig.Integrations {
			if integration.Name == cc.integration {
				selectedConfig.Integration = &sampleConfig.Integrations[i]
				break
			}
		}
		if selectedConfig.Integration == nil {
			return nil, fmt.Errorf("integration '%s' not found", cc.integration)
		}
	} else {
		selectedConfig.Integration = &sampleConfig.Integrations[0]
	}

	if selectedConfig.Integration.HasMultipleClients() {
		if cc.client == "" {
			return nil, fmt.Errorf("this sample has multiple clients. Use --client flag.\nAvailable: %v", selectedConfig.Integration.Clients)
		}
		selectedConfig.Client = cc.client
	}

	if selectedConfig.Integration.HasMultipleServers() {
		if cc.server == "" {
			return nil, fmt.Errorf("this sample has multiple servers. Use --server flag.\nAvailable: %v", selectedConfig.Integration.Servers)
		}
		selectedConfig.Server = cc.server
	}

	return &selectedConfig, nil
}

// ListCmd prints a list of all the available sample projects
type ListCmd struct {
	Cmd *cobra.Command
}

// NewListCmd creates and returns a list command for samples
func NewListCmd() *ListCmd {
	listCmd := &ListCmd{}
	listCmd.Cmd = &cobra.Command{
		Use:   "list",
		Args:  validators.NoArgs,
		Short: "List Stripe Samples supported by the CLI",
		Long:  `A list of available Stripe Sample integrations that can be setup and bootstrapped by the CLI.`,
		RunE:  listCmd.runListCmd,
	}

	return listCmd
}

func (lc *ListCmd) runListCmd(cmd *cobra.Command, args []string) error {
	fmt.Println("A list of available Stripe Samples:")
	fmt.Println()

	fmt.Println("Loading...")

	list, err := samples.GetSamples("list")
	if err != nil {
		fmt.Println("Error: please check your internet connection and try again!")
		return err
	}

	names := samples.Names(list)
	sort.Strings(names)

	for _, name := range names {
		fmt.Println(list[name].BoldName())
		fmt.Println(list[name].Description)
		fmt.Printf("Repo: %s\n", list[name].URL)
		fmt.Println()
	}

	return nil
}

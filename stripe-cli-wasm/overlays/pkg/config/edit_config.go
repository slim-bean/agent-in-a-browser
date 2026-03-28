//go:build !wasip1

package config

import (
	"fmt"

	"github.com/stripe/stripe-cli/pkg/git"
)

// EditConfig opens the configuration file in the default editor.
func (c *Config) EditConfig() error {
	fmt.Println("Opening config file:", c.ProfilesFile)

	editor, err := git.NewEditor(c.ProfilesFile)
	if err != nil {
		return err
	}

	_, err = editor.EditContent()
	return err
}

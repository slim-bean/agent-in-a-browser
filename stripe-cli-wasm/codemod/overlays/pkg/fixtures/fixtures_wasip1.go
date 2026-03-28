//go:build wasip1

package fixtures

import (
	"context"
	"fmt"

	"github.com/spf13/afero"
)

const SupportedVersions = 0

type MetaFixture struct {
	Version         int  `json:"template_version"`
	ExcludeMetadata bool `json:"exclude_metadata"`
}

type FixtureData struct {
	Meta     MetaFixture       `json:"_meta"`
	Requests []FixtureRequest  `json:"fixtures"`
	Env      map[string]string `json:"env"`
}

type FixtureRequest struct {
	Name              string                 `json:"name"`
	ExpectedErrorType string                 `json:"expected_error_type"`
	Path              string                 `json:"path"`
	Method            string                 `json:"method"`
	Params            map[string]interface{} `json:"params"`
	IdempotencyKey    string                 `json:"idempotency_key,omitempty"`
	Context           string                 `json:"context,omitempty"`
	APIBase           string                 `json:"api_base,omitempty"`
	Headers           map[string]string      `json:"headers,omitempty"`
}

type Fixture struct {
	Fs            afero.Fs
	APIKey        string
	StripeAccount string
}

func NewFixtureFromFile(fs afero.Fs, apiKey, stripeAccount, apiBaseURL, jsonFile string, skip, override, add, remove []string, edit bool) (*Fixture, error) {
	return nil, fmt.Errorf("fixtures are not supported in WASM mode")
}

func NewFixtureFromRawString(fs afero.Fs, apiKey, stripeAccount, apiBaseURL, raw string) (*Fixture, error) {
	return nil, fmt.Errorf("fixtures are not supported in WASM mode")
}

func (f *Fixture) Execute(ctx context.Context, apiVersion string) ([]string, error) {
	return nil, fmt.Errorf("fixtures are not supported in WASM mode")
}

func (f *Fixture) UpdateEnv() error {
	return fmt.Errorf("fixtures are not supported in WASM mode")
}

// Events is a mapping of trigger events to fixture files
var Events = map[string]string{}

func EventNames() []string {
	return []string{}
}

func EventList() string {
	return "  (fixtures not available in WASM mode)\n"
}

func Trigger(ctx context.Context, event string, stripeAccount string, baseURL string, apiKey string, skip, override, add, remove []string, raw string, apiVersion string, edit bool) ([]string, error) {
	return nil, fmt.Errorf("fixtures/triggers are not supported in WASM mode")
}

func BuildFromFixtureFile(fs afero.Fs, apiKey, stripeAccount, apiBaseURL, jsonFile string, skip, override, add, remove []string, edit bool) (*Fixture, error) {
	return nil, fmt.Errorf("fixtures are not supported in WASM mode")
}

func BuildFromFixtureString(fs afero.Fs, apiKey, stripeAccount, apiBaseURL, raw string) (*Fixture, error) {
	return nil, fmt.Errorf("fixtures are not supported in WASM mode")
}

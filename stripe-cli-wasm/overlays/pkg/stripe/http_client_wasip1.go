//go:build wasip1

package stripe

import (
	"net/http"

	"github.com/stripe/stripe-cli/pkg/wasmbridge"
)

func newHTTPClient(verbose bool, printableHeaders []string, unixSocket string) *http.Client {
	return &http.Client{
		Transport: &wasmbridge.Transport{},
	}
}

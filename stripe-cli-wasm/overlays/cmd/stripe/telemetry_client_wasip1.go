//go:build wasip1

package main

import (
	"net/http"

	"github.com/stripe/stripe-cli/pkg/wasmbridge"
)

func newTelemetryHTTPClient() *http.Client {
	return &http.Client{
		Transport: &wasmbridge.Transport{},
	}
}

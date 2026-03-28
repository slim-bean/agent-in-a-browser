//go:build !wasip1

package main

import (
	"net/http"
	"time"
)

func newTelemetryHTTPClient() *http.Client {
	return &http.Client{
		Timeout: time.Second * 3,
	}
}

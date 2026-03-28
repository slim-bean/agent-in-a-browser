//go:build !wasip1

package stripe

import (
	"context"
	"net"
	"net/http"
	"os"
	"time"
)

func newHTTPClient(verbose bool, printableHeaders []string, unixSocket string) *http.Client {
	var httpTransport http.RoundTripper

	if unixSocket != "" {
		dialFunc := func(network, addr string) (net.Conn, error) {
			return net.Dial("unix", unixSocket)
		}
		dialContext := func(_ context.Context, _, _ string) (net.Conn, error) {
			return net.Dial("unix", unixSocket)
		}
		httpTransport = &http.Transport{
			DialContext:           dialContext,
			DialTLS:               dialFunc,
			ResponseHeaderTimeout: 30 * time.Second,
			ExpectContinueTimeout: 10 * time.Second,
			TLSHandshakeTimeout:   10 * time.Second,
		}
	} else {
		httpTransport = &http.Transport{
			Proxy: http.ProxyFromEnvironment,
			DialContext: (&net.Dialer{
				Timeout:   30 * time.Second,
				KeepAlive: 30 * time.Second,
			}).DialContext,
			TLSHandshakeTimeout: 10 * time.Second,
		}
	}

	if verbose {
		if printableHeaders == nil {
			printableHeaders = inspectHeaders
		}

		httpTransport = &verboseTransport{
			Transport:        httpTransport,
			Out:              os.Stderr,
			PrintableHeaders: printableHeaders,
		}
	}

	return &http.Client{
		Transport: httpTransport,
	}
}

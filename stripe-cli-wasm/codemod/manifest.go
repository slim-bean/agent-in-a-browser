package main

// Manifest defines all modifications the codemod applies to a stripe-cli checkout.
// This is the single source of truth — update this when upstream changes require
// new exclusions, stubs, or extractions.

// BuildTagExclusion describes a file that needs `//go:build !wasip1` prepended.
type BuildTagExclusion struct {
	// File is a path relative to the stripe-cli root.
	// Mutually exclusive with Glob.
	File string
	// Glob is a glob pattern relative to the stripe-cli root.
	// All matching .go files (minus Exclude patterns) get the tag.
	Glob string
	// Exclude patterns within a Glob match (e.g., "*_test.go").
	Exclude []string
}

// BuildTagAmendment describes an existing build tag that needs `!wasip1` added.
type BuildTagAmendment struct {
	File string
}

// FuncExtraction describes a function to remove from a source file.
// The function body is already provided in an overlay file; this just removes
// the original from the source so Go doesn't see duplicate definitions.
type FuncExtraction struct {
	File     string // Relative path to the Go source file
	FuncName string // Function name to remove
	Receiver string // Receiver type (e.g., "*Config"), empty for package-level functions
}

// InlineReplacement describes a targeted text replacement in a source file.
type InlineReplacement struct {
	File string
	From string
	To   string
}

// ImportRemoval describes an import to remove from a file after function extraction.
type ImportRemoval struct {
	File       string
	ImportPath string
}

// GoModReplace describes a replace directive to add to go.mod.
type GoModReplace struct {
	Module      string // e.g., "github.com/sirupsen/logrus"
	Replacement string // e.g., "../patches/logrus"
	Comment     string
}

// Spec is the complete codemod specification.
type Spec struct {
	BuildTagExclusions  []BuildTagExclusion
	BuildTagAmendments  []BuildTagAmendment
	FuncExtractions     []FuncExtraction
	InlineReplacements  []InlineReplacement
	ImportRemovals      []ImportRemoval
	GoModReplaces       []GoModReplace
}

// Manifest is the complete specification of all modifications needed to make
// stripe-cli compile under GOOS=wasip1 GOARCH=wasm.
var Manifest = Spec{
	BuildTagExclusions: []BuildTagExclusion{
		// pkg/rpcservice/ — gRPC server (requires network syscalls)
		{File: "pkg/rpcservice/events_resend.go"},
		{File: "pkg/rpcservice/fixtures.go"},
		{File: "pkg/rpcservice/listen.go"},
		{File: "pkg/rpcservice/login.go"},
		{File: "pkg/rpcservice/login_status.go"},
		{File: "pkg/rpcservice/logs_tail.go"},
		{File: "pkg/rpcservice/middleware.go"},
		{File: "pkg/rpcservice/rpc_service.go"},
		{File: "pkg/rpcservice/sample_configs.go"},
		{File: "pkg/rpcservice/sample_create.go"},
		{File: "pkg/rpcservice/samples_list.go"},
		{File: "pkg/rpcservice/trigger.go"},
		{File: "pkg/rpcservice/triggers_list.go"},
		{File: "pkg/rpcservice/version.go"},
		{File: "pkg/rpcservice/webhook_endpoint_create.go"},
		{File: "pkg/rpcservice/webhook_endpoints_list.go"},

		// pkg/cmd/ — commands that require subprocess/network
		{File: "pkg/cmd/daemon.go"},
		{File: "pkg/cmd/resource/terminal.go"},
		{File: "pkg/cmd/resource/terminal_quickstart.go"},
		{File: "pkg/cmd/samples/create.go"},
		{File: "pkg/cmd/samples/list.go"},

		// pkg/fixtures/ — requires file I/O patterns unavailable in WASI
		{File: "pkg/fixtures/fixtures.go"},
		{File: "pkg/fixtures/triggers.go"},

		// pkg/git/ — requires subprocess (git, editor)
		{File: "pkg/git/editor.go"},
		{File: "pkg/git/git.go"},

		// pkg/samples/ — requires git/subprocess
		{File: "pkg/samples/create.go"},
		{File: "pkg/samples/list.go"},
		{File: "pkg/samples/os.go"},
		{File: "pkg/samples/samples.go"},

		// pkg/terminal/ — hardware terminal interactions
		{File: "pkg/terminal/p400/user_prompts.go"},
		{File: "pkg/terminal/quickstart_p400.go"},
		{File: "pkg/terminal/user_prompts.go"},
	},

	BuildTagAmendments: []BuildTagAmendment{
		// Existing !windows tag needs !wasip1 added
		{File: "pkg/useragent/uname_unix.go"},
	},

	FuncExtractions: []FuncExtraction{
		// newHTTPClient extracted to http_client.go / http_client_wasip1.go
		{File: "pkg/stripe/client.go", FuncName: "newHTTPClient"},
		// EditConfig extracted to edit_config.go / edit_config_wasip1.go
		{File: "pkg/config/config.go", FuncName: "EditConfig", Receiver: "*Config"},
	},

	InlineReplacements: []InlineReplacement{
		// Replace inline http.Client creation with extracted function call
		{
			File: "cmd/stripe/main.go",
			From: "httpClient := &http.Client{\n\t\t\tTimeout: time.Second * 3,\n\t\t}",
			To:   "httpClient := newTelemetryHTTPClient()",
		},
	},

	ImportRemovals: []ImportRemoval{
		// After extracting newHTTPClient, these imports are no longer needed in client.go
		{File: "pkg/stripe/client.go", ImportPath: "net"},
		{File: "pkg/stripe/client.go", ImportPath: "time"},
		// After extracting EditConfig, this import is no longer needed in config.go
		{File: "pkg/config/config.go", ImportPath: "github.com/stripe/stripe-cli/pkg/git"},
		// After inline replacement in main.go
		{File: "cmd/stripe/main.go", ImportPath: "net/http"},
		{File: "cmd/stripe/main.go", ImportPath: "time"},
	},

	GoModReplaces: []GoModReplace{
		{
			Module:      "github.com/sirupsen/logrus",
			Replacement: "../patches/logrus",
			Comment:     "wasip1 WASM compatibility patches",
		},
	},
}

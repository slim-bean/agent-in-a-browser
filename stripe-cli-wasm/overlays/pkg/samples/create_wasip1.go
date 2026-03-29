//go:build wasip1

package samples

import (
	"context"
	"fmt"
	"os"

	log "github.com/sirupsen/logrus"

	"github.com/stripe/stripe-cli/pkg/validators"

	"github.com/spf13/afero"

	"github.com/go-git/go-git/v5"
)

// CreationStatus is the current step in the sample creation routine
type CreationStatus int

const (
	// WillInitialize means this sample will be initialized
	WillInitialize CreationStatus = iota

	// DidInitialize means this sample has finished initializing
	DidInitialize

	// WillCopy means the downloaded sample will be copied to the target path
	WillCopy

	// DidCopy means the downloaded sample has finished being copied to the target path
	DidCopy

	// WillConfigure means the .env of the sample will be configured with the user's Stripe account details
	WillConfigure

	// DidConfigure means the .env of the sample has finished being configured with the user's Stripe account details
	DidConfigure

	// DidConfigureWithoutTestPubKey means the .env of the sample has finished being configured without the publishable key
	DidConfigureWithoutTestPubKey

	// Done means sample creation is complete
	Done
)

// CreationResult is the return value sent over a channel
type CreationResult struct {
	State       CreationStatus
	Path        string
	PostInstall string
	Err         error
}

// Create creates a sample at a destination with the selected integration, client language, and server language.
// WASM version: identical to native but without os.Signal/signal.Notify handling (WASI doesn't support signals).
func (s *SampleManager) Create(
	ctx context.Context,
	sampleName string,
	selectedConfig *SelectedConfig,
	destination string,
	forceRefresh bool,
	resultChan chan<- CreationResult,
) {
	defer close(resultChan)

	cacheFolder, err := s.appCacheFolder("samples-list")
	if err != nil {
		logger := log.Logger{
			Out: os.Stdout,
		}

		logger.WithFields(log.Fields{
			"prefix": "samples.create.cacheFolder",
			"error":  err,
		}).Debug("Could not create cacheFolder for samples")
	}
	s.SampleLister = newCachedGithubSampleLister(s, sampleListGithubURL, cacheFolder)

	exists, _ := afero.DirExists(s.Fs, destination)
	if exists {
		resultChan <- CreationResult{Err: fmt.Errorf("path already exists for: %s", destination)}
		return
	}

	if forceRefresh {
		err := s.DeleteCache(sampleName)
		if err != nil {
			logger := log.Logger{
				Out: os.Stdout,
			}

			logger.WithFields(log.Fields{
				"prefix": "samples.create.forceRefresh",
				"error":  err,
			}).Debug("Could not clear cache")
		}
	}

	resultChan <- CreationResult{State: WillInitialize}

	err = s.Initialize(sampleName)
	if err != nil {
		switch e := err.Error(); e {
		case git.NoErrAlreadyUpToDate.Error():
			break
		case git.ErrRepositoryAlreadyExists.Error():
			break
		default:
			resultChan <- CreationResult{Err: err}
			return
		}
	}

	resultChan <- CreationResult{State: DidInitialize}

	s.SelectedConfig = *selectedConfig

	// NOTE: os.Signal/signal.Notify removed for WASM — WASI doesn't support signals

	resultChan <- CreationResult{State: WillCopy}

	targetPath, err := s.MakeFolder(destination)
	if err != nil {
		resultChan <- CreationResult{Err: err}
		return
	}

	err = s.Copy(targetPath)
	if err != nil {
		resultChan <- CreationResult{Err: err}
		return
	}

	resultChan <- CreationResult{State: DidCopy}

	resultChan <- CreationResult{State: WillConfigure}

	err = s.WriteDotEnv(ctx, targetPath)
	if err != nil {
		if err == validators.ErrPubKeyNotConfigured {
			resultChan <- CreationResult{State: DidConfigureWithoutTestPubKey}
		} else {
			resultChan <- CreationResult{Err: err}
			return
		}
	} else {
		resultChan <- CreationResult{State: DidConfigure}
	}

	resultChan <- CreationResult{State: Done, Path: targetPath, PostInstall: s.PostInstall()}
}

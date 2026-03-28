//go:build !wasip1

package fixtures

import (
	"strings"

	"github.com/stripe/stripe-cli/pkg/git"
)

// Edit is separated into a var so we can mock this in fixtures_test.
// On non-WASM platforms, it launches the user's default editor via git.
var Edit = func(path string, filedata []byte) ([]byte, error) {
	filename := getFixtureFilenameWithWildcard(path)
	editor, err := git.NewTemporaryFileEditor(filename, filedata)
	if err != nil {
		return nil, err
	}

	return editor.EditContent()
}

func getFixtureFilenameWithWildcard(path string) string {
	pathComponents := strings.Split(path, "/")
	fixtureName := strings.Split(pathComponents[len(pathComponents)-1], ".")
	// Add a wildcard that is replaced by a random string when passing this filename to os.CreateTemp
	return strings.Join(fixtureName[0:len(fixtureName)-1], ".") + ".*." + fixtureName[len(fixtureName)-1]
}

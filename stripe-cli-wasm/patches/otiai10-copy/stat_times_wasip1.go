//go:build wasip1

package copy

import (
	"os"
)

func getTimeSpec(info os.FileInfo) timespec {
	return timespec{
		Mtime: info.ModTime(),
		Atime: info.ModTime(),
		Ctime: info.ModTime(),
	}
}

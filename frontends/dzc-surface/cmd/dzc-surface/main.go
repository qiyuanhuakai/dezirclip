package main

import (
	"encoding/json"
	"fmt"
	"os"

	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/quickpaste"
)

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--build-info" {
		if json.NewEncoder(os.Stdout).Encode(buildinfo.Report()) != nil {
			os.Exit(1)
		}
		return
	}
	if len(os.Args) != 1 {
		fmt.Fprintln(os.Stderr, "invalid surface arguments")
		os.Exit(2)
	}
	if err := quickpaste.Run(); err != nil {
		fmt.Fprintln(os.Stderr, "surface initialization failed")
		os.Exit(1)
	}
}

package uikit

import (
	"embed"

	"github.com/egoist/mygo/ui"
)

//go:embed icons/*.svg
var icons embed.FS

func icon(name string) *ui.SVG {
	data, err := icons.ReadFile("icons/" + name + ".svg")
	if err != nil {
		panic(err)
	}
	return ui.MustParseSVG(data)
}

var (
	chevronDown  = icon("ChevronDown")
	chevronRight = icon("ChevronRight")
	circleHelp   = icon("CircleHelp")
	closeIcon    = icon("X")
)

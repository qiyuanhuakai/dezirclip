package main

import (
	"embed"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"log"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

//go:embed character-assets
var characterAssets embed.FS

type characterPreviewManifest struct {
	Flags   []string
	Columns int
	Tile    int
	Fonts   []struct {
		Asset      string
		Family     string
		Characters string
	}
}

var characterPreviews = sync.OnceValue(func() characterPreviewManifest {
	data, err := characterAssets.ReadFile("character-assets/previews.json")
	if err != nil {
		panic(err)
	}
	var manifest characterPreviewManifest
	if err := json.Unmarshal(data, &manifest); err != nil {
		panic(err)
	}
	return manifest
})

type flagAtlasImage interface {
	image.Image
	SubImage(image.Rectangle) image.Image
}

var flagAtlas = sync.OnceValue(func() flagAtlasImage {
	file, err := characterAssets.Open("character-assets/flags.png")
	if err != nil {
		log.Printf("Character preview atlas: %v", err)
		return nil
	}
	defer file.Close()
	bitmap, err := png.Decode(file)
	if err != nil {
		log.Printf("Character preview atlas: %v", err)
		return nil
	}
	atlas, _ := bitmap.(flagAtlasImage)
	return atlas
})

var flagPreviews = sync.OnceValue(func() map[string]*uikit.CharacterPreview {
	manifest := characterPreviews()
	flags := make(map[string]*uikit.CharacterPreview, len(manifest.Flags))
	for i, value := range manifest.Flags {
		x := i % manifest.Columns * manifest.Tile
		y := i / manifest.Columns * manifest.Tile
		bounds := image.Rect(x, y, x+manifest.Tile, y+manifest.Tile)
		flags[value] = &uikit.CharacterPreview{Bitmap: sync.OnceValue(func() *ui.Bitmap {
			if atlas := flagAtlas(); atlas != nil {
				return ui.NewBitmap(atlas.SubImage(bounds))
			}
			return nil
		})}
	}
	return flags
})

var symbolPreviews = sync.OnceValue(func() map[string]*uikit.CharacterPreview {
	symbols := make(map[string]*uikit.CharacterPreview)
	for _, font := range characterPreviews().Fonts {
		data, err := characterAssets.ReadFile("character-assets/" + font.Asset)
		if err == nil {
			err = ui.RegisterFont(data, font.Family)
		}
		if err != nil {
			log.Printf("Character preview font %s: %v", font.Family, err)
			continue
		}
		preview := &uikit.CharacterPreview{Font: font.Family}
		for _, value := range font.Characters {
			symbols[string(value)] = preview
		}
	}
	return symbols
})

func previewCharacterChoices(entries []uikit.CharacterChoice, tab string, searching bool) []uikit.CharacterChoice {
	choices := make([]uikit.CharacterChoice, 0, len(entries))
	if tab == "emoji" {
		flags := flagPreviews()
		for _, entry := range entries {
			entry.Preview = flags[entry.Value]
			choices = append(choices, entry)
		}
		return choices
	}
	symbols := symbolPreviews()
	for _, entry := range entries {
		entry.Preview = symbols[entry.Value]
		if entry.Preview == nil {
			if !searching {
				continue
			}
			codes := make([]string, 0, 1)
			for _, value := range entry.Value {
				codes = append(codes, fmt.Sprintf("U+%04X", value))
			}
			entry.Preview = &uikit.CharacterPreview{Code: strings.Join(codes, " ")}
		}
		choices = append(choices, entry)
	}
	return choices
}

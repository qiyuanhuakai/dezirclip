package theme

import (
	_ "embed"
	"encoding/json"
	"slices"
)

//go:embed builtin-presets.json
var builtinPresets []byte

var builtin = func() map[string]map[string]Tokens {
	var themes map[string]map[string]Tokens
	if err := json.Unmarshal(builtinPresets, &themes); err != nil {
		panic("invalid built-in theme presets: " + err.Error())
	}
	return themes
}()

// Builtin returns the palette resolved from the corresponding Web theme.
func Builtin(name, mode string) Tokens {
	if mode != "dark" {
		mode = "light"
	}
	if tokens, found := builtin[name][mode]; found {
		tokens.FontFamilies = slices.Clone(tokens.FontFamilies)
		return tokens
	}
	return Select(mode)
}

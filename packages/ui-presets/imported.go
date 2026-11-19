package presets

import (
	"encoding/hex"
	"fmt"
	"strconv"
	"strings"

	"github.com/tdewolff/parse/v2"
	"github.com/tdewolff/parse/v2/css"
)

// Imported reads the semantic palette and supported component rules of a local CSS theme.
func Imported(base, mode, source string) Preset {
	preset := Select(base, mode)
	styles, rules := importedThemeRules(source, preset.mode)
	colors := &preset.colors
	hasOverrides, windowChanged := false, false
	for _, entry := range []struct {
		target *string
		names  []string
	}{
		{&colors.Background, []string{"--dc-surface-window", "--bg-window", "--bg-body"}},
		{&colors.Panel, []string{"--dc-surface-input", "--bg-input", "--dc-surface-panel"}},
		{&colors.Text, []string{"--dc-text-primary", "--text-primary"}},
		{&colors.Muted, []string{"--dc-text-secondary", "--text-secondary", "--dc-text-muted"}},
		{&colors.Accent, []string{"--dc-accent", "--accent-color"}},
		{&colors.Selected, []string{"--dc-accent-soft", "--bg-selected"}},
		{&colors.Border, []string{"--dc-border", "--border-light"}},
	} {
		for _, name := range entry.names {
			if value := styles.color(styles[name].value); value != "" {
				*entry.target = value
				hasOverrides = true
				windowChanged = windowChanged || entry.target == &colors.Background
				break
			}
		}
	}
	if pixels, ok := strings.CutSuffix(styles.resolve(styles["--dc-radius"].value), "px"); ok {
		if radius, err := strconv.Atoi(pixels); err == nil && radius >= 0 && radius <= 32 {
			colors.Radius = radius
			hasOverrides = true
		}
	}
	if colors.SurfaceTint != "" {
		if value := rgbChannels(styles.resolve(styles["--surface-shell-rgb"].value)); value != "" {
			colors.SurfaceTint = value
			hasOverrides = true
		} else if windowChanged {
			colors.SurfaceTint = colors.Background
		}
	}
	if hasOverrides {
		// Captured built-in widget paints contain resolved colors. The
		// semantic native styles consume the imported palette for each role.
		preset.widgets = nil
	}
	preset.widgets = importedWidgets(preset, styles, rules)
	return preset
}

type property struct {
	value       string
	specificity int
	important   bool
}

type themeProperties map[string]property

type declaration struct {
	name string
	property
}

func importedThemeRules(source, mode string) (themeProperties, map[string][]declaration) {
	nodes := [2]themeProperties{{}, {}}
	widgets := make(map[string][]declaration)
	visitThemeRules(source, func(selector string, declarations []declaration) {
		for i, score := range rootSelectorScores(selector, mode) {
			if score < 0 {
				continue
			}
			for _, item := range declarations {
				if !strings.HasPrefix(item.name, "--") {
					continue
				}
				item.specificity = score
				if compareProperty(nodes[i][item.name], item.property) <= 0 {
					nodes[i][item.name] = item.property
				}
			}
		}
		addWidgetRules(widgets, selector, mode, declarations)
	})
	for key, value := range nodes[1] {
		nodes[0][key] = value
	}
	return nodes[0], widgets
}

func compareProperty(a, b property) int {
	if a.important != b.important {
		if a.important {
			return 1
		}
		return -1
	}
	return a.specificity - b.specificity
}

func visitThemeRules(source string, visit func(string, []declaration)) {
	if len(source) > 256*1024 {
		return
	}
	parser := css.NewParser(parse.NewInputString(source), false)
	var selector string
	var declarations []declaration
	depth := 0
	for {
		grammar, _, name := parser.Next()
		switch grammar {
		case css.ErrorGrammar:
			return
		case css.BeginAtRuleGrammar:
			depth++
		case css.BeginRulesetGrammar:
			if depth == 0 {
				selector = themeTokens(parser.Values())
				declarations = declarations[:0]
			}
			depth++
		case css.EndAtRuleGrammar:
			depth--
		case css.EndRulesetGrammar:
			depth--
			if depth == 0 {
				visit(selector, declarations)
				selector = ""
			}
		case css.CustomPropertyGrammar, css.DeclarationGrammar:
			if depth != 1 || selector == "" {
				continue
			}
			key := string(name)
			value := themeTokens(parser.Values())
			important := false
			if before, suffix, found := strings.Cut(value, "!"); found && strings.EqualFold(strings.TrimSpace(suffix), "important") {
				value = strings.TrimSpace(before)
				important = true
			}
			declarations = append(declarations, declaration{name: key, property: property{value: value, important: important}})
		}
	}
}

func themeTokens(tokens []css.Token) string {
	var value strings.Builder
	for _, token := range tokens {
		value.Write(token.Data)
	}
	return strings.TrimSpace(value.String())
}

func rootSelectorScores(list, mode string) [2]int {
	scores := [2]int{-1, -1}
	for _, selector := range strings.Split(list, ",") {
		parts := strings.Split(strings.TrimSpace(selector), ".")
		hasTheme, matches := false, true
		for _, class := range parts[1:] {
			switch class {
			case "theme-custom":
				hasTheme = true
			case "light-mode", "dark-mode":
				matches = matches && class == mode+"-mode"
			default:
				matches = false
			}
		}
		if !hasTheme || !matches {
			continue
		}
		score := (len(parts) - 1) * 10
		switch parts[0] {
		case ":root":
			scores[0] = max(scores[0], score+10)
		case "html":
			scores[0] = max(scores[0], score+1)
		case "body":
			scores[1] = max(scores[1], score+1)
		case "":
			scores[0], scores[1] = max(scores[0], score), max(scores[1], score)
		}
	}
	return scores
}

func (p themeProperties) resolve(value string) string {
	budget := 32
	return p.expand(value, &budget)
}

func (p themeProperties) expand(value string, budget *int) string {
	if *budget == 0 {
		return ""
	}
	*budget -= 1
	inner, variable := strings.CutPrefix(strings.TrimSpace(value), "var(")
	if !variable || !strings.HasSuffix(inner, ")") {
		return strings.TrimSpace(value)
	}
	name, fallback, _ := strings.Cut(inner[:len(inner)-1], ",")
	if resolved := p.expand(p[strings.TrimSpace(name)].value, budget); resolved != "" {
		return resolved
	}
	return p.expand(fallback, budget)
}

func (p themeProperties) color(value string) string {
	value = p.resolve(value)
	if len(value) > 4 && strings.EqualFold(value[:4], "rgb(") && strings.HasSuffix(value, ")") {
		return rgbChannels(p.resolve(value[4 : len(value)-1]))
	}
	raw, hash := strings.CutPrefix(value, "#")
	if !hash {
		return ""
	}
	if len(raw) == 3 {
		raw = fmt.Sprintf("%c%c%c%c%c%c", raw[0], raw[0], raw[1], raw[1], raw[2], raw[2])
	}
	if len(raw) != 6 {
		return ""
	}
	if _, err := hex.DecodeString(raw); err != nil {
		return ""
	}
	return "#" + strings.ToLower(raw)
}

func rgbChannels(value string) string {
	parts := strings.Fields(value)
	if strings.Contains(value, ",") {
		parts = strings.Split(value, ",")
	}
	if len(parts) != 3 {
		return ""
	}
	var rgb [3]uint8
	for i, part := range parts {
		channel, err := strconv.ParseUint(strings.TrimSpace(part), 10, 8)
		if err != nil {
			return ""
		}
		rgb[i] = uint8(channel)
	}
	return "#" + hex.EncodeToString(rgb[:])
}

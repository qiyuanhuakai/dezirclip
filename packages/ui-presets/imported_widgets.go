package presets

import (
	"maps"
	"math"
	"slices"
	"strconv"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type widgetRule struct {
	name     string
	base     string
	selector string
}

var importedWidgetRules = []widgetRule{
	{"header", "header", "header"},
	{"headerTitle", "headerTitle", ".header-title"},
	{"headerInput", "headerInput", ".search-input"},
	{"historyCard", "historyCard", ".history-item"},
	{"historyCardHover", "historyCard", ".history-item:hover"},
	{"historyCardSelected", "historyCardSelected", ".history-item.selected"},
	{"historyCardCompact", "historyCardCompact", ".history-item.compact"},
	{"historyCardCompactHover", "historyCardCompact", ".history-item.compact:hover"},
	{"historyCardCompactSelected", "historyCardCompact", ".history-item.compact.selected"},
}

func addWidgetRules(widgets map[string][]declaration, selectorList, mode string, declarations []declaration) {
	for _, selector := range strings.Split(selectorList, ",") {
		scope := strings.Fields(selector)
		if len(scope) != 2 {
			continue
		}
		score := rootSelectorScores(scope[0], mode)
		specificity := max(score[0], score[1])
		if specificity < 0 {
			continue
		}
		part := scope[1]
		specificity += 10 * (strings.Count(part, ".") + strings.Count(part, ":"))
		if part == "header" {
			specificity++
		}
		for _, rule := range importedWidgetRules {
			if !matchesWidgetRule(part, rule.selector) {
				continue
			}
			for _, item := range declarations {
				if strings.HasPrefix(item.name, "--") {
					continue
				}
				item.specificity = specificity
				widgets[rule.name] = append(widgets[rule.name], item)
			}
		}
	}
}

func matchesWidgetRule(selector, widget string) bool {
	if selector == widget {
		return true
	}
	if !strings.HasPrefix(widget, ".history-item") {
		return false
	}
	switch selector {
	case ".history-item":
		return true
	case ".history-item:hover":
		return strings.HasSuffix(widget, ":hover")
	case ".history-item.selected":
		return strings.HasSuffix(widget, ".selected")
	case ".history-item.compact":
		return strings.HasPrefix(widget, selector)
	}
	return false
}

func importedWidgets(preset Preset, variables themeProperties, rules map[string][]declaration) map[string]theme.WidgetStyle {
	if len(rules) == 0 {
		return preset.widgets
	}
	var styles map[string]theme.WidgetStyle
	for _, rule := range importedWidgetRules {
		declarations := rules[rule.name]
		if len(declarations) == 0 {
			continue
		}
		style, ok := preset.widgets[rule.base]
		if !ok {
			style = uikit.DefaultWidgetStyle(rule.base, preset.colors.Tokens)
		}
		if rule.name == "historyCardCompactSelected" {
			selected, ok := preset.widgets["historyCardSelected"]
			if !ok {
				selected = uikit.DefaultWidgetStyle("historyCardSelected", preset.colors.Tokens)
			}
			style.Fill, style.Border, style.Outline, style.Shadows = selected.Fill, selected.Border, selected.Outline, selected.Shadows
		}
		slices.SortStableFunc(declarations, func(a, b declaration) int { return compareProperty(a.property, b.property) })
		changed := false
		for _, item := range declarations {
			changed = applyWidgetDeclaration(&style, variables, item.name, variables.resolve(item.value)) || changed
		}
		if !changed {
			continue
		}
		if styles == nil {
			styles = maps.Clone(preset.widgets)
			if styles == nil {
				styles = make(map[string]theme.WidgetStyle)
			}
		}
		styles[rule.name] = style
	}
	if styles == nil {
		return preset.widgets
	}
	return styles
}

func applyWidgetDeclaration(style *theme.WidgetStyle, variables themeProperties, name, value string) bool {
	switch name {
	case "background", "background-color":
		if color, ok := widgetColor(variables, value); ok {
			style.Fill = color
			if name == "background" {
				style.Gradient = nil
			}
			return true
		} else if name == "background" {
			if gradient := widgetGradient(variables, value); gradient != nil {
				style.Gradient = gradient
				return true
			}
		}
	case "border", "border-bottom", "border-left", "border-inline-start":
		return applyWidgetBorder(style, variables, name, value)
	case "color", "border-color":
		if color, ok := widgetColor(variables, value); ok {
			if name == "color" {
				style.Text = color
			} else {
				style.Border = color
			}
			return true
		}
	case "margin", "padding", "border-radius":
		if edges, ok := widgetEdges(value, name == "margin"); ok {
			switch name {
			case "margin":
				style.Margin = edges
			case "padding":
				style.Padding = edges
			case "border-radius":
				style.Radius, style.CornerRadii = edges[0], &edges
			}
			return true
		}
	case "font-size", "letter-spacing":
		if pixels, ok := widgetPixels(value); ok {
			if name == "font-size" && pixels >= 8 && pixels <= 72 {
				style.Size = pixels
				return true
			} else if name == "letter-spacing" {
				style.LetterSpacing = max(-8, min(16, pixels))
				return true
			}
		}
	case "font-weight":
		if weight, err := strconv.Atoi(value); err == nil && weight >= 100 && weight <= 900 {
			style.Weight = weight
			return true
		}
	case "text-transform":
		if value == "uppercase" || value == "none" {
			style.Uppercase = value == "uppercase"
			return true
		}
	case "box-shadow":
		if shadows, ok := widgetShadows(variables, value, style.Text); ok {
			style.Shadows = shadows
			return true
		}
	}
	return false
}

func applyWidgetBorder(style *theme.WidgetStyle, variables themeProperties, name, value string) bool {
	parts := widgetParts(value, false)
	if value == "none" {
		parts = []string{"0", "solid", "transparent"}
	}
	if len(parts) != 3 || parts[1] != "solid" && parts[1] != "dashed" {
		return false
	}
	width, sizeOK := widgetPixels(parts[0])
	color, colorOK := widgetColor(variables, parts[2])
	if !sizeOK || width < 0 || width > 16 || !colorOK {
		return false
	}
	if name == "border" {
		style.BorderWidth, style.BorderWidths, style.Border = width, nil, color
		style.Outline, style.Dashed = nil, parts[1] == "dashed"
		return true
	}
	widths := [4]float32{style.BorderWidth, style.BorderWidth, style.BorderWidth, style.BorderWidth}
	if style.BorderWidths != nil {
		widths = *style.BorderWidths
	}
	if widths == [4]float32{} {
		style.Border, style.Dashed = color, parts[1] == "dashed"
	}
	side := 3
	if name == "border-bottom" {
		side = 2
	}
	widths[side] = width
	style.BorderWidths, style.Outline = &widths, nil
	return true
}

func widgetColor(variables themeProperties, value string) ([4]uint8, bool) {
	value = variables.resolve(value)
	if value == "transparent" || value == "none" {
		return [4]uint8{}, true
	}
	if hex := variables.color(value); hex != "" {
		value = hex
	} else if !strings.HasPrefix(value, "#") || len(value) != 5 && len(value) != 9 {
		return [4]uint8{}, false
	}
	if _, err := strconv.ParseUint(value[1:], 16, 32); err != nil {
		return [4]uint8{}, false
	}
	color := ui.Hex(value)
	return [4]uint8{color.R, color.G, color.B, color.A}, true
}

func widgetGradient(variables themeProperties, value string) *theme.Gradient {
	inner, ok := strings.CutPrefix(value, "linear-gradient(")
	if !ok || !strings.HasSuffix(inner, ")") {
		return nil
	}
	parts := widgetParts(inner[:len(inner)-1], true)
	if len(parts) != 3 {
		return nil
	}
	rawAngle, degrees := strings.CutSuffix(parts[0], "deg")
	angle, err := strconv.ParseFloat(rawAngle, 64)
	from, fromOK := widgetColor(variables, strings.TrimSuffix(parts[1], " 0%"))
	to, toOK := widgetColor(variables, strings.TrimSuffix(parts[2], " 100%"))
	if !degrees || err != nil || math.IsNaN(angle) || math.IsInf(angle, 0) || !fromOK || !toOK {
		return nil
	}
	return &theme.Gradient{Angle: float32(math.Mod(angle, 360)), From: from, To: to}
}

func widgetShadows(variables themeProperties, value string, text [4]uint8) ([]theme.Shadow, bool) {
	if value == "none" {
		return nil, true
	}
	layers := widgetParts(value, true)
	if len(layers) == 0 || len(layers) > 4 {
		return nil, false
	}
	shadows := make([]theme.Shadow, 0, len(layers))
	for _, layer := range layers {
		shadow := theme.Shadow{Color: text}
		count := 0
		for _, part := range widgetParts(layer, false) {
			if pixels, ok := widgetPixels(part); ok && count < 4 {
				shadow.Values[count] = pixels
				count++
			} else if part == "inset" {
				shadow.Inset = true
			} else if color, ok := widgetColor(variables, part); ok {
				shadow.Color = color
			} else {
				return nil, false
			}
		}
		if count < 2 || shadow.Values[2] < 0 || shadow.Values[2] > 64 {
			return nil, false
		}
		shadows = append(shadows, shadow)
	}
	slices.Reverse(shadows)
	return shadows, true
}

func widgetEdges(value string, allowNegative bool) ([4]float32, bool) {
	var edges [4]float32
	parts := widgetParts(value, false)
	if len(parts) == 0 || len(parts) > 4 {
		return edges, false
	}
	for i, part := range parts {
		pixels, ok := widgetPixels(part)
		if !ok || pixels < 0 && !allowNegative {
			return edges, false
		}
		edges[i] = pixels
	}
	if len(parts) == 1 {
		edges[1] = edges[0]
	}
	if len(parts) <= 2 {
		edges[2] = edges[0]
	}
	if len(parts) <= 3 {
		edges[3] = edges[1]
	}
	return edges, true
}

func widgetPixels(value string) (float32, bool) {
	if value == "0" {
		return 0, true
	}
	number, ok := strings.CutSuffix(value, "px")
	pixels, err := strconv.ParseFloat(number, 32)
	return float32(pixels), ok && err == nil && pixels >= -256 && pixels <= 256
}

func widgetParts(value string, comma bool) []string {
	var parts []string
	current, depth := 0, 0
	for position, symbol := range value {
		switch symbol {
		case '(':
			depth++
		case ')':
			depth--
		default:
			if depth == 0 && (comma && symbol == ',' || !comma && unicode.IsSpace(symbol)) {
				if part := strings.TrimSpace(value[current:position]); part != "" {
					parts = append(parts, part)
				}
				current = position + len(string(symbol))
			}
		}
	}
	if part := strings.TrimSpace(value[current:]); part != "" {
		parts = append(parts, part)
	}
	return parts
}

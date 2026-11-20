package uikit

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

type Kit struct {
	surfaceOpacity *int
	Mode           string
	tokens         *theme.Tokens
	widgets        map[string]theme.WidgetStyle
	interfaceFont  string
}

// NewKit sets a palette and a catalog of widget styles supplied by the caller.
// The caller may omit individual widgets to use the standard native style.
// The palette and catalog should remain immutable while the Kit is in use.
func NewKit(mode string, tokens theme.Tokens, widgets map[string]theme.WidgetStyle) Kit {
	return Kit{Mode: mode, tokens: &tokens, widgets: widgets}
}

// WithInterfaceFont overrides the UI font without changing the catalog or its monospace styles.
func (k Kit) WithInterfaceFont(font string) Kit {
	k.interfaceFont = strings.TrimSpace(font)
	return k
}

func (k Kit) palette() theme.Tokens {
	var tokens theme.Tokens
	if k.tokens != nil {
		tokens = *k.tokens
	} else {
		tokens = theme.Select(k.Mode)
	}
	if k.interfaceFont != "" {
		tokens.FontFamilies = []string{k.interfaceFont}
	}
	return tokens
}

func (k Kit) Apply(c *ui.Context) {
	ApplyAppearance(c, k.palette(), k.Mode == "dark")
}

func (k Kit) style(widget string) theme.WidgetStyle {
	if style, ok := k.widgets[widget]; ok {
		if k.surfaceOpacity != nil && *k.surfaceOpacity != 65 {
			style.Fill = materialColor(style.Fill, style.FillRange, *k.surfaceOpacity)
			style.Border = materialColor(style.Border, style.BorderRange, *k.surfaceOpacity)
		}
		if k.interfaceFont != "" && style.Font != "monospace" && !strings.HasSuffix(style.Font, ", monospace") {
			style.Font = k.interfaceFont
		}
		return style
	}
	return DefaultWidgetStyle(widget, k.palette())
}

// WithSurfaceOpacity adapts CSS-derived materials to the current window opacity.
func (k Kit) WithSurfaceOpacity(percent int) Kit {
	percent = min(100, max(0, percent))
	k.surfaceOpacity = &percent
	return k
}

func materialColor(current [4]uint8, values *theme.ColorRange, percent int) [4]uint8 {
	if values == nil {
		return current
	}
	from, to := values.Clear, current
	progress := float32(percent) / 65
	if percent > 65 {
		from, to = current, values.Full
		progress = float32(percent-65) / 35
	}
	if from[3] == 0 {
		from[0], from[1], from[2] = to[0], to[1], to[2]
	}
	if to[3] == 0 {
		to[0], to[1], to[2] = from[0], from[1], from[2]
	}
	for i := range current {
		a, b := float32(from[i]), float32(to[i])
		current[i] = uint8(a + (b-a)*progress + 0.5)
	}
	return current
}

func styleColor(rgba [4]uint8) ui.Color {
	return ui.Color{R: rgba[0], G: rgba[1], B: rgba[2], A: rgba[3]}
}

func paint(element ui.Element, style theme.WidgetStyle) ui.Element {
	borderWidth, border := style.BorderWidth, style.Border
	if outline := style.Outline; outline != nil && style.BorderWidths == nil && outline.Width > borderWidth {
		// A CSS inset outline covers the outer edge without adding to the box size.
		for i := range style.Padding {
			style.Padding[i] = max(0, style.Padding[i]+borderWidth-outline.Width)
		}
		borderWidth, border = outline.Width, outline.Color
	}
	element.Background(styleColor(style.Fill)).Border(borderWidth, styleColor(border)).Radius(style.Radius).
		Padding(style.Padding[:]...).Opacity(style.Opacity)
	if style.BorderWidths != nil {
		element.BorderWidth(style.BorderWidths[:]...)
	}
	if style.CornerRadii != nil {
		element.Radius(style.CornerRadii[:]...)
	}
	if style.Dashed {
		element.BorderStyle(ui.BorderDashed)
	}
	if gradient := style.Gradient; gradient != nil {
		element.Gradient(styleColor(gradient.From), styleColor(gradient.To), gradient.Angle)
	}
	for _, shadow := range style.Shadows {
		x, y, blur, spread := shadow.Values[0], shadow.Values[1], shadow.Values[2], shadow.Values[3]
		if shadow.Inset {
			element.InsetShadow(x, y, blur, spread, styleColor(shadow.Color))
		} else {
			element.Shadow(x, y, blur, spread, styleColor(shadow.Color))
		}
	}
	return element
}

func paintTextField(element ui.Element, style theme.WidgetStyle) ui.Element {
	padding := style.Padding
	padding[1] = max(8, padding[1])
	padding[3] = max(8, padding[3])
	return paint(element, style).Padding(padding[:]...)
}

func styledText(element ui.Element, style theme.WidgetStyle) ui.Element {
	element.TextColor(styleColor(style.Text)).Font(style.Font).FontSize(style.Size).FontWeight(style.Weight).LetterSpacing(style.LetterSpacing)
	if style.LineHeight > 0 {
		element.LineHeight(style.LineHeight)
	}
	return element
}

func text(c *ui.Context, value string, style theme.WidgetStyle) ui.Element {
	if style.Uppercase {
		value = strings.ToUpper(value)
	}
	return styledText(ui.Text(c, value), style)
}

func (k Kit) button(c *ui.Context, label, widget string) ui.Element {
	style := k.style(widget)
	button := ui.ButtonBase(c).Label(label).Height(style.Height)
	paint(button, k.interactive(c, button, style, false))
	button.Children(func() { text(c, label, style) })
	return button
}

func (k Kit) interactive(c *ui.Context, control ui.Element, style theme.WidgetStyle, active bool) theme.WidgetStyle {
	if control.Pressed() || control.Hovered() || control.FocusVisible() {
		strength := float32(0.45)
		if control.Pressed() {
			strength = 0.8
		} else if active {
			strength = 0.25
		}
		fill := styleColor(style.Fill)
		if fill.A == 0 {
			fill = c.Theme().Surface
		}
		style.Fill = rgba(fill.Mix(c.Theme().Selection, strength))
		if style.BorderWidth > 0 {
			style.Border = rgba(styleColor(style.Border).Mix(c.Theme().Accent, strength))
		}
	}
	return k.animatePaint(control, style)
}

func (k Kit) animatePaint(control ui.Element, style theme.WidgetStyle) theme.WidgetStyle {
	if ms := k.palette().TransitionMs; ms > 0 {
		animation := ui.Animation{Duration: time.Duration(ms) * time.Millisecond}
		style.Fill = rgba(control.AnimateColor("deziro-fill", styleColor(style.Fill), animation))
		style.Border = rgba(control.AnimateColor("deziro-border", styleColor(style.Border), animation))
		style.Text = rgba(control.AnimateColor("deziro-text", styleColor(style.Text), animation))
	}
	return style
}

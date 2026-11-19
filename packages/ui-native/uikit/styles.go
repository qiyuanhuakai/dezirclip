package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

type Kit struct {
	Mode    string
	tokens  *theme.Tokens
	widgets map[string]theme.WidgetStyle
}

// NewKit sets a palette and a catalog of widget styles supplied by the caller.
// The caller may omit individual widgets to use the standard native style.
// The palette and catalog should remain immutable while the Kit is in use.
func NewKit(mode string, tokens theme.Tokens, widgets map[string]theme.WidgetStyle) Kit {
	return Kit{Mode: mode, tokens: &tokens, widgets: widgets}
}

func (k Kit) palette() theme.Tokens {
	if k.tokens != nil {
		return *k.tokens
	}
	return theme.Select(k.Mode)
}

func (k Kit) Apply(c *ui.Context) {
	ApplyAppearance(c, k.palette(), k.Mode == "dark")
}

func (k Kit) style(widget string) theme.WidgetStyle {
	if style, ok := k.widgets[widget]; ok {
		return style
	}
	return standardWidgetStyle(widget, k.palette())
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

func styledText(element ui.Element, style theme.WidgetStyle) ui.Element {
	return element.TextColor(styleColor(style.Text)).Font(style.Font).FontSize(style.Size).FontWeight(style.Weight).LetterSpacing(style.LetterSpacing)
}

func text(c *ui.Context, value string, style theme.WidgetStyle) ui.Element {
	if style.Uppercase {
		value = strings.ToUpper(value)
	}
	return styledText(ui.Text(c, value), style)
}

func (k Kit) button(c *ui.Context, label, widget string) ui.Element {
	style := k.style(widget)
	button := paint(ui.ButtonBase(c).Label(label).Height(style.Height), style)
	button.Children(func() { text(c, label, style) })
	return button
}

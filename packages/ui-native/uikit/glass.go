package uikit

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/shader"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

var glassBackdrop = shader.MustCompile(`package main
var Strength float

func Fragment(dst vec4, pos vec2) vec4 {
	return backdropSample(pos) * Strength
}
`)

type glassMaterial struct {
	radius, strength float32
	tint             ui.Color
	gradient         *theme.Gradient
}

func (material glassMaterial) PaintMaterial(p *ui.Painter, box ui.Rect, radii [4]float32) {
	glassBackdrop.Draw(p, box, shader.Options{
		Radius:       radii[0],
		BackdropBlur: material.radius,
		Uniforms:     map[string]any{"Strength": material.strength},
	})
	if gradient := material.gradient; gradient != nil {
		p.FillGradient(box, ui.LinearGradient{
			From: styleColor(gradient.From), To: styleColor(gradient.To), Angle: gradient.Angle,
		}, radii[0])
		return
	}
	p.Fill(box, material.tint, radii[0])
}

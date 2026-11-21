package theme

type Shadow struct {
	Color  [4]uint8
	Values [4]float32
	Inset  bool
}

type Outline struct {
	Offset float32
	Width  float32
	Color  [4]uint8
}

type Gradient struct {
	Angle float32
	From  [4]uint8
	To    [4]uint8
}

type ColorRange struct {
	Clear [4]uint8
	Full  [4]uint8
	Stops []ColorStop
}

// ColorStop stores a material color at a position on the surface-opacity slider.
type ColorStop struct {
	Percent int
	RGBA    [4]uint8
}

type WidgetStyle struct {
	BackdropBlur  float32
	BorderWidths  *[4]float32
	CornerRadii   *[4]float32
	Outline       *Outline
	Gradient      *Gradient
	LetterSpacing float32
	LineHeight    float32
	MinHeightEm   float32
	MinWidth      float32
	MinHeight     float32
	MaxHeight     float32
	MaxLines      int
	ColumnGap     float32
	RowGap        float32
	Top           float32
	Left          float32
	Right         float32
	TranslateX    float32
	TranslateY    float32
	Caption       string
	Margin        [4]float32
	Fill          [4]uint8
	Text          [4]uint8
	Border        [4]uint8
	FillRange     *ColorRange
	BorderRange   *ColorRange
	BorderWidth   float32
	Radius        float32
	Width         float32
	Height        float32
	MarginTop     float32
	MarginBottom  float32
	Padding       [4]float32
	Font          string
	UsesMonoFont  bool
	Size          float32
	Weight        int
	Uppercase     bool
	Opacity       float32
	Dashed        bool
	Shadows       []Shadow

	MaxWidthPercent float32
	Bottom          float32
}

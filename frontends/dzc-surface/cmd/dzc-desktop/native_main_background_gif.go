package main

import (
	"bytes"
	"encoding/binary"
	"image"
	"image/draw"
	"image/gif"
	"time"
)

type nativeBackgroundGIF struct {
	file           *gif.GIF
	front, back    *image.RGBA
	restore        *image.RGBA
	index, repeats int
}

func loadBackgroundGIF(raw []byte) *nativeBackgroundGIF {
	if !backgroundGIFWithinBudget(raw) {
		return nil
	}
	file, err := gif.DecodeAll(bytes.NewReader(raw))
	if err != nil || len(file.Image) < 2 {
		return nil
	}
	canvas := image.Rect(0, 0, file.Config.Width, file.Config.Height)
	animation := &nativeBackgroundGIF{file: file, front: image.NewRGBA(canvas), back: image.NewRGBA(canvas)}
	animation.drawFrame(animation.front)
	return animation
}

func (a *nativeBackgroundGIF) nextDelay() (time.Duration, bool) {
	if a.index == len(a.file.Image)-1 && a.file.LoopCount != 0 && a.repeats >= a.file.LoopCount {
		return 0, false
	}
	delay := a.file.Delay[a.index]
	if delay < 2 {
		delay = 10
	}
	return time.Duration(delay) * 10 * time.Millisecond, true
}

func (a *nativeBackgroundGIF) advance() *image.RGBA {
	if a.index == len(a.file.Image)-1 {
		a.index = 0
		a.repeats++
		clear(a.back.Pix)
	} else {
		copy(a.back.Pix, a.front.Pix)
		previous := a.file.Image[a.index].Bounds()
		switch a.file.Disposal[a.index] {
		case gif.DisposalBackground:
			draw.Draw(a.back, previous, image.Transparent, image.Point{}, draw.Src)
		case gif.DisposalPrevious:
			draw.Draw(a.back, previous, a.restore, a.restore.Bounds().Min, draw.Src)
		}
		a.index++
	}
	a.drawFrame(a.back)
	a.front, a.back = a.back, a.front
	return a.front
}

func (a *nativeBackgroundGIF) drawFrame(canvas *image.RGBA) {
	frame := a.file.Image[a.index]
	bounds := frame.Bounds()
	if a.file.Disposal[a.index] == gif.DisposalPrevious {
		rect := image.Rectangle{Max: bounds.Size()}
		if a.restore == nil || a.restore.Bounds() != rect {
			a.restore = image.NewRGBA(rect)
		}
		draw.Draw(a.restore, rect, canvas, bounds.Min, draw.Src)
	}
	draw.Draw(canvas, bounds, frame, bounds.Min, draw.Over)
}

// GIF frames are decoded to one byte per pixel. Check their descriptors
// before DecodeAll so compressed animations cannot retain unbounded pixels.
func backgroundGIFWithinBudget(raw []byte) bool {
	if len(raw) < 13 {
		return false
	}
	colorTableSize := func(flags byte) int {
		if flags&0x80 == 0 {
			return 0
		}
		return 3 << (1 + flags&7)
	}
	offset := 13 + colorTableSize(raw[10])
	pixels, frames := uint64(0), 0
	for offset < len(raw) {
		switch raw[offset] {
		case 0x3b:
			return frames > 1
		case 0x21:
			offset += 2
		case 0x2c:
			if len(raw)-offset < 10 {
				return false
			}
			descriptor := raw[offset : offset+10]
			width := binary.LittleEndian.Uint16(descriptor[5:7])
			height := binary.LittleEndian.Uint16(descriptor[7:9])
			pixels += uint64(width) * uint64(height)
			frames++
			if frames > 1200 || pixels > 32<<20 {
				return false
			}
			offset += 11 + colorTableSize(descriptor[9])
		default:
			return false
		}
		// Extension and image data are both length-prefixed sub-blocks.
		for {
			if offset >= len(raw) {
				return false
			}
			length := int(raw[offset])
			offset += 1 + length
			if length == 0 {
				break
			}
		}
	}
	return false
}

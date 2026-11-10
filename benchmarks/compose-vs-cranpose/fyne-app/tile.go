// One wall tile as Fyne draws it: a rectangle, its label, a dot and its
// value, four canvas objects. The still wall, the patchwork's patches and
// the cards share them.
package main

import (
	"fmt"
	"image/color"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
)

// tileLook is what the tiles of one height share: the text styles, the
// dot, and the size of the label and of the value. Their digits are as wide
// as one another, so one measure of each serves every tile.
type tileLook struct {
	label, value         style
	padding, dot         float32
	labelSize, valueSize fyne.Size
}

func newTileLook(height float32) tileLook {
	label, value := style{height * 0.30, true, false}, style{height * 0.26, false, false}
	return tileLook{label, value, height * 0.06, height * 0.22, measure("00000", label), measure("000", value)}
}

// tile is the objects of one tile.
type tile struct {
	fill        *canvas.Rectangle
	name, value *canvas.Text
	dot         *canvas.Circle
}

func wallLabel(index int) string { return fmt.Sprintf("%05d", index) }

// newTile is tile `index`, `width` by `height` points, with its corner at
// x, y; its fill is the wall's.
func (l tileLook) newTile(index int, x, y, width, height float32) tile {
	t := tile{
		fill:  canvas.NewRectangle(palette(index)),
		name:  newText(wallLabel(index), white, l.label),
		value: newText(wallValue(index), white, l.value),
		dot:   canvas.NewCircle(rgb(chipBackgroundRGB[(index/8)%8])),
	}
	t.fill.CornerRadius = wallCorner
	place(t.fill, x, y, width, height)
	place(t.name, x+l.padding, y+l.padding, l.labelSize.Width, l.labelSize.Height)
	line := y + l.padding + l.labelSize.Height
	place(t.dot, x+l.padding, line+(l.valueSize.Height-l.dot)/2, l.dot, l.dot)
	place(t.value, x+l.padding+l.dot*1.5, line, l.valueSize.Width, l.valueSize.Height)
	return t
}

func (t tile) objects() []fyne.CanvasObject {
	return []fyne.CanvasObject{t.fill, t.name, t.dot, t.value}
}

// bind shows tile `index` on `fill`; whatever it shows already stays.
func (t tile) bind(index int, fill color.NRGBA) {
	setFill(t.fill, &t.fill.FillColor, fill)
	setText(t.name, wallLabel(index), white)
	setText(t.value, wallValue(index), white)
	setFill(t.dot, &t.dot.FillColor, rgb(chipBackgroundRGB[(index/8)%8]))
}

// setFill changes a fill only when it differs.
func setFill(object fyne.CanvasObject, current *color.Color, fill color.NRGBA) {
	if *current != fill {
		*current = fill
		object.Refresh()
	}
}

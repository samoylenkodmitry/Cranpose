// The still wall in Fyne (`../README.md`): a window of tiles that never
// change, under a clock line whose text changes every frame, with a cursor
// outline that rests on another tile every frame. Its data follows
// `perf-data/src/wall.rs` bit for bit.
//
// In Fyne's idiom the tiles are canvas objects placed once in a container
// the app lays out itself; the animation sets the clock's text and moves the
// cursor's rectangle, the two objects a frame changes.
package main

import (
	"fmt"
	"image/color"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
)

// wallTier is the size of a tier's tiles, in whole points.
type wallTier struct{ width, height float32 }

var wallTiers = [20]wallTier{
	{64, 40}, {54, 34}, {46, 29}, {40, 25}, {34, 21}, {30, 18}, {26, 16}, {22, 14}, {19, 12}, {16, 10},
	{14, 9}, {12, 8}, {10, 7}, {8, 6}, {7, 5}, {6, 4}, {5, 4}, {5, 3}, {4, 3}, {3, 3},
}

const (
	wallGap         = 1
	wallClockHeight = 24
	wallClockText   = 14
	wallCorner      = 2
	wallCursorWidth = 2
)

func tierOfWall(tier int) wallTier {
	return wallTiers[min(max(tier, 1), len(wallTiers))-1]
}

// wallGrid is the tiles across and down that fill a wall width by height
// points.
func wallGrid(t wallTier, width, height float32) (int, int) {
	fit := func(space, tile float32) int { return max(1, int((space-wallGap)/(tile+wallGap))) }
	return fit(width, t.width), fit(height, t.height)
}

func wallValue(index int) string {
	mixed := (uint32(index) * 2654435761) >> 22
	return fmt.Sprintf("%03d", mixed%1000)
}

func wallClock(frame int) string { return fmt.Sprintf("frame %06d", frame) }

// wallOrigin is where tile `index` sits on a wall `columns` tiles across.
func wallOrigin(index, columns int, t wallTier) (float32, float32) {
	return wallGap + float32(index%columns)*(t.width+wallGap), wallGap + float32(index/columns)*(t.height+wallGap)
}

// runWall shows the still wall in a window of its own and runs the app.
func runWall(a fyne.App, l launch) {
	w := newWindow(a, "Still wall")
	tier := tierOfWall(l.tier)
	top := float32(56 + wallClockHeight)
	columns, rows := wallGrid(tier, windowWidth, windowHeight-top)
	tiles := columns * rows
	background := canvas.NewRectangle(rgb(0x0F1220))
	place(background, 0, 0, windowWidth, windowHeight)
	topBar, heading := newTopBar("Still wall")
	clockStyle := style{wallClockText, false, false}
	clock := newText(wallClock(0), white, clockStyle)
	objects := make([]fyne.CanvasObject, 0, 5+4*tiles)
	objects = append(objects, background, topBar, heading, clock)
	look := newTileLook(tier.height)
	for index := range tiles {
		x, y := wallOrigin(index, columns, tier)
		objects = append(objects, look.newTile(index, x, top+y, tier.width, tier.height).objects()...)
	}
	cursor := canvas.NewRectangle(color.Transparent)
	cursor.StrokeColor = white
	cursor.StrokeWidth = wallCursorWidth
	cursor.CornerRadius = wallCorner
	objects = append(objects, cursor)
	w.SetContent(container.NewWithoutLayout(objects...))
	frame := 0
	show := func() {
		setText(clock, wallClock(frame), white)
		size := measure(clock.Text, clockStyle)
		place(clock, 8, 56+(wallClockHeight-size.Height)/2, size.Width, size.Height)
		x, y := wallOrigin(frame*37%tiles, columns, tier)
		place(cursor, x, top+y, tier.width, tier.height)
	}
	show()
	animate(l, &frame, show)
	w.ShowAndRun()
}

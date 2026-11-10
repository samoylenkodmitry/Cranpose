// The patchwork in Fyne (`../README.md`): nine patches of the still wall's
// tiles beside a chat. Two patches never change; in the others a frame
// changes a small part. The data follows `perf-data/src/patchwork.rs` and
// `chat.rs` bit for bit.
//
// In Fyne's idiom every object is made once and a frame sets only the fields
// that changed and refreshes those objects alone: a refresh drops one
// object's texture, a move drops none. A patch is a container clipped by a
// scroll widget that cannot scroll. A row of tiles that changes sets the
// fills of its rectangles, a nest its digits, the cards move as containers
// and swap only the order they are drawn in when a move begins, the reorder
// patch moves the items whose place changed, the scroll patch moves one
// container and binds the row that came in to the one that left, the
// paragraph sets the two lines that hold the changed words, the pile sets
// two fills. The charts are one raster drawn once. Fyne draws every object
// again each frame it paints; nothing keeps a picture of what did not change.
package main

import (
	"fmt"
	"image/color"
	"math"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/widget"
)

// patchGrid is what every patch knows: the tier, its own tiles and its size
// in points.
type patchGrid struct {
	tier          wallTier
	tiles         patchTiles
	width, height float32
	look          tileLook
}

// patchView is a patch's content in a container of its own, and what a frame
// changes in it; show is nil for a patch no frame changes.
type patchView struct {
	content *fyne.Container
	show    func(frame int)
}

// patchBuilders make the patches in the screen's order, row by row.
var patchBuilders = [patchColumns * patchRows]func(patchGrid) patchView{
	newStill, newCharts, newRecolor, newCardStack, newReorder, newDeep, newScrollList, newParagraphPatch, newPile,
}

func runPatchwork(a fyne.App, l launch) {
	w := newWindow(a, "Patchwork")
	tier := tierOfWall(l.tier)
	top := float32(56 + wallClockHeight)
	width := float32(math.Floor(float64(windowWidth-chatWidth) / patchColumns))
	height := float32(math.Floor(float64(windowHeight-top) / patchRows))
	grid := patchGrid{tier, newPatchTiles(tier, width, height), width, height, newTileLook(tier.height)}
	background := canvas.NewRectangle(rgb(0x0F1220))
	place(background, 0, 0, windowWidth, windowHeight)
	topBar, heading := newTopBar("Patchwork")
	clockStyle := style{wallClockText, false, false}
	clock := newText(wallClock(0), white, clockStyle)
	size := measure(wallClock(0), clockStyle)
	place(clock, 8, 56+(wallClockHeight-size.Height)/2, size.Width, size.Height)
	objects := []fyne.CanvasObject{background, topBar, heading, clock}
	var shows []func(frame int)
	for index, build := range patchBuilders {
		view := build(grid)
		clip := newClip(float32(index%patchColumns)*width, top+float32(index/patchColumns)*height, width, height, view.content)
		objects = append(objects, clip)
		if view.show != nil {
			shows = append(shows, view.show)
		}
	}
	chat := newChat(l, windowWidth-chatWidth, top, windowHeight-top)
	objects = append(objects, chat.objects()...)
	w.SetContent(container.NewWithoutLayout(objects...))
	frame := 0
	paint := func() {
		setText(clock, wallClock(frame), white)
		for _, patch := range shows {
			patch(frame)
		}
	}
	paint()
	animate(l, &frame, func() {
		paint()
		chat.show(frame)
	})
	w.ShowAndRun()
}

// newClip shows `content` in a window of its own that cuts what lies past it:
// a scroll widget that cannot scroll, as the gauntlet clips its panels.
func newClip(x, y, width, height float32, content *fyne.Container) *container.Scroll {
	clip := container.NewScroll(content)
	clip.Direction = container.ScrollNone
	place(clip, x, y, width, height)
	place(content, 0, 0, width, height)
	return clip
}

// tileObjects is the objects of `tiles` laid out as the wall lays them out.
func (g patchGrid) tileObjects(tiles []tile) []fyne.CanvasObject {
	objects := make([]fyne.CanvasObject, 0, 4*len(tiles))
	for _, t := range tiles {
		objects = append(objects, t.objects()...)
	}
	return objects
}

// wallTiles makes the tiles of a patch, `columns` by `rows` of them.
func (g patchGrid) wallTiles() []tile {
	tiles := make([]tile, 0, g.tiles.count())
	for index := range g.tiles.count() {
		x, y := wallOrigin(index, g.tiles.columns, g.tier)
		tiles = append(tiles, g.look.newTile(index, x, y, g.tier.width, g.tier.height))
	}
	return tiles
}

// newStill is tiles that never change.
func newStill(g patchGrid) patchView {
	return patchView{content: container.NewWithoutLayout(g.tileObjects(g.wallTiles())...)}
}

// newRecolor is tiles whose fill changes in turn: a frame sets the fills of
// one row of rectangles and refreshes those.
func newRecolor(g patchGrid) patchView {
	tiles := g.wallTiles()
	turn := g.tiles.inTurn()
	show := func(frame int) {
		row := turn.row(frame)
		for index := row * g.tiles.columns; index < (row+1)*g.tiles.columns; index++ {
			setFill(tiles[index].fill, &tiles[index].fill.FillColor, rgb(patchworkRecolorRGB(index, frame, turn)))
		}
	}
	return patchView{container.NewWithoutLayout(g.tileObjects(tiles)...), show}
}

func (p patchTiles) inTurn() inTurn { return inTurn{p.columns, p.rows} }

// newDeep is nests of boxes around digits that change in turn: the boxes
// never change, a frame sets the digits of one row of nests.
func newDeep(g patchGrid) patchView {
	columns, rows := patchworkPairs(g.tiles.columns, g.tiles.rows)
	width, height := patchworkDouble(g.tier.width), patchworkDouble(g.tier.height)
	padding := g.look.padding
	digits := style{g.look.label.size, true, false}
	var objects []fyne.CanvasObject
	numbers := make([]*canvas.Text, 0, columns*rows)
	for index := range columns * rows {
		x, y := wallGap+float32(index%columns)*(width+wallGap), wallGap+float32(index/columns)*(height+wallGap)
		for level := range patchworkDepth {
			inset := float32(level) * padding
			box := canvas.NewRectangle(palette(index + level))
			place(box, x+inset, y+inset, width-2*inset, height-2*inset)
			objects = append(objects, box)
		}
		inner := float32(patchworkDepth) * padding
		number := newText("", white, digits)
		size := measure("000", digits)
		place(number, x+inner, y+inner, size.Width, size.Height)
		numbers = append(numbers, number)
		objects = append(objects, number)
	}
	turn := inTurn{columns, rows}
	set := func(index, frame int) {
		setText(numbers[index], fmt.Sprintf("%03d", patchworkDeepNumber(index, frame, turn)), white)
	}
	for index := range numbers {
		set(index, 0)
	}
	show := func(frame int) {
		row := turn.row(frame)
		for index := row * columns; index < (row+1)*columns; index++ {
			set(index, frame)
		}
	}
	return patchView{container.NewWithoutLayout(objects...), show}
}

// newParagraphPatch is one wrapping text of five-digit words, one of which
// changes every frame: a wrapping label under a theme that gives it the
// patch's text size and white. A frame sets its text and Fyne wraps and draws
// all of it again.
func newParagraphPatch(g patchGrid) patchView {
	words := patchworkParagraphWords(g.tiles.count())
	text := style{g.look.label.size, false, false}
	label := widget.NewLabel(patchworkParagraph(0, words))
	label.Wrapping = fyne.TextWrapWord
	th := widgetTheme{
		Theme: fyne.CurrentApp().Settings().Theme(), text: text.size, foreground: white,
		lineSpacing: max(text.size*1.4-measure("Ag", text).Height, 0),
	}
	paragraph := container.NewThemeOverride(label, th)
	place(paragraph, wallGap, wallGap, g.width-2*wallGap, g.height-2*wallGap)
	show := func(frame int) { label.SetText(patchworkParagraph(frame, words)) }
	return patchView{container.NewWithoutLayout(paragraph), show}
}

// pileFill is the fill of pile tile `index`: translucent, or opaque black.
func pileFill(index int, opaque bool) color.NRGBA {
	if opaque {
		return color.NRGBA{A: 0xFF}
	}
	fill := palette(index)
	fill.A = patchworkPileAlpha
	return fill
}

// newPile is translucent tiles lying over one another, one of which is
// opaque: a frame sets two fills.
func newPile(g patchGrid) patchView {
	columns, rows := patchworkPileGrid(g.tiles.columns, g.tiles.rows)
	width, height := patchworkDouble(g.tier.width), patchworkDouble(g.tier.height)
	tiles := make([]tile, 0, columns*rows)
	opaque := patchworkPileOpaque(0, columns*rows)
	for index := range columns * rows {
		x, y := wallOrigin(index, columns, g.tier)
		t := g.look.newTile(index, x, y, width, height)
		t.fill.FillColor = pileFill(index, index == opaque)
		tiles = append(tiles, t)
	}
	show := func(frame int) {
		before := opaque
		opaque = patchworkPileOpaque(frame, len(tiles))
		for _, index := range [2]int{before, opaque} {
			setFill(tiles[index].fill, &tiles[index].fill.FillColor, pileFill(index, index == opaque))
		}
	}
	return patchView{container.NewWithoutLayout(g.tileObjects(tiles)...), show}
}

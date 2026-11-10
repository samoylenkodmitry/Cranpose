package main

import (
	"slices"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
)

// patchCard is one card as a container of its own: its header over its rows
// of tiles. The container moves; what the card shows changes only when a
// card of another number takes its place.
type patchCard struct {
	box    *fyne.Container
	id     int
	header *canvas.Rectangle
	title  *canvas.Text
	tiles  []tile
}

func newCardView(g patchGrid, c cards) *patchCard {
	width := patchworkReorderWidth(c.columns, g.tier.width)
	v := &patchCard{id: -1, header: canvas.NewRectangle(white)}
	v.header.CornerRadius = wallCorner
	place(v.header, 0, 0, width, g.tier.height)
	v.title = newText("", white, g.look.label)
	place(v.title, g.look.padding, g.look.padding, measure("Group 00000", g.look.label).Width, g.look.labelSize.Height)
	objects := []fyne.CanvasObject{v.header, v.title}
	for index := range c.tiles() {
		x := float32(index%c.columns) * (g.tier.width + wallGap)
		y := g.tier.height + wallGap + float32(index/c.columns)*(g.tier.height+wallGap)
		t := g.look.newTile(0, x, y, g.tier.width, g.tier.height)
		v.tiles = append(v.tiles, t)
		objects = append(objects, t.objects()...)
	}
	v.box = container.NewWithoutLayout(objects...)
	return v
}

// bind shows card `id`.
func (v *patchCard) bind(id int, tiles int) {
	v.id = id
	setFill(v.header, &v.header.FillColor, rgb(patchworkCardRGB(id)))
	setText(v.title, patchworkCardTitle(id), white)
	for index, t := range v.tiles {
		tileIndex := patchworkCardTile(id, index, tiles)
		t.bind(tileIndex, palette(tileIndex))
	}
}

// newCardStack is cards that move every frame. A card keeps its container
// for as long as it is in the patch. When a move begins the stack binds the
// card that came in to the container of the card that left and draws the
// containers in the order of the cards' places; a frame moves the containers.
func newCardStack(g patchGrid) patchView {
	c := newCards(g.tiles.columns, g.tiles.rows, g.tier.height, g.height)
	views := make([]*patchCard, c.places())
	for index := range views {
		views[index] = newCardView(g, c)
	}
	content := container.NewWithoutLayout()
	shown := make([]*patchCard, c.places())
	began := -1
	begin := func(frame int) {
		clear(shown)
		for place := range shown {
			if card, ok := c.place(place, frame); ok {
				for _, view := range views {
					if view.id == card.id {
						shown[place] = view
					}
				}
			}
		}
		for place := range shown {
			card, ok := c.place(place, frame)
			if !ok || shown[place] != nil {
				continue
			}
			index := slices.IndexFunc(views, func(view *patchCard) bool { return !slices.Contains(shown, view) })
			views[index].bind(card.id, c.tiles())
			shown[place] = views[index]
		}
		content.Objects = content.Objects[:0]
		for _, view := range shown {
			if view != nil {
				content.Objects = append(content.Objects, view.box)
			}
		}
	}
	show := func(frame int) {
		if move := frame / patchworkCardFrames; move != began {
			began = move
			begin(frame)
		}
		for place, view := range shown {
			if card, ok := c.place(place, frame); ok && view != nil {
				view.box.Move(fyne.NewPos(wallGap, card.top))
			}
		}
	}
	return patchView{content, show}
}

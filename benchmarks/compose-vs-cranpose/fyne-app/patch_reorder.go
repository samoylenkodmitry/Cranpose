package main

import (
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/container"
)

// flowCursor is where a wrapping row of items stands before an item: how far
// along its line, and which line.
type flowCursor struct {
	along float32
	line  int
}

// newReorder is a wrapping row of items 1 to 3 tiles wide, two neighbours of
// which change places every frame. Each item is a container of its own that
// keeps its tiles. The row stores its cursor before each place, so a frame
// flows the items again only from each place that changed, until the flow
// meets a cursor it had before, and moves the containers whose place is new.
func newReorder(g patchGrid) patchView {
	count := patchworkReorderCount(g.tiles.count())
	available := g.width - 2*wallGap
	items := make([]*fyne.Container, count)
	widths := make([]float32, count)
	objects := make([]fyne.CanvasObject, count)
	for item := range items {
		widths[item] = patchworkReorderWidth(patchworkReorderUnits(item), g.tier.width)
		t := g.look.newTile(item, 0, 0, widths[item], g.tier.height)
		items[item] = container.NewWithoutLayout(t.objects()...)
		objects[item] = items[item]
	}
	order := make([]int, count)
	cursors := make([]flowCursor, count+1)
	flow := func(from, last int) {
		for place := from; place < count; place++ {
			cursor := cursors[place]
			width := widths[order[place]]
			if cursor.along > 0 && cursor.along+width > available {
				cursor = flowCursor{0, cursor.line + 1}
			}
			items[order[place]].Move(fyne.NewPos(wallGap+cursor.along, wallGap+float32(cursor.line)*(g.tier.height+wallGap)))
			next := flowCursor{cursor.along + width + wallGap, cursor.line}
			if place > last && cursors[place+1] == next {
				return
			}
			cursors[place+1] = next
		}
	}
	for place := range order {
		order[place] = place
	}
	swap := patchworkReorderSwap(0, count)
	for place := swap; place <= min(swap+1, count-1); place++ {
		order[place] = patchworkReorderItem(place, 0, count)
	}
	flow(0, count)
	show := func(frame int) {
		before := swap
		swap = patchworkReorderSwap(frame, count)
		for _, pair := range [2]int{before, swap} {
			for place := pair; place <= min(pair+1, count-1); place++ {
				order[place] = patchworkReorderItem(place, frame, count)
			}
		}
		// The two pairs lie apart: each flows on its own, and ends where its
		// cursor meets the one it had before.
		flow(min(before, swap), min(before, swap)+1)
		flow(max(before, swap), max(before, swap)+1)
	}
	return patchView{container.NewWithoutLayout(objects...), show}
}

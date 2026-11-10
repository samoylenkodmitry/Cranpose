package main

import (
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/container"
)

// scrollRow is one row of tiles as a container of its own.
type scrollRow struct {
	box   *fyne.Container
	row   int
	tiles []tile
}

// newScrollList is a list of rows of tiles that scrolls by one row every
// frame. It keeps as many rows as the patch shows, as a recycling list does:
// a scroll moves the one container that holds the rows, and the row that
// came in at the bottom is bound to the container of the row that left at the
// top. A row stays at its own place in the container, so no row moves.
func newScrollList(g patchGrid) patchView {
	columns := g.tiles.columns
	total := patchworkScrollRows(columns)
	pitch := g.tier.height + wallGap
	visible := int((g.height-wallGap)/pitch) + 1
	rows := make([]*scrollRow, visible)
	objects := make([]fyne.CanvasObject, visible)
	for index := range rows {
		r := &scrollRow{row: -1}
		for column := range columns {
			r.tiles = append(r.tiles, g.look.newTile(0, wallGap+float32(column)*(g.tier.width+wallGap), 0, g.tier.width, g.tier.height))
		}
		r.box = container.NewWithoutLayout(g.tileObjects(r.tiles)...)
		rows[index], objects[index] = r, r.box
	}
	list := container.NewWithoutLayout(objects...)
	show := func(frame int) {
		first := patchworkScrollFirst(frame, total, g.tiles.rows)
		list.Move(fyne.NewPos(0, -float32(first)*pitch))
		for row := first; row < min(first+visible, total); row++ {
			r := rows[row%visible]
			if r.row == row {
				continue
			}
			r.row = row
			r.box.Move(fyne.NewPos(0, wallGap+float32(row)*pitch))
			for column, t := range r.tiles {
				index := row*columns + column
				t.bind(index, palette(index))
			}
		}
	}
	return patchView{container.NewWithoutLayout(list), show}
}

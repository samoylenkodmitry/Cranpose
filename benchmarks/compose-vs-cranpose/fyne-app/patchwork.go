// The patchwork's data, as `perf-data/src/patchwork.rs` makes it bit for bit:
// nine patches of the still wall's tiles, three across and three down. Two
// never change; in the other seven a frame changes a small part and leaves
// the rest as it was.

package main

import (
	"fmt"
	"math"
	"strings"
)

const (
	patchColumns = 3
	patchRows    = 3
)

// patchTiles is one patch's tiles: columns by rows of them, width by height
// points each.
type patchTiles struct {
	columns, rows int
	width, height float32
}

func newPatchTiles(t wallTier, patchWidth, patchHeight float32) patchTiles {
	columns, rows := wallGrid(t, patchWidth, patchHeight)
	return patchTiles{columns, rows, t.width, t.height}
}

func (p patchTiles) count() int { return p.columns * p.rows }

// jump is the step, in tiles or words, of everything that jumps.
const jump = 37

// inTurn is the items of a patch that change in turn: columns by rows of
// them. Each frame changes the items of one row, the next row down the frame
// after.
type inTurn struct{ columns, rows int }

// step is the changes of item `index` up to frame `frame`.
func (t inTurn) step(index, frame int) int {
	return (index/max(t.columns, 1) + frame) / max(t.rows, 1)
}

// row is the row whose items change in frame `frame` (against the frame
// before): the row `r` for which the step of its items grows.
func (t inTurn) row(frame int) int {
	rows := max(t.rows, 1)
	return (rows - frame%rows) % rows
}

const (
	patchworkChartPoints = 512
	patchworkChartStroke = 1
)

// patchworkChartValue is the height of chart `index` at `point`, as a
// fraction of the chart.
func patchworkChartValue(index, point int) float32 {
	at := float32(point) * 0.125
	first := float32(math.Sin(float64(float32(at*0.32) + float32(float32(index)*0.77))))
	second := float32(math.Sin(float64(float32(at*1.7) + float32(index))))
	return float32(0.5) + float32(0.38*first) + float32(0.08*second)
}

// patchworkRecolorRGB is the fill of tile `index` of the recolor patch in
// frame `frame`.
func patchworkRecolorRGB(index, frame int, turn inTurn) uint32 {
	return paletteRGB[(index+turn.step(index, frame))%len(paletteRGB)]
}

// patchworkCardFrames is the frames one move of the cards patch takes.
const patchworkCardFrames = 32

// cards is the cards patch: a card is `columns` tiles across and `rows` rows
// of tiles under a header, `height` points tall, and `slots` cards rest in
// the patch.
type cards struct {
	columns, rows int
	height        float32
	slots         int
}

// cardPlace is a card of the cards patch in a frame: its number and its top
// in points from the patch's top.
type cardPlace struct {
	id  int
	top float32
}

func newCards(columns, rows int, tileHeight, patchHeight float32) cards {
	cardRows := max(rows/4, 1)
	height := tileHeight + float32(cardRows)*(tileHeight+wallGap)
	return cards{columns, cardRows, height, max(int((patchHeight-wallGap)/(height+wallGap)), 1)}
}

func (c cards) places() int { return c.slots + 1 }

func (c cards) tiles() int { return c.columns * c.rows }

// place is the card at place `place` in frame `frame`, the topmost first;
// none at the last place while no card is on its way in. The moves
// alternate: a new card slides in and the others move down by a card, then
// the first and the third card move to each other's places and back.
func (c cards) place(place, frame int) (cardPlace, bool) {
	moves := frame / patchworkCardFrames
	along := float32(frame%patchworkCardFrames) / patchworkCardFrames
	pitch := c.height + wallGap
	came := (moves+1)/2 + (moves+1)%2
	if moves%2 == 0 {
		id := came + c.slots - 1 - place
		if id < 0 {
			return cardPlace{}, false
		}
		return cardPlace{id, wallGap + (float32(place)-1+along)*pitch}, true
	}
	if place >= c.slots {
		return cardPlace{}, false
	}
	other := min(2, c.slots-1)
	reach := float32(other) * pitch * (1 - float32(math.Abs(float64(2*along-1))))
	moved := float32(0)
	switch place {
	case 0:
		moved = reach
	case other:
		moved = -reach
	}
	return cardPlace{came + c.slots - 1 - place, wallGap + float32(place)*pitch + moved}, true
}

func patchworkCardTitle(id int) string { return fmt.Sprintf("Group %05d", id%100_000) }

func patchworkCardRGB(id int) uint32 { return gradientEndRGB[id%len(gradientEndRGB)] }

// patchworkCardTile is the wall tile at place `tile` of card `id`, whose
// card holds `tiles` tiles.
func patchworkCardTile(id, tile, tiles int) int { return (id*tiles + tile) % 100_000 }

// patchworkReorderCount is the items of the reorder patch for a patch of
// `tiles` tiles.
func patchworkReorderCount(tiles int) int { return max(tiles*2/5, 1) }

// patchworkReorderSwap is the first of the two neighbouring places whose
// items change places in frame `frame`.
func patchworkReorderSwap(frame, count int) int { return frame * jump % max(count-1, 1) }

// patchworkReorderItem is the item at place `position` of the reorder patch
// in frame `frame`.
func patchworkReorderItem(position, frame, count int) int {
	swap := patchworkReorderSwap(frame, count)
	switch {
	case count < 2:
		return position
	case position == swap:
		return position + 1
	case position == swap+1:
		return position - 1
	}
	return position
}

// patchworkReorderUnits is the width of item `item` of the reorder patch, in
// tiles: 1 to 3.
func patchworkReorderUnits(item int) int {
	return int(uint32(item)*2_654_435_761>>22)%3 + 1
}

// patchworkReorderWidth is the width in points of an item `units` tiles wide.
func patchworkReorderWidth(units int, tileWidth float32) float32 {
	return float32(units)*tileWidth + float32(units-1)*wallGap
}

// patchworkDepth is the boxes around the digits of one nest of the deep patch.
const patchworkDepth = 12

// patchworkPairs is the nests or the charts across and down a patch of
// `columns` by `rows` tiles.
func patchworkPairs(columns, rows int) (int, int) { return max(columns/2, 1), max(rows/2, 1) }

// patchworkDouble is the width or height of an item that covers 2 tiles of
// `tile` points and the gap between them.
func patchworkDouble(tile float32) float32 { return 2*tile + wallGap }

// patchworkDeepNumber is the number inside nest `index` in frame `frame`,
// under 1000.
func patchworkDeepNumber(index, frame int, turn inTurn) int {
	return (index*7 + turn.step(index, frame)) % 1000
}

// patchworkScrollRows is the rows of the scroll patch's list for rows of
// `columns` tiles.
func patchworkScrollRows(columns int) int { return 100_000 / max(columns, 1) }

// patchworkScrollFirst is the first row the scroll patch shows in frame
// `frame`, of a list of `total` rows of which `visible` show at once.
func patchworkScrollFirst(frame, total, visible int) int {
	return frame % (max(total-visible, 0) + 1)
}

// patchworkParagraphWords is the words of the paragraph patch for a patch of
// `tiles` tiles.
func patchworkParagraphWords(tiles int) int { return tiles * 3 }

// patchworkParagraph is the paragraph in frame `frame`: `words` words a
// space apart, each its index in five digits, but for one that shows the
// frame instead, another one every frame.
func patchworkParagraph(frame, words int) string {
	changed := frame * jump % max(words, 1)
	var text strings.Builder
	text.Grow(words * 6)
	for word := range words {
		if word > 0 {
			text.WriteByte(' ')
		}
		if word == changed {
			text.WriteString(wallLabel(frame % 100_000))
		} else {
			text.WriteString(wallLabel(word))
		}
	}
	return text.String()
}

// patchworkPileGrid is the tiles across and down the pile patch for a patch
// of `columns` by `rows` tiles.
func patchworkPileGrid(columns, rows int) (int, int) { return max(columns-1, 1), max(rows-1, 1) }

// patchworkPileAlpha is the opacity of the pile patch's tiles' fills, of 255.
const patchworkPileAlpha = 0x99

// patchworkPileOpaque is the tile of the pile patch whose fill is opaque,
// black, in frame `frame`.
func patchworkPileOpaque(frame, tiles int) int { return frame * jump % max(tiles, 1) }

// The chat beside the patchwork in Fyne (`../README.md`): an agent's answers
// arrive from a data goroutine as tokens and the list shows them as they
// come.
//
// In Fyne's idiom a block is a container of canvas objects made once, when
// the block begins, and a token sets only the text of the last block: its last
// lines, the row of its table. A block never moves once it is placed, the
// list is one container that moves to keep the end in view, and a frame takes
// the tokens that arrived, sets the last block once and the status line once.
// The list keeps the blocks of the last two turns; the rest leave it.
package main

import (
	"image/color"
	"slices"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
)

const (
	chatPadding = 8
	chatGap     = 8
	// chatTurnsHeld is the turns whose blocks the list holds: more than it shows.
	chatTurnsHeld = 2
)

// chatItem is one block of the chat on screen.
type chatItem interface {
	box() *fyne.Container
	height() float32
	// show shows the first `tokens` tokens of the block.
	show(tokens int)
}

// block is what every kind of block shares; a block that shows all of itself
// at once has nothing to show.
type block struct {
	c *fyne.Container
	h float32
}

func (b *block) box() *fyne.Container { return b.c }
func (b *block) height() float32      { return b.h }
func (b *block) show(int)             {}

// listed is a block in the list and its top.
type listed struct {
	item chatItem
	top  float32
}

// chatView is the chat: the list's clip and content, and the status line.
type chatView struct {
	feed   chatFeed
	assets chatAssets
	// content holds the blocks; it moves up as they grow past the clip.
	content *fyne.Container
	clip    *container.Scroll
	back    *canvas.Rectangle
	status  *statusLine
	blocks  []listed
	// first is the number of the first block held, since the chat began.
	first int
	token chatToken
	// codeColors is the color of each byte of a code block's text.
	codeColors []color.NRGBA
	height     float32
	x, y       float32
	statusBack *canvas.Rectangle
}

func newChat(l launch, x, y, height float32) *chatView {
	c := &chatView{
		feed: startChatFeed(), assets: loadChatAssets(l.chat), content: container.NewWithoutLayout(),
		back: canvas.NewRectangle(rgb(chatBackgroundRGB)), statusBack: canvas.NewRectangle(rgb(chatPanelRGB)),
		height: height - chatStatusHeight, x: x, y: y,
	}
	c.codeColors = codeColors(chatCodeSource)
	place(c.back, x, y, chatWidth, height)
	c.clip = newClip(x, y, chatWidth, c.height, c.content)
	place(c.statusBack, x, y+c.height, chatWidth, chatStatusHeight)
	c.status = newStatusLine(c.assets, x, y+c.height)
	return c
}

// objects are the chat's objects in the order they are drawn.
func (c *chatView) objects() []fyne.CanvasObject {
	return []fyne.CanvasObject{c.back, c.clip, c.statusBack, c.status.box}
}

// codeHead is the line above a code block that names its turn.
func codeHead(turn int) string { return "// " + chatTurnLabel(turn) + "\n" }

// codeColors is the color of each byte of a code block's text: its head, then
// the code with its stretches colored. A turn's number is as long as any
// other, so one array serves every block.
func codeColors(code string) []color.NRGBA {
	head := len(codeHead(0))
	colors := make([]color.NRGBA, head+len(code))
	for index := range colors {
		colors[index] = rgb(codeRGB[codePlain])
		if index < head {
			colors[index] = rgb(codeRGB[codeComment])
		}
	}
	for _, span := range chatCodeSpans(code) {
		for at := span.start; at < span.end; at++ {
			colors[head+at] = rgb(codeRGB[span.kind])
		}
	}
	return colors
}

// show takes the frame's tokens, shows them in the list's last block and
// turns the status line.
func (c *chatView) show(frame int) {
	arrived := false
	for range chatTokensPerFrame {
		token, ok := c.feed.take()
		if !ok {
			break
		}
		c.begin(token)
		c.token, arrived = token, true
	}
	if arrived {
		last := c.blocks[len(c.blocks)-1].item
		last.show(c.token.shown)
		c.follow()
	}
	c.status.show(c.token, frame)
	c.animate(frame)
}

// begin adds the block of `token` when it is a new one, after the block
// before it showed all of itself.
func (c *chatView) begin(token chatToken) {
	if token.block < c.first+len(c.blocks) {
		return
	}
	if len(c.blocks) > 0 {
		c.blocks[len(c.blocks)-1].item.show(chatTurn[(token.block-1)%len(chatTurn)].tokens())
	}
	c.drop(chatTurnOf(token.block))
	top := float32(chatPadding)
	if len(c.blocks) > 0 {
		last := c.blocks[len(c.blocks)-1]
		top = last.top + last.item.height() + chatGap
	}
	item := c.newItem(token.block)
	item.box().Move(fyne.NewPos(chatPadding, top))
	c.content.Add(item.box())
	c.blocks = append(c.blocks, listed{item, top})
}

// drop removes the blocks of turns that the list no longer holds, and moves
// the others up to where the list starts.
func (c *chatView) drop(turn int) {
	count := 0
	for count < len(c.blocks) && chatTurnOf(c.first+count) < turn-chatTurnsHeld+1 {
		count++
	}
	if count == 0 {
		return
	}
	c.content.Objects = slices.Delete(c.content.Objects, 0, count)
	c.first += count
	c.blocks = slices.Delete(c.blocks, 0, count)
	if len(c.blocks) == 0 {
		return
	}
	shift := c.blocks[0].top - chatPadding
	for index := range c.blocks {
		c.blocks[index].top -= shift
		c.blocks[index].item.box().Move(fyne.NewPos(chatPadding, c.blocks[index].top))
	}
}

// follow keeps the end of the list in view.
func (c *chatView) follow() {
	last := c.blocks[len(c.blocks)-1]
	total := last.top + last.item.height() + chatPadding
	c.content.Move(fyne.NewPos(0, min(0, c.height-total)))
}

// animate shows the picture of frame `frame` in the animated blocks in view.
func (c *chatView) animate(frame int) {
	offset := c.content.Position().Y
	for index := len(c.blocks) - 1; index >= 0; index-- {
		b := c.blocks[index]
		if b.top+b.item.height()+offset < 0 {
			return
		}
		if picture, ok := b.item.(*gifBlock); ok {
			picture.animate(frame)
		}
	}
}

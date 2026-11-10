package main

import (
	"image"
	"image/color"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/widget"
)

const chatBlockWidth = chatWidth - 2*chatPadding

var (
	chatInk    = style{chatText, false, false}
	chatBold   = style{chatText, true, false}
	chatHeader = style{chatHeadingText, true, false}
	chatMono   = style{chatCodeText, false, true}
	chatCell   = style{chatCodeText, false, false}
	chatCellB  = style{chatCodeText, true, false}
)

// newItem is block `index` of the chat, empty if it is typed.
func (c *chatView) newItem(index int) chatItem {
	spec := chatTurn[index%len(chatTurn)]
	label := chatTurnLabel(chatTurnOf(index))
	switch spec.kind {
	case chatUser:
		return newUserBlock(spec.text)
	case chatHeading:
		return newTypedBlock(label, spec.text, chatHeader)
	case chatParagraph:
		return newTypedBlock(label, spec.text, chatInk)
	case chatCode:
		return newCodeBlock(codeHead(chatTurnOf(index)), spec.text, c.codeColors)
	case chatTable:
		return newTableBlock(spec.rows)
	case chatGif:
		return newGifBlock(c.assets.gif)
	}
	return newButtonsBlock(spec.labels)
}

// newUserBlock is the user's question, whole at once in a bubble at the right.
func newUserBlock(text string) chatItem {
	size := measure(text, chatInk)
	width, height := size.Width+20, size.Height+12
	bubble := canvas.NewRectangle(rgb(chatUserRGB))
	bubble.CornerRadius = 10
	place(bubble, chatBlockWidth-width, 0, width, height)
	words := newText(text, white, chatInk)
	place(words, chatBlockWidth-width+10, 6, size.Width, size.Height)
	return &block{container.NewWithoutLayout(bubble, words), height}
}

// typedBlock is a paragraph or a heading: the turn's number, then as much of
// the text as arrived.
type typedBlock struct {
	block
	flow  *flowText
	label int
	text  string
}

func newTypedBlock(label, text string, s style) chatItem {
	b := &typedBlock{block: block{c: container.NewWithoutLayout()}, label: len(label) + 1, text: text}
	b.flow = newFlowText(b.c, fyne.NewPos(0, 0), label+" "+text, nil, chatBlockWidth, s)
	return b
}

func (b *typedBlock) show(tokens int) {
	b.flow.show(b.label + chatShownChars(b.text, tokens))
	b.h = b.flow.height()
}

// codeBlock is code on a panel, with its stretches colored.
type codeBlock struct {
	block
	panel *canvas.Rectangle
	flow  *flowText
	head  int
	code  string
}

func newCodeBlock(head, code string, colors []color.NRGBA) chatItem {
	b := &codeBlock{block: block{c: container.NewWithoutLayout()}, panel: canvas.NewRectangle(rgb(chatPanelRGB)), head: len(head), code: code}
	b.panel.CornerRadius = 6
	b.c.Add(b.panel)
	b.flow = newFlowText(b.c, fyne.NewPos(chatPadding, chatPadding), head+code, colors, chatBlockWidth-2*chatPadding, chatMono)
	return b
}

func (b *codeBlock) show(tokens int) {
	b.flow.show(b.head + chatShownChars(b.code, tokens))
	b.h = b.flow.height() + 2*chatPadding
	place(b.panel, 0, 0, chatBlockWidth, b.h)
}

// tableBlock is a table whose first row is its header; a row appears a token.
type tableBlock struct {
	block
	rows  [][chatTableColumns]string
	added int
}

func newTableBlock(rows [][chatTableColumns]string) chatItem {
	return &tableBlock{block: block{c: container.NewWithoutLayout()}, rows: rows}
}

func (b *tableBlock) show(tokens int) {
	rowHeight := measure("Ag", chatCell).Height + 6
	column := float32(chatBlockWidth-12) / chatTableColumns
	for ; b.added < min(tokens, len(b.rows)); b.added++ {
		top := float32(b.added) * rowHeight
		if b.added%2 == 1 || b.added == 0 {
			panel := canvas.NewRectangle(rgb(chatPanelRGB))
			place(panel, 0, top, chatBlockWidth, rowHeight)
			b.c.Add(panel)
		}
		s := chatCell
		if b.added == 0 {
			s = chatCellB
		}
		for index, cell := range b.rows[b.added] {
			text := newText(cell, white, s)
			size := measure(cell, s)
			place(text, 6+float32(index)*column, top+3, size.Width, size.Height)
			b.c.Add(text)
		}
	}
	b.h = float32(b.added) * rowHeight
}

// gifBlock is the animated picture: a picture object for each frame, one of
// which shows.
type gifBlock struct {
	block
	frames []*canvas.Image
	shown  int
}

func newGifBlock(frames []image.Image) chatItem {
	b := &gifBlock{block: block{c: container.NewWithoutLayout(), h: chatGifSize}}
	for index, frame := range frames {
		picture := canvas.NewImageFromImage(frame)
		place(picture, 0, 0, chatGifSize, chatGifSize)
		if index > 0 {
			picture.Hide()
		}
		b.frames = append(b.frames, picture)
		b.c.Add(picture)
	}
	return b
}

// animate shows the picture's frame for screen frame `frame`.
func (b *gifBlock) animate(frame int) {
	shown := chatGifFrame(frame, len(b.frames))
	if shown != b.shown {
		b.frames[b.shown].Hide()
		b.frames[shown].Show()
		b.shown = shown
	}
}

// newButtonsBlock is a row of buttons: Fyne's buttons under a theme that
// gives them the chat's fill, white bold text and no padding of their own;
// each is sized to its text and 10 by 6 points of room.
func newButtonsBlock(labels []string) chatItem {
	b := &block{c: container.NewWithoutLayout()}
	th := widgetTheme{
		Theme: fyne.CurrentApp().Settings().Theme(), text: chatText, foreground: white,
		button: rgb(chatUserRGB), radius: 6,
	}
	left := float32(0)
	for _, label := range labels {
		size := measure(label, chatBold)
		button := container.NewThemeOverride(widget.NewButton(label, nil), th)
		place(button, left, 0, size.Width+20, size.Height+12)
		b.c.Add(button)
		b.h = size.Height + 12
		left += size.Width + 20 + 8
	}
	return b
}

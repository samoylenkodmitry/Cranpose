package main

import (
	"image/color"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
)

// statusLine is the line under the chat's list: a spinner and what the agent
// does. Fyne draws no rotated object and colors a text in one color, so the
// spinner is a ring of pictures of it turned to each angle, one of which
// shows, and the text is one canvas text per character, each in the color
// the gradient has there.
type statusLine struct {
	box      *fyne.Container
	spinner  []*canvas.Image
	turn     int
	glyphs   []*canvas.Text
	advances map[rune]float32
	text     style
	// left and top are where the text starts.
	left, top float32
}

func newStatusLine(assets chatAssets, x, y float32) *statusLine {
	s := &statusLine{box: container.NewWithoutLayout(), advances: map[rune]float32{}, text: style{chatText, false, false}}
	for turn, picture := range assets.spinner {
		spinner := canvas.NewImageFromImage(picture)
		place(spinner, x+8, y+(chatStatusHeight-chatSpinnerSize)/2, chatSpinnerSize, chatSpinnerSize)
		if turn > 0 {
			spinner.Hide()
		}
		s.spinner = append(s.spinner, spinner)
		s.box.Add(spinner)
	}
	s.left = x + 8 + chatSpinnerSize + 6
	s.top = y + (chatStatusHeight-measure("Ag", s.text).Height)/2
	return s
}

// show turns the spinner to frame `frame` and sets the text for `token`
// under the gradient of the frame.
func (s *statusLine) show(token chatToken, frame int) {
	turn := frame % len(s.spinner)
	if turn != s.turn {
		s.spinner[s.turn].Hide()
		s.spinner[turn].Show()
		s.turn = turn
	}
	dim, bright := rgb(chatShimmerDimRGB), white
	channel := func(from, to uint8, weight float32) uint8 {
		return uint8(float32(from) + (float32(to)-float32(from))*weight)
	}
	along := float32(0)
	glyphs := 0
	for _, character := range chatStatus(token) {
		advance, ok := s.advances[character]
		if !ok {
			advance = measure(string(character), s.text).Width
			s.advances[character] = advance
		}
		if character != ' ' {
			for len(s.glyphs) <= glyphs {
				glyph := newText(" ", white, s.text)
				s.glyphs = append(s.glyphs, glyph)
				s.box.Add(glyph)
			}
			glyph := s.glyphs[glyphs]
			weight := chatShimmer(along, frame)
			ink := color.NRGBA{channel(dim.R, bright.R, weight), channel(dim.G, bright.G, weight), channel(dim.B, bright.B, weight), 0xFF}
			setText(glyph, string(character), ink)
			place(glyph, s.left+along, s.top, advance, s.text.size)
			glyph.Show()
			glyphs++
		}
		along += advance
	}
	for _, glyph := range s.glyphs[glyphs:] {
		glyph.Hide()
	}
}

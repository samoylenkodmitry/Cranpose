package main

import (
	"image/color"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
)

// span is one line of a wrapped text, as byte offsets.
type span struct{ start, end int }

// flowText is a text wrapped by the app, as the gauntlet wraps its paragraphs
// (Fyne's layouts size objects without asking for a width): one canvas text
// per line, and one per color within a line. Text that only grows keeps every
// line but its last: a token sets the text of the last line and of the lines
// that came after it, and the other lines stay as they were.
type flowText struct {
	box    *fyne.Container
	origin fyne.Position
	full   string
	// colors is the color of each byte of `full`; nil for one color.
	colors []color.NRGBA
	s      style
	width  float32
	step   float32
	line   float32
	space  float32
	// word is the width of a run of letters; chars is the width of a
	// character of a monospaced text.
	word  func(string) float32
	chars float32
	spans []span
	lines [][]*canvas.Text
}

// newFlowText is `full` in lines `width` wide, at `origin` in `box`, in
// style `s`; its objects join `box` as they are needed.
func newFlowText(box *fyne.Container, origin fyne.Position, full string, colors []color.NRGBA, width float32, s style) *flowText {
	f := &flowText{
		box: box, origin: origin, full: full, colors: colors, s: s, width: width,
		step: s.size * 1.4, line: measure("Ag", s).Height,
	}
	if s.mono {
		f.chars = measure("0", s).Width
		f.space = f.chars
		f.word = func(text string) float32 { return float32(len(text)) * f.chars }
	} else {
		f.space = measure(" ", s).Width
		f.word = func(text string) float32 { return measure(text, s).Width }
	}
	return f
}

// height is the height of the lines so far.
func (f *flowText) height() float32 {
	if len(f.spans) == 0 {
		return 0
	}
	return float32(len(f.spans)-1)*f.step + f.line
}

// show shows the first `chars` bytes of the text.
func (f *flowText) show(chars int) {
	shown := f.full[:chars]
	keep := max(len(f.spans)-1, 0)
	from := 0
	if keep < len(f.spans) {
		from = f.spans[keep].start
	}
	f.spans = f.wrap(shown, f.spans[:keep], from)
	for index := keep; index < len(f.spans); index++ {
		f.setLine(index, shown)
	}
}

// wrap appends to `spans` the lines of `shown` from byte `from`, where a
// line starts, to its end: a word goes to the next line when it does not fit.
func (f *flowText) wrap(shown string, spans []span, from int) []span {
	for pos := from; ; {
		start, at, end, along := pos, pos, pos, float32(0)
		next := -1
		for at < len(shown) && shown[at] != '\n' {
			word := at
			for word < len(shown) && shown[word] == ' ' {
				word++
			}
			stop := word
			for stop < len(shown) && shown[stop] != ' ' && shown[stop] != '\n' {
				stop++
			}
			run := float32(word-at)*f.space + f.word(shown[word:stop])
			if along > 0 && along+run > f.width {
				next = word
				break
			}
			along += run
			at, end = stop, stop
		}
		spans = append(spans, span{start, end})
		switch {
		case next >= 0:
			pos = next
		case at < len(shown):
			pos = at + 1
		default:
			return spans
		}
	}
}

// setLine sets line `index` to its stretch of `shown`.
func (f *flowText) setLine(index int, shown string) {
	for len(f.lines) <= index {
		f.lines = append(f.lines, nil)
	}
	line := f.spans[index]
	used := 0
	for at := line.start; at < line.end; used++ {
		end := line.end
		ink := white
		if f.colors != nil {
			ink = f.colors[at]
			end = at + 1
			for end < line.end && f.colors[end] == ink {
				end++
			}
		}
		if used == len(f.lines[index]) {
			part := newText("", white, f.s)
			f.lines[index] = append(f.lines[index], part)
			f.box.Add(part)
		}
		part := f.lines[index][used]
		setText(part, shown[at:end], ink)
		place(part, f.origin.X+float32(at-line.start)*f.chars, f.origin.Y+float32(index)*f.step, f.width, f.line)
		part.Show()
		at = end
	}
	for _, part := range f.lines[index][used:] {
		part.Hide()
	}
}

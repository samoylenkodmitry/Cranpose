// The chat beside the patchwork, as `perf-data/src/chat.rs` makes it bit for
// bit: an agent's answers arrive from a data goroutine as tokens, several a
// frame, and the screen shows them as they come. Under the list a status
// line says what the agent does.

package main

import (
	"fmt"
	"math"
	"slices"
	"strings"
)

const (
	chatWidth          = 320
	chatTokensPerFrame = 4
	chatTokenChars     = 6
	chatTableColumns   = 4
	chatGifHold        = 2
	chatStatusHeight   = 28
	chatSpinnerSize    = 16
	chatSpinDegrees    = 12
	chatShimmerStep    = 6
	chatShimmerSpan    = 120
	chatGifSize        = 64
	chatText           = 12
	chatHeadingText    = 14
	chatCodeText       = 11
)

const (
	chatBackgroundRGB = 0x14182B
	chatUserRGB       = 0x2B3A67
	chatPanelRGB      = 0x0B0E1A
	chatShimmerDimRGB = 0x64748B
)

// chatKind is what one block of a turn is.
type chatKind int

const (
	chatUser chatKind = iota
	chatHeading
	chatParagraph
	chatCode
	chatTable
	chatGif
	chatButtons
)

// chatBlock is one block of a turn: its text, or the rows of a table, or the
// labels of the buttons.
type chatBlock struct {
	kind   chatKind
	text   string
	rows   [][chatTableColumns]string
	labels []string
}

const chatCodeSource = "// Measure only the row that changed.\n" +
	"fn relayout(rows: &mut [Row], dirty: usize) -> f32 {\n" +
	"    let row = &mut rows[dirty];\n" +
	"    if row.width == 320.0 {\n" +
	"        return row.height;\n" +
	"    }\n" +
	"    row.label = \"measured\";\n" +
	"    row.height = row.measure(320.0);\n" +
	"    row.height\n" +
	"}\n" +
	"\n" +
	"// A row of fixed height ends the pass.\n" +
	"fn place(rows: &mut [Row], first: usize) {\n" +
	"    let mut top = 0.0;\n" +
	"    for row in &mut rows[first..] {\n" +
	"        row.top = top;\n" +
	"        top = top + row.height;\n" +
	"    }\n" +
	"}\n"

// chatTurn is one turn of the chat: a question and a long answer, most of
// which is text that arrives a few characters a token.
var chatTurn = [15]chatBlock{
	{kind: chatUser, text: "Why does the list stutter when one row changes?"},
	{kind: chatHeading, text: "What happens in a frame"},
	{kind: chatParagraph, text: "The list stutters because a small change starts a large amount of work. The row " +
		"that changed shows a counter, and the counter gets a new value on every frame. " +
		"The row measures its text again, which is right, because the text is new. The " +
		"row then tells its parent that its size may have changed. The parent does not " +
		"check whether the size really changed. It measures and places every one of its " +
		"children again, the ones on the screen and the ones kept ready beyond its edges. " +
		"Each child measures its own text again, although none of that text changed. A " +
		"frame that should cost one row costs the whole list, and the cost grows with " +
		"the number of rows, not with the size of the change."},
	{kind: chatParagraph, text: "The profile shows the same thing from the other side. Most of the frame goes to " +
		"text shaping, and nearly all of the shaped strings are equal to the strings of " +
		"the frame before. The cache of shaped text does not help, because the list asks " +
		"for each string at a width that differs by a fraction of a point from the width " +
		"of the last frame, so every lookup misses. Drawing takes a small part of the " +
		"frame and does not change, because the picture is almost the same."},
	{kind: chatHeading, text: "Three things to change"},
	{kind: chatParagraph, text: "- Give the row a fixed height. A row whose size cannot change needs no new " +
		"layout of its parent, so the work stops at the row."},
	{kind: chatParagraph, text: "- Round the width that the list passes to its rows to whole points. Equal " +
		"strings then meet equal widths, and the cache of shaped text answers."},
	{kind: chatParagraph, text: "- Read the counter in the draw phase where the toolkit allows it. A value " +
		"that changes only what is drawn should not start composition or layout."},
	{kind: chatCode, text: chatCodeSource},
	{kind: chatParagraph, text: "With the boundary in place the frame lays out one row and leaves the others " +
		"alone. I ran the list of two thousand rows before and after the change, on the " +
		"same device and at the same scroll position. These are the passes of one frame, " +
		"as the median of three hundred frames:"},
	{kind: chatTable, rows: [][chatTableColumns]string{
		{"Pass", "Before", "After", "Change"},
		{"Compose", "1.92 ms", "0.31 ms", "-84%"},
		{"Measure", "4.70 ms", "0.22 ms", "-95%"},
		{"Place", "1.18 ms", "0.09 ms", "-92%"},
		{"Draw", "2.46 ms", "2.41 ms", "-2%"},
		{"Frame", "10.26 ms", "3.03 ms", "-70%"},
	}},
	{kind: chatGif},
	{kind: chatParagraph, text: "Drawing is now the largest pass, and it did not move. The next step is to keep " +
		"the drawing of the rows that did not change and to draw only the row with the " +
		"counter. That needs a look at how the list records its rows, which I have not " +
		"read yet. The picture above turns while the benchmark runs; it stops when the " +
		"run ends. Tell me whether to apply the three changes, or to show the diff first."},
	{kind: chatButtons, labels: []string{"Apply", "Show diff", "Dismiss"}},
	{kind: chatParagraph, text: "Waiting for your answer."},
}

// status is what the status line says the agent does while the block is the
// last one.
func (b chatBlock) status() string {
	switch b.kind {
	case chatUser:
		return "Reading"
	case chatHeading, chatParagraph:
		return "Writing"
	case chatCode:
		return "Writing code"
	case chatTable:
		return "Filling the table"
	case chatGif:
		return "Drawing"
	}
	return "Waiting"
}

// tokens is the tokens the block takes to appear whole.
func (b chatBlock) tokens() int {
	switch b.kind {
	case chatHeading, chatParagraph, chatCode:
		return (len(b.text) + chatTokenChars - 1) / chatTokenChars
	case chatTable:
		return len(b.rows)
	}
	return 1
}

// chatToken is what the data goroutine sends: the chat's last block and how
// much of it shows. Every block before it shows whole.
type chatToken struct {
	// count is the number of this token since the chat began, from 1.
	count int
	// block is the index of the last block since the chat began: block
	// `block` is chatTurn[block%len(chatTurn)] of turn block/len(chatTurn).
	block int
	// shown is the tokens of that block that arrived.
	shown int
}

var chatStarts, chatTurnTokens = func() (starts [len(chatTurn)]int, total int) {
	for index, block := range chatTurn {
		starts[index] = total
		total += block.tokens()
	}
	return
}()

// chatTokenAfter is the chat after `count` tokens, `count` at least 1.
func chatTokenAfter(count int) chatToken {
	index := max(count, 1) - 1
	turn, within := index/chatTurnTokens, index%chatTurnTokens
	block := len(chatStarts) - 1
	for chatStarts[block] > within {
		block--
	}
	return chatToken{max(count, 1), turn*len(chatTurn) + block, within - chatStarts[block] + 1}
}

// chatTurnOf is the turn of block `block`.
func chatTurnOf(block int) int { return block / len(chatTurn) }

// chatShownChars is the characters of `text` that `shown` tokens show.
func chatShownChars(text string, shown int) int { return min(shown*chatTokenChars, len(text)) }

// chatStatus is the status line's text after token `token`.
func chatStatus(token chatToken) string {
	return fmt.Sprintf("%s… (%06d tokens)", chatTurn[token.block%len(chatTurn)].status(), token.count%1_000_000)
}

// chatShimmer is how bright the status line's gradient is `x` points from
// the text's start in frame `frame`, from 0 (dim) to 1 (bright).
func chatShimmer(x float32, frame int) float32 {
	remainder := func(value float32) float32 { return float32(math.Mod(float64(value), chatShimmerSpan)) }
	start := remainder(float32(frame) * chatShimmerStep)
	along := remainder(remainder(x-start)+chatShimmerSpan) / chatShimmerSpan
	return 1 - float32(math.Abs(float64(2*along-1)))
}

// chatTurnLabel is the line above a paragraph or a code block that names its
// turn.
func chatTurnLabel(turn int) string { return fmt.Sprintf("#%05d", turn%100_000) }

// codeKind is what a stretch of code is, for its color.
type codeKind int

const (
	codePlain codeKind = iota
	codeKeyword
	codeText
	codeNumber
	codeComment
)

// codeRGB is the color of code of kind `k`.
var codeRGB = [...]uint32{0xFFFFFF, 0xC084FC, 0x86EFAC, 0xFDBA74, 0x64748B}

var codeKeywords = [...]string{"fn", "let", "mut", "if", "for", "in", "return", "usize", "f32", "Row"}

// codeSpan is a colored stretch of code, as byte offsets.
type codeSpan struct {
	start, end int
	kind       codeKind
}

func isAlpha(b byte) bool { return b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' }
func isDigit(b byte) bool { return b >= '0' && b <= '9' }

// chatCodeSpans is the colored stretches of `code`, in order: its keywords,
// its quoted text, its numbers and its `//` comments.
func chatCodeSpans(code string) []codeSpan {
	var spans []codeSpan
	at := 0
	for at < len(code) {
		start := at
		b := code[at]
		switch {
		case b == '/' && at+1 < len(code) && code[at+1] == '/':
			at = len(code)
			if end := strings.IndexByte(code[start:], '\n'); end >= 0 {
				at = start + end
			}
			spans = append(spans, codeSpan{start, at, codeComment})
		case b == '"':
			at = len(code)
			if end := strings.IndexByte(code[start+1:], '"'); end >= 0 {
				at = start + end + 2
			}
			spans = append(spans, codeSpan{start, at, codeText})
		case isAlpha(b) || b == '_':
			for at < len(code) && (isAlpha(code[at]) || isDigit(code[at]) || code[at] == '_') {
				at++
			}
			if slices.Contains(codeKeywords[:], code[start:at]) {
				spans = append(spans, codeSpan{start, at, codeKeyword})
			}
		case isDigit(b):
			for at < len(code) && (isDigit(code[at]) || code[at] == '.') {
				at++
			}
			spans = append(spans, codeSpan{start, at, codeNumber})
		default:
			at++
		}
	}
	return spans
}

// chatGifFrame is the frame of the animated picture that frame `frame` of
// the screen shows, of `frames` frames.
func chatGifFrame(frame, frames int) int { return frame / chatGifHold % max(frames, 1) }

// chatFeed is the tokens of the chat as they leave the data layer: a
// goroutine of its own sends them through a bounded channel and waits while
// the screen is behind.
type chatFeed chan chatToken

func startChatFeed() chatFeed {
	feed := make(chatFeed, chatTokensPerFrame*4)
	go func() {
		for count := 1; ; count++ {
			feed <- chatTokenAfter(count)
		}
	}()
	return feed
}

// take is the next token, when one has arrived. The screen never waits for
// one.
func (f chatFeed) take() (chatToken, bool) {
	select {
	case token := <-f:
		return token, true
	default:
		return chatToken{}, false
	}
}

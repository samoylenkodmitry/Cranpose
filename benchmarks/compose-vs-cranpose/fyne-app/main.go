// The gauntlet in Fyne: the screen `compose-app/.../Gauntlet.kt` draws,
// element for element, in Fyne's own idiom: canvas objects in containers the
// app lays out itself, as Fyne's custom widgets do. Fyne's layouts size
// objects without asking for a width first, so the app wraps paragraphs and
// places the cards and rows by hand. The benchmark's README describes it;
// `desktop.py` runs it with the tier in `PERF_TIER` and the Roboto files in
// `PERF_FONTS`.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. A Fyne animation, which the driver ticks once per frame it
// draws, advances it, sets every value of the frame and places every object.
package main

import (
	"fmt"
	"image"
	"image/color"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/theme"
)

// The window every desktop app opens for the gauntlet, in points.
const windowWidth, windowHeight = 1280, 820

// Blocks of five card rows and a cluster: no measurement window reaches the end.
const rows = 2000 * (cardRowsPerCluster + 1)

// How far the list scrolls each frame, in points.
const scrollPerFrame = 3

// The segments of the line one cover polygon spans: with its two top corners
// it stays within the 16 vertices Fyne fills.
const coverPoints = 13

var footerLabels = [3]string{"likes", "replies", "shares"}

func rgb(value uint32) color.NRGBA {
	return color.NRGBA{uint8(value >> 16), uint8(value >> 8), uint8(value), 0xFF}
}

func alpha(value color.NRGBA, share float32) color.NRGBA {
	value.A = uint8(float32(value.A) * share)
	return value
}

var (
	ink        = rgb(0x111827)
	bodyColor  = rgb(0x374151)
	muted      = rgb(0x6B7280)
	hairline   = rgb(0xE5E7EB)
	panelColor = rgb(0xE2E8F0)
	white      = rgb(0xFFFFFF)
	upColor    = rgb(0x16A34A)
	downColor  = rgb(0xDC2626)
	levels     = [2]color.NRGBA{rgb(0xF1F5F9), rgb(0xCBD5E1)}
)

func palette(index int) color.NRGBA { return rgb(paletteRGB[index%8]) }

// launch is what `desktop.py` asked of the gauntlet.
type launch struct {
	tier, freeze int
	fonts        string
}

func readLaunch() launch {
	number := func(name string, fallback int) int {
		if value, err := strconv.Atoi(os.Getenv(name)); err == nil {
			return value
		}
		return fallback
	}
	fonts := os.Getenv("PERF_FONTS")
	if fonts == "" {
		fonts = "fonts"
	}
	return launch{number("PERF_TIER", 5), number("PERF_FREEZE", 0), fonts}
}

// robotoTheme draws every text in the Roboto files every desktop app loads.
type robotoTheme struct {
	fyne.Theme
	regular, bold fyne.Resource
}

func (t robotoTheme) Font(style fyne.TextStyle) fyne.Resource {
	if style.Bold {
		return t.bold
	}
	return t.regular
}

// style is a text's size and weight; its line height is 1.4 em.
type style struct {
	size float32
	bold bool
}

func (s style) textStyle() fyne.TextStyle { return fyne.TextStyle{Bold: s.bold} }

// measure is a text's size, measured once per text and style.
var measured = map[style]map[string]fyne.Size{}

func measure(text string, s style) fyne.Size {
	sizes, ok := measured[s]
	if !ok {
		sizes = map[string]fyne.Size{}
		measured[s] = sizes
	}
	if size, ok := sizes[text]; ok {
		return size
	}
	size := fyne.MeasureText(text, s.size, s.textStyle())
	sizes[text] = size
	return size
}

func newText(content string, c color.Color, s style) *canvas.Text {
	text := canvas.NewText(content, c)
	text.TextSize = s.size
	text.TextStyle = s.textStyle()
	return text
}

// setText changes a text only when it differs, so an unchanged frame
// touches nothing.
func setText(text *canvas.Text, content string, c color.Color) {
	if text.Text != content || text.Color != c {
		text.Text = content
		text.Color = c
		text.Refresh()
	}
}

// place moves and sizes an object to a rectangle.
func place(object fyne.CanvasObject, x, y, width, height float32) {
	object.Move(fyne.NewPos(x, y))
	object.Resize(fyne.NewSize(width, height))
}

// placeText puts a one-line text at x, y at its own size; returns its width.
func placeText(text *canvas.Text, x, y float32, s style) float32 {
	size := measure(text.Text, s)
	place(text, x, y, size.Width, size.Height)
	return size.Width
}

// paragraph is a wrapped text cut after its last line with an ellipsis: one
// canvas text per line, the lines 1.4 em apart.
type paragraph struct {
	words []string
	lines []*canvas.Text
	s     style
	c     color.Color
}

func newParagraph(lines int, c color.Color, s style) *paragraph {
	p := &paragraph{s: s, c: c}
	for range lines {
		p.lines = append(p.lines, newText("", c, s))
	}
	return p
}

func (p *paragraph) set(text string) { p.words = strings.Fields(text) }

func (p *paragraph) objects() []fyne.CanvasObject {
	objects := make([]fyne.CanvasObject, len(p.lines))
	for index, line := range p.lines {
		objects[index] = line
	}
	return objects
}

// place wraps the words `width` wide from x, y; returns the height.
func (p *paragraph) place(x, y, width float32) float32 {
	space := measure(" ", p.s).Width
	word := 0
	shown := 0
	for index, line := range p.lines {
		if word >= len(p.words) {
			line.Hide()
			continue
		}
		start, used := word, float32(0)
		for word < len(p.words) {
			wordWidth := measure(p.words[word], p.s).Width
			if word > start && used+space+wordWidth > width {
				break
			}
			if word > start {
				used += space
			}
			used += wordWidth
			word++
		}
		content := strings.Join(p.words[start:word], " ")
		if index == len(p.lines)-1 && word < len(p.words) {
			content = ellipsized(content+" "+strings.Join(p.words[word:], " "), width, p.s)
		}
		setText(line, content, p.c)
		line.Show()
		placeText(line, x, y+float32(index)*p.s.size*1.4, p.s)
		shown = index + 1
	}
	if shown == 0 {
		return 0
	}
	return float32(shown-1)*p.s.size*1.4 + measure("Ag", p.s).Height
}

// ellipsized cuts text with an ellipsis to fit width.
func ellipsized(text string, width float32, s style) string {
	if measure(text, s).Width <= width {
		return text
	}
	runes := []rune(text)
	for end := len(runes); end > 0; end-- {
		cut := strings.TrimRight(string(runes[:end]), " ") + "…"
		if fyne.MeasureText(cut, s.size, s.textStyle()).Width <= width {
			return cut
		}
	}
	return "…"
}

// bar is a rounded track filled to a share of it.
type bar struct{ track, fill *canvas.Rectangle }

func newBar(c color.Color) bar {
	track := canvas.NewRectangle(hairline)
	fill := canvas.NewRectangle(c)
	return bar{track, fill}
}

func (b bar) place(x, y, width, height, share float32) {
	share = min(max(share, 0), 1)
	b.track.CornerRadius = height / 2
	b.fill.CornerRadius = height / 2
	place(b.track, x, y, width, height)
	place(b.fill, x, y, width*share, height)
}

// tickerTile is one quote: symbol, price, change and a bar, all from the frame.
type tickerTile struct {
	t                     ticker
	tile                  *canvas.Rectangle
	symbol, price, change *canvas.Text
	bar                   bar
}

type screen struct {
	launch    launch
	tier      gauntletTier
	s         float32
	posts     []post
	avatars   []image.Image
	frame     int
	tiles     []*tickerTile
	panel     *fyne.Container
	panelBack *canvas.Rectangle
	list      *fyne.Container
	shown     map[int]*rowView
	spare     []*rowView
	spareLv   []*rowView
	anchor    int
	anchorTop float32
	styles    styles
}

type styles struct {
	ticker, tickerBold, title, subtitle, subtitleBold, body, percent, stat, statLabel, badge, level, chip style
}

func newScreen() *screen {
	l := readLaunch()
	tier := tierOf(l.tier)
	s := tier.scale
	sc := &screen{
		launch: l, tier: tier, s: s, posts: makePosts(), shown: map[int]*rowView{}, anchorTop: 8 * s,
		styles: styles{
			ticker: style{10 * s, false}, tickerBold: style{10 * s, true}, title: style{13 * s, true},
			subtitle: style{11 * s, false}, subtitleBold: style{11 * s, true}, body: style{12 * s, false},
			percent: style{10 * s, false}, stat: style{12 * s, true}, statLabel: style{9 * s, false},
			badge: style{9 * s, true}, level: style{9 * s, false}, chip: style{10 * s, false},
		},
	}
	for index := range avatarCount {
		avatar := image.NewNRGBA(image.Rect(0, 0, avatarSize, avatarSize))
		copy(avatar.Pix, avatarRGBA(index))
		sc.avatars = append(sc.avatars, avatar)
	}
	sc.panelBack = canvas.NewRectangle(panelColor)
	sc.panel = container.NewWithoutLayout(sc.panelBack)
	for index, t := range makeTickers(tier.tickers) {
		tile := &tickerTile{
			t: t, tile: canvas.NewRectangle(white),
			symbol: newText(t.symbol, ink, sc.styles.tickerBold), price: newText("", bodyColor, sc.styles.ticker),
			change: newText("", upColor, sc.styles.ticker), bar: newBar(palette(index)),
		}
		tile.tile.CornerRadius = 6 * s
		sc.tiles = append(sc.tiles, tile)
		sc.panel.Add(tile.tile)
		sc.panel.Objects = append(sc.panel.Objects, tile.symbol, tile.price, tile.change, tile.bar.track, tile.bar.fill)
	}
	sc.list = container.NewWithoutLayout()
	return sc
}

// placePanel sets the tiles' values and wraps them `width` wide; returns
// the panel's height.
func (sc *screen) placePanel(width float32) float32 {
	s := sc.s
	x, y, line := float32(0), float32(0), float32(0)
	inner := width - 12*s
	for _, tile := range sc.tiles {
		cents := tickerCents(tile.t, sc.frame)
		changeColor := upColor
		if cents < tile.t.baseCents {
			changeColor = downColor
		}
		setText(tile.price, centsText(cents), bodyColor)
		setText(tile.change, changeText(tile.t, cents), changeColor)
		widths := [3]float32{measure(tile.symbol.Text, sc.styles.tickerBold).Width,
			measure(tile.price.Text, sc.styles.ticker).Width, measure(tile.change.Text, sc.styles.ticker).Width}
		textHeight := measure("Ag", sc.styles.ticker).Height
		height := max(textHeight, 4*s) + 6*s
		tileWidth := 6*s + widths[0] + widths[1] + widths[2] + 3*4*s + 20*s + 6*s
		if x > 0 && x+tileWidth > inner {
			y += line + 4*s
			x, line = 0, 0
		}
		left, top := 6*s+x, 6*s+y
		place(tile.tile, left, top, tileWidth, height)
		at := left + 6*s
		for index, text := range []*canvas.Text{tile.symbol, tile.price, tile.change} {
			place(text, at, top+3*s, widths[index], textHeight)
			at += widths[index] + 4*s
		}
		share := float32(cents-tile.t.baseCents+tile.t.swingCents) / float32(2*tile.t.swingCents)
		tile.bar.place(at, top+3*s+(textHeight-4*s)/2, 20*s, 4*s, share)
		x += tileWidth + 4*s
		line = max(line, height)
	}
	height := y + line + 12*s
	place(sc.panelBack, 0, 0, width, height)
	return height
}

// cardView is one post: header, body, progress, sparkline, chips and
// footer, with a shadow and a border, and every fifth card a badge.
type cardView struct {
	box                  *canvas.Rectangle
	avatar               *canvas.Image
	title, body          *paragraph
	by, author, dot, tag *canvas.Text
	bar                  bar
	percent              *canvas.Text
	fade                 *canvas.LinearGradient
	above                []*canvas.ArbitraryPolygon
	segments             []*canvas.Line
	chipBacks            [3]*canvas.Rectangle
	chips                [3]*canvas.Text
	stats, statLabels    [3]*canvas.Text
	dividers             [2]*canvas.Rectangle
	badgeBack            *canvas.Rectangle
	badge                *canvas.Text
	objects              []fyne.CanvasObject
	card                 int
	sparkColor           color.NRGBA
}

func (sc *screen) newCard() *cardView {
	s, st := sc.s, sc.styles
	c := &cardView{
		box:    canvas.NewRectangle(white),
		avatar: canvas.NewImageFromImage(sc.avatars[0]),
		title:  newParagraph(2, ink, st.title), body: newParagraph(4, bodyColor, st.body),
		by: newText("by ", muted, st.subtitle), author: newText("", muted, st.subtitleBold),
		dot: newText(" · ", muted, st.subtitle), tag: newText("", muted, st.subtitle),
		bar: newBar(palette(0)), percent: newText("", muted, st.percent),
		fade:      canvas.NewVerticalGradient(white, white),
		badgeBack: canvas.NewRectangle(alpha(palette(0), 0.9)), badge: newText("HOT", white, st.badge),
	}
	c.box.CornerRadius = 12 * s
	c.box.StrokeColor = hairline
	c.box.StrokeWidth = 1
	c.box.Shadow = canvas.Shadow{Color: color.NRGBA{0, 0, 0, 61}, BlurRadius: 3 * s, Offset: fyne.NewPos(0, s)}
	c.avatar.FillMode = canvas.ImageFillStretch
	c.avatar.CornerRadius = 16 * s
	c.badgeBack.CornerRadius = 8 * s
	c.objects = append(c.objects, c.box, c.avatar)
	c.objects = append(c.objects, c.title.objects()...)
	c.objects = append(c.objects, c.by, c.author, c.dot, c.tag)
	c.objects = append(c.objects, c.body.objects()...)
	c.objects = append(c.objects, c.bar.track, c.bar.fill, c.percent, c.fade)
	// Fyne fills a polygon of at most 16 vertices: the white above the line
	// is a few polygons side by side.
	for range (sparkPoints - 1 + coverPoints - 1) / coverPoints {
		cover := canvas.NewArbitraryPolygon(nil, white)
		c.above = append(c.above, cover)
		c.objects = append(c.objects, cover)
	}
	for range sparkPoints - 1 {
		segment := canvas.NewLine(white)
		segment.StrokeWidth = 1.5 * s
		c.segments = append(c.segments, segment)
		c.objects = append(c.objects, segment)
	}
	for index := range 3 {
		c.chipBacks[index] = canvas.NewRectangle(white)
		c.chipBacks[index].CornerRadius = 10 * s
		c.chips[index] = newText("", white, st.chip)
		c.stats[index] = newText("", ink, st.stat)
		c.statLabels[index] = newText(footerLabels[index], muted, st.statLabel)
		c.objects = append(c.objects, c.chipBacks[index], c.chips[index], c.stats[index], c.statLabels[index])
	}
	for index := range 2 {
		c.dividers[index] = canvas.NewRectangle(hairline)
		c.objects = append(c.objects, c.dividers[index])
	}
	c.objects = append(c.objects, c.badgeBack, c.badge)
	return c
}

// bind shows card `card`: its post and avatar, what does not change per frame.
func (sc *screen) bind(c *cardView, card int) {
	p := sc.posts[card%postCount]
	c.card = card
	c.avatar.Image = sc.avatars[card%avatarCount]
	c.avatar.Refresh()
	c.title.set(p.title)
	c.body.set(p.body)
	setText(c.author, p.author, muted)
	setText(c.tag, p.tags[0].text, rgb(gradientEndRGB[p.tags[0].color]))
	c.bar.fill.FillColor = palette(card)
	c.bar.fill.Refresh()
	c.sparkColor = palette(p.color)
	c.fade.StartColor = alpha(c.sparkColor, 0.25)
	c.fade.EndColor = alpha(c.sparkColor, 0)
	c.fade.Refresh()
	for _, segment := range c.segments {
		segment.StrokeColor = c.sparkColor
	}
	for index, t := range p.tags {
		c.chipBacks[index].FillColor = rgb(chipBackgroundRGB[t.color])
		c.chipBacks[index].Refresh()
		setText(c.chips[index], t.text, rgb(gradientEndRGB[t.color]))
		setText(c.stats[index], p.stats[index], ink)
	}
	hot := card%5 == 0
	for _, object := range []fyne.CanvasObject{c.badgeBack, c.badge} {
		if hot {
			object.Show()
		} else {
			object.Hide()
		}
	}
}

// placeCard lays the card out `width` wide at x, y on the frame; returns its height.
func (sc *screen) placeCard(c *cardView, x, y, width float32) float32 {
	s, st := sc.s, sc.styles
	inner := width - 20*s
	top := y + 10*s
	// Header: the avatar beside the title and subtitle, centered.
	textX := x + 50*s
	textWidth := inner - 40*s
	titleHeight := c.title.place(textX, 0, textWidth)
	subtitleHeight := measure("Ag", st.subtitle).Height
	header := max(32*s, titleHeight+subtitleHeight)
	place(c.avatar, x+10*s, top+(header-32*s)/2, 32*s, 32*s)
	textTop := top + (header-titleHeight-subtitleHeight)/2
	c.title.place(textX, textTop, textWidth)
	at := textX
	for _, part := range []struct {
		text *canvas.Text
		s    style
	}{{c.by, st.subtitle}, {c.author, st.subtitleBold}, {c.dot, st.subtitle}, {c.tag, st.subtitle}} {
		at += placeText(part.text, at, textTop+titleHeight, part.s)
	}
	top += header + 6*s
	top += c.body.place(x+10*s, top, inner) + 6*s
	// Progress: the bar takes what the percent leaves.
	permille := progressPermille(c.card, sc.frame)
	setText(c.percent, fmt.Sprintf("%d%%", permille/10), muted)
	percent := measure(c.percent.Text, st.percent)
	row := max(percent.Height, 6*s)
	barWidth := inner - percent.Width - 6*s
	c.bar.place(x+10*s, top+(row-6*s)/2, barWidth, 6*s, float32(permille)/1000)
	place(c.percent, x+10*s+barWidth+6*s, top+(row-percent.Height)/2, percent.Width, percent.Height)
	top += row + 6*s
	// The sparkline: a fading fill under the line, which a white polygon
	// covers above it, and the line's segments.
	chart := 36 * s
	place(c.fade, x+10*s, top, inner, chart)
	step := inner / (sparkPoints - 1)
	points := make([]fyne.Position, 0, sparkPoints)
	previous := fyne.Position{}
	for index := range sparkPoints {
		point := fyne.NewPos(float32(index)*step, chart*(1-sparkValue(c.card, index, sc.frame)))
		points = append(points, point)
		if index > 0 {
			segment := c.segments[index-1]
			segment.Position1 = fyne.NewPos(x+10*s+previous.X, top+previous.Y)
			segment.Position2 = fyne.NewPos(x+10*s+point.X, top+point.Y)
			segment.Refresh()
		}
		previous = point
	}
	for index, cover := range c.above {
		first := index * coverPoints
		last := min(first+coverPoints, sparkPoints-1)
		outline := make([]fyne.Position, 0, last-first+3)
		outline = append(outline, fyne.NewPos(points[first].X, 0))
		outline = append(outline, points[first:last+1]...)
		outline = append(outline, fyne.NewPos(points[last].X, 0))
		cover.Points = outline
		place(cover, x+10*s, top, inner, chart)
		cover.Refresh()
	}
	top += chart + 6*s
	// Chips in a flow row.
	chipX, chipY, line := float32(0), float32(0), float32(0)
	for index, chip := range c.chips {
		size := measure(chip.Text, st.chip)
		chipWidth, chipHeight := size.Width+16*s, size.Height+6*s
		if chipX > 0 && chipX+chipWidth > inner {
			chipY += line + 4*s
			chipX, line = 0, 0
		}
		place(c.chipBacks[index], x+10*s+chipX, top+chipY, chipWidth, chipHeight)
		place(chip, x+10*s+chipX+8*s, top+chipY+3*s, size.Width, size.Height)
		chipX += chipWidth + 4*s
		line = max(line, chipHeight)
	}
	top += chipY + line + 6*s
	// Footer: three counters split by dividers as tall as the tallest.
	column := (inner - 2) / 3
	footer := float32(0)
	for index := range 3 {
		left := x + 10*s + float32(index)*(column+1)
		stat := measure(c.stats[index].Text, st.stat)
		label := measure(c.statLabels[index].Text, st.statLabel)
		place(c.stats[index], left+(column-stat.Width)/2, top, stat.Width, stat.Height)
		place(c.statLabels[index], left+(column-label.Width)/2, top+stat.Height, label.Width, label.Height)
		footer = max(footer, stat.Height+label.Height)
	}
	for index, divider := range c.dividers {
		place(divider, x+10*s+float32(index+1)*(column+1)-1, top, 1, footer)
	}
	top += footer + 10*s
	if c.card%5 == 0 {
		// Fyne draws no rotated object: the badge stays upright.
		size := measure("HOT", st.badge)
		badgeWidth, badgeHeight := size.Width+12*s, size.Height+4*s
		place(c.badgeBack, x+width-6*s-badgeWidth, y+6*s, badgeWidth, badgeHeight)
		place(c.badge, x+width-6*s-badgeWidth+6*s, y+8*s, size.Width, size.Height)
	}
	height := top - y
	place(c.box, x, y, width, height)
	return height
}

// level is one level of a cluster: its background, three labels above the
// next level.
type level struct {
	remaining int
	back      *canvas.Rectangle
	chipBacks [3]*canvas.Rectangle
	chips     [3]*canvas.Text
}

// rowView is a list row: a row of cards, or a deep cluster.
type rowView struct {
	cards  []*cardView
	levels []*level
	box    *fyne.Container
}

func (sc *screen) newRow(cluster bool) *rowView {
	r := &rowView{box: container.NewWithoutLayout()}
	if cluster {
		for remaining := sc.tier.depth; remaining >= 0; remaining-- {
			l := &level{remaining: remaining, back: canvas.NewRectangle(levels[remaining%2])}
			l.back.CornerRadius = 4 * sc.s
			r.box.Add(l.back)
			for index := range 3 {
				l.chipBacks[index] = canvas.NewRectangle(white)
				l.chipBacks[index].CornerRadius = 2 * sc.s
				l.chips[index] = newText("", white, sc.styles.level)
				r.box.Add(l.chipBacks[index])
				r.box.Add(l.chips[index])
			}
			r.levels = append(r.levels, l)
		}
		return r
	}
	for range sc.tier.columns {
		c := sc.newCard()
		r.cards = append(r.cards, c)
		r.box.Objects = append(r.box.Objects, c.objects...)
	}
	return r
}

func (sc *screen) bindRow(r *rowView, row int) {
	cards, index := gauntletRow(row, sc.tier.columns)
	if cards {
		for column, c := range r.cards {
			sc.bind(c, index+column)
		}
		return
	}
	for _, l := range r.levels {
		for chip := range 3 {
			l.chipBacks[chip].FillColor = palette(l.remaining + chip + index)
			l.chipBacks[chip].Refresh()
			setText(l.chips[chip], fmt.Sprintf("C%d.L%d.%d", index, l.remaining, chip), white)
		}
	}
}

// placeRow lays the row out `width` wide; returns its height.
func (sc *screen) placeRow(r *rowView, width float32) float32 {
	s := sc.s
	if r.levels == nil {
		column := (width - 8*s*float32(len(r.cards)-1)) / float32(len(r.cards))
		tallest := float32(0)
		for index, c := range r.cards {
			tallest = max(tallest, sc.placeCard(c, float32(index)*(column+8*s), 0, column))
		}
		return tallest
	}
	// Each level sits inside the one before it: place them from the
	// innermost out, so every level knows the height below its labels.
	labelHeight := measure("Ag", sc.styles.level).Height
	heights := make([]float32, len(r.levels))
	for index := len(r.levels) - 1; index >= 0; index-- {
		heights[index] = s + labelHeight + s
		if index+1 < len(r.levels) {
			heights[index] += s + heights[index+1]
		}
	}
	left, top, levelWidth := float32(0), float32(0), width
	for index, l := range r.levels {
		place(l.back, left, top, levelWidth, heights[index])
		content := levelWidth - 4*s
		column := (content - 4*s) / 3
		for chip := range 3 {
			chipX := left + 3*s + float32(chip)*(column+2*s)
			place(l.chipBacks[chip], chipX, top+s, column, labelHeight)
			size := measure(l.chips[chip].Text, sc.styles.level)
			place(l.chips[chip], chipX, top+s, min(size.Width, column), labelHeight)
		}
		left += 3 * s
		top += s + labelHeight + s
		levelWidth = content
	}
	return heights[0]
}

// placeList shows the rows from the first one on screen down to the
// bottom, `width` wide and `height` tall, each recycled once it scrolled off.
func (sc *screen) placeList(width, height float32) {
	s := sc.s
	gap := 8 * s
	offset := float32(sc.frame * scrollPerFrame)
	row, top := sc.anchor, sc.anchorTop-offset
	for top < height && row < rows {
		r, ok := sc.shown[row]
		if !ok {
			cards, _ := gauntletRow(row, sc.tier.columns)
			pool := &sc.spare
			if !cards {
				pool = &sc.spareLv
			}
			if len(*pool) > 0 {
				r = (*pool)[len(*pool)-1]
				*pool = (*pool)[:len(*pool)-1]
			} else {
				r = sc.newRow(!cards)
			}
			sc.bindRow(r, row)
			sc.list.Add(r.box)
			sc.shown[row] = r
		}
		rowHeight := sc.placeRow(r, width-2*gap)
		place(r.box, gap, top, width-2*gap, rowHeight)
		if top+rowHeight+gap <= 0 && row == sc.anchor {
			// Scrolled off above: built no more.
			sc.recycle(row)
			sc.anchor = row + 1
			sc.anchorTop += rowHeight + gap
		}
		top += rowHeight + gap
		row++
	}
	for stale := range sc.shown {
		if stale >= row {
			sc.recycle(stale)
		}
	}
}

func (sc *screen) recycle(row int) {
	r := sc.shown[row]
	delete(sc.shown, row)
	sc.list.Remove(r.box)
	if r.levels == nil {
		sc.spare = append(sc.spare, r)
	} else {
		sc.spareLv = append(sc.spareLv, r)
	}
}

// layout places the whole screen on the frame: the top bar, then the ticker
// strip and the list in a column whose width follows the frame.
func (sc *screen) layout(topBar *canvas.Rectangle, heading *canvas.Text, clip fyne.CanvasObject) {
	place(topBar, 0, 0, windowWidth, 56)
	headingSize := measure("Gauntlet", style{20, true})
	place(heading, 16, (56-headingSize.Height)/2, headingSize.Width, headingSize.Height)
	width := float32(int(windowWidth*widthFraction(sc.frame) + 0.5))
	panelHeight := sc.placePanel(width)
	place(sc.panel, 0, 56, width, panelHeight)
	listHeight := windowHeight - 56 - panelHeight
	place(clip, 0, 56+panelHeight, width, listHeight)
	sc.placeList(width, listHeight)
	sc.panel.Refresh()
	sc.list.Refresh()
}

func main() {
	l := readLaunch()
	a := app.NewWithID("dev.perfcompare.fyne")
	read := func(file string) fyne.Resource {
		bytes, err := os.ReadFile(filepath.Join(l.fonts, file))
		if err != nil {
			fmt.Fprintln(os.Stderr, "no", file, err)
			return nil
		}
		return fyne.NewStaticResource(file, bytes)
	}
	a.Settings().SetTheme(robotoTheme{theme.LightTheme(), read("Roboto-Regular.ttf"), read("Roboto-Bold.ttf")})
	w := a.NewWindow("Gauntlet")
	w.SetPadded(false)
	w.SetFixedSize(true)
	w.Resize(fyne.NewSize(windowWidth, windowHeight))
	sc := newScreen()
	topBar := canvas.NewRectangle(rgb(0x1E2A4A))
	heading := newText("Gauntlet", white, style{20, true})
	clip := container.NewScroll(sc.list)
	clip.Direction = container.ScrollNone
	background := canvas.NewRectangle(rgb(0xEEF0F5))
	place(background, 0, 0, windowWidth, windowHeight)
	w.SetContent(container.NewWithoutLayout(background, topBar, heading, sc.panel, clip))
	sc.layout(topBar, heading, clip)
	ticks := fyne.NewAnimation(time.Hour, func(float32) {
		if sc.launch.freeze > 0 && sc.frame >= sc.launch.freeze {
			return
		}
		if sc.frame == 0 {
			fmt.Println("PERF first_frame")
		}
		sc.frame++
		sc.layout(topBar, heading, clip)
		if sc.launch.freeze > 0 && sc.frame >= sc.launch.freeze {
			fmt.Printf("PERF frozen frame=%d\n", sc.frame)
		}
	})
	ticks.RepeatCount = fyne.AnimationRepeatForever
	ticks.Start()
	w.ShowAndRun()
}

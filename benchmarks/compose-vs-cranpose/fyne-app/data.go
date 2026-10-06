// Deterministic benchmark data: `perf-data/src/lib.rs` and the other apps'
// copies implement the same generator bit for bit, so every app draws
// identical content. Only what the gauntlet shows is kept.

package main

import (
	"fmt"
	"math"
	"strings"
)

const (
	postCount          = 5000
	barCount           = 24
	cardRowsPerCluster = 5
	avatarCount        = 8
	avatarSize         = 64
	sparkPoints        = 48
)

var paletteRGB = [8]uint32{0xEF4444, 0xF97316, 0xEAB308, 0x22C55E, 0x14B8A6, 0x3B82F6, 0x8B5CF6, 0xEC4899}
var chipBackgroundRGB = [8]uint32{0xFEE2E2, 0xFFEDD5, 0xFEF9C3, 0xDCFCE7, 0xCCFBF1, 0xDBEAFE, 0xEDE9FE, 0xFCE7F3}
var gradientEndRGB = [8]uint32{0x7F1D1D, 0x7C2D12, 0x713F12, 0x14532D, 0x134E4A, 0x1E3A8A, 0x4C1D95, 0x831843}

var words = []string{
	"lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit", "sed", "do", "eiusmod",
	"tempor", "incididunt", "ut", "labore", "et", "dolore", "magna", "aliqua", "enim", "ad", "minim", "veniam",
	"quis", "nostrud", "exercitation", "ullamco", "laboris", "nisi", "aliquip", "ex", "ea", "commodo",
	"consequat", "duis", "aute", "irure", "in", "reprehenderit", "voluptate", "velit", "esse", "cillum",
	"fugiat", "nulla", "pariatur",
}
var firstNames = []string{
	"Ada", "Linus", "Grace", "Alan", "Barbara", "Dennis", "Ken", "Margaret", "Edsger", "Donald", "Frances",
	"John", "Radia", "Tim", "Guido", "Bjarne",
}
var lastNames = []string{
	"Lovelace", "Torvalds", "Hopper", "Turing", "Liskov", "Ritchie", "Thompson", "Hamilton", "Dijkstra",
	"Knuth", "Allen", "McCarthy", "Perlman", "Berners", "Rossum", "Stroustrup",
}
var tagNames = []string{
	"#rust", "#kotlin", "#compose", "#android", "#gpu", "#layout", "#text", "#perf", "#wgpu", "#skia", "#ui",
	"#mobile",
}

type rng struct{ state uint32 }

func newRng(seed uint32) *rng {
	state := seed*0x9E3779B9 ^ 0xA5A5A5A5
	if state == 0 {
		state = 1
	}
	return &rng{state}
}

func (r *rng) next() uint32 {
	x := r.state
	x ^= x << 13
	x ^= x >> 17
	x ^= x << 5
	r.state = x
	return x
}

func (r *rng) below(bound uint32) uint32 { return r.next() % bound }

func (r *rng) unit() float32 { return float32(r.next()>>8) / 16_777_216 }

type tag struct {
	text  string
	color int
}

type post struct {
	author string
	color  int
	title  string
	body   string
	tags   [3]tag
	stats  [4]string
}

func sentence(r *rng, min, max uint32) string {
	count := min + r.below(max-min+1)
	parts := make([]string, count)
	for index := range parts {
		parts[index] = words[r.below(46)]
	}
	text := strings.Join(parts, " ")
	return strings.ToUpper(text[:1]) + text[1:]
}

func compactCount(value uint32) string {
	if value >= 1000 {
		return fmt.Sprintf("%d.%dk", value/1000, (value%1000)/100)
	}
	return fmt.Sprint(value)
}

func makePost(index int) post {
	r := newRng(uint32(index) + 1)
	first := firstNames[r.below(16)]
	last := lastNames[r.below(16)]
	r.below(59) // the minutes of the handle, which the gauntlet does not show
	r.below(1000)
	color := int(r.below(8))
	title := sentence(r, 4, 8)
	body := sentence(r, 22, 38) + "."
	for range barCount {
		r.unit()
	}
	var tags [3]tag
	for index := range tags {
		chosen := int(r.below(uint32(len(tagNames))))
		tags[index] = tag{tagNames[chosen], chosen % 8}
	}
	var stats [4]string
	for index := range stats {
		stats[index] = compactCount(r.below(20_000))
	}
	return post{first + " " + last, color, title, body, tags, stats}
}

func makePosts() []post {
	posts := make([]post, postCount)
	for index := range posts {
		posts[index] = makePost(index)
	}
	return posts
}

// gauntletTier is what one load tier puts on screen; the README lists them.
type gauntletTier struct {
	columns int
	scale   float32
	tickers int
	depth   int
}

var tiers = []gauntletTier{
	{1, 1.0, 8, 6}, {2, 0.85, 12, 8}, {2, 0.7, 16, 10}, {3, 0.6, 20, 12}, {3, 0.5, 28, 14}, {4, 0.45, 36, 16},
	{4, 0.4, 44, 20}, {5, 0.35, 56, 24}, {6, 0.3, 72, 28}, {7, 0.27, 96, 32}, {8, 0.25, 120, 40},
	{10, 0.2, 160, 48}, {12, 0.18, 200, 56}, {14, 0.16, 240, 64}, {16, 0.14, 300, 72}, {20, 0.12, 400, 80},
}

func tierOf(tier int) gauntletTier {
	return tiers[min(max(tier, 1), len(tiers))-1]
}

type ticker struct {
	symbol     string
	baseCents  int
	swingCents int
	step       int
	phase      int
}

func makeTickers(count int) []ticker {
	r := newRng(31_337)
	tickers := make([]ticker, count)
	for index := range tickers {
		length := 3 + r.below(2)
		symbol := make([]byte, length)
		for at := range symbol {
			symbol[at] = byte('A' + r.below(26))
		}
		tickers[index] = ticker{
			symbol:     string(symbol),
			baseCents:  1_000 + int(r.below(99_000)),
			swingCents: 50 + int(r.below(950)),
			step:       1 + int(r.below(9)),
			phase:      int(r.below(2_000)),
		}
	}
	return tickers
}

// triangle is a triangle wave over period: 0 at the ends, period/2 in the middle.
func triangle(value, period int) int {
	position := ((value % period) + period) % period
	half := period / 2
	if position > half {
		return half - (position - half)
	}
	return half - (half - position)
}

func tickerCents(t ticker, frame int) int {
	wave := triangle(frame*t.step+t.phase, 2_000)
	return t.baseCents + t.swingCents*(wave-500)/500
}

func centsText(cents int) string {
	sign := ""
	if cents < 0 {
		sign = "-"
		cents = -cents
	}
	return fmt.Sprintf("%s%d.%02d", sign, cents/100, cents%100)
}

func changeText(t ticker, cents int) string {
	basisPoints := (cents - t.baseCents) * 10_000 / t.baseCents
	sign := "+"
	if basisPoints < 0 {
		sign = "-"
		basisPoints = -basisPoints
	}
	return fmt.Sprintf("%s%d.%02d%%", sign, basisPoints/100, basisPoints%100)
}

func progressPermille(card, frame int) int { return (frame*3 + card*37) % 1_000 }

func badgeDegrees(card, frame int) float32 {
	return float32(triangle(frame*2+card*30, 40))*0.5 - 5
}

func widthFraction(frame int) float32 {
	return 0.92 + 0.08*float32(triangle(frame*3, 200))/100
}

func sparkValue(card, point, frame int) float32 {
	phase := (float32(frame) + float32(card)*7) * 0.11
	return 0.5 + 0.38*float32(math.Sin(float64(float32(point)*0.32+phase))) +
		0.08*float32(math.Sin(float64(float32(point)*1.7+float32(card))))
}

var avatarColors = [avatarCount][6]int{
	{0xEF, 0x44, 0x44, 0x7F, 0x1D, 0x1D}, {0xF9, 0x73, 0x16, 0x7C, 0x2D, 0x12},
	{0xEA, 0xB3, 0x08, 0x71, 0x3F, 0x12}, {0x22, 0xC5, 0x5E, 0x14, 0x53, 0x2D},
	{0x14, 0xB8, 0xA6, 0x13, 0x4E, 0x4A}, {0x3B, 0x82, 0xF6, 0x1E, 0x3A, 0x8A},
	{0x8B, 0x5C, 0xF6, 0x4C, 0x1D, 0x95}, {0xEC, 0x48, 0x99, 0x83, 0x18, 0x43},
}

// avatarRGBA is avatar index as tightly packed RGBA: a radial gradient
// crossed by diagonal stripes.
func avatarRGBA(index int) []byte {
	colors := avatarColors[index%avatarCount]
	pixels := make([]byte, 0, avatarSize*avatarSize*4)
	for y := 0; y < avatarSize; y++ {
		for x := 0; x < avatarSize; x++ {
			dx, dy := x-avatarSize/2, y-avatarSize*3/8
			t := min((dx*dx+dy*dy)*255/(40*40), 255)
			stripe := ((x+y+index*3)/6)%2 == 0
			for channel := 0; channel < 3; channel++ {
				value := (colors[channel]*(255-t) + colors[channel+3]*t) / 255
				if stripe {
					value += (255 - value) / 6
				}
				pixels = append(pixels, byte(value))
			}
			pixels = append(pixels, 0xFF)
		}
	}
	return pixels
}

// gauntletRow is what list row holds: the first card of a row of cards, or
// every sixth row a deep cluster.
func gauntletRow(row, columns int) (cards bool, index int) {
	block := row / (cardRowsPerCluster + 1)
	within := row % (cardRowsPerCluster + 1)
	if within == cardRowsPerCluster {
		return false, block
	}
	return true, (block*cardRowsPerCluster + within) * columns
}

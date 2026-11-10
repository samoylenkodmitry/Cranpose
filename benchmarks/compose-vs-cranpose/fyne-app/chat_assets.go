package main

import (
	"bytes"
	"image"
	"image/draw"
	"image/gif"
	"log"
	"math"
	"os"
	"path/filepath"

	"github.com/fyne-io/oksvg"
	"github.com/srwiley/rasterx"
)

// spinnerTurns is the frames of the status line's spinner: it turns by
// `chatSpinDegrees` a frame, so it is upright again after this many.
const spinnerTurns = 360 / chatSpinDegrees

// spinnerPixels is the side of a spinner frame, in pixels: four to a point,
// which Fyne scales down to the screen.
const spinnerPixels = 4 * chatSpinnerSize

// chatAssets is the chat's two pictures, as images: the animated picture's
// frames and the spinner turned to each of its positions. Fyne draws no
// rotated object, so the spinner is drawn at each angle once.
type chatAssets struct {
	gif     []image.Image
	spinner [spinnerTurns]image.Image
}

// loadChatAssets reads `spin.gif` and `progress.svg` in `folder`.
func loadChatAssets(folder string) chatAssets {
	var assets chatAssets
	file, err := os.ReadFile(filepath.Join(folder, "spin.gif"))
	if err != nil {
		log.Fatal(err)
	}
	animation, err := gif.DecodeAll(bytes.NewReader(file))
	if err != nil {
		log.Fatal(err)
	}
	// Each frame lies over the one before it, as the picture's frames ask.
	shown := image.NewRGBA(image.Rect(0, 0, animation.Config.Width, animation.Config.Height))
	for _, frame := range animation.Image {
		draw.Draw(shown, frame.Bounds(), frame, frame.Bounds().Min, draw.Over)
		assets.gif = append(assets.gif, copyImage(shown))
	}
	svg, err := os.ReadFile(filepath.Join(folder, "progress.svg"))
	if err != nil {
		log.Fatal(err)
	}
	icon, err := oksvg.ReadIconStream(bytes.NewReader(svg))
	if err != nil {
		log.Fatal(err)
	}
	icon.SetTarget(0, 0, spinnerPixels, spinnerPixels)
	// The icon's transform moves the paths but not their strokes.
	for index := range icon.SVGPaths {
		icon.SVGPaths[index].LineWidth *= spinnerPixels / icon.ViewBox.W
	}
	fit, middle := icon.Transform, float64(spinnerPixels)/2
	for turn := range assets.spinner {
		spun := image.NewRGBA(image.Rect(0, 0, spinnerPixels, spinnerPixels))
		turned := rasterx.Identity.Translate(middle, middle).Rotate(float64(turn*chatSpinDegrees)*math.Pi/180).Translate(-middle, -middle)
		icon.Transform = turned.Mult(fit)
		scanner := rasterx.NewScannerGV(spinnerPixels, spinnerPixels, spun, spun.Bounds())
		icon.Draw(rasterx.NewDasher(spinnerPixels, spinnerPixels, scanner), 1)
		assets.spinner[turn] = straight(spun)
	}
	return assets
}

func copyImage(source *image.RGBA) *image.RGBA {
	copied := image.NewRGBA(source.Bounds())
	copy(copied.Pix, source.Pix)
	return copied
}

// straight is `source` with its colors not multiplied by its alpha, as Fyne
// takes the pixels of a picture.
func straight(source *image.RGBA) *image.NRGBA {
	converted := image.NewNRGBA(source.Bounds())
	draw.Draw(converted, converted.Bounds(), source, source.Bounds().Min, draw.Src)
	return converted
}

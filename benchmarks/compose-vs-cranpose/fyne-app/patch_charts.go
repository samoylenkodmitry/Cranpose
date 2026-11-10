package main

import (
	"image"
	"image/color"

	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"github.com/srwiley/rasterx"
	"golang.org/x/image/math/fixed"
)

// newCharts is line charts of 512 points that never change. Fyne draws every
// object again each frame, and a line of that many segments is as many
// objects, so the charts are one raster: Fyne asks for its picture once, at
// the pixels of the screen, and keeps it as a texture.
func newCharts(g patchGrid) patchView {
	columns, rows := patchworkPairs(g.tiles.columns, g.tiles.rows)
	width, height := patchworkDouble(g.tier.width), patchworkDouble(g.tier.height)
	picture := canvas.NewRaster(func(pixelsWide, pixelsHigh int) image.Image {
		scale := float64(pixelsWide) / float64(g.width)
		picture := image.NewRGBA(image.Rect(0, 0, pixelsWide, pixelsHigh))
		scanner := rasterx.NewScannerGV(pixelsWide, pixelsHigh, picture, picture.Bounds())
		stroker := rasterx.NewStroker(pixelsWide, pixelsHigh, scanner)
		stroker.SetStroke(fixed.Int26_6(patchworkChartStroke*scale*64), 4<<6, rasterx.ButtCap, nil, rasterx.FlatGap, rasterx.MiterClip)
		chartWidth, chartHeight := float64(width)*scale, float64(height)*scale
		step := chartWidth / (patchworkChartPoints - 1)
		// The charts of one color are one path: the vector rasterizer draws
		// the whole picture each time it draws.
		for first := range paletteRGB {
			for index := first; index < columns*rows; index += len(paletteRGB) {
				left := (wallGap + float64(index%columns)*float64(width+wallGap)) * scale
				top := (wallGap + float64(index/columns)*float64(height+wallGap)) * scale
				for point := range patchworkChartPoints {
					at := fixed.Point26_6{
						X: fixed.Int26_6((left + float64(point)*step) * 64),
						Y: fixed.Int26_6((top + chartHeight*float64(1-patchworkChartValue(index, point))) * 64),
					}
					if point == 0 {
						stroker.Start(at)
					} else {
						stroker.Line(at)
					}
				}
				stroker.Stop(false)
			}
			line := rgb(paletteRGB[first])
			scanner.SetColor(color.RGBA{line.R, line.G, line.B, 0xFF})
			stroker.Draw()
			stroker.Clear()
		}
		return picture
	})
	place(picture, 0, 0, g.width, g.height)
	return patchView{content: container.NewWithoutLayout(picture)}
}

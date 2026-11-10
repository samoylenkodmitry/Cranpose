package main

import (
	"image/color"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/theme"
)

// widgetTheme is the app's theme with what one Fyne widget needs to look as
// the benchmark's screen asks: Fyne sizes and colors a widget from its theme.
// Its widgets have no padding of their own; a button has no fill and a
// label no line spacing unless the theme gives one.
type widgetTheme struct {
	fyne.Theme
	text, lineSpacing, radius float32
	foreground, button        color.Color
}

func (t widgetTheme) Color(name fyne.ThemeColorName, variant fyne.ThemeVariant) color.Color {
	switch name {
	case theme.ColorNameForeground:
		return t.foreground
	case theme.ColorNameButton:
		if t.button != nil {
			return t.button
		}
	}
	return t.Theme.Color(name, variant)
}

func (t widgetTheme) Size(name fyne.ThemeSizeName) float32 {
	switch name {
	case theme.SizeNameText:
		return t.text
	case theme.SizeNameInnerPadding:
		return 0
	case theme.SizeNameLineSpacing:
		return t.lineSpacing
	case theme.SizeNameButtonRadius:
		return t.radius
	}
	return t.Theme.Size(name)
}

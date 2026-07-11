# Bottom tab-bar glass form (light scheme)

Full-res stills from
`../../iphone17_records/bottom_bar_glass_effects_and_form.MP4`
(iOS 26 App Store, 1320×2868 @60 fps, 40 s), crop `1320:340:0:2500`.
The regular light glass material of the floating tab bar over vividly
colored tiles — the form reference for the bar/menu shader.

| File | t | Shows |
|---|---|---|
| `bar_over_orange_purple.png` | 4.0 s | body transparency + saturation over orange/purple tiles; selected "Search" pill |
| `bar_tiles_refracting.png` | 14.5 s | a tile edge crossing the bar: refraction stretch at the top rim, calculator ghost inside |
| `bar_headers_folded.png` | 21.0 s | **section headers under the bar's top edge render INSIDE the bar mirrored upside-down** — the same rim FOLD optic as the text loupe (`liquid_glass.wgsl` loupe mode), on the bar's long edge |
| `bar_over_headers.png` | 30.0 s | bar riding over headers/tiles on blue/green |

Material observations:

- Body: strong blur + white lift, but saturation preserved (orange stays
  orange through the glass); content ghosts stay legible.
- Rim: thin bright line all around (~2 px @3x), soft inner brightening
  band; no hard dark outline in light scheme.
- Edge optic: content just beyond an edge is pulled INTO the bar and
  inverts near the rim — the loupe's fold, not a plain outward stretch.
  The current bar material (`GlassVariant::Regular`, dome_dir +1
  stretch) does not fold; matching this exactly is part of the tab-bar
  visual milestone (#32/#33) since it re-pins every liquid robot
  contract.
- Selected tab: a brighter translucent capsule inside the bar (same
  family as the edit menu's chevron disc), tinting its glyph blue.

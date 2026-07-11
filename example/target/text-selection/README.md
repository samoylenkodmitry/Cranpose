# Text-selection targets: handles, loupe bubble, edit menu

Ground truth extracted from
`../../iphone17_records/text_handles_bubble_and_popup.MP4`
(iOS 26, 1320×2868 @ 120 fps VFR, 19.8 s). All pixel values are physical
@3x pixels; divide by 3 for pt. These are the specs the cranpose text
selection UI is matched against.

| Directory / file | Source | Shows |
|---|---|---|
| `idle_full.png` | t=0.75 full frame | settled selection + both handles + edit menu |
| `loupe-steady/` | t=9.8/10.04/10.3, crop `1320:520:0:660` | loupe mid-drag (shape, dome magnification, rim fold, dispersion, follow lag) |
| `loupe-grow/` | t=1.442–1.792 @60fps eff. | loupe inflating out of the grab point, 8% overshoot |
| `loupe-dissolve/` | t=4.392–4.492 @120fps | release: loupe deflates into the line, ~55 ms |
| `menu-materialize/` | t=6.958–7.192 @60fps eff. | edit menu fade+sharpen in place, disc flash |

Segment crops are `crop=1320:520:0:660` of the full frame (add 660 to y
for full-frame coordinates).

## Selection handles (from `idle_full.png`)

- Accent `srgb(246,53,142)`; selection highlight = accent at ~0.28 alpha.
- Dot diameter 48.5 px (16 pt), stem width 6 px (2 pt).
- Line box (selection rect) height 59 px (~19.7 pt); font cap 33 px,
  baseline pitch 72 px (24 pt) — highlight rects have vertical gaps
  between lines.
- Start handle: dot ON TOP. Dot bottom overlaps the line-box top by
  ~5 px (1.7 pt); stem continues from the dot to the line-box bottom.
  Start: dot y1028–1075 (center 19.5 px above line top), stem y1076–1129,
  line box y1071–1129.
- End handle mirrored: stem y1072–1122 from line top, dot y1123–1169
  (top overlaps line-box bottom by ~6 px, center 17 px below it).
- Handle appearance does NOT change while dragged (no glow/scale).
- Drag by the dot (finger below the line) shows NO loupe; loupe appears
  only when the touch covers the text line.

## Loupe (magnifier bubble)

Shape and placement (`loupe-steady/h_030.png`):
- Capsule (stadium), 350×246 px = 117×82 pt, corner = semicircle H/2.
- Center sits 226 px (75 pt) above the grabbed line's vertical mid;
  bottom edge ≈ 24 pt above the line top. Vertical position is locked to
  the line, NOT to the finger y.
- Horizontal: follows the touch with a first-order lag τ ≈ 70–90 ms
  (measured 20–35 px trail at ~356 px/s drag; no oscillation).

Optics (dome lens, all sampled from the live scene = backdrop lens):
- Magnification at center 1.7×, monotonically decreasing dome profile:
  a 48 px dot displayed at ~60% radius measures 61 px (1.27×).
- Focus point = (smoothed touch x, line mid y): the loupe displays
  content from under the finger, offset up.
- Outer ~18–20% band: refraction fold — sampling distance peaks past the
  loupe edge then reverses ⇒ an INVERTED, compressed image of content
  just beyond (next line renders upside-down at the bottom rim).
- Chromatic dispersion in the band: per-channel refraction offsets,
  visible 3–5 px RGB fringes on rim glyphs.
- Bright rim stroke ~2–3 px, brightest top-left and bottom-right arcs;
  subtle dark ring just inside the rim; no drop shadow.
- Magnified content includes highlight and handle (whole-scene lens).

Motion:
- Grow-in (`loupe-grow/`): starts ~90 px wide AT the grab point on the
  line, rises to the offset position while inflating; magnification ramps
  1→1.7; peak width 378 px at ~200 ms (+8% overshoot), settles to 350 px
  over a further ~150 ms (underdamped spring).
- Dissolve (`loupe-dissolve/`): ~55 ms; the rim/glass vanishes in 1–2
  frames while the magnified content deflates back into the line
  (scale + magnification collapse toward the grab point) with fade.

## Edit menu (Cut / Copy / ✦ / AutoFill / Look Up / ›)

Geometry (`idle_full.png`, menu at y894–1027):
- Glass capsule, height 133 px = 44 pt, corner = H/2; width content-driven
  (1180 px here). Anchored: menu bottom = selection line top − 44 px
  (14.7 pt); centered on the selection x, clamped to 20 pt screen margins.
- Body: high-transparency dark glass — weak blur (text behind stays
  readable), near-zero brightness shift over a dark card, top rim
  highlight line ~2 px (+30 lum), faint side rims.
- Right end cap: chevron `›` (white) inside a lighter neutral glass disc,
  diameter = pill height (inscribed end-cap circle), disc ≈ +45 lum over
  backdrop (white @ ~0.19 alpha equivalent).
- Items: white ~15 pt text; 1 pt hairline separators (white @ ~0.07),
  52 px tall (39% of pill height), vertically centered; generous ~20 pt
  padding each side of labels. No separator against the disc.
- Chevron press feedback: white filled circle flash inside the disc.

Behavior/timing:
- Hides in ~70 ms plain fade when a handle grab starts.
- Rematerializes ~250 ms after release: ~140 ms in-place fade+de-blur
  (no scale/slide); the disc briefly overshoots brighter and settles.
- While dragging by the dot (no loupe) the menu stays hidden until
  release.

Regenerate segments, e.g.:

```sh
V="../../iphone17_records/text_handles_bubble_and_popup.MP4"
ffmpeg -y -v error -ss 9.8 -to 10.3 -i "$V" \
  -vf "fps=120,crop=1320:520:0:660" h_%03d.png
```

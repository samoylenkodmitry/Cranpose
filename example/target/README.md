# Liquid Glass target frames

Ground-truth frame sequences extracted from the reference recording
`../ScreenRecording_07-10-2026 08-34-03_1.MP4` (iOS 26, 1320×2868 @ 60 fps,
duration 18.72 s). These are the per-frame targets the cranpose-liquid
components are matched against — treat them as the spec for geometry,
material, and motion.

The source screen recordings/photos are kept outside git (43 MB raw media);
the frames in this directory are the canonical reference.

| Directory | Frames | Source segment | Crop (w:h:x:y) | Rate | Shows |
|---|---|---|---|---|---|
| `overview/` | 75 | 0 – 18.7 s | full frame, scaled to 330×717 | 4 fps | whole walkthrough: page scroll, toggle flips, menu open/close, slider drags, segmented control, tab-bar lens ride |
| `toggle-press/` | 54 | 0.50 – 1.40 s | 340:190:980:2325 | 60 fps | toggle press → whole thumb becomes 58×39 lens leaning past track end, track color follows drag, release settle (~0.6 s linger) |
| `menu-open/` | 108 | 9.60 – 11.40 s | 920:700:400:100 | 60 fps | "…" button swells on touch, droplet grows with overshoot, content materializes near settle, close morph sucks back |
| `tab-swipe/` | 57 | 14.40 – 16.30 s | 1320:400:0:2360 | 30 fps | bottom tab-bar lens drag: magnification, speed-driven rounding, search-circle merge, settle |

Measured invariants (used by the widgets/shader):

- Toggle: 63×28 capsule track, 37×25 capsule thumb (margin 1.5),
  OFF track `srgb(187,186,188)`; pressed lens 58×39 leaning ~12 dp toward the
  travel side; the lens magnifies the track end-cap and the background beyond
  it; the white thumb dissolves while the lens is up.
- Menu: opens in ~0.33 s with a size overshoot, closes in ~0.22 s; the anchor
  button stays crisp inside the growing droplet before being swallowed;
  content stays smudged (blurred/transparent) until near-settle.
- Tab bar: mid-drag the lens magnifies ~1.6–2×, gets rounder and taller with
  speed, and necks into the search accessory circle only when nearly touching.

Regenerate any sequence with, e.g.:

```sh
V="../ScreenRecording_07-10-2026 08-34-03_1.MP4"
ffmpeg -y -v error -ss 9.6 -to 11.4 -i "$V" \
  -vf "fps=60,crop=920:700:400:100" menu-open/f_%03d.png
```

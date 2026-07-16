# Segmented control targets (Receiving | Sending | Errored)

Ground truth extracted from
`../../iphone17_records/16-jul-2026/ScreenRecording_07-16-2026 19-44-19_1.mov`
(iOS 26, 998×712 @ 60 fps, 12.74 s). A light Settings page ("Transfers")
with a three-segment control; the selection lens is dragged between cells
and tapped across them.

All directories share `crop=900:130:50:60`, 60 fps (16.7 ms per frame).

| Directory | Frames | Source segment | Shows |
|---|---|---|---|
| `tap-flight/` | 45 | 6.95 – 7.70 s | tap on a non-selected cell: the lens flies from Errored toward Sending, glyphs deform through it mid-flight, settle |
| `drag/` | 132 | 7.70 – 9.90 s | continuous drag Receiving ↔ Sending: the lens rides the finger, magnifies the glyphs under it, spectral fringes on glyph edges, ownership (bold label) flips as it crosses cell boundaries |

Motion and material invariants:

- The riding lens magnifies the label under it; the displaced glyphs show
  clear per-channel rainbow fringes at their edges (`drag` mid frames:
  "S⟨fringe⟩nding").
- The lens is a quiet white capsule at rest; while ridden it lifts and its
  rim brightens, but the face stays near-transparent — the deformed
  backdrop glyphs are the face.
- The label under the lens goes BOLD (selected weight) as soon as the lens
  owns the cell, while the departed label relaxes to regular weight.
- Release flies the lens to the nearest cell center in ~150–200 ms with no
  visible overshoot.

# Menu open/expand/collapse targets (dark menu, colored backdrop)

Ground truth extracted from
`../../iphone17_records/16-jul-2026/ScreenRecording_07-16-2026 19-32-24_1.mov`
(iOS 26, 780×974 @ 60 fps, 25.75 s). A dark Sort-by/Filter menu opens out of
the header's filter pill over a magenta/purple backdrop, then EXPANDS in
place: tapping a row grows the container taller while the new rows
materialize (the same droplet law as the open, applied to a size change).

All directories share `crop=700:800:70:50`, 60 fps (16.7 ms per frame).

| Directory | Frames | Source segment | Shows |
|---|---|---|---|
| `open/` | 45 | 4.85 – 5.60 s | pill swells into a white droplet, grows into the two-row menu; content smudged until near-settle |
| `expand/` | 45 | 5.55 – 6.30 s | "Sort by" tapped: container grows DOWNWARD, sub-rows (Sections / Newest to oldest) materialize while the Filter row crossfades out |
| `collapse-row/` | 48 | 6.30 – 7.10 s | expanded menu shrinks back to the two-row form, sub-rows dissolve |
| `close/` | 48 | 19.80 – 20.60 s | menu sucks back into the pill (fast: gone within ~15 frames) |

Motion and material invariants:

- The menu body is DARK translucent glass; over the magenta header region it
  picks up a strong purple wash (the backdrop tint dominates the face).
- The expand is a container-size morph, not a new popup: the header row
  stays put, the body grows below it with overshoot, and the incoming rows
  stay smudged until the growth settles (~350 ms).
- The tapped row flashes brighter (selection highlight) as the expand
  starts.
- Close is faster than open (~250 ms) and pulls the whole body back into
  the pill's circle, content smudging immediately.

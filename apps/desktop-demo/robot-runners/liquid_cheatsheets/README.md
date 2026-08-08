# Liquid reference fixtures

Each directory under `cases/` is a frozen visual-reference case:

- `case.toml` records the source recording, crop, frame rate, and selected
  source timestamps.
- `target-sheet.png` is the one-time extraction used by future visual review.
  Robot runs never decode or inspect the source recording.

Each `robot_liquid_<case>_cheatsheet` Cargo example launches one visible
fixture window with `headless(false)`. The Cranpose robot injects pointer
events internally. The initiating event must finish and reach the visible
surface before the capture epoch opens, and every capture worker is waiting at
the start gate before that event is sent. Continued drags share that epoch;
gestures with another state-changing event are divided into separately
synchronized phases. A scheduler opens a fresh X11 snapshot client at each
deadline, so transient overlay surfaces are captured without moving or
drawing the host cursor.

The measured X11 snapshot midpoint is written as `A +…ms`; the matching source
time is written as `T …`. Settled-state cases say so explicitly instead of
implying an initiating event. Named phases identify independently staged
source clips, whose source times may reset. The actual montage uses the
target's column and tile geometry so frames map by position. Frames are stored in
`target/liquid-cheatsheets/<case>/actual/keyframes/`, and
`comparison-sheet.png` vertically combines the stored target and actual
keyframes. A rerun replaces the current generated keyframes without archiving
an unbounded run history.

The comparison image is an artifact, not a pass/fail assertion. Its purpose is
to make layout, material, and motion differences inspectable in one image.

`robot_liquid_visual` is an ad-hoc interactive walkthrough. It is not a
fixture/timestamp case and is not invoked by `liquid_cheatsheets.sh`.

Run all cases in release mode:

```bash
./liquid_cheatsheets.sh
```

Run selected cases using the faster debug profile while developing the
harness:

```bash
CRANPOSE_CHEATSHEET_PROFILE=debug ./liquid_cheatsheets.sh toggle_press menu_open
```

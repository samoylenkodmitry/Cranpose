# On-white liquid interaction targets

Canonical state grids from the iPhone 17 recordings in
`../../iphone17_records/on_white/`. The raw recordings and exhaustive decoded
frames stay outside git. Every source frame is preserved without frame-rate
resampling under `../../iphone17_records/on_white/extracted/`, together with
its presentation timestamp in `frames.csv`.

| Recording | Native frames | Duration | Overview | Detailed state grids |
|---|---:|---:|---|---|
| `touched_up_state.mov` | 403 | 6.700 s | `touched-up-state/overview.png` | `touched-up-state/touch-drag-release.png` |
| `text_handle_bubble.mov` | 1,804 | 30.065 s | `text-handle-bubble/overview.png` | `text-handle-bubble/loupe-birth.png`, `text-handle-bubble/loupe-release.png` |
| `bottom_bar_click_to_change_then_hold_a_little.mov` | 1,118 | 18.627 s | `bottom-bar-click-hold/overview.png` | `bottom-bar-click-hold/gesture-1.png` through `gesture-4.png` |
| `bottom_bar_click_to_change.mov` | 592 | 9.868 s | `bottom-bar-click/overview.png` | `bottom-bar-click/translate-to-conversation.png`, `bottom-bar-click/conversation-to-translate.png` |

Overview grids sample every 30th native frame. Detailed grids retain every
third or fifth frame around an interaction transition. Tile labels are the
exact exhaustive frame filenames.

## Motion and material invariants

- A selected bottom-bar item is a quiet gray fill at rest. It does not retain
  the raised liquid lens.
- Touch raises the same selection surface continuously: depth, refraction,
  chroma, edge light, scale, and curvature increase together. Release lowers
  those properties continuously until the surface merges into the rest fill.
- The active lens tracks the finger every frame. Fast horizontal travel
  stretches the lens along travel and compresses it orthogonally; it settles
  at the finger position instead of animating independently between items.
- Holding an item keeps the raised optical state stable. Releasing returns to
  the rest fill in roughly 10-15 frames.
- The text loupe is born at the touched selection handle as a narrow capsule,
  rises above the finger while expanding over roughly 12 frames, and then
  follows with a stable grab offset.
- Loupe contents remain crisp while geometry, magnification, edge reflection,
  and chromatic separation change. A general blur over the sampled content is
  not part of the target.
- Releasing a text handle collapses the loupe into the handle over roughly 15
  frames. The edit menu appears only after that collapse.
- The top circular action can merge with a neighboring circular action through
  a narrow neck while touched, then relaxes through translucent intermediate
  states rather than switching shape in one frame.

These grids are the visual authority for implementation and review. Current
renders must be placed beside the relevant target frames in one comparison
bitmap before accepting a material or motion change.

# Frame cost attribution on the Kirin 980 (Mate 20 X / EVR-AL00)

Device-measured findings from 2026-08-29, taken with the
`debug.cranpose.frame_telemetry` stage instrument on two real apps:
cranscan (list scrolling) and cranpose-orbit (animated showcase). Two
of these findings are statements about Cranpose itself, independent of
either app. Every number is a real-device capture; the methods notes at
the end are the traps that produced wrong readings before the right
ones.

## Finding 1 — the GPU redraws unchanged layers every presented frame

**Claim.** Cranpose has no layer-level GPU reuse and no damage
scissoring: on every presented frame, the full scene is re-rendered on
the GPU, including layers whose pixels provably did not change. Any app
that presents continuously — which is any app with an ambient
animation, a cursor blink, or a progress indicator — pays full fill
cost for its static backdrop on every frame.

**Evidence.** A fullscreen decorative layer (one radial-gradient rect
plus 160 solid circles) was measured three ways on the same screen,
single variable per arm, order-alternated, temperature-logged:

| arm | fps | render encode p50 | present p50 |
|---|---|---|---|
| layer animated (baseline) | 22.5-24.6 | 11.8 ms | 31.9 ms |
| layer frozen, primitives present | 33-35 | 5.6-6.0 ms | 21.7-22.9 ms |
| layer removed | 42.2 | 5.7 ms | 16.4 ms |

Freezing the layer returns the CPU encode to the removed level — the
retained scene is not re-encoded, retention works — but present
recovers only half: ~6 ms of GPU per frame remains for pixels that
never change, and only removal recovers it. Texture-caching stable
layers or damage-rect scissoring would recover that cost for every
decorated app. This is an architectural piece of work, scheduled after
the v0.1.105 release; it should not be attempted as a side change.

## Finding 2 — re-recording costs ~37µs per solid-brush primitive

When a `draw_behind` layer IS invalid (its animation genuinely changed
it), re-recording its primitives costs ~37µs per solid circle on this
device: 160 circles = ~6 ms of encode per frame. Two different bugs
with two different owners live here, and they must not be merged:

- **App-side (Orbit's, and any app with a particle backdrop):** do not
  re-record hundreds of primitives per frame for an effect a single
  fullscreen shader or an alpha-animated cached layer can carry.
- **Framework-side (ours):** whether a re-recorded solid circle needs
  to cost 37µs at all is a question about the encode path
  (shape-convert, brush upload, per-primitive bookkeeping), and it
  stays open until that path is profiled. Fixing the app hides the
  meter; it does not fix the rate.

## Finding 3 — frame-driven animation recomposes the world

cranpose-orbit at IDLE spends 13-19 ms of `update` per frame because an
app-level ambient animation recomposes the entire tree every frame and
rebuilds fourteen `RuntimeShader` objects from shared source. Even with
the backdrop removed entirely, the app tops out at 42 fps on this CPU
cost alone. The remedies are the ones the retention work built:
clean-slot reuse (#548) once apps can pin a release carrying it,
leaf-scoped state reads so a per-frame value invalidates leaves rather
than the root, and an answer to whether per-frame
`RuntimeShader::from_shared_source` rebuilds defeat the sharing they
exist for.

## Where cranscan landed for contrast (post-#548, scrolling)

update 2.1 / render 4.2 / present 2.4 / cpu 9.2 ms p50, fps pinned at
~58 by the 60Hz panel. Acquire (~12 ms) is backpressure slack, proven
by its GROWTH when retention removed ~1 ms of CPU: less work, longer
wait, period constant. Never optimize a wait that expands to absorb
wins. Against a 120 fps budget (8.33 ms), render is the largest
remaining stage.

## Reading the instrument (before doing any arithmetic on it)

`cpu = update + sync + render + present`; `poll` and `acquire` are
excluded BY DESIGN as waits, and `acquire` measures only
`get_current_texture`. Per-stage percentiles are computed from
independently sorted arrays: **cross-stage p50 comparisons are
marginals, not per-frame statements** — the same window can order two
stages one way at p50 and the opposite way at mean. Means are the
additive statistic; confirm any cross-stage claim on means. Orbit's
cpu > period crossing (pipelined against the swapchain) holds on means
in all three idle windows — 56.5/60.3/60.0 vs 40.0/41.5/41.5 — which is
what makes it structural rather than distribution shape.

## Finding 4 — offscreen round-trips lose to redrawing on the Mali-G76

Two architecturally-obvious fixes for the starfield were tried on the
device by the Orbit team and BOTH lost, which is a statement about the
GPU's economics, not about the fixes' authors:

- A fullscreen `shader_background` left fps flat even with a
  constant-color shader — the cost is the full-screen texture
  round-trip itself, not per-pixel math. A shader pass over the screen
  costs more than re-recording and drawing 160 solid primitives.
- Per-group `graphics_layer_block` alpha measured WORSE than baseline —
  each `GraphicsLayer` takes its own offscreen composite pass, and the
  passes cost more than not grouping at all.

What worked instead: quantizing the animation (`twinkle`/`parallax`
into discrete steps) so the existing scene diff can skip re-encoding —
27.0-29.6 → 38.2-38.6 fps at 39.0°C flat, against a 48.7 ceiling with
the field removed.

The consequence for the unchanged-layer work (Finding 1): on this
class of GPU, bandwidth is the scarce resource. Texture-caching stable
layers pays only when sampling the cached texture is cheaper than
re-rendering the layer's content — true for expensive layers, false
for cheap ones, and the two failures above are the measured proof that
"render once, composite forever" is not free here. Any design must
either skip GPU work without adding round-trips (damage-aware
rendering) or cache selectively with a cost model, and must be
device-measured, not reasoned into existence.

## Methods notes (the traps)

- xvfb frame rates are software presentation, not the GPU.
- Thermal drift on this device is ~2 fps across a back-to-back arm
  sequence at 38-39°C battery temperature; alternate arm order and log
  temps, or a late-running baseline reads as a regression.
- Force AOT (`cmd package compile -m speed -f`) and md5-verify the
  on-device APK against the host file before trusting an arm.
- One anomalous fast baseline window (30.3 fps against 23-25
  everywhere else) is recorded and NOT explained; single-window
  captures are not evidence.
- Orbit's detail screen (fullscreen fbm sun, 13-15 fps) is UNRESOLVED:
  1-2 telemetry blocks per arm, all removal arms within noise of each
  other. Longer captures are required before any claim about it.

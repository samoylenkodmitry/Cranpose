# Frame cost attribution on the Kirin 980 (Mate 20 X / EVR-AL00)

Device-measured findings from 2026-08-29, taken with the
`debug.cranpose.frame_telemetry` stage instrument on two real apps:
cranscan (list scrolling) and cranpose-showcase (the animated showcase
app, formerly named cranpose-orbit — not cranorbit, the Wear game). Two
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
never change, and only removal recovers it.

**Resolution (2026-08-30): prefix snapshots.** The first fix admitted
fullscreen chunks into the flatten cache (viewport-relative admission),
and its A/B — +4.2 fps frozen backdrop, +1.5 fps quantized — proved the
ECONOMICS: replaying a fullscreen texture beats re-rasterizing a stable
backdrop on this GPU. But that mechanism was retired without shipping,
because flattening cannot be byte-exact and a fullscreen inexact entry
is a fullscreen wrong answer. The impossibility is structural, not a
precision bug: the direct path rounds to the composition format between
EVERY overlapping write — each ROP blend reads an already-rounded
destination — while a flatten collapses that chain of roundings into
one. No entry precision fixes that (f32 included); overlapping
anti-aliased content diverges by construction.

What shipped instead is exact by construction. The scene's stable
bottom prefix — every op below the first retained or feed-captured one
— is rendered once into an entry through the same segment pipeline,
over the same clear color: identical op sequence from identical initial
state reproduces the direct path's rounding chain bit for bit. Replay
is a single REPLACE composite of that entry (Src, not SrcOver: the
entry is the target's whole post-prefix state, and a SrcOver replay is
off by one bit wherever stored alpha rounds below one at an AA pixel).
From its first sighting the prefix range is claimed away from the
flatten chunker entirely, so observe, store, and replay frames all
produce the direct bytes — the mechanism is a per-position pure
function, enforced by `scene_range_cache_exactness.rs` and the
instanced/retained/feed parity suites. The flatten class survives only
below its flat 2 MB floor, where its measured envelope is 1 LSB on
0.05% of bytes (the same suite measures and reports it).

Same-binary sysprop ladder on the Mate 20 X (idle list screen,
alternated, temps 40.0-41.0 °C flat, `debug.cranpose.no_prefix_snap`
the only variable): OFF 37.2 / 36.2 fps, ON 40.3 / 40.3 fps — +3.1 and
+4.1 fps, present p50 18.6/18.8 -> 16.6/16.7 ms.

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

cranpose-showcase at IDLE spends 13-19 ms of `update` per frame because an
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

The consequence for the unchanged-layer work (Finding 1), refined by
later ablations: the scarce resources on this GPU are per-pixel shader
ALU and render-pass overhead, NOT raw sampling fill. The fill-Mpx model
(cost proportional to composited megapixels) is retired — removing
4.8 Mpx/frame of composites bought ~0 fps, while each additional
backdrop effect pass costs ~0.18 ms against ~18 passes/frame. That
re-reads both failures above: the fullscreen shader pass and the
per-group layers lost because each added a PASS (a tile store and
reload of the target), not because sampling is expensive. So
"render once, composite forever" pays exactly when the replay rides an
EXISTING pass and displaces per-pixel rasterization ALU — which is what
the prefix snapshot does (see Finding 1's resolution) — and loses when
it adds passes. Any such design must be device-measured, not reasoned
into existence.

## Finding 5 — scroll costs a pass explosion, not a fill explosion

Idle vs continuous drag on the showcase list screen, same binary
(post-prefix, post-recompose-fix), `debug.cranpose.gpu_stats` sampled
frames: passes 16-17 -> 38-39, blur passes 0 -> 9, offscreen acquires
0-1 -> 18, layer cache 97-100% hit -> 51% on sampled frames, while
fill grows only 9.3 -> 11.8 Mpx. present p50 goes 16.6 -> ~31 ms — the
pass count, not the fill, is what doubles the GPU cost, consistent
with Finding 4's ALU-and-passes model.

Two CPU-side lessons from the same campaign:

- The showcase's `ListScreen` returned `scroll.value()` to a caller
  that discarded it — a composition-scope read that recomposed the
  whole screen every scroll frame. Removing it took scroll-time
  `update` from 15.5 to 5.7 ms p50 (two-binary A/B, alternated) and
  moved fps not at all: `acquire` absorbed every freed millisecond,
  which is the "never optimize a wait" rule doing its job. CPU-side
  fixes under a GPU-bound scroll change headroom, not frame rate.
- `debug.cranpose.layer_cache_diag` (now property-mirrored) shows the
  scroll misses are almost entirely `BackdropEffect` keys — glass
  re-captures and re-blurs as content translates beneath it, which is
  inherent to live backdrop effects — plus one avoidable class: the
  same surface oscillating between two pixel sizes one device pixel
  apart (276x277 vs 276x276) as its snapped bounds cross rounding
  boundaries during translation, churning keys for content that did
  not change.

Both candidate mechanisms were then bounded on the device BEFORE being
built, and both bounds redirected the program:

- **Blur-atlas pass batching, demoted.** With card glass replaced by
  solid rects, sampled scroll frames still ran blur=8 passes=37 and
  present improved only 1.2/3.5 ms — the blur lives in the FULL-WIDTH
  surfaces (nav ramp, search bar, tab bar) whose backdrop inputs
  change every scroll frame by nature. Pass count alone is not the
  binding constraint, so an atlas that only relieves pass count does
  not lead.
- **Half-scale backdrops, validated as a term and rejected as a
  form.** A one-sysprop probe rendering the whole backdrop chain at
  half scale measured the blur-ALU term as real (present -2.25 ms
  scroll, -2 ms idle, idle fps +0.7/+1.7) and disqualified the naive
  design twice over: `scale != root` silently forfeits the
  copy-texture fast path, putting 1.8 ms of CPU encode back inside
  the same period the GPU win came out of (a mechanism paying for
  itself with its own savings, every individual number looking
  right); and the liquid-glass refraction shaders carry pixel-space
  uniforms that do not survive a rescaled capture — square seams with
  displaced content around every planet thumbnail, probe-off arm of
  the same binary clean.

The surviving design is narrower than the one first proposed and
better for it: reduce the resolution of the PURE BLUR INTERMEDIATE
alone — the vertical pass writes a low-res target and the existing
composite upsamples — while capture keeps the full-res copy path and
every mask and refraction tail stays at composite time at full
resolution. It dodges both measured failure modes by construction,
and the blur frequency argument makes it a static win, not a
motion-gated one. It ships only behind a red-first quality test (mask
edges byte-crisp, blur envelope bounded against a full-res reference
— the naive probe's seams would have failed exactly that test) and
its own ladder.

The economics of this section: one APK build and eight device minutes
of probing demoted two mechanisms that would have cost days to build,
and the fps table alone would have credited one of them while the
screen was visibly wrong. Probe before architecting, and read the
probe's screenshots, not only its numbers.

## Methods notes (the traps)

- xvfb frame rates are software presentation, not the GPU.
- `[GPU f#N]` lines print every 60th frame and their per-frame counters
  describe THAT frame only. A sampled frame can be an ambient-step
  epoch frame (every cache missing at once) while the all-frames diag
  average sits at 1-2 misses; treating one sampled frame as the
  per-frame typical misattributed a whole scroll campaign before
  `layer_cache_diag` totals over the full window corrected it.
- A stage-level win can be self-funded by a stage-level tax in the
  same period: the half-scale probe's present p50 fell 2.25 ms while
  render encode rose 1.8 ms, and fps did not move. Before believing
  any single-stage improvement, sum the stages and compare periods.
- Shaders that take pixel-space uniforms (refraction offsets,
  container geometry) bind the capture scale into their contract; a
  capture rendered at a different scale feeds them wrong geometry
  silently. Any mechanism that rescales an effect input must audit
  the effect chain for pixel-space uniforms first — or keep the
  rescale strictly inside stages that carry no such uniforms, which
  is what confines the blur-intermediate design to the pure blur.
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

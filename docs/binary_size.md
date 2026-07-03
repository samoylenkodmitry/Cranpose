> **Status (2026-07-03): the slim Vulkan backend described in later sections
> was REVERTED** (removed from the tree; archived outside the repo). The
> measurements referencing `renderer-vulkan-slim` are historical records of
> that experiment. The durable findings — app build profiles dominate binary
> size; naga/gles and the embedded font were the big framework levers — all
> apply to the wgpu renderer and remain valid.

# Binary Size: Current State and the Road to <1 MiB

Goal: a fully GPU-accelerated hello-world with the Compose API under 1 MiB on
Linux x86_64. This document records measured data, the levers that are landed,
and the staged plan with its one hard decision point.

All numbers: minimal hello-world app (`Text` in a window, desktop +
renderer-wgpu), Linux x86_64, measured 2026-07-02 on cranpose 0.1.28.

## Measured ladder

| Configuration | Size (decimal MB) |
|---|---|
| cargo default `--release`, no tuning (what apps get by accident) | ~15 MB |
| tuned profile (opt3, fat LTO, strip, panic=abort), 0.1.27 framework | 10.62 MB |
| same, 0.1.28: GLES fallback + embedded font now opt-in | 8.71 MB |
| `release-small` profile (opt-level=z, fat LTO) | 6.02 MB |
| + `desktop-wayland` only (drop X11 stack) | 5.44 MB |
| + nightly build-std, `panic = "immediate-abort"`, `optimize_for_size` std | 4.61 MB |
| + `-Zlocation-detail=none -Zfmt-debug=none` (all of the above = `cargo xtask dist-min`) | **4.16 MB** |
| same but `desktop-x11` instead of wayland (verified running on X11/Vulkan) | 3.96 MB |
| + lld `--icf=all` identical-code folding | 3.88 MB |
| + `-Cforce-unwind-tables=no` (all of the above = final `dist-min`, verified running) | **3.35 MB** |

Real app cross-check (cranamp, ships own fonts, `default-features = false`):
45.1 MB (no profile) → 24.5 MB (tuned profile) → 22.6 MB (0.1.28 features)
→ 15.2 MB (monomorphization fixes, below) → 8.08 MB with `opt-level = "z"`
plus the `dist-min` flag set → **6.78 MB** after unifying on a single
symphonia stack (verified running; decode+seek regression tests added).
rodio 0.22 pulls a complete symphonia 0.5 (all codecs plus ~0.5 MB of FFT
twiddle tables in `.data`) beside any direct symphonia 0.6 use — cranamp now
decodes everything through its own symphonia source and keeps rodio
playback-only, a −1.3 MB app-side fix any rodio+symphonia app should copy — plus the safe-stack
graphics/window floor shared with hello-world.

## The real-app multiplier: monomorphization (fixed in 0.1.28)

Hello-world carries cranpose_core at ~0.5 MiB; cranamp carried it at
**7.3 MiB** (41% of all code). Three compounding causes, all "generic over a
closure that only ever gets erased anyway":

1. `Composer::with_group_in_active_pass` and the slot-table payload path
   re-instantiated the full group/payload machinery (~7–12 KiB) for every
   composable call site — ~5,800 copies in cranamp. Fixed by outlining into
   monomorphic cores (`with_group_in_active_pass_dyn` over
   `&mut dyn FnMut(&Composer)`, `value_slot_with_kind_dyn` over a
   `PayloadInit` type-id + boxed factory) behind thin generic shims.
2. `Composer::set_recranpose_callback` duplicated its observer wiring per
   call site (393 copies, 703 KiB). Fixed by boxing at the entry point into
   an `#[inline(never)]` monomorphic core.
3. The `#[composable]` macro made every generated helper/recompose fn
   generic over the caller's closure types even though the skip path only
   ever reads callbacks back out of the type-erased `CallbackHolder`. Widget
   bodies (Button, Layout, app composables) therefore compiled once per call
   site. Fixed in the macro: zero-arg Fn params are boxed at the public-fn
   boundary (`CallbackHolder::update_boxed`; the holder boxed anyway, so no
   new allocation) and the matching generics are stripped from the generated
   helpers, which are now compiled once per composable.

`#[inline(never)]` on the dyn cores is load-bearing: fat LTO otherwise
inlines the single-caller bodies straight back into every shim and silently
undoes the split (measured: byte-identical binaries without it).

Result in cranamp: cranpose_core 7.3 → 1.1 MiB, cranpose_ui 727 → 460 KiB,
.text 17.7 → 10.7 MiB. Perf-sensitive robot tests (lazy-list O(1)
virtualization with identical initial-render time, idle-frames, precise
fling 5/5) show no regression — the boxing sites replace allocations the
callback holder already performed.

## Where the remaining bytes live (release-small, .text via cargo-bloat)

| Component | .text | Notes |
|---|---|---|
| naga | 1.1 MiB | SPIR-V writer + WGSL parser + validator; required by wgpu's Vulkan backend at pipeline build time |
| std | 1.1 MiB | → ~0.2–0.3 MiB effective with build-std + immediate-abort |
| wgpu (core+hal+api) | 0.9 MiB | command validation, Vulkan backend |
| cranpose (core+render-wgpu+ui+facade+render-common) | ~1.65 MiB | spread wide; no single hot monomorphization monsters |
| winit + wayland + x11 stacks | ~1.1 MiB | halves when only one display server is compiled |
| tiny-skia + ttf-parser + ab_glyph | ~0.3 MiB | CPU glyph raster feeding the GPU atlas |

## Landed levers (0.1.28)

- `embedded-default-font` — 1.3 MiB NotoSansMerged is opt-in (on in `cranpose`
  default features; apps with `default-features = false` + `with_fonts` drop
  it). Also removed the double-embedding in the demo binaries (app copy +
  framework copy).
- `backend-gles` / `renderer-wgpu-gles` — GLES fallback and naga's GLSL writer
  are opt-in on desktop; Android keeps GLES via the `android` feature; web
  keeps WebGL.
- `desktop-x11` / `desktop-wayland` — display-server backends are individually
  selectable; `desktop` still means both. The X11 native-window probe (x11rb)
  compiles only with `desktop-x11`.
- App-side guidance (README "Binary Size") + tightened CI gate: isolated-demo
  release-small budget 28 MiB → 12.5 MiB.

## Dead end, measured: build-time SPIR-V cannot drop naga's WGSL frontend

wgpu 29 hardwires `wgpu-core/wgsl` (and `renderdoc`) in its own dependency
tables for native targets — the WGSL frontend is compiled in regardless of
which `wgpu` features an application selects. A full precompile lever was
built and measured (build.rs compiling all 14 framework shaders to SPIR-V,
including a shape-shader ladder for the 16/64 KiB uniform classes, with the
GPU pixel-comparison suites passing after fixing a double Y-flip from naga's
default `ADJUST_COORDINATE_SPACE` writer flag): net **+0.21 MB**, because
`spv-in` plus the embedded blobs outweigh nothing — wgsl-in stays resident.
A second route was then built and measured: `Features::PASSTHROUGH_SHADERS`
with `create_shader_module_passthrough` (unsafe, build-time-validated SPIR-V
handed straight to Vulkan, emission mirroring wgpu-hal's own writer flags).
All GPU pixel suites passed through raw passthrough — but the binary showed
net **+0.07 MB**: wgpu-core's `create_render_pipeline` branches on module
kind internally, so the naga-consuming path stays reachable and the linker
cannot strip it. Conclusion, twice-measured: **naga (~0.6 MB) and the wgpu
runtime are irreducible under wgpu 29 by any client-side route** — features,
`ShaderSource::SpirV`, or unsafe passthrough. Both sets of machinery were
reverted; the `.wgsl` file extraction was kept. The only remaining removal
paths are an upstream wgpu change (make `wgpu-core/wgsl` optional and the
pipeline path monomorphic over module kind — worth filing), a maintained
fork, or the full custom Vulkan backend.

## Staged plan to <1 MiB

Stage numbers are hello-world estimates, each stage cumulative.

1. **4.61 MiB — landed.** `release-small` + `desktop-wayland` + build-std with
   immediate-abort panics and `optimize_for_size` std, wrapped as
   `cargo xtask dist-min` (nightly-only).
2. **~4.5 MiB — feature-gate the effect subsystem.** Blur, backdrop, liquid
   glass, RuntimeShader pipelines and their WGSL sources in
   cranpose-render-wgpu behind an `effects` feature (default on at the
   facade). Hello-world class apps drop the effect renderers and shaders.
3. **~4 MiB — ship precompiled shader IR.** Compile framework WGSL to naga IR
   at build time (`wgpu::ShaderSource::Naga`, `naga-ir` feature); the WGSL
   parser (wgsl-in), codespan error rendering, and validator's parse-error
   paths leave the binary. User-facing `RuntimeShader` (arbitrary WGSL at
   runtime) becomes a feature that re-adds wgsl-in.
4. **~3.5 MiB — cranpose code diet.** cranpose_core (524 KiB) + render-wgpu
   (508 KiB) shrink by devirtualizing format machinery, trimming log/fmt
   strings from hot paths, and auditing per-widget monomorphization. Broad
   work, no single hotspot: expect 20–30%, not 2×.
5. **~3 MiB — the safe-stack floor.** What remains is wgpu_core + wgpu_hal +
   naga spv-out (~1.7 MiB even trimmed: Vulkan pipelines are built through
   naga's SPIR-V writer at runtime), winit+wayland (~0.6 MiB), std residue,
   and the dieted framework. **This is the floor while the renderer speaks
   through wgpu and the window through winit.**
6. **<1 MiB — requires replacing the GPU/window stack.** A purpose-built
   backend in the style of makepad: raw Vulkan (ash) or GL with **precompiled
   SPIR-V/GLSL shaders** (no shader compiler in the binary, ~200–400 KiB), a
   minimal Wayland client shim instead of winit (~150–250 KiB), build-std
   (~150 KiB std), and the Stage-4 framework diet (~400 KiB cranpose). Budget:
   ~0.9–1.2 MiB. The Compose API is untouched — this is a new implementation
   of the existing `Renderer`/platform seams, selected by cargo feature like
   `renderer-pixels`/`renderer-wgpu` today.

## The decision point

Stage 6 cannot be written under the workspace-wide `#![deny(unsafe_code)]`
rule: every raw GPU/display API (ash, GL, a Wayland shim — and wgpu itself
internally) is unsafe at the FFI boundary. The options:

- **A. Allow one isolated `unsafe` backend crate** (e.g.
  `cranpose-render-vulkan-slim`), everything above it stays
  `#![deny(unsafe_code)]`, the crate gets adversarial review + robot-test
  parity. This is the only route to <1 MiB.
- **B. Stay safe-stack-only** and land Stages 1–4: ~3 MiB hello-world,
  ~1.2 MiB compressed. wgpu/naga/winit keep improving upstream, but the
  2.3 MiB they cost is outside our control.

Everything in Stages 1–4 is worth landing under either answer.

## Vulkan-backend probe (measured, dist-min pipeline)

A probe binary linking the full framework rendering stack on the raw
Vulkan backend — compose core, layout, text engine, scene graph, the
graph walker and frame executor, `ash` — renders a shared-contract
fixture on real hardware and measures:

| binary | bytes |
| --- | --- |
| probe, no embedded font | **483,536 (0.46 MB)** |
| probe, with NotoSansMerged | 1,841,712 (1.84 MB) |
| hello-cranpose (wgpu, windowed, font) | 3,514,568 (3.35 MB) |

The renderer swap therefore removes ~1.5 MB (wgpu + naga in, ~0.15 MB of
backend + ash loader out). Projections: windowed hello on the Vulkan
backend ≈ 1.3–1.5 MB without the font; **cranamp ≈ 6.78 − 1.5 ≈ 5.3 MB,
and ≈ 4.8 MB with the per-display-server split — under the 5 MB target.**
The path is measured viable; what stands between the probe and the real
number is the remaining walker coverage (shadows, blend modes, runtime
shader effects) and the windowed present path.

## GOAL MEASUREMENT: cranamp on the slim Vulkan backend (2026-07-02)

| binary | bytes | MB |
| --- | --- | --- |
| cranamp, wgpu renderer (dist-min) | 7,109,000 ≈ | 6.78 |
| **cranamp, renderer-vulkan-slim (dist-min)** | **4,925,576** | **4.70** |

**Under the 5 MB target by 310 KB** — measured, launch-verified on a real
display, zero wgpu/naga symbols in the binary. The swap saved 2.08 MB
(wgpu + naga + the wgpu shell machinery out; ~0.2 MB of ash + backend +
slim shell in). Build: `--no-default-features --features
desktop,renderer-vulkan-slim,native-audio,native-dialogs` with the
dist-min pipeline.

Parity update (2026-07-03): the slim shell now hosts declared native
windows (registry reconciliation per the wgpu shell's protocol) and feeds
app fonts into the software text stack. cranamp renders **pixel-identical
to the wgpu renderer** (ImageMagick AE=0, RMSE=0 on the three-window
scene), interaction-verified (play → timer + audio; EQ toggle closes its
window through the declaration lifecycle), and the walker reports **zero
unsupported features** on cranamp's scenes. Final size with full hosting +
fonts: **4,984,024 B (4.75 MB)** — under the target with 259 KB of
headroom. Still open: window-graph docking (snap/drag between windows),
IME, robot-suite runs on slim, RuntimeShader effects (unused by cranamp),
publishing 0.1.28 to drop cranamp's local path patch.

## Final size decision (2026-07-03): full parity at 5.70 MB

Measured, settled: cranamp on the COMPLETE slim renderer is 5,698,808 bytes
(5.70 MB / 5.44 MiB) vs the wgpu renderer's stable 6,782,704 (6.78 MB) —
the slim backend is **1.08 MB smaller with full rendering parity**. The
4.98 MB earlier figure was the *hollow* renderer (effects counted, not
drawn). The +0.72 MB from hollow→complete is genuine functionality
(effects, runtime RuntimeShader compilation, batched single-command-buffer
frames), confirmed not-drift (wgpu stable) and not-a-leak (no wgpu/pollster
in the slim tree; naga is build-dep-only, DCE'd for effect-free apps).

Decision: **keep full parity at 5.70 MB.** Capability is not sacrificed for
the absolute 5 MB number. RuntimeShader::new(<any WGSL>) works; naga is free
for apps that use no custom shaders (LTO strips it). The pipeline-boilerplate
was de-duplicated into pipeline_builder.rs (DRY; byte-neutral since LTO
already shared it). Corrections to earlier claims: the display split is a
non-lever (LTO strips the unused winit backend) and the embedded font was
already excluded.

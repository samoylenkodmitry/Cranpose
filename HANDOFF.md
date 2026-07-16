# HANDOFF — liquid-glass 12-item goal (Fable → Codex/GPT-5.6)

Session `ad41042a` (Fable) ran out of usage mid-loop. Everything below is
**committed and pushed to `origin/main`** (HEAD `a50a6911`, 18 commits on top
of the `0.1.59` release `c5e9e733`). Working tree is clean. Delete this file
when the goal is closed.

## The goal (from `/goal`, verbatim intent)

1. Text-selection handles/mechanics on **all** platforms, not just android/ios.
2. Liquid glass **follows the finger** every frame, never jump-animates source→target.
3. **Physics-based** bubble de: compress on the acceleration axis, decompress on decel; the orthogonal axis does the opposite (water-bubble volume conservation).
4. Close attention to the shader — the **z-axis shape** of the glass in the target etalon is non-trivial.
5. Touching a text handle: it moves **with** the finger, then rides **above** it so the user sees it, then follows strictly (grab-offset drift). Same for the loupe bubble.
6. Wrapped-multiline handle **Y offset** is wrong (tested on android cranscan) — precise finger coordination, not a Y-shifted one.
7. Every other component still doesn't match `example/target`; pay attention to each.
8. Menu opening as a **single gesture**: long-press opens, finger slides over items (they highlight), lift fires — no lift-off between.
9. On every component the glass bubble follows the finger **every frame**, not a jump-animate.
10. Glass **frost adapts** to backdrop+foreground so text is never white-on-white.
11. Each of the above is a **separate out-of-context judge** whose verdict is **binding** and re-run fresh each time.
12. All `example/target` reproduced precisely in `desktop-demo`, working on all platforms.
13. on text input tab i don't see any text handles on desktop app;
14. liquid glass bubble doesn't follow the touch, it lags behind with ugly freezes;
15. on liquid ui tab the toggles can't toggle back;segmented has wrong y for liquid bubble; all controls expected to be touched-up-zoomed on finger touch, instead they are pressed down on touch;;
16. the glass z-geometry is wrong, it just lens, but if you look carefully at ./examples/target etalone glass you could see the glass z-geometry is beveled-in;
17. the glass recolors wrong: in target it "colors" what inside glass while what ouside stays non-coored, but in liquid ui tab i just see the selected tab being blue and hovered tab being blue - totally miss the complexity of the behavior;
18. the menu long press-and-move gesture still not works (tested with mouse)
19. menu openeing/closing shape is truncated and physics is wrong
20. strange "shadow"-like artefact around opened menu
21. you should absolutely see the result yourself before desiding that issue is resolved/fixed/implementated

## Reopened audit (Codex, 2026-07-12)

The previous close was invalid: it proved a narrow robot contract, not this
goal. Current evidence contradicts completion:

| Requirements | Current evidence |
|---|---|
| 1, 8, 13, 18 | `TextFieldHandleMetrics.touch` rejects mouse input, and the press stream that owns long-press/slide is only published for touch-like sources. The Liquid UI menu anchor is a release-click button and has no continuous long-press handoff. |
| 2, 5, 9, 14 | Segmented and tab lenses spring toward live pointer coordinates. The loupe has an explicit horizontal follow spring. These paths lag by construction. |
| 3 | Shared dynamics has velocity/acceleration inputs and inverse-axis area, but no fresh visual proof covers every travelling component or pointer cadence. |
| 4, 16, 17 | The shader has a one-direction bezel slope plus component-specific rim bands. It does not model the target's signed outer-rise/inner-recess cross-section or inside-only foreground coloration. |
| 6 | Wrap-affinity unit tests exist, but there is no current Android capture proving finger/handle alignment on the reported wrapped case. |
| 7, 12, 15 | The desktop showcase has no target-aligned proof for the full control set. The toggle pointer coroutine captures its first `checked` value. Segmented uses a lagging spring coordinate for both X and deformation. |
| 10 | Adaptive contrast currently relies on a caller-selected sign; the material is not resolved from both foreground and backdrop contrast. |
| 19, 20 | Fresh menu captures show the opening surface detached/truncated near the anchor and large black shadow/compositing rectangles during growth. |
| 11, 21 | Existing tests and previous sheets are not fresh independent verdicts for the full numbered scope. Fresh native-resolution A/B evidence is required after each fix. |

Ranked root causes:

1. Pointer position and animated settle position are the same state in several
   controls. Direct manipulation must use the raw pointer; springs begin only
   after release.
2. Pointer-source gating prevents the desktop app from exposing the requested
   selection UI and continuous menu gesture with a mouse.
3. Morphing glass uses static layer/drop-shadow geometry while its visible SDF
   changes independently, producing mismatched halos and intermediate damage.
4. The optical model treats the bezel as a single convex slope. The target has
   a signed meniscus profile with an outer rise and inset recessed face.
5. Foreground accent/contrast is changed per component outside the glass;
   target coloration is spatially confined by the live glass field.

## Reopened visual audit (Codex, 2026-07-13, round 50)

The global material and pointer-cadence changes invalidate every earlier
component-material verdict. Current target/current sheets always place the
target on the left and the current render on the right at the same logical
scale:

- `/tmp/menu-ab-r49.png`
- `/tmp/toggle-ab-r49.png`
- `/tmp/bar-ab-r49-horizontal.png`
- `/tmp/flight-ab-r50.png` (eight phase-matched launch-to-dissolve rows)

Fresh evidence:

1. `robot_liquid_bubble_physics` fails its rendered incompressible-strain
   contract. Direct tracking is within 0-3 px, but eligible flight silhouettes
   span only `87-97 x 74-81` dp; both axis-exchange ratios miss the pinned
   target. The event-velocity continuity filter fixed micro-movement impulses,
   but `STRETCH_PER_SPEED` was reduced from the measured law and now starves
   real motion.
2. The flight A/B shows a thick concentric double bevel, a milky lavender face,
   and abrupt oversized icon zoom. The target has one thin bright meniscus,
   a clear readable face, and continuous refraction through the travel.
3. The shader computes a signed raised-lip/inset slope, then independently
   paints `outer_curve`, `inner_curve`, `raised_lip_body`,
   `inset_recess_band`, `inner_wall_shadow`, `inner_contour`, and `rim_dark`.
   Those unrelated masks are the visible false bevel. Whole-face
   magnification is nearly uniform for lenses and drops to identity in a tiny
   edge feather, causing the zoom seam.
4. The toggle footprint is correctly `58 x 39` dp, but its 10 dp endpoint lean
   places it about 6-7 dp farther right than the reference and exposes a closed
   green/white donut instead of the reference's track-connected inset.
5. The settled menu is about the right width, but the row grid is too loose:
   current leading centers are roughly 10/25/41 dp farther right than target
   for check/icon/label, and the second-row baseline is about 12 dp too low.
   Its white lift also erases more backdrop than the reference.
6. Two reference bars are distinct products: Apple Developer is four tabs plus
   a detached Search circle (`tab-swipe`); App Store is one five-tab capsule
   with an inset Search selection (`bottom-bar-form`). The demo currently puts
   the detached variant over the App Store tile scene, so the complete result
   cannot match either reference.

Ranked root-cause suspicions:

1. Competing shader masks, not component color constants, create the false
   bevel and opacity.
2. The discontinuous magnification mapping creates the reported strange zoom;
   a continuous height-field mapping must own both displacement and Fresnel.
3. Pointer velocity must remain event-filtered and persistent, while the
   measured deformation gain is restored for intentional motion.
4. Bar configuration is an API/state-model issue and must represent both
   reference forms explicitly.
5. Menu and toggle residuals are local geometry/material calibration after the
   shared optic is corrected.

Architecture decision: use one continuous signed height-field optic for every
glass surface, with component parameters selecting profile depth, frost, and
magnification. Constant-only tuning is rejected because it retains the false
ridge topology; separate per-widget shaders are rejected because they would
duplicate the same optical and adaptive-contrast pipeline.

### Round 57 paired evidence and z-profile decision

Target-left/current-right composites from the fresh passing motion run are
`/tmp/menu-ab-r57-4x.png`, `/tmp/toggle-ab-r57-correct-5x.png`, and
`/tmp/tabbar-ab-r57-250.png`.

1. The toggle footprint and track alignment are close, but the current lens
   carries green across almost its entire face. The target transitions near
   the lens center into a broad pale recessed chamber.
2. `recessed_depth_slope` is defined only against `x_full = -d / bezel` and
   returns zero after `recess_start`. More importantly, its direction is the
   nearest-boundary SDF normal. In the straight middle of a capsule that normal
   is vertical, so this model cannot produce the target's horizontal face bend
   even with larger displacement constants.
3. The menu's target grid is now close, but its comparison backdrop is blank
   under most of the current panel. Material opacity cannot be calibrated from
   that sheet until the demo supplies target-like text/detail behind the menu.
4. The demo previously combined App Store backdrop tiles with the Apple
   Developer detached-Search bar. The public API now represents unified and
   detached-accessory forms separately, and a regression test pins the absence
   of an accessory gap in the unified form.

Ranked options:

1. Add one shared recessed-face gradient derived from normalized face
   coordinates, blended continuously into the signed perimeter profile. This
   gives the inset chamber a real 2D z-gradient while preserving the SDF for
   silhouette, lip, coverage, and motion deformation. **Chosen.**
2. Increase per-widget edge displacement or magnification. Rejected: the
   capsule core keeps the wrong SDF-normal direction and merely enlarges the
   existing green echo.
3. Paint a pale overlay over the toggle face. Rejected: it hides the backdrop
   instead of refracting it and cannot generalize to tab, slider, or loupe
   optics.

The first face-gradient render (`/tmp/toggle-ab-r58-5x.png`) creates the missing
pale chamber, confirming the model, but leaves a narrow green spear through its
center. Pixel topology traces that residue to the positive raised-crest slope:
it still pulls the full-color track inward at full strength. The target crest
retains chromatic dispersion but only a weak achromatic fold. The next pass
therefore attenuates the crest in the achromatic displacement only; dispersion
continues to use the unattenuated signed slope.

`/tmp/toggle-ab-r59-5x.png` confirms that crest attenuation removes the spear,
but also removes most rim color. The shader currently stores all surface
refraction in `disp_c`; spectral taps scale that same vector, coupling base
image fold and chromatic separation. The production fix is two explicit
channels: base displacement for the achromatic image and a zero-mean spectral
carrier used only as `(scale - 1)`. This preserves loupe folding while letting
interactive crests disperse without duplicating the backdrop image.

## Current wcKSRD checkpoint (Codex, 2026-07-15)

The configurable z-profile experiment is rejected. It did not reproduce the
reference and its UI must not return. The single material reference is the
wcKSRD shader in `example/shaders.txt`; every component uses that shared
implementation. The other downloaded shader variants have been removed.

The release represented by `/tmp/liquid-wc-r56-toggle-target-current.png` and
`/tmp/liquid-wc-r56-tab-target-current.png` is a useful visual checkpoint, not
an acceptance verdict. Both sheets put the target above/left and the current
render below/right. Direct visual inspection records these open defects:

1. The text-handle/loupe zero-blur path previously ran the 81-tap blur kernel
   with a forced 0.5 px step. It now samples the backdrop exactly once, and
   `/tmp/liquid-wc-r57-loupe-target-current.png` confirms crisp glyphs. The
   overall loupe remains unaccepted: its face is opaque gray, too flat/wide,
   and its lower mirrored band maps the wrong source region.
2. The toggle has the right footprint but the wrong internal topology: the
   current white circular fold does not match the target's asymmetric mirrored
   chamber and lower-edge reflection.
3. The bottom bar is close while touched. At rest it must be an ordinary bar,
   and only the touched item should become liquid and move toward the viewer.
   The lens currently leaks beyond the bar's right edge in some poses.
4. Buttons move away from the viewer on press. Target behavior lifts the glass
   toward the viewer and highlights it while the pointer is down.
5. Slider fling can reverse briefly after release. Pointer velocity, inertial
   travel, and spring settling must remain directionally continuous.
6. The blurred header flickers along its top edge.
7. The segmented lens is too thin, over-stretches into a worm-like shape, and
   settles at a geometry that does not match its selected segment.
8. Every fix requires a fresh target/current composite on one bitmap and a
   direct vision inspection before automated tests can support acceptance.

### Continuous tab-glass correction (2026-07-15, r61)

The tab selection is one persistent SDF body. It is never conditionally
mounted and has no replacement rest capsule. `GlassDynamics::activity` drives
uniform 111 and continuously scales refraction depth/curve, blur, dispersion,
highlight, tint, saturation, lift, contrast, dither, rim, adaptive frost,
shadow, wobble, bulge, ellipse blend, and incompressible strain toward
identity. Direct pointer frames still force activity to one immediately.

An inactive backdrop shader must return transparent, not an opaque sample of
its captured texture. The latter exposes the padded capture coordinate space
as a duplicate capsule even when all optical parameters are zero. Both the
backdrop output and the selection content mask now fade by the same activity;
activity zero returns exact transparent identity while retaining the node and
its pointer path.

The tab bar body remains a regular wcKSRD glass surface. Its frost was reduced
from 16dp to 4dp so the refracted scene survives instead of becoming a flat
white panel. The moving lens uses zero shader blur, so its content is sampled
once at native capture resolution. The active overlay renders only the
lens-owned cell: at the canonical Account-to-WWDC half-cell drag, WWDC becomes
blue while Account remains black. Recoloring every overlapped tab was the main
source of doubled, soft-looking glyphs.

The aligned visual harness now derives the Apple reference title position from
the actual tab-bar crop origin. Its prior scroll restoration put the
orange/purple optics tiles behind the current bar while the target used the
Apple scene, invalidating material comparisons. The fresh exact-alignment
sheet is `/tmp/liquid-r61-tab-target-current-half.png` (target top, current
bottom). It confirms the inactive duplicate is gone and foreground ownership
matches. Remaining tab delta: current surface frost/lift is still slightly
more uniform than the target's localized gray dome and the wcKSRD returned-ray
contour needs further pixel calibration; this is not final acceptance.

Regression/verification at this point:

- continuous activity material tests pass at exact zero and one;
- zero activity transparent-identity shader test passes;
- lens-owned cell composition test passes;
- real-X11 `robot_liquid_visual` passes with a 3x target-aligned capture;
- real-X11 `robot_liquid_bubble_physics` passes direct follow, deformation,
  monotonic travel, and settle;
- workspace tests and strict all-target/all-feature clippy pass with zero
  warnings;
- live release hash is
  `98ca3e0f86e2bda822bfc47a6c1bee209ea8aa5464eda154775bc59f6952e303`.

## Project orientation (fresh checkout)

**cranpose** is a Jetpack-Compose-style declarative UI framework in Rust: `#[composable]` functions, `remember`/`mutableStateOf`/`State`, modifier chains, closed-form value-space spring animation, rendered by wgpu (Vulkan on this Linux/X11 host) with a software text raster. Targets: desktop (winit), wasm/WebGL2, android, ios. There is a headless **robot** test harness that drives real gestures and captures screenshots.

Crate map (`crates/`):
- `cranpose-core` — runtime, recompose scheduler, state/snapshot, frame clock.
- `cranpose-ui` — widgets, text (measure/layout/wrap), layout engine, modifiers, `basic_text_field.rs`, `text_selection.rs`, `text_field_modifier_node.rs`.
- `cranpose-ui-graphics` — `Color`, draw primitives, **`shaders/liquid_glass.wgsl`** (the glass shader; uniform accessors `get_float(N)`/`get_vec*`), `liquid_glass.rs`.
- `cranpose-liquid` — the **Liquid Glass** component library (`Glass` material, `dynamics.rs`, `widgets/{tab_bar,toggle,segmented,slider,menu,button,card,nav_bar,search_field}.rs`, `motion.rs`, `theme.rs`).
- `cranpose-render/{common,wgpu}`, `cranpose-app-shell`, `cranpose-foundation`, `cranpose-animation`, `cranpose-testing` (robot assertions/helpers), `cranpose-macros`.
- `cranpose` — top-level crate + platform entry points: `src/{desktop,web,android,ios}.rs`, `src/robot.rs` (`capture_keyframes`, `touch_down/move/up`, `find_button_bounds_exact`, `measure_text`).

Apps: `apps/desktop-demo` (the showcase + `robot-runners/` + `examples/`), `apps/android-demo`, `apps/desktop-demo-platform` (android/wasm exported libs).

Reference media (the etalon): `example/iphone17_records/*.MP4` (source recordings — `bottom_bar_glass_effects_and_form.MP4` 1320×2868 @60fps, `text_handles_bubble_and_popup.MP4` @120fps) and `example/target/{overview,toggle-press,menu-open,tab-swipe,text-selection,bottom-bar-form}/` (canonical pre-extracted PNG frame sequences, each with a README of measured invariants).

Load-bearing architecture facts (deeper notes are in the Fable memory dir `~/.claude/projects/-home-s-develop-projects-compose-rs-proposal/memory/*.md`, which Codex won't share — the key ones):
- **Glass geometry is dp + node-size container**, never bake density into shader uniforms (robot captures render at scale 1.0; desktop density ≈1.354). Shader has two modes: explicit-rect (container>0, all-dp) and cover (container==0, px).
- **sRGB pass-through**: never pick `*UnormSrgb` swapchains; colors are sRGB bytes.
- The glass **backdrop effect** composites premultiplied-src-over and is **transparent outside the SDF**; it ignores layer alpha (this is why the demo unmounts covered buttons rather than fading them).
- Animatable springs stamp `start_time` on their **first frame after `animateTo`**, so a pose lags its label by one keyframe step — say so on judge sheets and add 1ms "stamp-steps" in keyframe sequences.

## Running independent judges (the full method — user directive #11, reproducible)

Every visual-match claim is settled by a **fresh, out-of-context sonnet judge** comparing A (target) vs B (ours), and its **CONFIRMED verdicts are binding** — but you **pixel-arbitrate every claim first** because judges mis-measure. Concrete pipeline (all commands run this session):

**1. Extract A (target) frames.** Prefer the pre-extracted `example/target/<component>/`; regenerate a tighter crop with ffmpeg:
```sh
ffmpeg -ss <start_s> -to <end_s> -i example/iphone17_records/<file>.MP4 -vf "fps=<N>,crop=W:H:X:Y" out_%03d.png
```
**Find the true event frame** (press/growth start) — do NOT trust wall-time labels — via consecutive-frame diff; the first frame where the mean jumps is t=0:
```sh
magick f_030.png f_031.png -compose difference -composite -format "%[fx:mean*255]" info:
```
(Mislabeling A's press frame caused a whole wasted judge round this session.)

**2. Capture B (ours) on the ANIMATION clock**, never wall-clock sleeps (they flake under host load — a sleep-sampled morph "confirmed" three innocent subsystems in a bisect). A robot runner calls:
```rust
robot.capture_keyframes(1.0, &[(0.0,false),(1.0,false),(20.0,true),(40.0,true), …]) // (advance_ms, capture)
```
It advances `last_frame + dt` atomically and returns one screenshot per capturing step. Include a `(1.0,false)` stamp-step after the trigger (spring start-time stamping). Run: `ROBOT_SHOT_DIR=<dir> cargo run -p desktop-app --example <runner> --features desktop,robot-app`. The `robot_liquid_motion_contract`, `robot_liquid_bubble_physics`, and `robot_text_loupe` runners already save labelled keyframe series.

**3. Build the A/B sheet at NATIVE resolution** (downscaled tiles read as "opaque/no refraction" — a real false-verdict source this session). Crop the component region, label each tile, montage two columns:
```sh
magick in.png -crop WxH+X+Y tile.png
magick tile.png -background gray15 -fill white -pointsize 18 label:"A t=83ms (cruise)" -append labelled.png
montage A00.png B00.png A01.png B01.png … -tile 2x -geometry +4+4 -background gray10 sheet.png
```
Align rows by animation **phase** (launch/cruise/brake/arrive/settle), not raw wall time, when framerates differ. (Python helpers used this session live in `scratchpad/judge*/build_sheet*.py`.)

**4. Dispatch a FRESH judge each round** — never reuse one (bias):
```
Agent(subagent_type: "general-purpose", model: "sonnet", prompt: <<sheet path + rubric>>)
```
Prompt must state: A=ground-truth / B=ours; per-row phase labels; **SCOPE EXCLUSIONS** (font, exact content, colors, A's video motion blur, sub-3px AA, absolute pixel sizes → judge proportions); any known **state caveats** (e.g. "B pressed an already-ON toggle in these rows"); and a per-aspect **MATCH/CLOSE/MISMATCH** rubric ending in a ranked list of concrete visual deltas. Ask it to zoom into crops for close calls.

**5. Pixel-arbitrate before acting.** For each claim, run a measurement — crop+diff (`-compose difference`), luma probe, or bbox (`numpy`/PIL) — to confirm or refute. Refuted this session: "frozen dissolve plateau" (pixel-diff showed continuous change; the pill's right edge is static *by design* since lens==pill width), "opaque no refraction" (downscaled-sheet artifact — full-res showed dome bowing), and several "sheet-state mismatch" claims (A presses an OFF toggle, we captured an ON one). Document the refutation, rebuild the sheet with a caveat, and **re-judge fresh**. Never overrule a *confirmed* verdict; when the judge misreads the TASK, improve the prompt/sheet and re-run (user directive: "if he understood task wrong make better description").

This loop ran ~6 rounds for the flight lens (converged deformation-law + volume to MATCH), plus menu and toggle rounds — see IN FLIGHT above for the open ones.

## DONE this round (committed, contract-pinned)

| Goal | What landed | Contract |
|---|---|---|
| 1 | Desktop winit routes touchscreen + pen-tip Contact presses like iOS (`ButtonSource::Touch{..}` / `TabletTool{Contact}` arms in both winit press handlers in `crates/cranpose/src/desktop.rs`); touch lift-off releases at position. No per-OS gates in selection code. | robot suite touch paths |
| 2,9 | `cranpose-liquid/src/dynamics.rs` `LiquidDynamics` (one integrator, animation-clock dt from `RuntimeHandle::last_frame_time_nanos()`) replaces 4 per-widget finite-diff copies (tab bar/toggle/segmented/slider). Tab-bar flight = `LiquidMotion::glide()` spring, instant arm, no jump. | `robot_liquid_bubble_physics` |
| 3 | Same integrator: stretch=1+2.5e-4·speed−1.1e-5·accel (clamp .78..1.42), ortho=1/stretch, brake bulge, asym attack/release taus, tensor `pose.size()`. 10 unit tests. | `dynamics::tests::*` |
| 5 | `ratchet_grab_bias` in `text_selection.rs`: 35% of downward finger travel drifts the handle above the finger (−(2R+4dp)), upward never un-ratchets. | unit tests |
| 6 | `LineAffinity` (Upstream/Downstream) for shared soft-wrap boundary bytes (mid-word wraps). END/cursor/caret/loupe upstream, START downstream. Fixed the cranscan wrapped-Y bug (failing tests first). | `caret_visual_line_upstream_*`, `shared_wrap_boundary_anchors_by_handle_affinity` |
| 8 | Single-gesture menu: node publishes `TouchPressTrack` + gesture-claim gate; controller adopts claim; fling-style frame-clock long-press watcher (500ms/12dp → word select → menu during hold); `LiquidTextMenu` `live_point` hover + fire-on-release. | `robot_menu_slide` |
| 10 | `Glass::adaptive_contrast(±s)` uniform 91 — backdrop-luma-keyed lift, byte-inert at 0; bar uses −0.14. | `robot_adaptive_frost` |
| 4 (partial) | Dome magnification (`liquid_glass.wgsl`, floor 0.62 + 2.5dp silhouette **edge feather** so straddling icons don't split), top-edge **fold** (`Glass::edge_fold`, uniform 92, bar 0.45), collapse-into-pill dissolve, native-res fringe (bar lens chroma 2.8), accent **rides the glass** (per-cell proximity-gated), dissolve tail 420-spring ≈370-430ms. | `robot_liquid_bubble_physics` |

New robot runners (registered in `apps/desktop-demo/Cargo.toml`):
`robot_liquid_bubble_physics`, `robot_menu_slide`, `robot_adaptive_frost`.

New shader uniforms: **91** = `adaptive_contrast`, **92** = `edge_fold`. Runtime added `last_frame_time_nanos()` on `Runtime`/`RuntimeHandle`.

## IN FLIGHT — judge loop not yet converged (pick up here)

Two round-2 judges were dispatched as Fable subagents and **will not reach a
Codex session** — re-run fresh judges from the already-built sheets below.

### Flight physics (goal 4) — round-7 fixes committed, round-7 sheet built, NOT judged
- Round 6 (last completed judge): **deformation law MATCH** ("velocity-coupled, not keyframe-interpolated"), **volume MATCH**, continuity/outline/settle/timing CLOSE, **material MISMATCH** (rim fragments piercing the silhouette + near-blank interior at 3/5 flight frames + "couple tab accent to glass position not tap time").
- Round-7 fixes (commits `9d18206d`, `541a6658`): partial-ortho height (`h = base_h + (raw_h−base_h)*0.35` — cruise lens stays tall enough to cover the icons whose tops pierced the rim), **accent rides the glass** (destination colors as lens arrives, source keeps accent until lens departs), edge feather, chroma 2.8, tail spring 420.
- **NEXT:** judge `scratchpad/judge1/sheet_flight7.png` (built, dense dissolve). Likely-remaining and needing arbitration, not a naive fix: (a) "blank white interior" — our demo page is near-white so a *clear* lens correctly shows little through it; measure vs target caveat rather than adding frost; (b) fringe visibility at native res over white. Rebuild the flight keyframes with `ROBOT_SHOT_DIR=<dir> cargo run -p desktop-app --example robot_liquid_bubble_physics --features desktop,robot-app` (saves `flight-*ms.png`).

### Menu growth (#32) — anchor smudge fixed; round-2 judge landed with 6 confirmed deltas
- Round 1: **anchor smudge CONFIRMED** (defocused blue button glass glowing through the settled card) → **FIXED** commit `3ce8de22` (covered nav buttons fade then **unmount** — their glass is a backdrop effect that ignores layer alpha; a fixed spacer box holds layout). Verified clean.
- **Round 2 (fresh judge, state-aligned sheet `sheet_menu_grow2.png`) — BINDING verdict, act on these (settle/material/overshoot MATCH; the rest MISMATCH):**
  1. **Front-load content materialization** (largest gap): item labels must begin (smudged) by ~35–40% of growth (~+120ms) and be sharp **at or before** shape-settle. Ours is blank until ~80% (+265ms) and still blurry at the +330ms settle, fully sharp only +500ms. The content reveal runs on the slower `reveal` spring (~stiffness 110 in `menu.rs`) — speed it up / start it earlier relative to the geometry spring.
  2. **Front-load the size growth** (fix easing): ~90% of final size by the animation midpoint. Ours is still visibly small at +205–265ms. Re-shape the `appear^1.6` remap (it currently *delays* early growth).
  3. **Add the horizontal-pill intermediate aspect**: A goes round → **wider-than-tall pill** (+67–133ms) → tall card late. Ours goes taller-than-wide from the start. The `t_w = t^1.9` width-lag is **backwards for this** — width should LEAD height early, then height catches up. Rework the w/h remaps so the blob is wide first.
  4. **Smooth the early silhouette**: +37–130ms shows a lumpy bi-lobed outline with a notch (the anchor `melt` bump + neighbor shapes union with too little glue early). Single clean oval — raise early glue / soften the melt-bump radius.
  5. **Anchor icon should MELT** (blur+shrink from ~30% mark), not stay pixel-crisp to +265ms then cut out.
  6. **Absorb the neighbor faster/softer** (~50–70ms), not a separate hard lobe for the first 40%.
- All six map to `crates/cranpose-liquid/src/widgets/menu.rs` (morph `appear`/`reveal` clocks, `t_w`/`t` remaps, `melt` staging, neighbor `glue` schedule) and the demo's content blur-in. Re-judge fresh after each change.

### Toggle (#33) — round-1 arbitrated, state-matched round-2 running
- Round 1 findings 1/2 were **sheet-state mismatch** (A presses an OFF/gray toggle and drags to ON — the "achromatic phase" is A's off-state, the "bowed green boundary" is the OFF→ON cap edge under the lens; B pressed an already-ON toggle where a flat green fill is optically correct). Finding 5's "double shape" decomposed as the **card corner** + the **split-track fill riding the thumb divider (by design, iOS)**.
- Rebuilt **state-matched** sheet `scratchpad/judge_toggle/sheet_toggle2.png`: B's OFF-side drag rows now correspond to A's drag phase; ON-side rows carry a state caveat.
- **NEXT:** re-judge `sheet_toggle2.png` fresh. Genuinely-plausible remaining item: toggle lens **rim chromatic fringe** may read weaker than A's multi-hue arcs — check the toggle lens `chromatic_aberration(3.2)` renders at native res over the green track; if weak, bump like the bar lens (1.2→2.8) did.

## Gate commands (all were GREEN at `a50a6911`)

```sh
cargo test --workspace                       # 91 test binaries; also the half-state-language guard (no "migration"/"legacy" in source)
cargo clippy --workspace --all-targets       # MUST be zero warnings
cargo fmt --all
apps/desktop-demo/build-web.sh               # WASM
JAVA_HOME=/usr/lib/jvm/java-21-openjdk apps/android-demo/android/gradlew -p apps/android-demo/android :app:assembleRelease
CRANPOSE_HOST_MAX_TEMP_C=97 CRANPOSE_HOST_RESUME_TEMP_C=93 CRANPOSE_HOST_TEMP_MAX_WAIT_SECS=900 ./run_robot_test.sh --sequential   # 127/127
```

Single robot runner: `ROBOT_SHOT_DIR=<dir> cargo run -p desktop-app --example <name> --features desktop,robot-app`.

## Release status

Workspace is at **0.1.59** (last published, `c5e9e733`). **This round's 18 commits are UNRELEASED on main.** Once the three judge loops (flight/#32/#33) converge and gates are green, the flow is the usual: bump `Cargo.toml` workspace version + intra-workspace pins + `Cargo.lock`, `Release X.Y.Z: …` commit on main, lightweight tag `git tag --no-sign vX.Y.Z` (global `tag.gpgsign=true`), push → `publish.yml`. Then integrate into cranscan (`~/develop/projects/ocr`, 7 cranpose pins) + release. Do NOT release with judge loops open unless the user says so.

## Constraints (AGENTS.md — non-negotiable)

Zero warnings everywhere; all tests pass ("never *not yours*"); no `git reset` (stash); no `rm -rf` (mv to `_old`); no "migration"/"legacy"/"deprecate" wording (there is a source guard test); fix root causes, failing test first for bugs; robot suite sequential with the thermal knobs above. Commit trailer:
```
Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V9mAB1Wg7LEZvRRp1xnpfa
```
(Codex should use its own trailer.)

## Scratchpad judge assets (this host)

`/tmp/claude-1000/-home-s-develop-projects-compose-rs-proposal/ad41042a-6471-49b0-819b-b09a34b0e226/scratchpad/`
- `judge1/sheet_flight{5,6,7}.png` + `phys/` measurement scripts (`measure_segment.py`, `find_segments.py`)
- `judge_menu/sheet_menu_grow2.png`, `judge_toggle/sheet_toggle2.png`
- `liquid-bubble{,2..7}/`, `menu-grow{,2,3}/`, `toggle-ab{,2}/` — raw keyframe captures

## Key files touched this round

`crates/cranpose-liquid/src/dynamics.rs` (new), `.../widgets/{tab_bar,toggle,segmented,slider}.rs`, `.../material.rs`, `.../motion.rs`; `crates/cranpose-ui-graphics/shaders/liquid_glass.wgsl`; `crates/cranpose-ui/src/{text_selection,text_field_modifier_node}.rs`, `.../widgets/{basic_text_field,text_selection_menu}.rs`; `crates/cranpose-core/src/runtime.rs`; `crates/cranpose/src/desktop.rs`; `apps/desktop-demo/src/app/liquid_ui.rs`; `apps/desktop-demo/robot-runners/robot_{liquid_bubble_physics,menu_slide,adaptive_frost}.rs` + `robot_liquid_motion_contract.rs`.

## 2026-07-13 menu morph diagnostic (paired r76)

The required target-left/current-right sheet is `/tmp/menu-r76-paired-key.png`;
native-size zooms are `/tmp/menu-r76-row{075,100,130,330}-2x.png`. Vision and
full-resolution target inspection establish these measurements (target source
is 3x device pixels, so values below are logical dp):

| phase | target frame | target glass contour | current behavior |
|---|---:|---:|---|
| source | `f_042` | two independent 44dp controls | matches |
| +33ms | `f_043` | one smooth ~96x78 droplet | still two independent controls |
| +67ms | `f_045` | ~127x94 | ~124x69 horizontal capsule |
| +100ms | `f_047` | ~178x107 | ~166x78; blurred row ink escapes left of glass |
| +133ms | `f_049` | ~214x108 | width is close, height still late |
| +167ms | `f_051` | ~235x106 | close footprint, wrong preceding trajectory |
| +200ms | `f_053` | ~244x103 | near settle |
| +267ms | `f_057` | ~256x103 width overshoot | current overshoot timing is broadly close |

Findings ranked by evidence:

1. The unit oracle in `menu_geometry_recoils_round_then_opens_through_a_wide_oval`
   incorrectly requires only 70-78dp height at the +75ms spring state. The target
   is already ~94dp and reaches a short ~107-108dp vertical overshoot before
   returning to ~103-104dp. This wrong test drove the visible flat-capsule error.
2. The menu API currently receives only the trailing anchor. The target's first
   animated pose is the aggregate volume of both trailing controls; the filter
   rect is already measurable by the UI but is discarded. Scaling one anchor
   cannot reproduce the +33ms footprint or center.
3. Rows use a full-width layer with blur and only a weak 0.92..1.0 transform.
   The morph backdrop is SDF-shaped but `.no_clip()` leaves child content
   unmasked, which is why the +100ms blurred icon appears left of the glass.
4. Reveal timing at stiffness 200 is now close enough to judge only after the
   correct geometry and mask land; diff-derived geometry extents are currently
   contaminated by unmasked blurred row pixels.

Architecture options:

1. **Chosen:** report the complete source cluster to `LiquidMenu`, derive one
   smooth aggregate birth ellipse from those real rects, and interpolate exact
   physical geometry from that source. In the shared material path, reuse the
   liquid shader's existing scene SDF in a content-mask mode so backdrop and
   child clipping are bit-identical. This fixes both root causes without a
   target-specific coordinate or duplicated SDF.
2. Restore separate neighbor SDF lobes with smooth-union glue. Rejected: the
   earlier implementation produced the independently judged notch/bi-lobed
   silhouette; the target has a single smooth aggregate outline by +33ms.
3. Delay/fade/scale the rows until they happen to fit. Rejected: it hides the
   clipping defect, regresses the binding early-materialization verdict, and
   still permits blur kernels outside the live surface.

## 2026-07-13 menu morph binding judge (paired r82)

The fresh out-of-context judge reviewed `/tmp/menu-r82-paired.png` with the
target on the left and the current capture on the right. Its binding verdict
was MISMATCH for source merging, silhouette trajectory, early topology, and
the absorbed blue control; content staging and the final surface were CLOSE.
The ranked deltas were:

1. The target body drops during its fast expansion, overshoots vertically,
   then rebounds into the settled card. The current body expands around an
   almost pinned center and therefore looks like a direct interpolation.
2. The target keeps the absorbed blue control recognizable through the early
   expansion, then stretches it into an irregular cyan vertical refraction.
   The current control loses its glyph immediately and settles into a round
   Gaussian spot.
3. At +37ms the current capture still contains both source-control shells,
   producing a scalloped crown. The target has already absorbed the menu
   trigger; only the neighboring blue source participates visibly.
4. Target row content rides the downward excursion. Current content stays
   higher, tighter, and reaches its final registration too soon.
5. The settled footprint is close; current frost is slightly more opaque and
   its shadow and perimeter are tighter.

Full-resolution pixel arbitration confirmed findings 1-4. Target center Y is
approximately 76dp at +33ms, 84dp at +67ms, 94-95dp at +100-133ms, 91dp at
+167ms, 87dp at +200ms, and 81dp at +267ms before settling near 80dp. The
current center has no corresponding downward excursion. Inspection of the
source layers also confirmed that the trigger's 120ms cover tween leaves its
backdrop glass mounted long after the target has absorbed it.

Architecture choices:

1. **Chosen for trajectory:** one pure `menu_vertical_overshoot(path)` law
   supplies the same offset to the shader geometry and row-content layer. The
   excursion rises quickly, peaks during the width/height acceleration phase,
   and returns before settle. A single law prevents glass and content from
   drifting apart.
2. Apply a transform only to the popup content. Rejected: the silhouette would
   remain wrong and rows would visibly detach from the refractive body.
3. Move the popup node itself. Rejected: popup hit rectangles and the anchor
   coordinate frame would move with animation, breaking the continuous
   press-slide-release gesture.
4. **Chosen for source topology:** unmount the trigger's backdrop effect at
   the measured absorption time instead of fading it for 120ms. Alpha cannot
   hide a backdrop effect in this renderer, so leaving it mounted is
   structurally wrong.
5. **Next architectural step for the blue source:** represent an absorbed
   source as geometry plus transferable visual content, so its backplate and
   icon can remain crisp initially and then deform inside the same menu surface
   mask. A menu-specific hardcoded filter drawing is rejected; the source
   transfer must work for any glass control.

The native-resolution source-only comparison
`/tmp/menu-r83-source-paired.png` confirms that choice. From +33ms through
+133ms the target retains a readable white glyph on a blue foreground core
above the growing glass. The current implementation shows only the fixed
button sampled through the popup's blur, so the new body clips it to a small
blue cap immediately. By +167ms the target foreground begins stretching and
blurring into the backdrop sample. Therefore an absorbed source needs both a
real source rect for the SDF trajectory and a transferable foreground visual;
backdrop refraction alone cannot reproduce the observed layer order.

The first transferred-source capture is
`/tmp/menu-r84-source-paired.png` (target left, current right). It confirms the
new layer order but rejects the initial calibration: current stays nearly
source-blue and sharp through +130ms, while target is pale cyan and begins
softening; current's +200ms smear is also too dark and too tall. Native pixels
put the target blue core near 38% of the 44dp outer control, versus the shared
icon button's current 50%. The correction therefore belongs partly in the
shared icon-button foreground geometry, with the menu transfer using lower
alpha, an earlier blur ramp, and a shorter stretch.

Follow-up pixel arbitration corrected the diameter estimate: color-isolated
blue bounds are 65x66 device pixels in target `f_042`, or approximately
22x22dp. `r85` measures 17x17dp after the 38% change. The target therefore
confirms the original 50% backplate diameter; only the glyph needs the smaller
ratio. The paired final frames also reveal a separate ownership error: keeping
the source foreground mounted below the popup creates a 39x33dp round blue
sample, while target's settled caustic is approximately 17x37dp. The live
button must retain its glass shell but transfer foreground ownership to the
menu for the entire open/close morph.

`r86` implements single ownership and is paired at
`/tmp/menu-r86-source-paired.png`. Fixed-coordinate center probes (red channel,
lower means stronger blue) are target/current: +33ms 219/191, +100ms 189/191,
+167ms 171/183, +267ms 177/213. The target does not monotonically fade: it
starts pale, concentrates blue energy as the glyph melts, then retains a
settled caustic. Shape inspection also shows a round blur through +167ms and
anisotropic stretch only afterward. The next law therefore separates early
isotropic shrink from late stretch and uses a peaked, persistent color-energy
curve rather than an opacity fade.

The `r87` same-image review shows that a single transferred overlay still has
the wrong topology: it remains a symmetric blurred ellipse above the material,
whereas target transitions from crisp foreground through +133ms into a source
that is refracted by the glass z profile. The selected architecture splits the
transfer into two ordered layers in the popup root: a foreground copy drawn
after the glass during the readable phase, and a deformed source drawn before
the glass during/after melt. The latter is sampled by the real backdrop shader,
so its settled asymmetry comes from the same z surface as every other source.

`r88` confirms that popup sibling ordering is sampled by the backdrop shader.
The late mark is now materially integrated, but its optical footprint reaches
full energy too late: tight source crops have minimum red target/current
161/198 at +200ms and 167/178 at +267ms. Since backdrop alpha is already one,
the remaining cause is the deformation area. The handoff and stretch must be
front-loaded so +200ms is already close to settled caustic dimensions.

The motion robot's old +75ms aspect check is invalid after the confirmed
vertical rebound. It derives height from a diff against the closed frame, so
its bbox is the union of the departed source and displaced body (108px), not
the live SDF height. Exact contour dimensions remain pinned by
`menu_geometry_merges_sources_and_matches_the_measured_growth_contour`; the
robot keeps render-presence and width progression checks and reports the
diff-center trajectory separately.

## 2026-07-13 menu binding judge and shared face-field decision (paired r89)

The fresh out-of-context judge reviewed `/tmp/menu-r89-paired.png` and
`/tmp/menu-r89-source-paired.png`, always target-left/current-right. Its
overall verdict was CLOSE. Pixel arbitration rejected the claimed source,
settled-footprint, horizontal-growth, and row-grid errors:

- The target blue core is `65x66` device pixels at 3x, or `21.7x22dp`;
  current is `22x22dp`. The claimed 25% deficit is not present.
- Source-center separation is approximately `51dp` target and `52dp`
  current. Current is not 15% tighter.
- The target growth widths are approximately `96, 127, 178, 214, 235, 244,
  256dp` at `+33, +67, +100, +133, +167, +200, +267ms`. The pure current
  geometry reaches `94-98, ~127, 174-182, 205-216, 228-238, 241-249,
  252-259dp` at the same phases. The alleged jump/plateau is not in the
  geometry law.
- Current settled coverage is `249x105dp`; the target contour is roughly
  `244x103dp`. Current is slightly wider, not 6-7% narrower. Both row pitches
  are approximately `42.5dp`, not 15% apart.

Three judge findings are confirmed and binding:

1. The absorbed blue source settles into a broad symmetric Gaussian plume.
   The target retains a compact folded caustic with separated blue/cyan
   structure and a darker lower bulb.
2. In-flight row ink is scaled and blurred but does not undergo enough lens
   displacement. The target visibly warps the rows through the growing
   surface.
3. The checkmark starts resolving by the current `+165ms` frame; the target
   keeps it absent at `+167ms` and materializes it around `+200ms`.

The target-left/current-right toggle sheet `/tmp/toggle-r94-paired.png`
provides a cleaner probe of the same z-field error. The README pins the track
at `63dp`: from its approximately x37 device-pixel left contour, the raw right
endpoint is near x226 at 3x. The pressed target terminates saturated green near
x209, a `5-6dp` inward fold, and leaves a broad pale chamber. Current saturated
green reaches its raw x844 endpoint. The target also has an asymmetric
green/yellow upper meniscus and cyan lower return, while current draws a
near-continuous cyan double rim. The bar sheet `/tmp/bar-r89-paired.png` shows
the same overdrawn rim around the selected tab, in addition to local
component-size differences.

Architecture options:

1. **Chosen foundation:** define the recessed face as an actual height field
   in physical units. Its center sits `recess_depth * bezel` below the
   shoulder, and the analytic derivative (including normalized-ellipse chain
   rule) supplies refraction opposite the surface gradient. Blend this field
   continuously into the signed perimeter meniscus. This makes depth scale
   with the authored glass cross-section and gives toggle, tab, and slider one
   optical model. It does not by itself solve the separately confirmed menu
   source-transfer blur or the duplicated spectral rim.
2. Raise per-widget displacement or magnification. Rejected: both retain the
   wrong face derivative and enlarge the current green echo/zoom.
3. Draw a pale face overlay or hand-shaped blue menu mark. Rejected: neither
   refracts arbitrary backdrop content, so they cannot reproduce the same
   target over text, tiles, and controls.

The pressed-toggle oracle therefore requires the strict saturated-green mask
to terminate at least `3dp` before the raw track endpoint. Pale green spectral
fringe is deliberately excluded. `lens_probe` now runs at the toggle's gentle
`1.02x` magnification so the physical face derivative can be inspected without
the default lens zoom hiding it.

## 2026-07-14 secondary slot-host ownership and callback promotion

The intermittent `state cell missing` panic after switching demo tabs was not
a liquid-widget state bug. A callback created by measure-time subcomposition
could remain locally active after the source composition that supplied its
state had been removed. Replaying that callback then read a released state
cell. The focused regression is
`subcomposition_scope_inherits_source_owner_lifetime`.

The first implementation put the captured source scope on the secondary
composer's normal scope stack. That stopped the stale callback, but a full
workspace test exposed a second deterministic failure:
`dismissed_row_inside_lazy_column_leaves_no_lingering_strip` exhausted the
100-round invalid-scope limit. The secondary root treated its source scope as
a normal recomposition parent, so a callbackless secondary group promoted its
invalidation into a different `SlotsHost` and re-enqueued the same work.

The scope graph now has two distinct weak relations:

1. `parent_scope` is structural ancestry inside one slot host. Callback
   promotion follows only this relation.
2. `lifetime_owner_scope` binds a secondary root to the source scope captured
   at the subcomposition call site. Effective activity follows this relation,
   but callback promotion never does.

`CapturedCompositionContext` carries locals plus a weak source owner. A new
secondary root receives that owner without placing it on the structural scope
stack; descendants inherit activity through their ordinary structural parent.
Expired or inactive links make the secondary scope effectively inactive, and
inactive callbacks are deferred until the tree is composed and reactivated
again. This preserves state lifetime without allowing recomposition to cross
slot-host boundaries.

Verification after the split:

- core ownership regression: passes and asserts the secondary root has no
  structural parent, has the captured lifetime owner, and has no cross-host
  callback-promotion target;
- LazyColumn dismiss regression: failed before the split with
  `RecompositionLimitExceeded`, passes afterward;
- `cargo test -p cranpose-core`: 661 unit tests plus integrations and docs pass;
- full workspace `cargo test` and `cargo clippy` on the isolated 12-core
  builder pass with zero warnings.

## 2026-07-14 checkpoint verification and visual truth

This checkpoint is functionally green but is not the final visual etalon.
Fresh target/current composites were built from the same image for each
inspection rather than judging separate screenshots from memory. The binding
remaining differences are:

- Toggle press: current dome is too flat and wide, with a doubled sharp
  spectral rim. The target has a taller egg-shaped chamber, one asymmetric
  green/yellow upper meniscus, and a cyan lower return.
- Tab swipe: current selected surface is too frosted and produces only mild
  icon doubling. The target is clearer and bends the blue WWDC foreground
  across most of the dome, with a separated Account-side image.
- Open menu: geometry and row pitch are close, but current introduces a dark
  horizontal backdrop smear. Target frost stays evenly luminous. Current also
  uses a filled grid glyph where the reference uses four outlined cells.
- Bottom-bar variants remain distinct requirements: the tab-swipe reference
  has a detached search circle, while `bottom-bar-form` integrates Search into
  the five-item surface. One variant cannot stand in for both.

Do not respond to these differences by increasing global magnification,
bevel, or chromatic gain. The paired images show that those parameters amplify
the wrong topology. The next shader work must use the configurable X-Z/Y-Z
profile to produce the target's tall recessed face and asymmetric returning
meniscus, then calibrate per-component aperture and frost against aligned raw
pixels.

Checkpoint gates:

- workspace `cargo test > 1.tmp`: pass, zero warnings;
- workspace `cargo clippy > 2.tmp`: pass, zero warnings;
- Android `:app:assembleRelease`: pass after moving Java source/target to 17,
  zero warnings;
- desktop-demo wasm build: pass;
- optimized desktop release build: pass, zero warnings;
- full real-X11 robot suite on Intel UHD 730: 128/128 pass;
- local NVIDIA/X11 smoke set: 7/7 pass, including liquid motion, menu, loupe,
  vertical selection grab, tab navigation, fused viewport, and external drag.

The verified release is installed at `target/release/desktop-app` and runs as
the user unit `cranpose-desktop-demo.service` with normal nice level 0. The
Liquid UI opens directly on the physical-profile playground.

## 2026-07-15 wcKSRD r84 visual checkpoint

The configurable physical-profile implementation and its playground were
removed. `example/shaders.txt` contains the single retained wcKSRD reference,
and every liquid component uses the shared wcKSRD runtime shader. Shader
source-text assertions were also removed: WGSL compilation validates the
program, while rendered target/current images decide visual correctness.

The currently installed desktop release is the visually inspected `r84`
checkpoint, SHA-256
`770dcdff9352bc72d8ac4d696fa768e59d8545b88dc8b16fa3fe1f27dfb3aaa8`.
The matching service process and `target/release/desktop-app` hashes were
verified equal after restart.

Fresh same-bitmap evidence:

- `/tmp/toggle-target052-current-r84.png`: target `f_052` left, current right;
- `/tmp/preview-r82-r84.png`: rejected broad-reflection shader left, corrected
  shared shader right over the same orange/purple scene;
- `/tmp/cranpose-liquid-r84-top.png`: complete desktop visual harness frame.

The rejected inner-reflection implementation mixed an opposite-wall backdrop
sample through a broad long-edge mask. It produced opaque horizontal bands on
arbitrary capsules, visible as the reported stadium shape. An incidence-only
Fresnel version (`r86`) still exposed the same incorrect screen-space source
mapping and was rejected before installation. The retained shader limits the
opposite-wall sample to the narrow meniscus path; the global stadium artifact
is absent.

The toggle is improved but not accepted. Its physical lower extent and cyan
return align substantially better with the target, while the left return is
still too thick and its target's broad neutral shoulder remains under-modeled.
Changing wcKSRD depth from `0.34` to the source shader's approximate `0.60`
ratio enlarged only the end-cap fold and was rejected. The missing shoulder
must come from a correct dome refraction/normal path, not another spatial
opacity mask or full opposite-wall mirror.

Four new native iPhone recordings under
`example/iphone17_records/on_white/` supersede inferred motion/material reads.
Their exhaustive per-frame extraction and canonical state grids are the next
visual authority for touch-up, text-handle/loupe, and bottom-bar behavior.

## 2026-07-15 breadth-first liquid checkpoint

This checkpoint was reviewed component-by-component against target and current
renders placed in the same bitmap. The inspected evidence is:

- `/tmp/toggle-transition-target-current-v2.png` — ten phase-aligned toggle
  frames, target row above current row;
- `/tmp/tabbar-target-current-v2.png` — aligned `tab-swipe/f_055` target above
  the direct-drag render;
- `/tmp/buttons-target-current-v4-overview.png` — the native touched-up action
  sequence beside the held grouped action;
- `/tmp/menu-target-current-crop.png` — settled target and current menu at a
  common physical scale;
- `/tmp/loupe-target-current-steady.png` — native on-white text handle/loupe
  beside the current steady drag state;
- `/tmp/segmented-shape-target-current.png` — target raised capsule above the
  current direct-follow segmented lens for shape-law inspection.

Implemented architecture and behavior:

1. Meniscus transmission absorption is an independent `Glass` material
   coordinate (uniform 100). Lowering it no longer suppresses reflection or
   spectral return. The toggle uses this channel to keep its transmitted track
   crisp without the broad stadium wall.
2. Toggle release keeps the optical body raised through its physical flight,
   then lowers the same material continuously before the white thumb returns.
   The fixed track reaches green at the target cadence and never moves with the
   thumb.
3. Tab flight uses one direct pointer coordinate, an isotropic depth projection
   separated from reciprocal fluid strain, and a target-aligned base footprint.
   The aligned lens is now within a small contour delta of `tab-swipe/f_055`.
4. Grouped icon actions grow toward the viewer to 1.45x, close into a real
   shared neck, retain independently owned base tints, and raise tint chroma
   through `GlassDynamics::saturation_boost`. The active foreground is absorbed
   into the raised material instead of being double-rendered.
5. Segmented depth lift and horizontal growth are separate. The selected body
   stays one cell wide, rises vertically, and receives only 18% of the shared
   motion strain, removing the compounded worm geometry.
6. Menu growth leaves the source phase immediately, grows height before card
   width, keeps content coherent during close, and uses a stronger independent
   rim/shadow path. Its single claimed gesture still opens, highlights while
   held, and fires on the same release.

Fresh robot results on the same tree: `robot_liquid_visual`,
`robot_liquid_motion_contract`, `robot_liquid_bubble_physics`,
`robot_adaptive_frost`, `robot_text_loupe`, `robot_drag_selection`,
`robot_selection_vertical_grab`, and `robot_menu_slide` all pass. The text
loupe remains slightly flatter than the native steady dome and the toggle's
left meniscus remains less sharply defined than the target. Those are visual
deltas for the next breadth pass, not reasons to reintroduce the rejected
profile playground or broaden screen-space reflection masks.

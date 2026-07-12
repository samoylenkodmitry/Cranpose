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

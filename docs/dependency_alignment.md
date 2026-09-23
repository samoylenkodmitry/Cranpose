# Dependency Alignment

How the workspace keeps one version of each dependency family: what the budget
gate enforces, which splits are upstream debt and what clears each one, and the
dependency-ownership decisions that must hold when versions change.

## Current Budget

`cargo xtask dependency-budget` resolves the duplicate-version graph for every
shipped target at once -- the three desktop triples, all four Android ABIs, iOS
device and simulator, and wasm (`SHIPPED_TARGETS` in `xtask/src/main.rs`) -- so
the verdict is identical on every host. Without the pins `cargo tree` filters by
host platform, and a budget that is green on a Linux CI runner can be red on
macOS. Architecture matters as much as OS here, because families such as
`windows_x86_64_msvc` are architecture-specific. Use
`cargo xtask dependency-budget --explain` to print the duplicate root
versions, their direct owners, and the recorded debt for each checked scope.
The same gate also rejects `cranpose/renderer-pixels` if it pulls the external
`pixels` crate or WGPU renderer packages. Repeated roots at the same package
version stay diagnostic-only.

Every duplicate-version family must either be collapsed or recorded as
upstream debt in `xtask/src/main.rs` (`WORKSPACE_DUPLICATE_DEBT`,
`ALL_FEATURES_EXTRA_DUPLICATE_DEBT`). The gate fails on any unrecorded family
and on any recorded family whose split no longer exists, so the table shrinks
the moment the upstream event lands. Recording is only for splits this
workspace cannot collapse: a split that can be fixed here -- by aligning a
version, dropping a dependency, or patching a crate to an upstream rev in
`[patch.crates-io]` -- must be fixed, not recorded.

The recorded debt is cross-platform dependency skew that every pinning crate
carries at its latest published release, so no `cargo upgrade` collapses it:

- `hashbrown`: `gpu-allocator 0.28.0`, which `wgpu-hal 30` uses for DX12 and
  Vulkan memory, holds `^0.16` in its latest release and on its upstream main,
  while `wgpu 30`, AccessKit and `indexmap 2.14` are on `^0.17`. It clears
  with the next `gpu-allocator` release.
- `objc2`, `objc2-app-kit`, `objc2-foundation`: this workspace is on
  `winit 0.31.0-beta.3`, whose `winit-appkit` is already on `objc2 0.6`, while
  `accesskit_macos 0.27` holds `objc2 0.5`. AccessKit is
  holding the bump (AccessKit/accesskit#616) precisely so that projects with
  both winit and AccessKit do not carry two objc2 stacks, and will merge it
  when winit 0.31 ships stable. The split is the cost of riding the winit
  beta and it clears when winit 0.31 is released; do not paper over it by
  patching the whole AccessKit stack to an unmerged fork branch, which would
  put the Windows, Unix, and iOS adapters on unreleased code to fix a macOS
  version split.
- `thiserror`, `thiserror-impl`, `jni-sys`: `ndk 0.9.0` and `ndk-sys 0.6.0`
  pin `thiserror ^1` and `jni-sys ^0.3` while the workspace is on
  `thiserror 2` and `jni 0.22` is on `jni-sys ^0.4`.
- `windows-sys`, `windows-targets`, `windows_x86_64_msvc`: `tempfile 3.27`
  and `rustls-platform-verifier 0.7` pin `^0.52`, and `arboard 3.6.1`,
  `dirs-sys 0.5` and `socket2 0.6` pin `^0.60`, while `winit-win32
  0.31.0-beta.3` and `tokio` are on `0.61`.
- `syn`: `serde_derive`, `thiserror-impl 2`, `bytemuck_derive` and
  `wasm-bindgen-macro` moved to `syn 3`, while `async-recursion`, `jni-macros`,
  `num_enum_derive` and `zerocopy-derive` are still on `syn 2` at their latest
  releases.
- `base64`: `reqwest 0.13.5` moved to `^0.23` while `hyper-util 0.1.20` is on
  `^0.22`.
- `miniz_oxide`: `flate2 1.1.10` moved to `^0.9` while `png 0.18.1` is on
  `^0.8`.
- `env_filter` (all-features only): `android_logger 0.15.1` pins `^0.1` while
  `env_logger 0.11` is past `1.0`.

## Evidence

`foldhash` and `hashbrown` are owned by the WGPU stack and adjacent tooling:

- `foldhash 0.2` and `hashbrown 0.17` come through `naga`, `wgpu`, `wgpu-core`,
  `wgpu-hal`, `indexmap 2.14`, AccessKit and proc-macro tooling.
- `hashbrown 0.16` comes only through `gpu-allocator -> wgpu-hal`.

`wgpu 30` no longer depends on `gpu-descriptor`, so the workspace carries no
`[patch.crates-io]` entry.

`rustc-hash` is no longer an active duplicate-version budget root. Stale lockfile package entries are not counted by the dependency-budget gate.

Inverse trees (`cargo tree -i <package>`) show that this slice was not a single
direct dependency problem:

- WGPU internals, `indexmap 2.14`, AccessKit and proc-macro tooling share `hashbrown 0.17` and `foldhash 0.2`.
- Cranpose-owned crates no longer depend on `rustc-hash` or `ahash` directly; core collection aliases use `foldhash 0.2`, the hasher `hashbrown` and `winit-wayland` already bring.

`tiny-skia` and `tiny-skia-path` are aligned in normal workspace and
all-features builds:

- `tiny-skia 0.12` comes from `sctk-adwaita -> winit-wayland -> winit` and
  Cranpose-owned rasterization code.

The all-features-only duplicate-version additions have been removed. `serde`
and `serde_core` may still appear as repeated roots at the same package version;
those are diagnostic-only and do not represent duplicate semver roots.

## Decisions

Local direct dependency ownership:

- Keep Cranpose-owned collection aliases on `foldhash`. Every key they hold is an internal identifier (`NodeId`, `SlotId`, `TypeId`, anchors), never untrusted input, so hash-flooding resistance buys nothing, and `foldhash` is faster for these keys: under the exclusive host lock on samarch-1 (x86-64 baseline, where `ahash` takes its non-AES path as it does on Android and wasm), the `cranpose-ui` pipeline benchmark ran 4.4% faster end to end, composition 3.6-6.7% and recursive measure 6.4%, and swapping `ahash` back regressed the same benchmarks by 5-8%; layout and render, which do not touch these maps, did not move. It is already in the graph through `hashbrown` and `winit-wayland`, and it seeds itself without `getrandom`, so a wasm application no longer needs a `getrandom` backend feature for Cranpose.
- Keep `tiny-skia 0.12.0` for Cranpose render/common code. The software text
  rasterizer compiles against the same tiny-skia line as the current
  `sctk-adwaita -> winit` platform stack, with PNG decoding disabled because it
  only rasterizes paths.
- Keep software text font, measurement, layout, cursor mapping, and rasterization
  ownership in `cranpose-render-common`. `cranpose-render-pixels` now depends on
  the common text backend instead of owning `ab_glyph` directly; WGPU text
  rendering uses the same in-tree software backend.
- Keep the in-tree pixels renderer independent from the external `pixels` crate.
  `cranpose/renderer-pixels` now enables only `cranpose-render-pixels`; it does
  not pull `pixels -> wgpu` default features into all-features builds.
- Keep SVG rasterization local to `cranpose-ui` and backed by the workspace-aligned `tiny-skia 0.12.0` line. The public `SvgPainter` behavior remains behind the `svg` feature, while the implementation no longer pulls `resvg/usvg/roxmltree` or a second tiny-skia line into all-features.
- Keep native system-theme detection in `cranpose-services` dependency-free. It uses platform settings commands when `system-theme` is enabled and falls back to `Light`; this preserves the service API without pulling portal async stacks into all-features.
- Keep Vulkan enabled for native WGPU. Disabling Vulkan clears the duplicate
  graph in a dependency probe, but `robot_renderer_micro_contract` fails on the
  Linux/X11 test host with no compatible WGPU adapter because the GL path cannot
  present to the provided surface.
- Keep WGPU backend features target-specific. Linux and Android use GLES/Vulkan,
  Windows uses DX12, macOS uses Metal, and wasm uses WebGPU/WebGL. This removes
  the broad desktop backend feature bundle without changing the Linux renderer
  contract that still requires Vulkan.

Future version-change candidates:

- Check whether a newer `gpu-allocator` release moves to `hashbrown 0.17`.
- Leave the SVG path on the in-crate parser/rasterizer unless full SVG coverage is explicitly required; adding a general-purpose SVG library must not reintroduce `roxmltree` or a second tiny-skia line into the all-features graph.

Each candidate changes library versions or library selection, so it must run through the dependency-change rule: read the duplicate roots from `cargo xtask dependency-budget --explain`, inspect inverse trees (`cargo tree -i <package> --target <triple>`) for the affected packages, apply one focused change, then run the full validation gate.

## Completed Alignments

These slices are closed. Each statement describes the tree as it stands, and
the dependency budget keeps it that way.

- **WGPU stack.** `wgpu 30` shares `hashbrown 0.17` and `foldhash 0.2` with
  AccessKit and `indexmap`, with no crates.io patch; only `gpu-allocator`
  remains on `hashbrown 0.16`, recorded above.
- **Renderer cache ownership.** Renderer call sites use
  `cranpose_render_common::bounded_lru_cache::BoundedLruCache`, and neither
  `cranpose-render/wgpu` nor `cranpose-render/pixels` carries a direct `lru`
  dependency.
- **Software text ownership.** `cranpose-render-common::software_text_raster`
  owns font, metrics, layout, cursor/offset mapping, and rasterization for both
  renderers, so `ab_glyph` enters through that crate rather than per renderer.
- **Renderer-pixels facade.** `cranpose/renderer-pixels` enables only
  `cranpose-render-pixels`. The budget re-checks this by resolving
  `cargo tree -p cranpose --no-default-features --features renderer-pixels` and
  rejecting `pixels`, `wgpu`, `wgpu-core`, `wgpu-hal`, and `naga`.
- **indexmap.** `2.14` shares `hashbrown 0.17` with `wgpu 30` and AccessKit.
- **Desktop platform.** `cranpose-render-common` and optional SVG both use the
  `tiny-skia 0.12.0` line that `sctk-adwaita -> winit-wayland -> winit` uses.
- **Optional all-features.** `cranpose-services` does not depend on
  `dark-light`, and `cranpose-ui/svg` does not depend on `resvg`/`usvg`, so
  `async-channel`, `event-listener`, `getrandom`, `roxmltree`, and the second
  tiny-skia line stay out of the all-features graph.
- **Native HTTP is opt-in.** `cranpose-services` defaults to no features, so
  `reqwest`, `rustls`, `hyper`, `aws-lc`, and `webpki` enter only under
  `http-native`. `isolated-demo`, the crate the size budget measures, pulls
  none of them. The `desktop-app` demo does enable `desktop-http` in its own
  defaults, so its graph does carry that stack -- that is the demo's choice,
  not the framework's. Ordered concurrency stays native through the small
  `pollster` executor either way.

## Validation Gate

After an approved dependency alignment change, run the recipes CI runs -- the
justfile is the single definition of every gate, so do not spell the commands
out here or in a workflow:

- `just ci` -- `fmt-check`, `typos`, `versions`, `test`, `clippy`, `doc`, and
  `budgets` (which is `featureless`, `dep-budget`, and `size-budget`).
- `just ci-full` -- adds `clippy-wasm`, `web`, `android`, and `robot` when the
  change can reach the wasm, Android, or presented-renderer paths.

While iterating, `cargo xtask dependency-budget --explain` prints the duplicate
roots, their direct owners, and the recorded debt. Do not read a verdict off a
raw `cargo tree --duplicates`: it resolves for the host platform only, so it
hides cross-platform skew such as the macOS-only objc2 split.

Because the budget resolves every shipped target, its verdict no longer depends
on which machine runs it; a dependency change that is clean on Linux is clean on
macOS and Windows too.

Large diagnostic files must stay out of tmpfs-backed directories. Use `CRANPOSE_ROBOT_OUTPUT_DIR`, a non-tmpfs `TMPDIR`, or repo-local small logs.

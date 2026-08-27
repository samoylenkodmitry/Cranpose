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
version, dropping a dependency, or patching a crate to an upstream rev the way
`gpu-descriptor` is patched -- must be fixed, not recorded.

The recorded debt is cross-platform dependency skew that every pinning crate
carries at its latest published release, so no `cargo upgrade` collapses it:

- `objc2`, `objc2-app-kit`, `objc2-foundation`: this workspace is on
  `winit 0.31.0-beta.2`, whose `winit-appkit` is already on `objc2 0.6`, while
  `accesskit_macos 0.26.3` deliberately holds `objc2 0.5`. AccessKit is
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
- `windows-sys`, `windows-targets`, `windows_x86_64_msvc`:
  `winit-win32 0.31.0-beta.2` pins `^0.59` and `arboard 3.6.1` pins `<0.61`
  while the rest of the graph is on `0.61`.
- `env_filter` (all-features only): `android_logger 0.15.1` pins `^0.1` while
  `env_logger 0.11` is past `1.0`.

## Evidence

`foldhash` and `hashbrown` were owned by the WGPU stack and adjacent tooling:

- `foldhash 0.1` and `hashbrown 0.15` come through `gpu-descriptor -> wgpu-hal`.
- `foldhash 0.2` and `hashbrown 0.16` come through `gpu-allocator`, `indexmap`, `naga`, `wgpu`, `wgpu-core`, `wgpu-hal`, and proc-macro tooling.

The WGPU-stack split is now aligned by patching `gpu-descriptor 0.3.2` to the upstream commit `79804e422186805f1ff5ab3d8310c07c145a6731`, which updates its `hashbrown` dependency to `0.16`. Upstream `master` already moved on to `hashbrown 0.17`, so this exact commit is intentionally pinned until an upstream release aligns with the rest of WGPU 29.

`rustc-hash` is no longer an active duplicate-version budget root. Stale lockfile package entries are not counted by the dependency-budget gate.

Inverse trees (`cargo tree -i <package>`) show that this slice was not a single
direct dependency problem:

- Before the alignment patch, `hashbrown 0.15` and `foldhash 0.1` entered through `gpu-descriptor -> wgpu-hal`.
- After the alignment patch, `gpu-descriptor`, WGPU internals, `gpu-allocator`, `indexmap 2.13`, and proc-macro tooling share `hashbrown 0.16` and `foldhash 0.2`.
- Cranpose-owned crates no longer depend on `rustc-hash` directly; core collection aliases use the existing `ahash` dependency.

`tiny-skia` and `tiny-skia-path` are aligned in normal workspace and
all-features builds:

- `tiny-skia 0.11` comes from `sctk-adwaita -> winit-wayland -> winit` and
  Cranpose-owned rasterization code.

The all-features-only duplicate-version additions have been removed. `serde`
and `serde_core` may still appear as repeated roots at the same package version;
those are diagnostic-only and do not represent duplicate semver roots.

## Decisions

Local direct dependency ownership:

- Keep Cranpose-owned collection aliases on `ahash`. Switching local code to WGPU-internal hashing does not remove the WGPU-owned `hashbrown`/`foldhash` split.
- Keep `tiny-skia 0.11.4` for Cranpose render/common code. The software text
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
- Keep SVG rasterization local to `cranpose-ui` and backed by the workspace-aligned `tiny-skia 0.11.4` line. The public `SvgPainter` behavior remains behind the `svg` feature, while the implementation no longer pulls `resvg/usvg/roxmltree` or a second tiny-skia line into all-features.
- Keep native system-theme detection in `cranpose-services` dependency-free. It uses platform settings commands when `system-theme` is enabled and falls back to `Light`; this preserves the service API without pulling portal async stacks into all-features.
- Keep Vulkan enabled for native WGPU. Disabling Vulkan clears the duplicate
  graph in a dependency probe, but `robot_renderer_micro_contract` fails on the
  Linux/X11 test host with no compatible WGPU adapter because the GL path cannot
  present to the provided surface.
- Keep WGPU backend features target-specific. Linux and Android use GLES/Vulkan,
  Windows uses DX12, macOS uses Metal, and wasm uses WebGPU/WebGL. This removes
  the broad desktop backend feature bundle without changing the Linux renderer
  contract that still requires Vulkan.
- Keep the `gpu-descriptor` crates.io patch pinned to upstream commit
  `79804e422186805f1ff5ab3d8310c07c145a6731`. This is the upstream
  `hashbrown 0.16` alignment commit; newer upstream `master` currently uses
  `hashbrown 0.17` and would reintroduce a duplicate-version family.

Future version-change candidates:

- Check whether a newer `winit`/`sctk-adwaita` stack aligns `tiny-skia` with the renderer stack.
- Check whether a newer WGPU line or `gpu-descriptor` crates.io release aligns `hashbrown` or `foldhash` without the patch.
- Leave `zip` unchanged for this slice. With `indexmap 2.13.0`, it shares the
  current `hashbrown 0.16` root instead of owning a separate `hashbrown 0.17`
  root.
- Leave the SVG path on the in-crate parser/rasterizer unless full SVG coverage is explicitly required; adding a general-purpose SVG library must not reintroduce `roxmltree` or a second tiny-skia line into the all-features graph.

Each candidate changes library versions or library selection, so it must run through the dependency-change rule: read the duplicate roots from `cargo xtask dependency-budget --explain`, inspect inverse trees (`cargo tree -i <package> --target <triple>`) for the affected packages, apply one focused change, then run the full validation gate.

## Completed Alignments

These slices are closed. Each statement describes the tree as it stands, and
the dependency budget keeps it that way.

- **WGPU stack.** `foldhash` and `hashbrown` are single-family. The
  `gpu-descriptor` patch above is what holds this; `wgpu 29.0.3` and
  `gpu-descriptor 0.3.2` remain the current crates.io releases, so there is no
  published upgrade that would replace the patch.
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
- **indexmap.** Pinned to `2.13.0`, which satisfies `naga` and `toml_edit`
  while sharing `hashbrown 0.16.1` instead of owning a `hashbrown 0.17` root.
- **Desktop platform.** `cranpose-render-common` and optional SVG both use the
  `tiny-skia 0.11.4` line that `sctk-adwaita -> winit-wayland -> winit` uses.
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

# Binary size

The application release profile and feature set control binary size. Measure
size with the same target, toolchain and feature set as the release. The
[`justfile`](../justfile) and [`xtask`](../xtask/src/main.rs) define the budgets.

## Release profile

Place the profile in the application workspace root:

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
panic = "abort"
```

The repository also provides `release-small` for size measurements. Compare
runtime performance on the target device before a size-focused profile change.

## Features

| Choice | Effect |
| --- | --- |
| `desktop` | Selects both X11 and Wayland support |
| `desktop-x11` or `desktop-wayland` | Selects one Linux display backend |
| `embedded-default-font` | Includes the default font; enabled by the facade's defaults |
| App font methods on `AppLauncher` | Select the app-font launcher path |
| `renderer-wgpu-gles` | Adds the desktop GLES fallback |
| `renderer-pixels` | Selects the software renderer |
| `cranpose-services/http-native` | Adds the native HTTP stack |
| `release-log-info` | Sets the release log ceiling to Info through the `log` crate |

Choose one release log ceiling across the application dependency graph. The
[facade manifest](../crates/cranpose/Cargo.toml) defines platform feature groups;
Android and web select their own graphics backends.

## Repository checks

```sh
just size-budget
just dep-budget
just android-size-report
```

`size-budget` measures an isolated consumer copy with local framework patches
and a 16 MiB limit. The original isolated consumer keeps its published registry
dependencies. `dep-budget` checks duplicate dependencies across shipped targets.
`android-size-report` reports the demo's ARM64 library size by section and crate.
The nightly Android size job retains the report in its run summary.

`cargo xtask dist-min --help` lists the advanced desktop size-build options.
The command uses the pinned nightly, rebuilds the standard library and removes
panic location and Debug output. Naga's DX12 shader writer requires Debug output
for numeric literals; use a profile with Debug output for Windows GPU releases.
See the [Windows shader report](windows-webgl-startup.md).

## Android release size

The [Cranpose Gradle plugin](../crates/cranpose/android) applies final-link size
options through `cargo ndk` and `cargo rustc`:

- `--icf=all` folds identical functions.
- `--pack-dyn-relocs=android` packs relocations when the app's `minSdk` is at
  least 23.
- `--no-eh-frame-hdr` and the release linker script remove unwind sections.
  Panic hooks retain the panic message and location; native crash stack detail
  depends on the retained unwind data.

The application can select size optimization for shader setup crates:

```toml
[profile.release.package.naga]
opt-level = "s"

[profile.release.package.codespan-reporting]
opt-level = "z"
```

These crates run during shader setup and error reports. Verify cold-start time
alongside size after each profile change.

## Recorded measurements

The following figures describe separate compose-vs-cranpose benchmark builds.
Each row has its own baseline; the deltas are independent.

| Date | Change | ARM64 result |
| --- | --- | --- |
| 2026-09-27 | Naga and diagnostic crate size profiles | 9.28 MB → 9.09 MB; Mate 20 X cold-start medians about 185 ms → 178 ms, eight runs per build |
| 2026-10-03 | Identical-code fold and packed relocations | 10,646 KB → 9,913 KB |
| 2026-10-03 | Unwind-section removal | 9,922 KB → 8,974 KB; ARMv7 7,224 KB → 7,111 KB |
| 2026-10-03 | Release log ceiling at Info | 8,874 KB → 8,724 KB |

The July 2026 `renderer-vulkan-slim` experiment exists in Git history. Current
renderer choices are WGPU and pixels. Earlier sub-1-MiB projections describe
an experimental backend; release budgets use the current renderer graph.

# Dependency alignment

The dependency budget checks duplicate package versions across the shipped
targets: all four Android ABIs, iOS device and simulator, desktop
and wasm. Its source of truth is `SHIPPED_TARGETS`,
`WORKSPACE_DUPLICATE_DEBT` and `ALL_FEATURES_EXTRA_DUPLICATE_DEBT` in
[`xtask/src/main.rs`](../xtask/src/main.rs).

```sh
cargo xtask dependency-budget --explain
```

The report lists each duplicate family, direct owners and the recorded reason.
The gate rejects unrecorded splits and obsolete debt entries. Repeated roots
at the same package version are diagnostic only. A raw
`cargo tree --duplicates` uses the host platform; the gate uses all shipped targets.

## Recorded ownership

The following groups explain the current debt; use the generated report for
exact versions.

| Dependency families | Dependency owner |
| --- | --- |
| `bitflags`, `heck`, `proc-macro-crate`, `serde_spanned`, `toml`, `toml_datetime`, `toml_edit`, `winnow` | Wry's GTK 3/WebKit stack and the workspace's newer tooling |
| `hashbrown` | `gpu-allocator` and the rest of the WGPU/AccessKit/indexmap graph |
| `jni-sys`, `thiserror`, `thiserror-impl` | Android NDK bindings and current workspace JNI/error dependencies |
| `miniz_oxide` | `flate2` and `png` |
| `objc2`, `objc2-app-kit`, `objc2-foundation` | AccessKit's macOS adapter and winit's AppKit backend |
| `syn` | Procedural macro dependencies on different major versions |
| `windows-sys` | Platform verifier, temporary files, clipboard and window/runtime dependencies |
| `env_filter` (all features) | Android and desktop loggers |
| `rustc-hash` | Fluent localization and the shader stack; UniFFI's all-features bindings also enable Askama |

Remove locally resolvable splits. Reserve the debt list for upstream constraints.
Inspect the current dependency requirements before each upgrade.

## Dependency boundaries

- Cranpose uses the [published WGPU forks](../forks/README.md) as path
  dependencies in the workspace. Library names remain `wgpu`, `wgpu_core` and
  `wgpu_hal`; package names have the `cranpose-` prefix. `wgpu-types`, Naga and
  `wgpu-naga-bridge` remain upstream dependencies.
- Internal collection aliases use `foldhash`. Choose a map based on key type
  and input trust.
- Shared software text measurement and rasterization live in
  `cranpose-render-common`; both renderers share the text backend.
- `cranpose/renderer-pixels` uses the in-tree software renderer. The budget
  rejects the external `pixels` crate, WGPU, the WGPU forks and Naga in this graph.
- Optional SVG rasterization lives in `cranpose-ui` and shares the `tiny-skia`
  line used by the platform stack. Account for feature and duplicate-version
  costs before a general SVG library change.
- WGPU backend features are target-specific. Inspect the renderer and facade
  manifests before a backend change, then verify frame presentation on the target.
- `cranpose-services` defaults to an empty feature set. Native HTTP requires
  `http-native`; the desktop demo selects the HTTP stack through `desktop-http`.
  The isolated size-budget consumer uses the smaller service feature set.

## Validation

After a dependency change, run `just ci`; use `just ci-full` for the platform
and presented-renderer gates. The exact gate lists live in the
[`justfile`](../justfile), with wasm Clippy in `ci` and Android, iOS,
watchOS, web, Vulkan validation and robot checks in the appropriate recipes.

During diagnosis, inspect inverse trees with
`cargo tree -i <package> --target <triple>`. Recheck the cross-target budget
after the change. Published consumers resolve their own dependency graph;
validate the published graph before each release.

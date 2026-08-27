# Contributing to Cranpose

Cranpose is pre-alpha. The API changes without deprecation cycles, and a change
that improves the architecture is preferred over one that preserves an existing
shape. Read [`AGENTS.md`](AGENTS.md) before submitting code -- it holds the
engineering standards this repository is strict about.

## Getting started

```bash
git clone https://github.com/samoylenkodmitry/cranpose.git
cd cranpose
just toolchains   # installs both pinned toolchains
just run          # the workspace demo
```

### Required tools

| Tool | Why |
| --- | --- |
| [`just`](https://github.com/casey/just) | Every gate lives in the [`justfile`](justfile); CI runs the same recipes. |
| Rust | Pinned by [`rust-toolchain.toml`](rust-toolchain.toml); rustup installs it on first use. |
| Rust nightly | Pinned by [`rust-toolchain-nightly.toml`](rust-toolchain-nightly.toml). Needed only by `just fmt` and `cargo xtask dist-min`. |
| Python 3 | The version and coverage check scripts under [`scripts/`](scripts). |

Nothing a downstream user of the `cranpose` crates builds requires nightly, and
that is a constraint worth keeping.

Platform work needs more: an Android SDK plus NDK 27 for `just android`, Xcode
for `just ios-sim`, `wasm-pack` and `binaryen` for `just web`, and an X11 stack
for the robot suite.

## Commands

`just` on its own lists every recipe. The ones you will use most:

```bash
just ci        # what a pull request is gated on -- run this before pushing
just test      # workspace tests
just clippy    # lint, warnings denied
just fmt       # format (runs on the pinned nightly)
just doc       # docs, rustdoc warnings denied
just robot     # the end-to-end robot suite
```

CI invokes these same recipes rather than spelling commands inline, so a gate
cannot mean one thing locally and another thing in a pull request. When you
change a gate, change it in the `justfile`.

## Architecture

Rust crates live in [`crates/`](crates), sorted here roughly by how central they
are:

| Crate | Purpose |
| --- | --- |
| `cranpose` | The facade users depend on. Re-exports the rest and runs an app with minimal boilerplate. |
| `cranpose-core` | The runtime: slot table, snapshot state, recomposition, effects. |
| `cranpose-ui` | UI primitives built on the core runtime. |
| `cranpose-foundation` | Modifiers, input handling, and the foundation elements. |
| `cranpose-ui-layout` | Layout contracts and policies. |
| `cranpose-ui-graphics` | Pure math and data for drawing and units. |
| `cranpose-macros` | The `#[composable]` procedural macro. |
| `cranpose-animation` | The animation system. |
| `cranpose-liquid` | Liquid UI: the first-party glass component library (iOS-26-style materials, spring motion). |
| `cranpose-app-shell` | Application orchestration shell. |
| `cranpose-render/common` | Rendering contracts shared by every backend. |
| `cranpose-render/wgpu` | The GPU renderer, used on every platform. |
| `cranpose-render/pixels` | Software renderer backend. |
| `cranpose-platform/desktop-winit` | Desktop platform adapter (X11, Wayland, macOS, Windows). |
| `cranpose-platform/android` | Android platform adapter. |
| `cranpose-platform/web` | Web platform adapter. |
| `cranpose-runtime-std` | Runtime services backed by `std`. |
| `cranpose-services` | Multiplatform system services: HTTP, URI, OS integrations. |
| `cranpose-audio` | Real-time audio (AAudio on Android and Wear OS, cpal on desktop). |
| `cranpose-media` | Desktop media playback (symphonia decoders, cpal output). |
| `cranpose-storekit` | StoreKit 2 in-app purchases for iOS and macOS. |
| `cranpose-assets` | Asset loading and management. |
| `cranpose-testing` | Testing utilities and the headless harness. |

Applications live in [`apps/`](apps):

- `desktop-demo` -- the comprehensive demo (package `desktop-app`). Also holds
  the iOS entry point and the `robot_*` end-to-end runners.
- `isolated-demo` -- the starter template. It is **its own workspace** and
  depends only on published crates, which is what makes it the canary proving a
  release is actually consumable. Copy this rather than starting from scratch.
  Its `Cargo.lock` is tracked and has to keep resolving every Cranpose crate
  from crates.io -- `just versions` fails if one turns into a path dependency.
  That is why the size budget measures a staged copy of the package under
  `target/patched-packages/`: it can patch in the local crates there without
  rewriting the lockfile that proves the release is consumable.
- `android-demo`, `ios-demo` -- the platform entry points and their build
  scripts.

[`xtask/`](xtask) holds the budget tooling: `binary-size`, `dependency-budget`,
`dist-min`, `bundle-macos`. [`scripts/`](scripts) holds the verification and
visual-comparison helpers. Design notes live in [`docs/`](docs).

## Submitting a change

- `just ci` passes. All of it -- a failing gate is never "pre-existing".
- New public API carries a `///` doc comment with an example. Internal code does
  not: good names beat narration, and comment bloat is worse than no comment.
- A bug fix starts with a failing test that catches the bug, so the repository
  cannot regress to it.
- No `unsafe` outside a platform FFI boundary that opts in explicitly.
- No half-migrated states, no deprecation shims, no "legacy" paths. Change the
  existing code instead.

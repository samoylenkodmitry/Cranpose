# Cranpose — what is not done

The framework-ownership plan is implemented; what it delivered is in the git
history, not here. This file is only the remainder: gaps that are real, corner
cases a caller has to know about, and limits that are correct but surprising.

An item leaves this file when the behaviour changes, not when someone decides it
is acceptable.

## Gaps in the framework

### Public API test coverage: 364 of 3400 functions

`python3 scripts/public_api_test_coverage.py` reports **3036/3400 (89.3%)**.
Treat the 364 as a map of where a change is unguarded, not as a backlog of 364
tests to write — a test written to raise the number tests the implementation it
was written against. Tests here are added the other way round: each pins a
defect found first, or covers a new module's own decisions, which is where a
path escaping its asset root, or a version sorting `0.1.10` before `0.1.9`,
would otherwise ship unnoticed.

The measure itself was wrong three times before it was trusted: it read every
file from its first `#[cfg(test)]` to the end as test code (138 files carry that
attribute near the top, handing 971KB of `render.rs` to the "test corpus"), it
matched names as substrings so `with_timeout` read as covered because
`exit_with_timeout` exists, and it excluded the robot suite. A proxy metric that
is quietly wrong sends work to the wrong places for as long as nobody reads it.

### Widget composition coverage is not universal

`cranpose-liquid` and `cranpose-ui` each run widgets through a real composition,
which is what catches a component reading a local nobody provided. Not every
exported widget is in those suites — **`LazyRow` has no composition test
anywhere**; others are covered only by the robot suite or by tests beside their
own module.

### `PointerEvent` carries no keyboard modifier state

`cranpose-foundation/src/nodes/input/types.rs` gives `PointerEvent` buttons,
position, scroll, zoom, source and timestamps — but no modifiers. The framework
already builds `cranpose_app_shell::Modifiers{shift,ctrl,alt,meta}` from winit
and attaches it to **wheel** events and to focused-widget `KeyEvent`s, never to
pointer events.

The consequence is not theoretical: an application wanting shift-click or
ctrl-click multi-select has to read the keyboard itself. CranAmp does, through
raw `x11rb`, which means the interaction **silently does nothing on macOS and
Windows** — `x11rb::connect` fails there and the code falls through to "no
modifiers held". The plumbing and the type both exist; they are just not joined.

### Effect keys are compared by hash, not by equality

`LaunchedEffect!` / `LaunchedEffectAsync!` / `DisposableEffect!` hash their keys
to a `u64` and compare that. Jetpack Compose's `LaunchedEffect(key1, block)` is
`remember(key1) { ... }` — exact structural equality. A hash collision therefore
makes Cranpose treat two distinct keys as unchanged and skip a relaunch or
dispose that Compose would always perform.

Fixing it means requiring `K: PartialEq + 'static` and storing the key itself,
which costs keys that are `Hash` but not `Eq`. That trade is the open question;
the divergence is not in doubt.

### A provided density does not reach measurement

`local_density()` scopes a grid to a subtree and composables read it, but the
layout pass does not: `LayoutNode` captures no density, so measurement reads
whatever the host installed on the shell. A `ProvideDensity` around a subtree
therefore changes what its composables compute and not what its children are
measured against.

Compose captures density onto the layout node at composition time, which is what
makes `MeasureScope` able to carry one into `MeasurePolicy.measure`. Here
`MeasurePolicy::measure` receives no scope at all, and `MeasureScope` — which
has exactly one implementor, the subcompose scope — still declares `density()`
and `font_scale()` with `1.0` defaults that would silently lie for the next one.

Closing it means deciding where the value is captured before threading anything:
give `LayoutNode` the grid it was composed with, source the scope from that, and
drop the defaults.

### The Android panic hook overwrites the application's

`crates/cranpose/src/android.rs` installs a panic hook unconditionally, and it
runs after the application's own launcher has already installed one — so an
application that wants a backtrace, a thread name or its own log tag never gets
it. The framework's hook logs file, line, column and message only. It needs to
either chain to a previously-installed hook or offer an extension point.

## Limits that are correct, and surprising

These are deliberate. They are here so nobody rediscovers them as bugs.

- **Desktop and iOS can discover an update but not install one.** App Store
  Review Guideline 3.3.2 forbids an iOS application replacing its own binary,
  and the framework owns no desktop installer. `AppUpdateCapabilities` splits
  `check` from `install` so these hosts answer `check: true, install: false`
  rather than registering an installer that can only fail. Without an HTTP
  client the backend reports `check: false` rather than claiming a request it
  cannot make.
- **`cranpose-services/http-native` is not forwarded through the `cranpose`
  umbrella.** Doing so would need one feature per target, which Cargo cannot
  express, so the opt-in stays where applications already write it.
- **The unused-API deletion rule stops at the Compose-shaped surface.**
  `rememberSaveable`, `ProvideLifecycle`, `rememberLifecycleState`,
  `DurableSaveEffect` and `interval` have no caller and are kept. What makes
  them API is that an application written against Compose expects them to
  exist, not that one of ours has reached for one yet.

## Release artifacts that cannot be installed

Found by installing releases onto real devices, not by reading the pipeline.

- **The iOS release IPA cannot go on a device.** `cranscan-*-ios.ipa` is ad-hoc
  signed (`flags=0x2(adhoc)`, `TeamIdentifier=not set`) with no embedded
  provisioning profile, so iOS refuses it. Installing requires re-signing
  locally against a development profile carrying the target device's UDID. A
  release should also publish a development-signed build, or the artifact should
  say plainly that it is App Store/TestFlight only.
- **CranOrbit publishes only an `.aab`.** An Android App Bundle cannot be
  installed with `adb`. The universal APK exists only inside the CI *artifact*
  `orbit-breaker-bundle-<n>`, which now expires after 14 days. A release should
  attach the APK beside the bundle.

## Duplication left in the applications

`percent_decode` is now one pair in `cranpose_services::content` —
`percent_decode` (strict, refuses what it cannot decode exactly) and
`percent_decode_lossy` (substitutes, for text that is only displayed). The two
behaviours were both already in use and both wanted, which is why five diverging
copies went unnoticed.

Three copies remain in CranScan (`app/src/services.rs`, `crates/core/src/qr.rs`)
and CranAmp (`src/sync/mod.rs`), plus CranAmp's two private `hex_value` helpers.
They can only be removed once those applications move to a release carrying the
shared pair.

## Robot suite corner cases

- **33 examples need an X11 session with `xdotool`** and are skipped everywhere
  else, so they are only ever exercised on Linux.
- **`robot_leetcodedaily_code_scroll_pixel_drift` needs Python Pillow** for its
  pixel comparison and is skipped on hosts without it.
- **`robot_shader_rect` and `robot_shader_backdrop_drag` need a display that
  can present a frame.** Both call `.with_headless(false)` because they verify
  real GPU presentation. They are skipped, with that reason printed, when
  `DISPLAY`/`WAYLAND_DISPLAY` is unset or `xset q` reports the X11 monitor
  asleep (Off/Standby/Suspend) — the condition that otherwise surfaces as an
  opaque "window ... refused N consecutive frames" failure. `xvfb-run`, which
  is what CI runs the suite under, has no DPMS extension, so this gate is a
  no-op there and the pair runs as before.

## Infrastructure

One Linux machine serves every `[self-hosted, Linux, cranpose-heavy]` job. Two
of those genuinely cannot move: the X11 robot suite needs a real X server, and
the binary-size budget is pinned against Linux codegen. The wasm job no longer
sits behind them. If Linux queueing keeps hurting, the remaining lever is a
second Linux runner, not more relabelling.

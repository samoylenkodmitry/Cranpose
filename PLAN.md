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

- **The iOS release IPA still needs re-signing, and now says so.**
  `cranscan-*-ios-unsigned.ipa` is ad-hoc signed with no embedded provisioning
  profile, so a device refuses it; installing means re-signing against a
  development profile carrying that device's UDID. The repository's only iOS
  secrets are an App Store *distribution* certificate and profile, so no CI job
  can produce a device-installable build. The artifact is named and documented
  for what it is rather than carrying a signing step that could only fail.
- **The published CranOrbit release carries only an `.aab`**, which `adb`
  cannot install. The release workflow attaches the universal APK beside the
  bundle, but no tag has been pushed since that landed, so `v1.3.1` is
  unchanged. Two separate faults kept a tag from producing one: the build ran
  on a hosted runner that exhausted its disk restoring a cargo cache holding a
  three-target `target/` directory, and it resolves a Gradle plugin that was
  published to `mavenLocal()` and nowhere else.

## Duplication left in the applications

`percent_decode` is now one pair in `cranpose_services::content` —
`percent_decode` (strict, refuses what it cannot decode exactly) and
`percent_decode_lossy` (substitutes, for text that is only displayed). The two
behaviours were both already in use and both wanted, which is why five diverging
copies went unnoticed.

Three copies remain in CranScan (`app/src/services.rs`, `crates/core/src/qr.rs`)
and CranAmp (`src/sync/mod.rs`), plus CranAmp's two private `hex_value` helpers.
They can only be removed once those applications move to a release carrying the
shared pair; the changes are written and pinned to it. CranAmp's two private
`hex_value` helpers are not part of this: they back an unrelated tab-safe hex
codec, not percent-decoding.

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

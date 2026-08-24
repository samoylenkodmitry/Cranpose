# Cranpose — what is not done

The framework-ownership plan is implemented; what it delivered is in the git
history, not here. This file is only the remainder: gaps that are real, corner
cases a caller has to know about, and limits that are correct but surprising.

An item leaves this file when the behaviour changes, not when someone decides it
is acceptable.

## Gaps in the framework

### Public API test coverage: 356 of 3408 functions

`python3 scripts/public_api_test_coverage.py` reports **3052/3408 (89.6%)**.
Treat the 356 as a map of where a change is unguarded, not as a backlog of 356
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

## CranOrbit on the watch

Found by installing releases on a Pixel Watch 3 and playing them, which is the
only way any of these surfaces. The radial menu that `9335cff` replaced with a
scrolling list is restored, and the blank screen on backing out of the pause
overlay is fixed in Cranpose `0.1.99` -- `SwipeToDismissBox` was holding its
content off screen after firing `on_dismiss`, right for a dismissed row whose
host removes it and wrong for a navigation gesture whose host stays composed.
Both are verified on the watch against `v1.3.3`. What is left:

### A level does not begin play when it is started

Reported from the watch: after the framework-ownership work, tapping to start a
level does not get the game playing.

Partly reproduced against `v1.3.3`, and the part that did not reproduce matters
as much as the part that did. Tapping `START` on the level intro **does** open
the arena, and it animates: five screenshots two seconds apart were all
different, so the render loop and some simulation are running. What is visible
in those frames is the ball still sitting on the paddle, unlaunched, with the
bricks untouched — a level that is loaded and idling rather than one that never
opened.

Whether the ball fails to launch, or is waiting for an input that is no longer
delivered, is **not** established. `robot_*` coverage does not reach this: the
arena is a real GPU surface and the launch is an input gesture.

What the next attempt needs to know, because it cost time here: **a dozing
watch freezes the picture and reads exactly like a frozen game.** Two
consecutive screenshots came back byte-identical after a tap and a drag, which
looked like input being ignored, and `dumpsys power` said `mWakefulness=Dozing`.
Check wakefulness before concluding anything from a still frame, and drive the
test faster than the display's idle timeout.

The discriminator to reach for first is whether the simulation is advancing at
all -- `AppState::needs_frames`/`simulation_runs` on a state driven headlessly
through `start_selected_level` -- because that separates "the ball is waiting
for input that no longer arrives" from "the session never started". A headless
test can settle it without a watch, and none exists.

### CranOrbit's list screens share one scroll position

Open Settings, scroll to the bottom, open Credits from the last row: Credits
opens already scrolled to its end. The scroll position is not per screen.

`WearListScreen` remembers exactly one list state, at
`app/src/ui/composed.rs:342`:

```rust
let list = rememberWearScalingListState(CentreAnchor {
    index: CENTRED_ITEM,
    offset: 0.0,
});
```

and `OrbitApp` calls `WearListScreen` from a single site, passing the screen as
an argument. One call site is one composition slot, so Settings, Credits,
Volume and Haptics all read and write the same remembered state, and switching
screens carries the offset across. `remember` is doing exactly what it
promises — the slot did not change, so the value does not — and the identity
the state should hang off, which screen is being shown, is never stated.

The framework already has what states it: `cranpose_core::with_key`, Compose's
`key(…) { }`. Keying the list screen by its screen gives each one its own slot
and its own scroll position, and drops the stale one when a screen goes away.
Resetting the anchor on a screen change would look similar and is not the same
thing: it would return to Credits from a sub-screen having forgotten where the
reader was, which is the bug in the other direction.

This is a hazard for any screen-switching composable that remembers scroll
state at one call site, not a quirk of this application. Nothing in the widget
or its documentation points at it, and the failure is quiet — a wrong starting
offset reads as a rendering glitch rather than as shared state.

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

One Linux machine serves every `[self-hosted, Linux, cranpose-heavy]` job, now
through two runners on that host rather than one. Two of those jobs genuinely
cannot move: the X11 robot suite needs a real X server, and the binary-size
budget is pinned against Linux codegen. The wasm job no longer sits behind
them. The second runner does not double the machine, so the robot suites take
a host-level `flock` against each other — two X11 suites sharing one display
interfere, and a suite competing with itself for the CPU produces exactly the
timing failures the suite is meant to catch.

The applications are still on one Linux runner each, and it shows: cranscan's
release sat queued behind its own `main` CI on `samarch-1-cranscan` while the
tag was already pushed. The lever there is the same one, applied per
repository.

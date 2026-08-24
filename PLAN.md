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

## CranOrbit on the watch: two faults in the shipped build

Found by installing `v1.3.2` on a Pixel Watch 3 and playing it, which is the
only way either of these surfaces. Both are CranOrbit's, not the framework's,
but they are here because nothing else tracks them and because the second one
is not yet proven to stop at the application boundary.

### The radial menu was replaced by a scrolling list

CranOrbit selected levels and modes from a radial menu -- the shape the game
itself is built on, and the reason a round display suits it. The shipped build
presents a vertical `WearScalingLazyColumn` of pill chips instead: `ORBIT
BREAKER` over `CAMPAIGN`/`DAILY`, then `CHAPTER 2` over `IGNITION`, then
`LEVEL 1` over `FIRST LIGHT`. Three taps down a scrolling list to start a
level, on a 408x408 screen.

The radial model was not deleted, only disconnected. `app/src/app_state.rs`
still builds a `RadialSpec` (line 628) and `composed_screen` still asks for one
-- `ComposedScreen::Menu` is constructed with `Some(state.radial_spec())` at
`app/src/ui/composed.rs:324` -- and then `app/src/ui/composed.rs:350` renders
that spec through `WearScalingLazyColumn`. A radial specification is being fed
to a linear list widget. There is no radial renderer left in the tree.

`composed_screen` routes *every* screen that is not `Playing` or `Tutorial` to
`ComposedScreen::Menu`, so one list widget now serves settings, credits, level
select and mode select alike. That is the actual error: the flat Wear list is
right for **settings**, where a scrolling column of rows is what the platform
expects, and wrong for level and mode selection, which is what the radial menu
existed for. Restoring it means a radial renderer for `Menu` and a separate
list path for the settings-shaped screens, not a flag on one widget.

### Backing out of the pause overlay wedges the app

Reproduced on the Pixel Watch 3 against `v1.3.2`, with the display confirmed
awake -- a dozing watch screenshots black and will otherwise be mistaken for
this:

1. Start a level and play. The arena draws.
2. Back-gesture once. `PAUSED / LEVEL 1 SCORE 0 / RESUME` appears, which is
   correct.
3. Back-gesture again. The screen goes to a flat `#121116`.

`#121116` is the application's own background, not the device's black, so the
renderer is running and painting an empty scene rather than having stopped.
Everything else agrees: the process stays alive, the activity keeps window
focus, `[android-frame-rate]` keeps voting 60 Hz, and
`cranpose_render_wgpu::render: [segment-encode]` keeps reporting work after the
screen goes blank. No panic and no `AndroidRuntime` entry.

Nothing on the device recovers it. A tap does nothing. Further back gestures
neither redraw nor leave -- the application still consumes back, so the user is
trapped in a blank screen with no way out but the system app switcher. Force
stopping and relaunching *does* recover, returning to `RESUME / LEVEL 1 FIRST
LIGHT / CONTINUE`, so the wedge is in-memory screen state and not the save
file.

The cause is `SwipeToDismissBox`, and it is the framework's, not CranOrbit's.
After the gesture completes the widget leaves its content translated off screen
and fires `on_dismiss`, because that is what a dismissed *row* wants: its host
is about to remove it. A full-content *navigation* dismissal is the opposite
case -- back means "go up one level", and the host may answer by staying
composed, which is exactly what backing out of a pause overlay does. The
content then never returns, and since `SwipeToDismissBox` owns its controller
internally the application has no state handle to reset it with. The gesture
stays consumed, which is why back could not escape either.

Two things are worth keeping from how this was found. CranOrbit's state
machine was tested first and was correct -- `Playing -> back -> Paused -> back`
returns a drawable `Playing` -- which is what moved the search up a layer
rather than deeper into the application. And the offset the regression test
reports without the fix, a full content width, is the blank screen stated as a
number.

Fixed by `SwipeToDismissSpec::reset_after_dismiss`, off by default so row
behaviour is unchanged, with `SwipeToDismissBox` opting in. CranOrbit picks it
up at the next Cranpose release.

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

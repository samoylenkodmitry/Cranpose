# Cranpose — what is not done

The framework-ownership plan is implemented; what it delivered is in the git
history, not here. This file is only the remainder: gaps that are real, corner
cases a caller has to know about, and limits that are correct but surprising.

An item leaves this file when the behaviour changes, not when someone decides it
is acceptable.

## Gaps in the framework

### Public API test coverage: 357 of 3409 functions

`python3 scripts/public_api_test_coverage.py` reports **3052/3409 (89.5%)**.
Treat the 357 as a map of where a change is unguarded, not as a backlog of 357
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
- **A device-installable iOS build is made locally, not by CI.** The release
  carries an App Store-signed `.ipa`, which TestFlight takes and a device
  refuses directly, and `cranscan-*-ios-unsigned.ipa`, which is ad-hoc signed
  with no embedded profile and which a device also refuses. The repository's
  only iOS secrets are an App Store *distribution* certificate and profile, so
  no CI job can sign for a particular device — that needs a development profile
  carrying that device's UDID, which is per-device and belongs on the machine
  that has it. Both routes to a phone work and both are outside CI: TestFlight
  from the release upload, or re-signing the `.ipa` locally against an
  Xcode-managed development profile and installing it with `devicectl`. The
  artifacts are named for what they are rather than carrying a signing step
  that could only fail.
- **The unused-API deletion rule stops at the Compose-shaped surface.**
  `rememberSaveable`, `ProvideLifecycle`, `rememberLifecycleState`,
  `DurableSaveEffect` and `interval` have no caller and are kept. What makes
  them API is that an application written against Compose expects them to
  exist, not that one of ours has reached for one yet.

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

### Leaving a run is crown-only, and the back gesture cannot do it

Back from play pauses, and back from the pause overlay resumes. So the gesture
alternates and never leaves: five swipes on a Pixel Watch 3 against `v1.3.4`
went paused, playing, paused, playing, paused, with the application still in
front the whole time. On a watch, where the swipe is the only back there is,
that reads as the application refusing to close.

**This is deliberate.** The way out of a run is the rotary crown, and
`back_while_paused_resumes` pins the resume behaviour as a contract. A change
to `on_back` here is a change to the design, not a bug fix — one was written
and reverted precisely because that test caught it.

What is worth revisiting is the design: the only way out of a run is an
affordance nothing on screen names, on a device where the swipe is what a user
reaches for first. The pause overlay already offers an explicit exit item, so
the gap is between what the gesture does and what a user expects it to do,
not a missing capability.

Two things reported alongside this are still unexplained and are NOT this
entry: sounds sometimes echoing as though played twice, and a level not
beginning play on the first tap. Neither reproduces through injected input.

The earlier reading of this section -- that `BackHandler` and
`SwipeToDismissBox` both reach `on_back` and one gesture fires both -- was
wrong twice over. Injected gestures deliver exactly one `on_back` per swipe,
and the alternation they were invoked to explain is the intended behaviour.
Both handlers do reach `on_back`, and that is worth knowing, but nothing
observed needs it as an explanation.

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

## CranAmp's network library: fixed, and what it cost

Reported from the device: `0.1.41` played the user's music over a Round Sync
mount and `0.1.42` did not. Root-caused, fixed, and verified on a Pixel 9 Pro —
a 416 MB album image off the mount now starts in seconds, seeks, stops and
starts again.

The cause was a capability the framework dropped without noticing. `0.1.41`
decoded in process: symphonia read the container and rodio opened the output
device, and Android's own media stack was never in the path. `0.1.42` replaced
that with a JNI `MediaPlayer` backend — and `MediaPlayer` plays a *file*. A
document provider whose bytes come off a network has nothing to seek in and
returns a pipe; `setDataSource` takes that descriptor, fails inside with
`setDataSourceFD failed`, and leaves an item that loads and never plays. An
in-process decoder needs no file, only bytes.

So the decoder came back, through the refactored API rather than around it:

- `cranpose-audio`'s device is now renderer-agnostic (`backend::Renderer`), so
  the one AAudio backend serves the mixer and the media decoder alike instead of
  `cranpose-media` carrying a second, cpal-only device that could not exist on
  Android.
- `cranpose-media` builds for Android and opens its stream through that device.
- `cranpose_services::open_media_source` is how a decoder asks the platform for
  a URI it cannot open itself; Android answers with the provider's descriptor.
- A stream that cannot seek is spooled to the application's cache as it
  arrives — `0.1.41`'s trick, with the two flaws it had fixed: a wait now gives
  up if nothing arrives at all, and the sink can cancel one, so a provider that
  stops talking fails the item instead of freezing the app.
- Android's `MediaPlayer`, `Visualizer` and `Equalizer` are gone from
  `CranposeMedia.java`. What is left is the half only Java has: audio focus and
  the `MediaSession` behind the lock screen.

One thing is deliberately withheld from the decoder: the spool never reports a
`byte_len`, though it knows one. A decoder told how long a stream is treats it
as random-access and reads the tail while probing, and the tail of a spool is
what arrives last. Publishing the length turned a track that started in two
seconds into one that never started at all — confirmed by removing it and
re-running, both ways.

### Still open

- **The playlist does not survive an upgrade.** Every install during this work
  came up with an empty playlist. Separate from playback, and CranAmp's own.
- **Duration is blank for a streamed document.** `probe_duration` refuses to
  spool a whole track to answer how long it is, so a playlist of two hundred
  network tracks does not download the library to fill in its labels. The length
  appears when the item is opened. `0.1.41` did the same.
- **A stalled provider leaves its downloader thread blocked in `read`.** The
  spool's readers give up and the item fails, but the thread that was reading
  the pipe stays in the kernel until the provider closes it. One thread per
  stalled stream, and nothing else waits on it.
- **No test covers a folder pick end to end.** In CranAmp: a folder pick
  yielding known file names produces the expected tracks, which would pin both
  the walk and `is_audio_name`. In Cranpose: a target that registers a platform
  media player has one installed before the first composition. Neither exists,
  which is how a music player could stop playing music without a single test
  going red.

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

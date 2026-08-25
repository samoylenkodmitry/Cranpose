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

### Conditional branches share one composition slot

Compose's compiler plugin gives every `if`/`else` and `match` branch a group of
its own, so a branch that leaves takes its nodes with it. Cranpose has no
plugin, and a `#[composable]` opens one group keyed on where it is *defined*,
not where it is called. Two branches emitting the same widget are therefore one
slot: the arriving branch is handed the node the departing one was using,
together with whatever that node was carrying.

What it cost, on a Pixel Watch 3: starting CranOrbit's Daily run from the title
ring left the ball parked on the paddle through any number of taps. The taps
arrived and the simulation ran — the run ended `TIME UP` at score 0 while it
was being tapped — but the ring's gesture loop was still the one reading them,
because `pointer_input` restarts only when its key changes and both branches
passed `()`. Campaign works only because it stops at a screen of a different
shape on the way to the arena. Two earlier fixes missed it by reasoning about
`AppState`, which was never at fault.

`a_branch_switch_hands_the_gesture_to_the_branch_that_is_on_screen` in
`cranpose-app-shell` is the reproduction: before the fix it reported
`(menu, arena) = (2, 0)`, the departed branch having eaten the arriving one's
tap.

Gestures are closed: a handler's identity is now the declaration — this call,
with these keys — so a node handed to a different `pointer_input` call
restarts. **The general case is open.** A reused node still carries the
departed branch's `remember`ed state, its animations and its scroll offsets,
and nothing warns the author. The same shape already produced a second
reported bug in the same application, where every list screen shared one
scroll position because one call site is one slot.

Applications can state the identity themselves with `cranpose_core::with_key`,
Compose's `key(…) { }`, and CranOrbit's router now does. That is the framework
asking every author to remember what Compose's compiler never makes them think
about, and the failure is quiet: a stale gesture or a wrong scroll offset reads
as a rendering glitch, not as shared state.

Closing it means what the plugin does — a group per branch. The macro parses
the function with `syn` and could wrap branch bodies, but the group API is
closure-shaped (`Composer::with_group_seed`), and wrapping an arbitrary branch
in a closure breaks `return`, `?`, `break` and `continue` inside it. So this
wants an RAII group guard on the composer first, then the macro transform,
then a sweep of the repo's own conditionals. It is a slot-table change and
deserves its own validation pass rather than riding along with a bug fix.

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
Both are verified on the watch against `v1.3.3`.

A Daily run that would not launch the ball on a tap is fixed in `v1.3.6`, by
CranOrbit naming its three gesture surfaces and keying the router on them. The
cause was not in this application, though: see *Conditional branches share one
composition slot* above, which is what let the ring go on reading the arena's
taps. What is left:

### Campaign's level intro is a scrolling list where Daily's is the ring

Reported from the watch on `v1.3.7`: starting Campaign reaches a vertically
scrollable screen with a `START` button on it, where Daily goes straight to the
round arena. The two modes take different routes to the same place, and the
scrolling one is the wrong shape for a watch -- a round screen showing a list
whose only content is one button.

Not yet root-caused. What is worth knowing before looking: the level intro is
the screen Campaign stops at on its way to the arena, and it is *why* Campaign
never showed the launch bug that Daily did -- it changes the composition's shape
between the ring and the arena. So this screen is load-bearing for the wrong
reason, and removing it without keying the router would bring the older bug
back on Campaign too.

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

That `flock` covers the robot suites and nothing else, and the host it
protects carries **nineteen other repositories' runners**. None of them know
the robot suite exists. A neighbour's Rust build takes twelve cores, the frame
the suite is timing takes twice as long, and the suite reports a per-frame
regression that reproduces nowhere: `robot_text_handle_cycle_stability` failed
on `main` at `drag work_avg_ms 0.73 -> 1.66` and `layer_cache_size 3 -> 13
(allowed 12)`, then passed on the same host on that commit **and** on the
commit before it once the box was quiet. Half an hour was spent bisecting
three innocent pull requests.

The suite now waits for the load average to come down before it starts timing
(`wait_for_host_quiet`, called after the build and before the first test), and
`host_state_summary` -- already printed before every attempt -- carries
`load_1m` so a red names its own conditions. It never refuses to run: after
ten minutes it starts anyway and says so, because a gate that will not start
is worse than one that starts late.

That is a confound removed, not the problem solved. The problem is that a
measurement gate shares a machine with nineteen unrelated build queues, and
the fix for that is a host the robot suite does not share.

The applications are still on one Linux runner each, and it shows: cranscan's
release sat queued behind its own `main` CI on `samarch-1-cranscan` while the
tag was already pushed. The lever there is the same one, applied per
repository.

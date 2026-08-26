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

### Branch groups: fold-only identity

Every conditional site of a `#[composable]` body — `if`/`else` branch,
`match` arm and guard, `for`/`while`/`loop` body — a `for` statement
whole, so a composing `IntoIterator`/`Iterator::next` outside the body
is covered too — each `&&`/`||` condition operand and `let` scrutinee, and every closure or executable
nested item defined inside a branch — pushes an RAII **branch fold**: a
location key on a per-pass stack. Nothing ever materializes; a guard costs a push and a pop, and
closing folds in any order is safe (entries are marked dead and trimmed).
Branch identity is mixed into every slot identity instead of being
represented structurally:

- A group's static key mixes the folds pushed since its parent group
  opened, so a group opened in one arm can never be resolved by another
  arm, keyed rows stay flat indexed siblings with their toggle cost, and a
  scoped recomposition re-enters cleanly because folds are always relative
  to the enclosing frame.
- A value slot's source stamp — the caller location of the `remember`/
  `use_state`/hook that created it, captured before the session borrow —
  mixes the same fold. A slot whose stamp mismatches is never adopted;
  when a branch's slots vanish, following same-group slots resynchronize
  by a forward scan for their `(type, source)` identity instead of
  reinitializing, so neighbors keep state across branch cardinality
  changes.
- A `#[composable]` fn is `#[track_caller]` and keys its group by its
  definition location mixed with the caller's location
  (`composable_identity_key`, definition key cached in a per-fn static):
  every call site is its own identity, Compose's positional-key parity,
  and two different composables selected through one collapsed call site
  — a fn pointer, a macro-expanded router — still key apart because
  their definitions differ. `emit_node` is `#[track_caller]` the same
  way, so a raw node record carries a folded source and an arm can never
  adopt another arm's node.

Branch departure needs no special lifecycle: an arm's groups and slots are
ordinary unvisited content, detached and dispose-or-retained exactly as
before branches existed (`docs/slot_table_invariants.md`,
`branch_group_tests`, `robot_recomposition_lab` end to end). The remaining
edges, each the price of running on names before expansion rather than on
typed IR:

- **A conditional expanded out of a `macro_rules!` body shares its slots
  unless the arms call different composables.** The attribute macro runs
  before function-like macros expand, and `Location::caller()` for code
  inside an expansion collapses to the invocation site, so folds and
  caller keys cannot tell the expanded arms apart. The definition half of
  `composable_identity_key` still separates arms that route to different
  composables
  (`two_composables_selected_through_one_macro_call_site_stay_distinct`);
  what remains collapsed is one composable called with different
  arguments across arms, and raw `remember` slots in the arms (pinned as
  `a_macro_rules_conditional_shares_slots_by_construction` and
  `a_macro_rules_conditional_collapses_composable_caller_identity`; the
  escape hatch is an explicit `with_key` per arm). Compose's plugin runs
  on IR after inlining, which is what closing this would take.
- **Composition reached only through a place path folds into the
  surrounding context.** A `ref` pattern binds into the place, so a `let`
  scrutinee that is a place expression keeps its structure; its value
  sub-parts carry folds, but a composing `Deref` impl on the place chain
  itself runs under the enclosing fold, and every `let`-bearing
  condition is covered by the
  whole-statement fold: an `if` or `while` whose condition contains any
  `let` is enclosed in a block holding a fold guard, so every scrutinee
  evaluation — chained or plain, place or value, however many times a
  `while let` repeats — carries an identity the code after the statement
  never shares, the place syntax is untouched, and no let-chain syntax is
  ever fabricated (a `let` is gated by its own span's edition, so a
  fabricated chain around a user's edition-2021 `let` would not compile;
  pinned as `a_short_circuited_place_deref_does_not_leak_into_a_later_deref`
  and `a_shrinking_while_let_scrutinee_does_not_feed_a_later_helper_call`). A closure
  consumed-and-returned by a helper (`store(make_pair(|| A(1)).0)`)
  still folds into the surrounding context, bounded by source stamps.
- **A reuse-retained scope inside a keyed wrapper recomposes fresh when the
  wrapper leaves composition.** Dispose-or-retain engages only when the
  detached root is itself the reuse scope; a `with_key` wrapper above it
  hides it. This reproduces on origin/main and is independent of branches
  (pinned as
  `keyed_wrapper_retention_behaves_identically_in_both_shell_classes`);
  making retention descend into detached subtrees is filed as its own task.

And identity across *data* is still the author's statement: one call site
fed different values is one slot in Compose too, so a list screen that
renders per-route content keys it with `cranpose_core::with_key`, as
CranOrbit's router does.

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
cause was not in this application, though: conditional branches used to share
one composition slot, which is what let the ring go on reading the arena's
taps — closed by branch groups (see *A branch that composes only through
names the macro cannot see* above for what remains). What is left:

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

So the lock is now over the machine's capacity rather than over the robot
suite, and it has two sides (`scripts/ci/with_host_lock.sh`). The three heavy
Linux builds that share this host -- the size budget, the wasm build, the
Android release build -- take the **shared** side for their whole run. The
robot suite builds without it and takes the **exclusive** side for the timed
part only, so builds still overlap builds and only a measurement empties the
machine. Neither side ever refuses to run: after forty-five minutes it starts
anyway and says on stdout that it did.

That lock covers this fleet. The other nineteen queues on the box are not
ours to serialise, so the suite also waits for the load average itself
(`wait_for_host_quiet`, after the build and before the first test), and
`host_state_summary` -- already printed before every attempt -- carries
`load_1m` so a red names its own conditions.

Both are confounds removed, not the problem solved. The problem is that a
measurement gate shares a machine with nineteen unrelated build queues, and
the fix for that is a host the robot suite does not share.

The applications are still on one Linux runner each, and it shows: cranscan's
release sat queued behind its own `main` CI on `samarch-1-cranscan` while the
tag was already pushed. The lever there is the same one, applied per
repository.

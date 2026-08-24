# Agent Notes for cranpose

- no unsafe
- just test, just clippy, just fmt # `just` lists every gate; CI runs these same recipes
- KISS, DRY, SOLID. don't copy-paste lazily
- Use `cargo add <crate>` to add dependencies.
- Use `cargo upgrade` to upgrade dependencies.
- Use `anyhow` for error handling in application code; use `thiserror` for library code.
- Write unit tests for all public functions and methods.
- Write integration tests in the `tests` directory.
- Follow idiomatic Rust naming conventions (snake_case for variables and functions, CamelCase for types and traits).
- CamelCase for #[composable] functions
- Use `Result<T, E>` for functions that can fail; prefer specific error types over `Box<dyn Error>`.
- Use `Option<T>` for values that can be absent.
- Use `async`/`await` for asynchronous code; prefer `tokio` as the async runtime.
- do proper review mitigation; don't short-cut and do a honest professional work; be very strict to your code & architecture decisions; keep repo clean and don't put unfinished parts here; fix everything
- do not create a half-migrated state of the repo; don't "deprecate"; always change the existing code
- just android # :app:assembleRelease in apps/android-demo/android
- just web # always --release; the fast path skips the wasm size budget
- (+robot tests)
- instead of accepting the shortcut always choose to fix the underlying architecture issue
- do not avoid and do not defer the big architecture refactoring when necessary
- if there is a bug start with writing a failing test that will catch it so we never regress to it again in the future
- do a code review; look for any shortcuts, laziness, taking the easy path instead of doing the hard necessary work, poor architectural choices, everything that will shoot in the foot, poorly written code like it was a deadline 1 minute before end of the work day; but not invent the problems if there arent any- don't fear the significant arch change; everything is still pre-alpha; this is the right time for a big change
- do not ever git reset, always stash if needed
- do not ever rm -rf, prefer mv to some _old name
- all tests should pass, its never *not yours*
- zero warnings on all build/clippy/test commands, never *was pre-existing*
- the #[cfg(feature = "robot-app")] is forbidden
- reference JC kt repo /media/huge/composerepo/
- use samarch-1 or the mac by ssh for builds where possible: `ssh samarch-1`
  (Linux, Android SDK at /home/s/develop/sdk, X11 for the robot suite) and
  `ssh macm3` (macOS, Apple toolchains). They are faster than this machine and
  keep long compiles off it. Note macOS logs in over ssh with zsh, which does
  not word-split unquoted expansions, so wrap remote scripts in `bash -lc "..."`.
- perf scripts are perf*.sh at project root
- e2e robot headless tests is `just robot` (should all pass)
- do not use big models as subagents (opus, codex xhigh thinking, etc), only small fast & cheap to not waste tokens
- no 'backwards compatibility' is allowed; we in a pre-alpha
- no comments in style "now it is like that" - we are not writing history
- duplicated code (10+ lines) without architecture is forbidden
- 'legacy'/'old way' etc not allowed. we are in a pre-alpha, everything is fresh, clean, single instance
- be aware of what you've done by looking at git status
- don't call anything 'migration'. Say no to half-states. Only complete entropy annihilation is allowed.
- don't hardcode things
- parallelization and SIMD where appropriate (note: the wasm target must not be forgotten)
- if you spot you wasted too much time on something, please put the discovered info into TIME_WASTERS.md so save future time for everyone
- not "if you want to"; should be "the proper fix for production-grade ui-framework"; not "I WANT"; should be "this is wrong, this is right, this is the cause, this has to be re-architectured and be rewritten"
- for non-trivial bugs: explore → document findings → rank suspicions with evidence → propose re-architecture options → implement → diagnostic verify → iterate until confirmed fixed. no one-shot guessing.
- confirm a suspected cause by REMOVING it and re-running, before writing the
  fix. three wrong fixes for one macOS signing failure came from reasoning
  about an error message instead of testing what it claimed.
- for a UI bug that reproduces on a device, write the state-machine test FIRST.
  If it passes, the fault is above `AppState` and reading more of `AppState` is
  wasted time. That test passed twice for bugs that turned out to live in the
  layer above -- a widget holding content off screen, and a pointer claim
  surviving a screen change.
- an existing test is a design decision. If a "fix" makes one fail, the
  behaviour was deliberate: read the test before changing the code.
- device testing on the Pixel Watch over adb: the watch dozes between commands
  and silently drops injected input, and a dozing screen captures as a ~1.6KB
  black PNG that looks exactly like a frozen app. Send `input keyevent
  KEYCODE_WAKEUP` before every step and check `dumpsys power | grep
  mWakefulness` before believing a screenshot. The rotary crown is
  `adb shell input rotaryencoder scroll --axis SCROLL,<n>`, and ring menus also
  take taps on the screen edge.
- `gh` has more than one account here and the active one flips. When a repo
  starts 404ing or a rerun says "must have admin rights", run
  `gh auth switch --user samoylenkodmitry` rather than believing the error.
- should never workaround bugs instead of fixing the root issue

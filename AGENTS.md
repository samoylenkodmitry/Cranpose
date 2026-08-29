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
- do not ever remove recursively by \r\m \-\r\f, prefer mv to some _old name
- all tests should pass, its never *not yours*
- zero warnings on all build/clippy/test commands, never *was pre-existing*
- the #[cfg(feature = "robot-app")] is forbidden
- reference JC kt repo (androidx/androidx, the actual Jetpack Compose source) on
  samarch-1 at /media/huge/projects/android/androidx -- NOT /media/huge/composerepo/,
  which does not exist and misdirected an earlier session. It is a fork (`origin` =
  samoylenkodmitry/androidx, `upstream` = androidx/androidx) on branch androidx-main,
  and it is STALE: as of 2026-08-29 its compose/*/api/current.txt is still at commit
  be18a1188a13a253d2a6784f812815c88454775c, dated 2023-06-26 (~Compose 1.5.0-beta era).
  Treat anything read from it as true as of mid-2023, not current, until someone
  re-syncs it -- see docs/compose_api_parity.md for what that staleness costs.
- use samarch-1 or the mac by ssh for builds where possible: `ssh samarch-1`
  (Linux, Android SDK at /home/s/develop/sdk, X11 for the robot suite) and
  `ssh macm3` (macOS, Apple toolchains). They are faster than this machine and
  keep long compiles off it. Note macOS logs in over ssh with zsh, which does
  not word-split unquoted expansions, so wrap remote scripts in `bash -lc "..."`.
- the CI runner names mislead. `mac-idle-Cranpose` is this Mac: it registers
  only while nobody is at the keyboard and exists for signing, so offline is its
  normal state, it is not spare macOS capacity, and bringing it up is not a way
  to speed CI. `dmitriis-mac-Cranpose` is macm3 and is the default macOS runner;
  every macOS job serialises on it. The Linux heavy pool is two --
  `samarch-1-cranpose` and `samarch-1-cranpose-2`; the Macs carry
  `cranpose-heavy` as well but do not match `[self-hosted, Linux, ...]`. A deep
  queue is queueing, not a stall: the jobs API lags the runner by minutes, so
  read that runner's own `_diag/Runner_*.log` for JobDispatcher lines and check
  its load before diagnosing one.
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
- confirm a suspected cause by REMOVING it and re-running, before writing the fix. (binary search by cutting half of the code until the only thin cause left)
- for a UI bug that reproduces on a device, write the robot e2e test FIRST.
- a test that has only ever been green is decoration: prove it red first (force the failure mode, see it fail, then remove the forcing) before trusting a pass.
- device testing on the Pixel Watch over adb: the watch dozes between commands and silently drops injected input, and a dozing screen captures as black PNG. Send `input keyevent KEYCODE_WAKEUP` before every step and check `dumpsys power | grep mWakefulness` before believing a screenshot. The rotary crown is `adb shell input rotaryencoder scroll --axis SCROLL,<n>`, and ring menus also take taps on the screen edge.
- `gh` has more than one account here and the active one flips. When a repo starts 404ing or a rerun says "must have admin rights", run  `gh auth switch --user samoylenkodmitry` 
- should never workaround bugs instead of fixing the root issue
- gates live in the justfile and CI calls the same recipes; change a gate there, never inline in a workflow
- frame-rate numbers measured under xvfb are software presentation, not the GPU (26 fps against 67 on the same scene); measure fps on a real display
- before diagnosing any red test, `git fetch origin main` and rebase: a stale base is indistinguishable from a regression, and today four "broken on main" robot tests were four commits already fixed upstream
- never invent a feature subset to check a target; run the exact command CI runs. `--features ios` without `renderer-wgpu` gates `ios.rs` out and invents three dead-code warnings that exist in no shipped build, and the same slip on the web target invents a compile error
- a system dialog (iOS document picker, permission sheets) can only be checked on a device: it decides what to enable from what the app asked for and hands nothing back. Do not ask a human to eyeball it once per iteration — drive it from a UI test. cranamp's `platform/ios/run-uitests.sh` is the shape: launch args open the dialog so no coordinate-tapping is needed, and it prints every row with `enabled=`
- iOS on-device UI tests need USB and `Settings > Developer > Enable UI Automation`. Over a network pairing the runner dies with "Timed out while enabling automation mode" before any test body runs, which reads exactly like the toggle being off

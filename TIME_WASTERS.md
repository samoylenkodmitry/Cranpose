# Time Wasters

Signature → cause → what to do. One lesson per line, no incident history.

## Triage before blaming the code

- **Rebase first.** `git fetch origin main && git log --oneline origin/main -3` costs seconds. Four "broken on main" robots were four commits already fixed upstream. A stashed clean tree proves the failure is not in *your* changes — it says nothing about your base being current.
- **Confirm a cause by REMOVING it and re-running, before writing the fix.** Binary-search by cutting half the code.
- **A/B two revisions on ONE machine, back to back.** Byte-identical output (same SHA-256, empty `ImageChops.difference` bbox) ends the question. Two machines — or one machine hours apart — compares host state as much as code.
- **"Fails on clean main too" ≠ environmental.** A shared harness mistake fails on clean main as well.
- **Host flake vs regression, from the log alone:** read the diagnostic fields, not the iteration count. `wait_for_idle: timed out after 1 iterations` means the process was never scheduled, but the converse does not hold: a huge count rules starvation *out* and proves nothing else. `needs_update=false, has_animations=false, waiting_for_present=true` is a slow compositor no matter how many iterations ran — `robot_idle_fps_after_tab_walk` burned 1,280,577 of them with composition already settled, and passed in isolation and in a full 121-test suite on the same commit. Only fields that accuse the application justify blaming it. Then sum the gaps between `Running robot_<name>...` lines excluding the suspect for a load index (the suite reproduces within a second — 843/843/844s — so a 970s run is unmistakable); then `gh workflow run heavy-selfhosted.yml --ref <branch>` to re-run the identical commit. Do that dispatch first, not last.
- **Dispatching `heavy-selfhosted.yml` cancels the run already in flight for that ref, by design.** Its `concurrency` is `heavy-${{ github.ref }}` with `cancel-in-progress: true` ("Supersede older runs of the same ref: fleet minutes are real machines"). Superseding your OWN older run is the point, so the re-run advice above stands; dispatching against a ref somebody else is waiting on destroys their run and surfaces to them as signal 15. Check who is on the ref before `gh workflow run`.
- **A failure one hair over its bound with a timing metric that moved with it is contention.** A real leak grows every cycle and clears the tolerance by a mile. Never widen the bound — for `robot_text_handle_cycle_stability` the accumulation ratio *is* the guard.
- CI preserves full stdout on the runner (`Preserving robot result artifacts after status 1: <dir>`) — complete, unlike the truncated Actions log.
- Before acting on a proxy metric, read how the tool decides. `scripts/public_api_test_coverage.py` matched names as *substrings* (`with_timeout` "covered" by `exit_with_timeout`) and scanned `crates/` only, so the robot suite — the sole exercise most of the driver API gets — counted as untested.
- "No test names it" and "no code calls it" are different questions. Uncalled *and* untested is dead; called but untested wants a test. Deleting on the first signal broke four modifier files (`InspectorInfo::add_dimension`: four callers, no test).

## Host and display gates

- **Thermal guard has TWO knobs**: `CRANPOSE_HOST_MAX_TEMP_C` (default 90) trips, `CRANPOSE_HOST_RESUME_TEMP_C` (default 85) releases. Raising only MAX is useless when ambient sits above RESUME — one spike arms a wait for a cooldown that never comes, and the run dies `host_not_ready` after `CRANPOSE_HOST_MAX_WAIT_SECS` (300). On a busy desktop use `CRANPOSE_HOST_MAX_TEMP_C=97 CRANPOSE_HOST_RESUME_TEMP_C=93` and a longer wait. `host_not_ready` with no robot binary launched is an environmental block — report it, do not wait it out. Schedule robot suites LAST, never beside a cargo/gradle build.
- **Robot over ssh needs an explicit `DISPLAY`.** Without it every test fails "neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set" — 153 failures that look catastrophic are one env var. `DISPLAY=:0` on samarch-1; `run_robot_test.sh` needs one even for `CRANPOSE_HEADLESS=1` (winit still builds an event loop).
- **Windowed Fifo/VSync against `DISPLAY=:0` crawls at ~1fps** (`present_ms≈998`, see `CRANPOSE_DESKTOP_FRAME_TELEMETRY_MS=1`): no consumer for the swapchain, so every present waits out its timeout. Looks exactly like a pacing bug. Run examples under `xvfb-run -a -s "-screen 0 1600x1200x24"`; `NoVsync` is unaffected.
- **Bare `xvfb-run` is 640×480 at ~30Hz** — clipped windows and unavailable cadence fail correct renderers. Size the screen explicitly, and never read its timing as production performance (26 fps under xvfb vs 67 on the same scene on a real display).
- **Fractional scale breaks scale-1 assumptions.** `robot_leetcodedaily_full_layout_scroll_stability` fails deterministically (improvement_ratio 0.0000, identical on clean main) on an X11 display with Xft.dpi 130 → winit scale ≈1.354. Run under Xvfb or `WINIT_X11_SCALE_FACTOR=1`. Headless runs still pick this up because `DISPLAY=:0.0` is set in every shell.
- **Apps launched from the assistant's shell inherit nice 5** → vsync apps stutter while the binary is fine (`ps -o ni`; the `N` in STAT is the tell). renice is denied and `systemd-run --scope` inherits it too. Launch user-facing apps with `systemd-run --user --unit=<name> --setenv=DISPLAY=:0 <binary>`.
- **X11 multi-monitor:** `xdotool getdisplaygeometry` lies (1920x1080 vs a real 5760x2160 virtual screen) and the WM parks windows in a top-left dead zone the pointer cannot enter, so clicks silently miss. `windowmove` onto a real monitor before driving input.
- **`xdotool click 4/5` (legacy XTEST wheel) does not scroll winit windows here** even with the pointer confirmed over the app. Clicks and drags work. Use in-process `robot.mouse_scroll`, pinning `with_frame_pacing_mode(Vsync)` if production pacing matters.
- **Synthetic X11 input splits by gesture shape, not by "X11 works".** On samarch-1 exactly the examples that inject a real wheel or split press/hold/move/release across separate `xdotool` calls fail while `xdotool click` and in-process scroll pass in the same run. The window does hold focus (`getactivewindow` polled at 100ms) and the click still does not register.
- **`scripts/x11_compare_*.sh` put cargo targets under the comparison label**, so changing only `--label` forces a fresh baseline *and* current build even with the source refs unchanged. Reuse one label while iterating thresholds, or call `scripts/x11_compare_window.sh` with explicit `--baseline-target`/`--current-target`.
- **Title-scoped window queries catch other processes' windows.** pid-scoped `xdotool search --pid` falls back to title-only matching; close concurrent same-title demos, and use `--expected-owner-command <app-binary>` (checks `_NET_WM_PID`) for native-window comparisons.
- **macOS: `.with_headless(false)` examples need an awake, compositing display.** "the window surface refused N consecutive frames over 5s and never reached generation 1" is the answer, not a symptom. Two waits emit near-identical text — read the prefix, not the duration: `present wait:` is the standalone wait (`wait_for_present_frame`, `pump_frames`, 5s), `wait_for_idle:` is an idle wait whose composition had already settled with only the frame outstanding (30s). Both accuse the compositor; neither accuses the app. `pmset -g log | grep -i "Display is turned"` dates the panel off/on; `pmset -g assertions` and `CGSSessionScreenIsLocked` answer for now. Host-capability skips gate on X11+xdotool, which these two do not need, so they run and fail instead of skipping.
- **`~/develop/projects/Cranpose` on samarch-1 is a synced mirror**: its `.git` is a worktree pointer at a macOS path, so every git command dies `not a git repository: (null)`. rsync into a scratch dir (exclude `target/`, `.git`). `just` lives in `~/.cargo/bin`, absent from a non-interactive ssh PATH.
- **`scripts/ci/with_host_lock.sh` exists and nobody was taking it.** Every session, including the ones telling others to go faster, was `ssh samarch-1`-ing straight past it. One day of that cost four phantom red CI runs: robot jobs refuse to run above a load of about 7, heavy compiles from sessions working over ssh held the box at 13–51, the robot jobs waited, timed out, and reported RED indistinguishable from a regression — two of them got diagnosed as a real regression before anyone checked host load. Separately, cranscan-82's query benchmark spread collapsed from 9.4–33.9ms to 7.6–7.8ms on identical code once they took the lock, overturning a retraction they had already made off the contended numbers. Take `--shared` for a build, `--exclusive` for a measurement or robot suite, every time — do not build on samarch-1 while a robot job runs, and do not wait for a quiet-looking load average instead of just taking the lock.
- **A PIN-locked phone fails `am start` with "Activity class ... does not exist" (Error type 3) and `screencap` writes a 0-byte PNG.** The resolver table still lists the activity, so the message reads like a broken APK; check `dumpsys window | grep -m1 mCurrentFocus` first -- `StatusBar` means the keyguard is up. `wm dismiss-keyguard`, keyevent 82 and swipe-up do nothing against a PIN, and entering it is off limits: ask for an unlock (or use the emulator, where the same APK, sysprops and telemetry work unchanged).
- **Android "SDK location not found" is missing env, not a missing SDK.** `~/Library/Android/sdk` and its NDKs are installed; `ANDROID_HOME`/`ANDROID_SDK_ROOT`/`ANDROID_NDK_HOME` are unset in a fresh shell and `android/local.properties` is untracked. `ls ~/Library/Android/sdk` before believing the error.
  ```bash
  export ANDROID_HOME="$HOME/Library/Android/sdk"
  export ANDROID_SDK_ROOT="$ANDROID_HOME"
  export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
  echo "sdk.dir=$ANDROID_HOME" > apps/android-demo/android/local.properties
  ```
  The release task builds arm64, x86_64 *and* armeabi-v7a in separate `cargo ndk` passes — `rustup target add` all three or it dies at the last one with `can't find crate for core`.
- Faster substitutes for an Android build: `--target aarch64-apple-ios` compiles everything gated `any(target_os = "ios", target_os = "android")` with no NDK (it caught a broken camera conversion desktop could not see); `--target wasm32-unknown-unknown` answers "does this compile off desktop".
- **Verifying an Android framework change on a real phone runs through cranscan**, which consumes Cranpose by path from `../cranpose-play` (a staging checkout — merge into its working tree, it carries uncommitted experiments). Build it with `-PcranscanTestInstall=true` so it installs as `com.cranscan.app.codex` beside the release app. Its device scripts have drifted twice: the Gradle task to exclude is now `cranposeBuildNativeDebug` (not `buildRustDebug`), and `cargo ndk -o` must write to `<workspace>/target/android` — the plugin's jniLibs dir — or the APK packages without `libcranscan.so` and dies "Unable to find native library". Their fixture `orders.txt` never loads either (`read_fixture_orders` runs in the `android_main!` launcher expression, before the platform registers `application_directories`), so drive real work through the app UI instead of expecting the scripted import.

## Shell, process and sync traps

- **`xtask`'s `gate_diff`/`complexity_gate`/`duplication_gate` tests' git-commit-based fixtures (in `xtask/src/main.rs`'s `mod tests`) fail over a fresh non-interactive SSH session on samarch-1** with `gpg: signing failed: Inappropriate ioctl for device` -- the account's global `commit.gpgsign=true` needs a pinentry prompt no SSH session without an existing agent can satisfy. Only relevant if you go looking for it: `test-quality-gates`/`complexity-gate`/`duplication-gate` are steps in rust.yml's mac-only "checks" job, so this never runs on samarch-1 in real CI -- confirmed clean by running `cargo test -p xtask` on a Mac instead, where it passes. Not a code bug; do not spend time "fixing" the test for a host it never runs on.
- **`git status`/`git diff` can silently lie after `cp -r`ing a repo with `core.fsmonitor=true` (the macOS default here).** A file rewritten seconds after the copy showed as fully clean in both porcelain commands -- 44 real changed lines in `Cargo.toml`, zero reported -- while `git hash-object` and `git show HEAD:<path> | diff -` on the same file showed the true content. The copied index carries an fsmonitor validity token the daemon in the new location never invalidates. `git -c core.fsmonitor=false status` (or `diff`) bypasses it and shows the truth; trust that over plain `git status` in any repo copy made by `cp` rather than `git clone`/`git worktree add`.
- **`pgrep -f`/`pkill -f` match the calling shell** — its command line contains the pattern, so a wait loop `until ! pgrep -qf "cargo build"` spins forever while looking exactly like patience, and `pkill -f <pat>` kills the batch mid-way so later commands silently never run. **Use `scripts/wait_until_quiet.sh` (`just test-shell-helpers`) rather than hand-rolling either.** The bracket trick is not the general fix and believing it is leaves you unprotected: `[c]argo build` only works when the pattern is an inline literal in the loop's own command line. Pass the pattern as an argument — `wait_until_quiet.sh "cargo build"` — and the script's argv holds the *unbracketed* string, which `[c]argo build` matches perfectly, as does every subshell inheriting that argv. Filter by ancestry instead (walk each candidate's parent chain, drop anything reaching `$$`); a process-group filter looks equivalent and is worse, because a caller that backgrounds the awaited process shares the group and the filter discards the very thing being waited for. `pgrep` also reports zombies — an exited child keeps its full command line until the parent's `wait(2)` — so a waiter can block on a process that finished minutes ago. Kill survivors by exact PID, never by pattern. This entry existed, with the bracket fix, before five of these were written by four separate callers (oldest 21h36m, orphaned to init); prose did not prevent them, which is why the tool exists.
- **On a shared host, scope every `pkill -f` to your own checkout, and attribute before you kill.** CI runs the same recipes you do — `just robot-gpu` is `heavy-selfhosted.yml:72` — so its process cmdlines are BYTE-IDENTICAL to yours and cannot tell your tree from CI's. Attribute by artifact instead: does `robot_test.log` or `target/robot/` exist in *your* cwd? One session concluded a `just robot-gpu` was its own on cmdline alone, killed it, and it was CI's; its own run had never started, which the missing artifacts said plainly.
- **Killing a `just <recipe>` wrapper orphans the work, it does not stop it.** `just robot-gpu` matches only the `just` process — not `xvfb-run`, whose cmdline carries no such token, and not `run_robot_test.sh`. Kill it and Xvfb, the script and its `flock` keep running, reparented to init with the script still blocked: an orphan tree reported as "parent gone", which reads exactly like an OOM kill and cost three sessions an afternoon. This is also what the runner's own cancellation does, which is how a GitHub run showing "cancelled" left 58 processes still executing — Runner.Worker included — holding the exclusive host lock. Kill the descendant set (walk `pstree -p` from the wrapper), never the wrapper alone, and never trust "cancelled" as evidence that anything stopped.
- **`ssh host 'pkill -f A; pkill -f B'` runs only the FIRST pkill.** The remote shell's cmdline is the whole command string, so it contains A — the first pkill kills the shell and B never executes. Reproduced: `ssh host 'pkill -f "TOKEN_A"; pkill -f "TOKEN_B"; echo BOTH_RAN'` prints nothing and exits 255, while the same line with both patterns bracketed (`[T]OKEN_A`) prints BOTH_RAN and exits 0. Bracketing only the first still dies, on the second. This is the measured form of the self-match trap above, and its practical effect is that half your cleanup silently does not happen while you believe all of it did.
- **Do not accept `pkill` as the explanation for a *signal 9* death.** `pkill`/`kill` default to SIGTERM (15), so a plain `pkill -f` produces "terminated by signal 15". Signal 9 needs an explicit `-9`/`-KILL`, an OOM kill, or something else — and a vanished `target/` is a delete, not a signal, so the two symptoms need two different culprits. Read the signal number before blaming a neighbour, and do not let one confirmed kill absorb every unexplained death near it.
- **zsh does not word-split unquoted expansions**, and ssh logs into zsh on *both* build hosts — samarch-1 is Linux but its login shell is zsh too, so this is not a macOS-only trap and reading it as one is what makes it bite twice. `for f in $(git grep -l ...)` passes the whole list as one argument ("File name too long"); `security list-keychains -s $keep` passes one space-prefixed path and corrupts the search list. The nastiest shape is a **loop that silently does nothing and then reports success**: `kill -TERM $pids` hands `kill` one bogus argument (suppressed by `2>/dev/null`), and the verification `for p in $pids; do [ -d /proc/$p ] && ...; done` iterates *once* over the whole string, finds no such directory, and prints "still alive: none". Nothing was killed and the instrument swore otherwise — it cost a full cancel-and-kill cycle on a wedged robot job that held the exclusive host lock the entire time. Wrap remote scripts in `bash -lc "..."`; use `-lz` with `xargs -0` or `IFS=$'\n'`. Verify a kill by re-reading /proc from a *separately* invoked shell, never from the same one that did the killing.
- **`cmd 2>&1 | tail -c N > log` reports tail's exit status** — a 59-error compile failure arrives as "exit 0". Append `; echo "EXIT_CODE:$?" >> log` inside the detached command and read the log, never the wrapper.
- **`codex exec "<prompt>"` reads *additional* stdin when stdin is not a tty** and hangs at "Reading additional input from stdin..." forever (ten hours, once). Always `< /dev/null` it in detached launchers, and write the exit sentinel unconditionally — under `set -e` a non-zero exit kills the script before the sentinel and the waiter polls an orphan log forever.
- **`git grep` reads the index, so it cannot see files the branch just added.** A 278-site rename silently skipped three new crates and the build failed on an import the rename reported success over. Drive tree-wide renames from `find`, or `git add -A` first, and always re-grep for the OLD name afterwards.
- **A single `ps` snapshot is not liveness.** Liveness = two CPU samples spaced apart AND newest-artifact mtime advancing AND the output file growing. Give every wait loop a staleness deadline (~10 min without new artifacts → investigate/kill). Check for orphaned Xvfb/app/cargo processes after any background session.
- **A shared checkout's branch can change under you mid-session.** The main
  `/Users/s/develop/projects/Cranpose` working copy is used by many
  concurrent agent sessions; it was on `main` at session start and had
  silently moved to an unrelated feature branch (`git branch --show-current`)
  46 commits behind `origin/main` by the time a diagnosis ran there — a
  task brief's "PR #549 merged as `<sha>`" was true upstream but absent
  locally, and `git show <sha>` still succeeds for a commit reachable from
  *any* ref, so it looks like proof the working tree has it. An hour went
  into "reproducing" a rendering ghost that was really this stale
  `has_focused_field` panic path, plus a second false alarm from a
  copy-pasted pixel-color heuristic (tuned for a different fixture) matching
  a button's anti-aliased label text. Before trusting file contents for
  diagnosis: `git branch --show-current` and `git merge-base --is-ancestor
  <claimed-sha> HEAD`, not just `git show <claimed-sha>`. Do real diagnostic
  work in a dedicated `git worktree add ... origin/main`, never in the
  shared root.
- **Two actors building the same cargo target race on `target/<profile>/<bin>`** — a "verified" capture can be the other build's. `cp` to a distinct scratch name before running whenever concurrent builds are possible. Editing macro/core sources mid-run hands later crates a different macro than earlier ones compiled against: freeze sources or relaunch on the warm target.
- **`{ cd base && cargo bench; cargo bench }` benches base twice** — the `cd` survives the arm. Use absolute paths in both arms; a suspiciously clean reversal means checking which binary ran.
- **Remote validation dirs must be source-exact.** Copying only `git ls-files` into a reused checkout leaves untracked runners behind — three stale ones grew a 140-test suite to 143 and produced an impossible failure. Matching hashes for tracked files does not catch it. Create a fresh source dir (or diff its inventory against `git ls-files`), share build artifacts through an explicit target dir, and preserve old dirs under an `_old` name. Never combine an excluded `target` with `rsync --delete-excluded` — that deletes the cache it appears to protect. Sync each file to its exact destination path; one directory destination for files from several sources plants a plausible stray `mod.rs`.
- **`python str.replace` silently no-ops on a missed needle** — assert the needle is present before writing.
- **`exec cmd 9>&-` closes fd 9 before `cmd`'s image loads, releasing any flock held on it immediately** — the redirections on an `exec` line are applied in the still-current process, before the execve, not handed to `cmd` to close later. `scripts/ci/with_host_lock.sh`'s old last line, `exec "$@" 9>&-`, released a held exclusive lock in 0.00s on samarch-1 while the four-second `sleep` standing in for the wrapped command still had four seconds left, so every `--shared` build wrap was holding the lock for milliseconds, not the build's real duration — silently defeating the isolation the whole script exists for. Confirmed by removing only the `exec`: a plain foreground `"$@" 9>&-` (which forks `cmd` as a child with fd 9 closed in that child only, while the wrapper process itself keeps fd 9 open until the child exits) holds the lock for the child's entire lifetime instead.
- **A first `git gc --prune` on a repository that has never used cruft packs collects almost nothing, even with gigabytes of unreachable content sitting there.** The cruft pack records an object's "unreachable since" time as the moment that gc run first isolates it, not the object's real commit date, so the default two-week grace period restarts on every repository's first cruft-pack run. Compare `git rev-list --objects --all` (reachable) against `git cat-file --batch-all-objects --batch-check` (everything physically in the packs) to see what a clean-looking `git gc --prune` report is hiding. `git gc --prune=now` collects it immediately, but only after checking `git fsck --unreachable` and the recent reflog for anything still worth keeping — this repo's own rule is always stash, never reset, so dropped-stash content should be assumed worth keeping until checked, not assumed disposable because it is unreachable.

- **`read` with `IFS=$'\t'` silently drops empty fields.** Tab is IFS *whitespace*, so bash collapses a run of tabs into one delimiter: a record like `a\tb\t\t/path` parses as three fields, shifting `/path` into the third variable and leaving the fourth empty. A loop that guards with `[ -n "$last_field" ] || continue` then skips exactly the records whose optional field was empty, which reads as "nothing matched" rather than as a parse bug. Emit a sentinel (`-`) instead of an empty field.
- **`du` exits non-zero when a file disappears under it**, which is constant in a `target/` dir with a live cargo. Under `set -o pipefail` that failure propagates out of `size=$(du -sk "$d" | awk ...)` and, with `set -e`, aborts the enclosing sweep partway through — silently truncating a listing that looks merely short. Wrap it: `{ du -sk "$d" 2>/dev/null || true; } | awk ...`.
- **`ls` on this Mac is an alias for `eza --icons` and can print absolutely nothing** -- not even `.` and `..` -- in a directory that is fully populated, while `pwd`, `git`, and `cargo` in the same shell all work. The Bash tool loads the user's profile, so the alias applies to agent shells too. An empty listing therefore says nothing about whether files exist; it is a lie that looks exactly like a deleted tree, which is a terrifying thing to see mid-cleanup. Confirm with `echo *`, `find . -maxdepth 1`, or `/bin/ls` before believing it.
- **macOS has no `flock(1)`.** `host_capacity_lock_available()` therefore returns false and the whole host-capacity lock is a no-op on the Macs; it only ever gates samarch-1. Do not reach for it to answer "is a build running here" on a Mac — use file mtimes.

## Cargo, build and test gates

- **`cargo test --workspace` starts the ~129 test binaries sequentially** (several shell out to rustc or stand up a wgpu device) — over an hour on an M5 with the box idle. The gate is still `just test` (`cargo test --profile ci --workspace`, what CI runs); `cargo nextest run --workspace` runs the binaries in parallel and is worth it for an exploratory full pass on Linux — see the macOS caveat below. Either way, start it in the background and do other work; a backgrounded command notifies on exit and a poll loop adds only latency and noise.
- **A `cargo test` alive for an hour is deadlocked, not slow**, and it holds the build lock so every later check/clippy waits silently behind it. Kill it first. `sample <pid>` names the test: its own function in a thread title with a `Condvar::wait` under it. The cause seen here is two tests sharing the process-wide `BlockingPool` in parallel — prefer `BlockingPool::new()` per test in anything asserting on pool growth. If instead every hung process sits at 0% CPU inside `_dyld_start`, that is macOS serializing code-signature validation of the ~129 unsigned debug binaries nextest launches at once to enumerate them; cap it with `-j 4` rather than debugging the suite.
- **Concurrent GPU tests deadlock the NVIDIA driver.** Test threads that each build their OWN `VkInstance`+`VkDevice` intermittently hang with two+ devices live in one process — one thread spinning in the driver's userspace fence-wait (wchan 0, 100% CPU) while another sleeps on a kernel rt_mutex. Killing the harness task orphans the test binary, which keeps burning a core (eight accumulated over hours). Tests share one process-wide `VulkanContext` behind a Mutex, and every submit waits on a fence with a 10s timeout instead of `queue_wait_idle`, so a stall is a clean `GpuTimeout`. Run GPU suites as `timeout -s KILL 120 <test-binary>` with timeout parenting the binary DIRECTLY (build first with `cargo test --no-run`), and afterwards check `ps -eo pid,etime,pcpu,args | grep cranpose_render`.
- **`cranpose-render-wgpu --test pass_timing_report` can SIGSEGV instead of hanging when samarch-1 is oversubscribed** (seen at load average 93 on 12 cores, swap in active use, `lsmod | grep nvidia` empty — the NVIDIA kernel module was not even loaded, so wgpu was on a software/Vulkan fallback). Confirmed environmental, not a regression, the fast way: clone plain `origin/main` to a second directory and run the identical `cargo test --profile ci -p cranpose-render-wgpu --test pass_timing_report` there — it crashed the same way with zero relation to the change under review. Cheaper than bisecting: check `uptime` and `lsmod | grep nvidia` before spending time on a GPU-test crash on this host.
- **Do not launch parallel `cargo test` commands in this workspace** — they serialize on cargo's package locks and only add noise. Parallelize file reads, not builds.
- **`cargo test --lib` skips doctests**, and `cargo test -p cranpose --doc` alone invents a featureless `AppLauncher` without `try_run`. The gate that matches CI is `cargo test --profile ci --workspace`.
- **Nothing local compiles the browser except `apps/desktop-demo/build-web.sh`.** `cargo test/clippy/check --workspace` all build the host target; v0.1.91 published with `cranpose-render-wgpu` broken for wasm32 and it surfaced from a Pages deploy. Both causes have a greppable shape: an argument added to `init_gpu` with every host caller updated except `web.rs`, and a `#[cfg(not(target_arch = "wasm32"))]` field read without the gate its siblings carry. Run `./build-web.sh --fast` before every tag. `gh pr merge --admin` bypasses the `wasm build (linux)` job, which is exactly the check for this class.
- **Repository rustflags diverge from what consumers compile with.** `--cfg web_sys_unstable_apis` (once in `.cargo/config.toml`) is not additive: `MouseEvent::offset_x` is `-> i32` without it and `-> f64` with it, so v0.1.96 shipped four `E0308` to every consumer while CI was green — and clippy, running under the same flag, called the compensating cast `unnecessary_cast` and had it removed. Diff the rustflags, not the source: reproduce with `RUSTFLAGS= cargo check --target wasm32-unknown-unknown` from a tree with no `.cargo/config.toml`, or from `apps/isolated-demo` with a `[patch.crates-io]`. Never trust a clippy cast suggestion in `cfg`-divergent code until clippy runs without the repo's flags. `no_cargo_config_enables_unstable_web_sys_bindings` in `apps/desktop-demo/tests/source_hygiene_aliases.rs` fails if the flag returns.
- **`cargo check --target wasm32-unknown-unknown` is not a web build** — it dies early on `getrandom`'s "wasm_js" `compile_error!` because the wasm RUSTFLAGS come from `build-web.sh` (wasm-pack), not from bare cargo. To verify a browser rendering bug: `build-web.sh --fast`, then drive the cached Playwright Chromium (`~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome`) with `playwright-core` and `--use-gl=angle --use-angle=swiftshader --enable-unsafe-swiftshader`, screenshot `#cranpose-canvas`, and measure the white-pixel fraction (blank first frame ≈0.99, rendered ≈0.01). Hook `window.requestAnimationFrame` from an init script to prove the loop stops spinning when idle (~0 frames over 2s).
- **`build-web.sh` must run with `set -e`** or a failed `wasm-pack` prints the success footer and reports the previous `pkg/desktop_app_bg.wasm` size.
- **`cargo clippy` cannot verify a consumer against unreleased framework crates.** `cargo check` honours `--config <patch.toml>` by reusing a working `Cargo.lock`; clippy re-resolves against the registry and fails on features the published version lacks. `--offline` does not help, and swapping to a `path` dependency stops the patch table applying to the *other* cranpose crates. Use `cargo check` locally; clippy on that consumer waits for the release.
- **Check the facade before adding a framework crate to a consumer** — `cranpose` already had `media-desktop = ["dep:cranpose-media"]` and the desktop shell already called `cranpose_media::install()`; the hand-rolled direct dependency on an unpublished crate is what broke resolution.
- **`cargo fmt --all` does not reach `apps/isolated-demo`** (its own workspace). `just fmt`/`just fmt-check` run a second pass with `--manifest-path apps/isolated-demo/Cargo.toml`.
- **`cargo doc --workspace` collides**: `desktop-app` and `desktop-app-platform` both produce a lib named `desktop_app` ("document output filename collision"). The doc gate excludes both plus `xtask`.
- **Do not set `CARGO_TARGET_DIR` for `xtask binary-size`/`dist-min`** — they resolve the binary from the manifest dir, so the build succeeds and the gate dies with `failed to inspect .../release-small/isolated-demo: No such file`.
- **Never invent a feature subset to check a target; run exactly what CI runs.** `--features ios` without `renderer-wgpu` gates `ios.rs` out and invents three dead-code warnings that exist in no shipped build.
- **Use the exact test name.** `cargo test -p desktop-app leetcodedaily_fps_perf_gate_...` built for minutes and ran zero tests; the real guard is `heavy_shell_entrypoints_use_local_resource_guards`.
- **Keep Vulkan on desktop, whatever the dependency graph looks like.** A no-Vulkan WGPU graph tidies the tree, but `robot_renderer_micro_contract` then fails on X11 with no compatible adapter (GL is not compatible with the provided surface). Never trade the tested renderer path for a dependency count.
- **Anything parsing cargo output must pass `--color never`, or it works locally and corrupts in CI.** `CARGO_TERM_COLOR: always` is set workspace-wide in `.github/workflows/rust.yml`, and cargo honours it even when its stdout is a pipe — while locally the same pipe auto-disables colour, so the bug is invisible on every developer machine and on every host. A nested `cargo tree` line arrives as `ESC[2m│ESC[0m   ESC[2m└──ESC[0m thiserror v1.0.69`, which does not start with a box-drawing character, so a "is this a root line?" check waves it through and the package name comes out carrying the tree drawing. The symptom is a green local gate and a CI failure naming families like `│   └── thiserror`. Reproduce with `CARGO_TERM_COLOR=always <the gate>`; do that before believing any parse-cargo-output gate.
- **`cargo tree --duplicates` answers for the HOST only, so a duplicate-dependency verdict read off it is worthless.** The budgets job runs on Linux, which is why an empty allowlist coexisted for months with `just dep-budget` failing on every Mac over an objc2 0.5/0.6 split. `cargo xtask dependency-budget` pins all ten shipped triples and is the only verdict; its `--explain` output is byte-identical on macOS and Linux. Add a shipped target to `SHIPPED_TARGETS` in `xtask/src/main.rs` or it is unguarded — architecture counts, not just OS (`windows_x86_64_msvc` is its own family).
- **The same cast hazard exists across TARGETS, not just rustflags, and an autofix will take it.** The rustflags entry above has clippy removing a compensating cast under `--cfg web_sys_unstable_apis`; the target-width version is `libc::timespec::tv_sec`, `i64` on the 64-bit Android ABIs and `i32` on `armeabi-v7a` and `x86`, all four of which `releaseAbis` ships. An arm64-only lint run calls the widening `as i64` an `unnecessary_cast`, and `cargo clippy --fix` removes it — a type error on two shipped ABIs, with the arm64 run green afterwards and nothing failing to reveal it. It was caught only by reading the autofix diff. **An autofix is a code change proposed by a tool that saw one target**; read every hunk for assumptions the checked target happens to satisfy. Lint the ABIs you ship (`just clippy-android` covers all four), and note that this field has no lint-clean spelling — `as i64` trips `unnecessary_cast` on 64-bit, `i64::from` trips `useless_conversion` there, and dropping the widening breaks 32-bit — so a scoped `#[allow]` stating the intent is the answer, not a rewrite.
- **A lint can be a false positive on one target and silent on another; suppress it at the recipe, not at sixteen items.** `missing_const_for_thread_local` fires 16 times for `*-linux-android` and zero times on host, on the same pinned toolchain and the same source, including on initializers already written `const { ... }` and on one that cannot be const at all (`HashMap::default()`). `thread_local!` expands per-target and the Android expansion defeats the lint's const detection. Sixteen scattered `#[allow]`s would also leave sixteen comments claiming code is not const when it plainly is; allow it on that target's recipe alone and leave the lint live everywhere else.
- **Do not go hunting for the upgrade that collapses a duplicate family; check whether one exists first.** All nine recorded families are pinned by crates already at their latest published release, so `cargo upgrade` moves nothing: `accesskit_macos 0.26.3` holds `objc2 0.5` deliberately (upstream will not merge the 0.6 bump until winit 0.31 ships stable, exactly so winit users avoid two objc2 stacks), `ndk 0.9.0` holds `thiserror ^1` and `jni-sys ^0.3`, `winit-win32`/`arboard` hold older `windows-sys`. Each is recorded with its unblock condition in `WORKSPACE_DUPLICATE_DEBT`; the gate fails when a recorded split disappears, so the table cannot go stale.

- **A linker warning never reaches `-D warnings`, so the workspace denies it.** `linker_messages` is a rustc lint on the *link* step: `just clippy` emits metadata and never links, so only `cargo build`/`cargo test` ever see one, and at its default `warn` level nothing turns red. `[workspace.lints.rust]` sets it to `deny`, which is what makes a linker warning fail a build; `the_workspace_lints_deny_linker_messages` and `every_workspace_member_inherits_the_workspace_lints` in xtask hold that line, the second because a member without `[lints] workspace = true` receives none of the workspace lint table. macOS `ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table` is the shape that fires it — on `desktop-app` and on `cranpose-ios`, both linked unoptimised — and it is a code-volume symptom, not a profile or feature-set one: `desktop-app`'s defaults are `renderer-wgpu,desktop-http`, and `panic = "abort"` on `[profile.ci]` buys a second copy of the whole graph (`cargo test --workspace` goes 551 → 765 units; measure it in seconds with `cargo +<nightly> test --profile ci --workspace --no-run -Z unstable-options --unit-graph`). Attribute it instead of guessing: link once under `RUSTFLAGS="-C link-arg=-Wl,-map,/tmp/x.map"`, take the `__eh_frame` address range from the map's `# Sections:` block, bucket the `# Symbols:` rows inside it by their `[N]` object index, and demangle with `rustfilt` (`cargo install rustfilt`; strip the leading Mach-O underscore first). That put 57% of a 21.4 MiB `__eh_frame` in the demo crate's own rlib and named the one shape responsible.
- **A macro that emits `get_or_init(|| ...)` monomorphises the whole `OnceLock` chain once per expansion.** `OnceLock::get_or_init` is generic over its initializer, so an initializer closure written into a proc-macro expansion carries a private closure type to every call site, and rustc codegens `get_or_init`, `get_or_try_init`, `initialize`, `Once::call_once_force` and the `FnOnce::call_once` vtable shim for each. `#[composable]` emitted one per composable and `branch_groups` one per branch guard: 152,446 of `desktop-app`'s 214,205 functions (71%) and 15.25 of its 24.56 MiB of `__text`. Outlining the initializer into non-generic `cranpose_core::cached_composable_definition_key`/`cached_branch_location_key` — the `static OnceLock` stays at the expansion site, only the closure moves — cut `__eh_frame` 21.37 → 11.30 MiB, `__text` 61.0 → 42.4 MiB and the `ci`-profile binary 268 → 155 MiB. The guards are the two `*_do_not_monomorphise_the_once_lock_initializer` tests in `cranpose-macros`. **Any generic std API called from macro-emitted code is a per-expansion multiplier** — pass values to a non-generic helper instead.

- **sccache with incremental compilation enabled caches nothing at all.** Setting `RUSTC_WRAPPER=sccache` while a profile asks for incremental does not fail; it produces `Non-cacheable calls` for every compile and a 0% hit rate. `[profile.robot]` sets `incremental = true` and `[profile.ci]` inherits `dev`, which enables it by default, so this workspace hits it by default. `CARGO_INCREMENTAL=0` overrides the profile setting and is what `enable_local_sccache` exports; check `sccache --show-stats` for `Non-cacheable reasons: incremental` before believing a cache is working. (Env `CARGO_INCREMENTAL=1` is a *hard error* — "incremental compilation is prohibited" — while the profile setting only degrades silently. Same cause, two very different signatures.)
- **`could not execute process target/<profile>/deps/<test>-<hash>` / `No such file or directory (os error 2)`, after other suites passed, is something deleting the target dir mid-run** -- not a build bug and not a flaky test. cargo listed the binary and then could not exec it. This machine runs many agent worktrees at once and more than one of them sweeps build artifacts, so a foreign `cargo clean` is a live hazard. Note `cargo test` writes nothing to `target/` while it *runs* binaries, so an mtime "is a build active" check goes blind for the whole execution phase; `target_gc.sh` protects on live processes (`pgrep -f <worktree>`) for exactly this reason.
- **`git merge-base --is-ancestor` answers NO for every squash-merged branch.** This repo squash-merges, so a branch whose work shipped is not an ancestor of `main` — `docs/frame-cost-attribution` landed as 6fca42ed and still answers NO. Anything keyed on "is this branch merged" (cleanup sweeps, staleness reports) will essentially never fire. Ask `gh` if you need the truth, or key on something else entirely.
- **A shared `CARGO_TARGET_DIR` serialises concurrent builds.** The second cargo blocks on `Blocking waiting for file lock on build directory` until the first finishes. It is not a way to deduplicate worktree target dirs when agents build in parallel; it converts a disk problem into a throughput one.
- **Worktree `target/` dirs are the largest thing on this machine and nothing used to collect them.** Roughly 350GB across ~20 worktrees filled a 926GB disk to 117MB free and hard-stopped an agent whose shell could no longer create a file. Incremental caches were 37.3G of one 80.8G target. `just gc` reports, `just gc-apply` reclaims, and `_disk-guard` runs ahead of the heavy recipes. See `scripts/dev/target_gc.sh`.

## CI, runners and signing

- **`Swatinem/rust-cache` prunes `~/.cargo/bin` on save** — on a self-hosted runner that is the host's own toolchain, so a green job can leave the machine with no `cargo`. Signature: `cargo-fmt`, `cargo-clippy`, `clippy-driver`, `rust-analyzer` still present, the rustup proxies (`cargo`, `rustc`, `rustdoc`, `rustup`) gone, `~/.rustup/toolchains` untouched. It does the same to `~/.cargo/registry` (a half-deleted crate failed a wasm build with `failed to read Cargo.toml: NotFound`), so `cache-bin: false` patches one directory rather than fixing it. Gate the action with `if: runner.environment == 'github-hosted'` — a self-hosted `~/.cargo` persists between jobs anyway. Cranpose carries none since #338 and cranamp is gated as above since #53, so check the app repos. Repair without disturbing toolchains:
  ```bash
  curl -sSf https://sh.rustup.rs -o rustup-init.sh && sh rustup-init.sh -y --no-modify-path --default-toolchain none && rustup default stable
  ```
- **macOS codesign "unable to build chain to self-signed root" / `errSecInternalComponent` means the intermediate is missing.** A `.p12` carries the leaf and its key, not the CA that links it to Apple Root. Check before theorising: `security find-certificate -c "Developer ID Certification Authority" ~/Library/Keychains/login.keychain-db` — finding it only in `SystemRootCertificates.keychain` (the anchor store, not a searched keychain) is the failure. `security find-identity -v` lists the identity as valid on a host that cannot sign, so a guard built on it passes and codesign fails later looking like a bad key. Fix in the job, not on the host: fetch `https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer` and `security import` it into the ephemeral signing keychain, pinned by SHA-256 (an unpinned CA download is a real hole).
- **A clean pass/fail alternation across releases is not automatically leaked state** — six alternating releases sent an hour into a leaked keychain search-list entry that was real, worth cleaning, and not the cause. Re-run the identical commit with the suspect removed first.
- **crates.io and the GitHub API answer 403 to a request with no `User-Agent`**, and it reads as "crate absent" / "release missing". A release monitor reported `cranpose = ABSENT` for published crates. Send a UA from every script; re-check with `curl -A` before believing a lookup.
- **The GitHub runner `.env` parser chokes on `#` comments** and a parse failure stops the runner starting at all. Document beside the runners instead (`/Volumes/files/actions-runners/README-caches.md`).
- **`gh` has more than one account here and the active one flips.** A repo 404ing or "must have admin rights" on a rerun → `gh auth switch --user samoylenkodmitry`.
- **`paths-ignore` on a required check is a permanent merge block, not a fast pass.** A filtered-out workflow never reports its check at all, so a job that is a required status check sits pending forever and the pull request becomes unmergeable rather than quick. Short-circuit inside the job instead — run it, read the diff, skip the steps — which costs seconds of runner time and survives branch protection being switched on later. `main` currently has none and its ruleset is `enforcement: disabled`, so this is a trap that springs on whoever enables one.
- **`**/*.md` does not match root-level markdown.** GitHub's `**` matches any characters including `/`, but the literal `/` after it still has to match something, so `AGENTS.md` and `TIME_WASTERS.md` at the repo root fall straight through a `paths-ignore: ['**/*.md']` and run the whole board. Match `*.md` against the entire path instead — that is what `scripts/ci/docs_only_change.sh` does.
- **`git diff --name-only` hides half a rename.** With rename detection on, a `.rs` renamed to a `.md` reports only the `.md` destination, so any "docs-only" predicate reads it as prose and the robot suite silently stops running while the board stays green. `--no-renames` lists both sides. `just test-ci-filters` pins this case and replays it against an inverted-predicate mutant, because asserting `false` proves nothing when the fail-safe branches also answer `false`.

## Release and publish

- **Pushing an annotated `vX.Y.Z` tag at `main` HEAD is the whole release.** `publish.yml` rewrites the versions itself, commits `release: vX.Y.Z` to `main`, moves the tag onto that commit, publishes, then points the isolated demo at the new version. Do not bump by hand. A **manual `workflow_dispatch`** takes the other branch, where `main` must *already* contain the release metadata.
- **The tag must be at `main` HEAD when the workflow READS it**, not when you created it — merges land continuously and three releases hit "Tag vX.Y.Z must be created from main HEAD". Recovery is cheap and is *not* a manual dispatch: `git tag -f -a vX.Y.Z <commit>`, `git push -f origin vX.Y.Z`. The version rewrite is idempotent, so the second run short-circuits at "No version changes needed" and publishes. A queued Publish is not proof it is fine.
- **Cancelling `publish.yml` mid-flight leaves `main` bumped, crates.io old, and the tag on the pre-bump commit** — which is no longer HEAD, so re-pushing it fails the same check. Same recovery: move the tag onto the `release:` commit and let the tag-push path run again.
- Watch for a branch that hand-bumps the workspace version: harmless while the numbers agree with the tag, a conflict the moment they do not. Deploy-Pages can be cancelled by the tag move and then needs `gh run rerun`.
- **`apps/isolated-demo` is the only tree shaped like a consumer** and until the publish canary built it, the "proves a release is consumable" check never ran. It hid a Gradle plugin defaulting `releaseProfile` to `release-fast` — a profile only *this* repo declares — so any app applying `dev.cranpose.android` failed its own release build. It compiles against the *published* `cranpose`, so between a framework change and its release it will not build against the pinned version; that is version skew a release resolves, not a defect. The Gradle plugin rides inside the crate and has nothing to publish separately.

## Robot harness

- **`robot.screenshot()` re-renders the retained scene offscreen** (`capture_frame_with_scale`) and can NEVER show swapchain/present-path artifacts. For "stale pixels after tab switch", run windowed (`apps/desktop-demo/examples/ghost_presented_probe.rs`) and grab externally with `import -window $(xdotool search --name …)`.
- **`run_robot_test.sh` already waits for a quiet host** — it polls `load_1m` against a ceiling of 0.6x cpu count (7.20 on 12 cpus) and logs `host quiet guard: waiting for the robot suite -- load_1m=8.97 over 7.20 on 12 cpus` until it clears. Do not wrap it in a hand-rolled "wait until the box settles" loop: yours duplicates a guard that already exists, and it is one more armed process that can misfire on a condition nobody is tracking. Just start the suite and let it wait.
- **The suite filters the environment.** Each example starts through `env -i` + `robot_process_env`, which forwards `CRANPOSE_*` and a few platform vars — but not `ROBOT_SHOT_DIR`, so runners that document it silently write to `target/liquid-*` under the suite while a direct `cargo run` honours it.
- **`find_text_in_semantics` is a SUBSTRING match.** A click on "Discover" matched a scrolled-away subtitle and landed on the tab strip; two hours went into hit-testing that was innocent. Use `find_button_exact_in_semantics` for interaction, substring only for presence. Signature: "clicks land nowhere near the coordinates you used" → print resolved bounds before suspecting dispatch. The tab strip also scrolls — use the set-tab hook.
- **Presence probes are tautologies if their band touches glyph ascenders** — they pass forever and hide that nothing rendered. Bands must sit strictly above all other chrome (capsule + halo).
- **Popup presence belongs in semantics, not pixel floors.** White-on-white glass over a page has max channel delta ≈10, under the d>12 diff threshold, so `diff_area` counted only text/icons/shadow (~2400px against a 2500 floor) — a coin flip that lost under load. Keep pixel floors as drew-something sanity only.
- **A failing runner must not `process::exit(1)` from the driver thread** — it races main-thread GPU teardown and dumps core (exit 139), masking the real failure. Set a flag, call `robot.exit()`, return the code from `main`. After handling `RobotCommand::Exit`, return from the event callback immediately: setting `ControlFlow::Poll` later in the same callback keeps the animation loop alive.
- **macOS has no GNU `timeout`** — use the portable one in `run_robot_test.sh`, cap focused debugging with `CRANPOSE_ROBOT_TEST_TIMEOUT_CAP_SECS`, and reuse warm builds with `--skip-build`.
- **Run the suite sequentially.** 16/8/4 parallel workers produced intermittent segfaults and timeouts while sequential passed 80/80; `--parallel N` stays an explicit opt-in.
- **A cold robot build can eat the entire outer timeout without reaching a scenario.** That is an insufficient budget, not a robot failure — prebuild the profile or raise the timeout. Same for `verify_slot_table.sh`/`stress_slot_table.sh` timing out in the robot or perf stage with everything else green.
- **Xvfb presentation is slow enough to fail presented-cadence contracts** (`p95_present_ms≈25-30`, `cadence_fps≈35-45` against 120/150Hz) while `work_fps` stays in the hundreds. CI passes because both robot jobs set `CRANPOSE_ROBOT_SOFTWARE_RENDERER=1`; `run_robot_test.sh` now owns that decision (`CRANPOSE_ROBOT_FORCE_HARDWARE_PERF_CONTRACTS=1` enforces them), so a plain `--sequential` run means what CI means. Judge loop pacing on the cadence-to-`work_fps` ratio, never on cadence alone: the presentation cost is real (`robot_novsync_free_runs` reads ~160fps under Xvfb against ~2500fps on a real display in pure `Poll`, and `CRANPOSE_PACING_DIAG=1` shows `poll=55 wait=0` — 55 iterations a second because each waits out a present).
- **A frame rate from a robot run is not the app's unless the test pins the mode.** `with_test_driver` lifts pacing to `NoVsync` unless the harness called `with_frame_pacing_mode` (setting `frame_pacing_explicit`), so a pacing test starting in the default mode is already in the mode it means to switch to and cannot tell a working control from a dead one. Pin at launch and assert an absolute cap (60fps reads ~60) — "fast" is also what a run does when nothing happened. A driven run no longer pins `ControlFlow::Poll` for its lifetime, so external perf numbers are real and smaller; do not "restore" old figures by re-forcing polling.
- **A "cooldown" is not frame-free in NoVsync/robot mode** — the loop free-runs, so it cannot drain frame-based accumulators.
- **`robot_regression_shader_visual_contract` means something only under the harness.** One-off `cargo run --example …` fails deterministically (bit-identical on two machines) because this non-headless test's drags depend on WM placement; hours went into bisecting provably innocent shader commits. Judge it only through `run_robot_test.sh`.
- **`robot_adaptive_frost` is red on GitHub-hosted CI only** (lavapipe Vulkan): the dark-backdrop scenario reads light, deterministically. NVIDIA Vulkan and llvmpipe GL both render byte-identically and pass; the real-GPU workflow is green. Do not debug through 30-minute CI cycles — install `vulkan-swrast` or run the Ubuntu mesa stack in a container, then chase the headless target/surface format path (`surface_format.rs` prefers non-sRGB, but headless offscreen targets choose elsewhere).
- **Timing-sensitive tests need a deterministic clock, not retry loops.** The headless loop free-runs (~5ms/frame) and scale-3 captures stall 25-60ms, so sleep-choreographed keyframes sample a 55ms tween anywhere including past its end. `capture_keyframes` advances an exact clock atomically.
- **`screenshot_with_scale(3.0)` stalls the driver ~200 ms** — every in-flight keyframe sampled through it is late. Capture motion as one capture per repeated gesture (press → sleep to offset → capture once → release), never several captures in one gesture.
- **Discrete move+capture loops cannot verify continuous-gesture physics.** Interleaving `xdotool mousemove_relative` with `import` inserts ~150ms per capture, and any tracker catches up during the pause — the loupe-follow bug shipped "verified" this way. Run the mouse stream in a background loop and capture concurrently, logging `getmouselocation` before each frame.

## Perf measurement

- **Check idle fps FIRST** when an app feels laggy: `CRANPOSE_GPU_STATS=1`, count `[GPU f#N]` lines/sec × 60. Idle must be 0fps and animating content exactly the refresh rate. Robot runs hide this — drivers measure throughput and the fixture idles between events. An always-mounted `rememberInfiniteTransition` and a NoVsync default once produced 477fps at idle on a 60Hz panel.
- **Run GPU perf scenarios one at a time**; parallel scenarios contend for the same GPU/driver state and the numbers are meaningless.
- **Diff two `perf record -p <pid> --call-graph dwarf` windows, early vs late,** when every counter is flat but frame work grows monotonically (18% → 51% of self-cost named the snapshot record chains in minutes — every state READ walks the chain via `readable_record_for`). The robot binary keeps its symbol table, so no rebuild is needed.
- **`CRANPOSE_GPU_STATS=1` also names cache pathologies directly** — `shadow_cache: shape_miss=15 miss_px=14.20MP` plus ~58MB/frame transient offscreens at scale 1.354, zero at scale 1, pinned shadow keys anchored to floored device-pixel bounds (device subpixel phase leaking into the content hash; floor/ceil flapping multiplying entries).
- **Slot-table stability:** keep each baseline adjacent to its same-benchmark comparison, leave CPU pinning opt-in, use fixed-step batches so Criterion does not compare different fixture states, keep two unrecorded warmup passes before recording, and retry noisy same-tree pairs before editing production code. A full-matrix failure whose isolated `--filter` rerun passes is host-noise evidence. Keep `stress_slot_table.sh` inside its 600s guard by shrinking the work, not by opening the default path.
- **`perf_robot_heap.sh --tool massif` on the wgpu harness is dominated by Vulkan/WGPU device and pipeline setup** — useful for RSS sanity, useless for isolating hot-path allocations.
- **A 10+ minute `release-fast` robot probe is a link/profile cost** unless process inspection shows exported-library targets (`desktop-app-platform`) or unrelated packages in the build; `desktop-app` owns rlib/bin desktop work.
- **Do not smooth wheel deltas in `cranpose-ui::modifier::scroll` to fix scroll fps.** A wheel animator dropped steady cache hits to zero, re-rendered a 1425x1365 isolated layer every sampled frame, and took fps to 12.3. The fix is retained translated/offscreen surface reuse, not more scroll-frame callbacks over an uncached capture.
- **Android on-device scroll perf: drive one long drag, and judge only `idle_iters≈0` windows.** Repeated `adb shell input swipe` calls leave a ~300-400ms gap between commands while nothing animates, which lands in `debug.cranpose.frame_telemetry` as `period max≈350ms` and fps in the 40s and reads exactly like a hitch. It is idle, not jank — those windows also report high `idle_iters` and no per-stage max anywhere near the gap. Use a single `input swipe ... 2500`-style drag per window instead.
- **`dumpsys SurfaceFlinger --latency` cannot see content jank on Android** — it reported steady 60fps presents while the app was visibly stuttering, because an animation or catch-up present keeps the buffer queue fed. The per-stage frame telemetry is the signal. (`screenrecord` also does not exist on the EMUI 10 build used for this.)
- **iOS: prefer in-process telemetry to Instruments** for repeated profiling — a detached `xctrace` recording can leave the phone listed offline while CoreDevice still talks to it. Launch via `devicectl --console` with `CRANPOSE_GPU_STATS=1`; when Instruments is needed, use a unique `.trace` path (it will not overwrite).
- **The diagnostic you need is probably already printing — check before building one, and check what your own filter is dropping.** #500 was about to get a new per-layer renderer diagnostic built for it. `gpu_stats` had been printing one line per isolated layer all along (`gpu_stats.rs`: node id, rect, target size, isolation reasons); it was missing from the captures only because they were read through `grep "GPU f#"`, which keeps the summary line and discards everything under it. Telemetry read through a grep shows you only what you already thought to ask for. Read one raw capture end to end before adding instrumentation.
- **A cache miss rate can hide two problems needing opposite fixes, and the aggregate counters cannot tell them apart.** #500's glass page reads `layer_cache 34.6% hits`, which sounds like one broken cache. Per frame, `hit` equals `isolated_layers` exactly (5/5 and 9/9, with 9 and 17 misses): each layer's *source content* caches fine and only the *backdrop effect* is at ~zero, so the headline number sends you after the half that already works. The backdrop half then splits by topology, and the halves want opposite fixes — glass **moving over** its own backdrop samples unchanged content through a translating window, so a shared background blur fixes it; **fixed** glass over scrolling content has a constant `capture_rect` over genuinely different pixels every frame, so no caching strategy can work and only a cheaper blur helps. `isolated_layers` 5-9 and `blur` 3-7 fit either story; the per-layer lines settle it — a node present in every sampled frame is fixed chrome, one that comes and goes is per-row glass. On that page: 4 fixed, 1-4 transient, and the fixed chrome is **89% of the isolated-layer area**. The cacheable case was a tenth of the cost, so the shared-texture backdrop cache that the issue and two readers all recommended would have been built, been correct in itself, and moved almost nothing. Cross-check any such attribution against the reported `area=`: summed rects gave 0.66 MP at scale 3 against a reported 0.67 MP, an agreement that was not fitted.

## Visual verification and judging

- **Measure, never eyeball.** A loupe magnification read off a 160%-scaled crop as "1.7x" was a uniform ~1.25x — the dome, plateau and baseline arc engineered around the wrong number all became unnecessary. Same for a bubble "misplacement" hunt: one pixel scan of the reference (bubble center vs cell center, width vs pitch) settled the contract in minutes.
- **Vision judges find WHERE to look; numbers decide WHAT is true.** Across six rounds judges measured a shadow halo as capsule height, inverted the prose spec, called a risen bubble "glued to the baseline" from a crop framing, and flip-flopped between rounds. Every mismatch that survived was confirmed by scripted pixel measurement; every fix applied on a judge's word alone was wasted.
- **Judge raw frames, never composed TARGET|ACTUAL strips.** The downscale smears sub-pixel optics and mutes saturation: a reference-matching lens read as "milky capsule washing the label out" and `(0,217,234)` electric cyan read as "dull teal". Crop/zoom the raw capture and pixel-probe with magick.
- **Capture glass optics at `CRANPOSE_ROBOT_CAPTURE_SCALE=2`+.** The refraction band is `inradius * refraction_depth` ≈2-4px at scale 1; with ≤2 intermediate SDF pixels, curve, dispersion and zoom are byte-identical no-ops that look exactly like a dead shader path. Reference recordings run ~5.4 px/dp.
- **Crops must share an internal anchor.** Equal-size crops are still invalid if the component sits at a different Y in each — a lens was mismeasured as 48dp instead of 39dp over a 26px track-center offset. Align a stable feature (track center, bar top, text baseline), keep native device scale, then measure. Shadows and white-on-white glass edges are not anchors.
- **Score the exact tile the contract uses.** A hand-cropped frame that looks identical at normal zoom placed artwork eight pixels off from what `extract_reference_tiles` consumes, and tuning against it made placement measurably worse. Extract from the checked-in target sheet with the same band/column registration and confirm the two candidates by pixel comparison first.
- **Derive capture bounds from one semantic stage owner** and express odd native dimensions as physical-pixel ratios (`400.0 / 3.0`), then assert output dimensions before tuning optics — mixed origins (`tab_center - 70dp` vs exact stage bounds) gave a repeatable 4-6px shift at 3×, and a 132dp stage for a 1320×400 reference produced a silently resized 1320×396 capture.
- **Re-capture after the last code change and crop generously.** Two judge rounds were spent on artifacts of the captures: a 45px crop cut a lens bottom, and one set predated the fix it was judging.
- **One experiment = one uniquely named capture dir + a grep'd state echo beside it.** Never mv/reuse dirs mid-bisection; shuffled stale dirs produced false "the knob does nothing" byte-diffs. Byte-diff two captures before reasoning about any knob.
- **A zero-valued correction is not a no-op in a modifier layout engine** — installing `.offset(0, 0)` for every tab changed raster rounding enough to move a strict RMSE across its gate. Omit the modifier node when it is exactly neutral.
- **Render the scene with the component REMOVED** when an artifact survives every disable experiment: a suspected "crown above the bar" was the tile artwork's own gradient seen through translucent glass.
- **Diff experiments on a dirty pre-alpha tree need env-var kill-switches, not `git stash`** — a stash swallowed 70 files of an uncommitted session and `pop` rewrote mtimes, destroying the forensics.

## Debugging method

- **Probe ladder beats reading code.** A Text color change updated the element, the measure pass and the modifier-slices snapshot while the screen kept the old color: the break was the last cache before the GPU (TextService `prepared_cache` keyed on `measurement_hash`, deliberately color-blind, storing the full visual style). Drop eprintln probes at the five stations — element update → node measure → `modifier_slices_snapshot` → scene builder → renderer cache — run once, and the first station still printing the OLD value brackets the bug. `CRANPOSE_RENDER_PHASE_DIRTY_DIAG=1` prints per-frame rebuild paths and dirty node ids for free.
- **Confirm node identity before any staleness theory.** Hours went into "stale popup layer bounds" for diag lines that belonged to in-page glass cards; the popup's own plan lines were correct the whole time. Match the diag rect against the widget's real spec size AND its window position. `sort -u` over a whole-run diag log mixes stages and frames — filter to the frames around one capture first.
- **Suspect the guard when a fast path has an equality check against a separately computed value.** An off-by-one silently disabled a whole material pipeline: the fallback "worked" (no error, plausible blur) so nothing logged. Probes that cracked it, in order: pure-red tint (is the material visible at all?), shader silhouette probe (where does the SDF land?), plan-size vs backdrop-size diff in `CRANPOSE_BACKDROP_DIAG=1` (`copied=false` every frame).
- **Measure the displacement before theorising about a transform.** Reading the underlay path end to end argued the code was correct; two background lines through the glass gave scale 0.688 about a fixed point, identifying a *rect ratio* rather than the 1.03 lift everyone suspects.
- **Count silent returns.** A silent `return` in a draw path must either count as unsupported or be provably legitimate — that is how the layer-local clip bugs surfaced.
- **Gates before handing a build to the user.** A slim-renderer build validated only under Xvfb at scale 1.0 reached the user's 1.354 desktop rendering an 800x600 scene in a 1083x812 window (root_scale never reached scene build) and froze with silent per-frame errors. Run at the user's real scale (`WINIT_X11_SCALE_FACTOR=1.354`), interact (switch tabs, resize, minimize/restore), build with `logging` enabled and read the log, and compare against the reference build side by side on the same display.
- **The demo release binary has no logger** (`logging` feature off): every `log::error!` in the render/present path is compiled out and errors are structurally invisible. Build with `--features desktop,logging` or use env-gated eprintln diags.
- **A guard that fails open is usually right, which is why it gets believed.** It agrees with a correct guard on every run where nothing is anomalous, so it accumulates a track record before it is ever wrong, and the one run it is wrong on arrives carrying that history. When #508 merged, the already-discarded grep-for-"pending" waiter fired "CI COMPLETE" at the same moment the hardened gate did — correct, for the last time, having been wrong earlier that evening on the same PR. This is a worse property than a guard that is simply broken: broken fails loudly on the first run and gets fixed. Four sessions in one evening believed four such guards. Treat "it has agreed with reality so far" as no evidence at all, and read the guard's *logic* rather than its track record.
- **A guard that reads remote state must pin every input its own environment supplies.** The CI gate above returned "no rollup data" — correctly not-green — the first time it ran from a scratchpad directory, because `gh` resolves the repo from cwd and had none. Written the natural way, treating a `gh` error as "nothing pending", it would have declared a fully-queued PR green on its first invocation. Pin `--repo`; the flipping-account entry under CI is the same hazard by another route, and `--repo` does not cover it.
- **Minimal isolated probes beat pipeline forensics.** A `lens_probe` example over a two-tone background plus shader-output dumps (`return vec4(uv,0,1)`, uniforms as colors) gave the ×1.354 ratio immediately, after hours lost on composite-path forensics that was innocent throughout.

## Renderer and framework gotchas (fixed — do not reintroduce)

- **Unify surface planning before chasing text crispness.** Scroll wobble, decorated-text breakage and WGPU OOM were one bug: restarting translated stable-capture boundaries inside already-isolated surfaces.
- **Washed-out color is a FORMAT bug, not a palette bug.** All four shells preferred an sRGB swapchain while the pipeline passes sRGB bytes through, so hardware re-encoded them ((52,199,89)→(125,229,160)). Sampled images looked right (texture decode cancels it), which framed it as "gradients look off". If solids are wrong but images are fine, check `surface.get_capabilities().formats` FIRST. `robot_color_fidelity` pins byte-exact output — and beware tests that encode the bug as truth (the micro-contract's expected RGB used to apply linear→sRGB on purpose).
- **Anchor raster cache keys to unfloored bounds** (quantized to 1/16 device px) with translation-stable surface sizes, or device subpixel phase leaks into the content hash at fractional scales.
- **Pack shader geometry in dp with the container = node size dp** (the shader derives px-per-dp per axis from the injected node pixel rect). Packing dp×`current_density()` while captures render at root_scale 1.0 puts geometry 1.354× off — and looks right only when scales match by accident. Guard: `morph_glass_packs_dp_geometry_with_node_size_container`.
- **`Modifier::size()` COERCES into the incoming constraints.** Any overlay that must exceed its parent (lenses, badges, glow halos) needs `Modifier::required_size()`. Signature: an effect "cropped/mis-anchored on one axis only" by exactly the parent's max constraint.
- **Restore full viewport+scissor after any composite that shares a pass.** `draw_prepared_shader_src_over` set a sub-viewport for its dest rect without restoring, so every later draw in the chunk was remapped into it and vanished. Guarded by `robot_regression_fused_viewport_contract`.
- **Per-primitive graph clips are LAYER-LOCAL** (`primitive_emit::resolve_primitive_clip`): intersecting them as world-space silently drops all clipped text in translated layers, and reassigning to world-visible then re-mapping double-applies the transform.
- **Every pending `copy_buffer_to_buffer` source range must be unique until submit.** Reusing offset zero in the shared upload buffer makes earlier copies read the later `queue.write_buffer` payload — stable geometry with a grey/dark corrupted full frame. Validate with a full-window color-health assertion, not crop diffs.
- **An underlay must be built and addressed at the same scale.** `magnifying_layer_scale` quantizes a scaling layer's surface UP to the next quarter step, so a parent-scale underlay addressed at child scale shrinks the world behind the glass ~20%. A regression test needs BOTH halves: `NonTranslationTransform` comes from the transform matrix via `direct_translation` while magnification comes from `graphics_layer.scale` via `layer_surface_scale` — a `GraphicsLayer { scale: 1.25 }` beside a plain translation never isolates, so the path under test is skipped. Build with `layer_transform_to_parent(bounds, placement, &layer)`.
- **`liquid_glass.wgsl` is validated only by `cargo test -p cranpose-render-wgpu --lib shader_cache`** — not by `cranpose-ui-graphics`/`cranpose-liquid` tests, and a validation failure does not panic the runners: every glass effect silently disappears (morphs render as full-rect cards, lenses stop following, the loupe never raises). Several unrelated liquid robots failing with "frozen geometry" at once = suspect a WGSL identifier error first.
- **Do not route scroll-translated static text to per-text cached `ImageBitmap` draws.** Upload bytes fell 168-292KB → 32-36KB per frame while rapid scroll regressed 162.4 → 104.1 work_fps, because each text image needs its own texture binding and draw. The direction is retained glyph geometry or a shared atlas batch — and not more lazy-list warmup work either.
- **Popup content re-registers per caller recomposition** (it used to be remembered once, freezing anything animated at first-composition values). Outside-tap dismissal is first-class via `PopupDismissable`; do not hand-roll scrims inside popup content — a child sized past the popup's bounds is culled from hit-testing and never receives taps.
- **Recompose callbacks must survive a direct recompose.** They were consumed per recompose and healed by ancestor promotion the NEXT frame, so a tween's final write — which has no next frame — froze until an arg change resurrected it. Wall-clock runs masked it for the project's whole life, plus an extra ancestor recompose per animation frame everywhere. See `recompose.rs` / the `animation_frame_pump` test.
- **Retarget springs per move with `animate_to_with_velocity`; no rate limiting.** `animateTo` used to reset `last_frame_nanos`, so a spring retargeted every frame integrated dt=0 forever and froze until the gesture stopped. Pinned by `spring_retargeted_every_frame_tracks_a_moving_target`.
- **Robot touch commands must carry `PointerSource::Touch`** or touch-gated UI never arms. And gesture loops must gate Move/Up on a live press — `PointerEventKind::Move` without a preceding press drove `on_drag`, so a hovering mouse moved the caret and the post-release synthesized hover re-armed the loupe.
- **`ForegroundServiceDidNotStartInTimeException` with no service-side log line means the service's `onCreate` never ran** — so do not go looking for a bug inside it. Two callers cause it. A screen-off `am start` delivers started→resumed→paused within 15 ms even on a cold process, so a pause with background work active asks from a state the framework accepts and never honours; and the app's own `stopService` landing inside the startForeground window is a deliberate framework kill ("Bringing down service while still waiting for start foreground"), measured 18 ms after the accepted start when a background-work lease closed right after opening. A "was resumed once" gate does not cover the first — the 3 ms resume arms it. Cranpose defers the ask 700 ms (a resume or the work finishing cancels it) and routes every stop through the service's own obligation handshake; guarded by `the_android_background_service_ask_survives_launch_shaped_pauses` and `..._never_stops_before_start_foreground`.
- **Keep Android Vulkan and GL in separate WGPU instances.** An emulator exposing a Vulkan loader with no usable render node falls through to GL inside the same instance and EGL fails `native_window_api_connect ... already connected to another API` → `EGL_BAD_ALLOC` → `SIGABRT` during the first `Surface::configure`, which reads as an activity-lifecycle failure. Probe Vulkan-only, then drop that instance and build a fresh GL-only one; the GL software path takes ~20s to initialize, so wait for `Rendering initialized successfully` before calling a black frame a failure.
- **`#[composable]` cannot be used in cranpose's own doctests unaided** — `proc_macro_crate` reports `FoundCrate::Itself` and emits `crate::Composer`, pointing at the doctest's empty crate root. Guide examples under `crates/cranpose/src/_docs` carry a hidden `# use cranpose::{...}` block and declare their own `fn main` (rustdoc's wrapper keeps the prelude glob from reaching the crate root). A downstream reader needs neither.
- **Fabricated proc-macro tokens carry the macro crate's edition**, not the spans': synthetic let-chain nodes with user-file spans still fail "let chains are only allowed in Rust 2024" while the proc-macro crate is 2021. Passed-through user tokens keep their own edition. Bump the proc-macro crate's edition alone to emit newer syntax.
- **`Span::mixed_site()` does not shield generated `let` patterns from call-site `const` items** — pattern resolution treats a visible const as a const pattern, and item lookup for mixed-site tokens happens at the call site. `macro_rules!` has the identical hole (probe: a macro emitting `let value = 1u32` under a call-site `const value: u8` fails E0308 "interpreted as a constant"); only nightly `def_site` hygiene closes it. Mixed-site still isolates generated locals from user *locals* — worth doing — but a module const named exactly like a crate-internal generated binding is unfixable on stable.
- Host-dependent dependency-budget trap: `just dep-budget` (and so `just budgets` / `just ci`) can be red on a Mac while the same tree is green in CI. The budget walks `cargo tree --duplicates`, which resolves for the host platform only, and the CI budgets job runs on Linux (`.github/workflows/rust.yml`, "architecture budgets (linux)"). Any Apple-only duplicate is therefore invisible to CI and unmissable locally — on 2026-08-27 it was `objc2`/`objc2-app-kit`/`objc2-foundation` (0.5.2 via `accesskit_macos`, plus 0.6.4). Before blaming your change, re-run the budget on a stashed tree; a duplicate that reproduces on a clean baseline is platform-scoped, not yours.
- Doubled event-delivery diagnosis: when a composition-scoped collector reports every platform event exactly twice, count the *service registry's observers* before suspecting a double publish — two live registrations feeding one collector look identical to one publisher firing twice, and only the observer count separates them. `two_event_streams_in_one_composable_each_deliver_exactly_once` in `crates/cranpose-core/tests/effects_and_frames.rs` is the harness shape: a fake service that counts registrations, plus per-publish delivery assertions.
- **A robot `wait_for_idle` timeout with `waiting_for_present=true` is a host-load symptom, not a composition bug.** The budget is an iteration count, not wall clock, so on a machine under load (an Android emulator and Android Studio alone put this one at `load_1m=24/10`) the app can still be legitimately mid-present when the count runs out — `needs_update=false, has_animations=false` says composition already settled. Check `uptime` and re-run the one example (`just robot-one <name>`) before believing it: the same test passed in isolation and the full 121-test suite passed on the second run, same commit.
- **Targeted `-p <crate>` test runs miss the source-hygiene gates, and they are the ones that take main red.** `apps/desktop-demo/tests/source_hygiene_aliases.rs` scans *every* crate's sources — `workspace_tests_do_not_default_to_tmpfs_paths` rejects a `/tmp/` literal anywhere, even in a pure in-memory fixture that never opens the path, and `no_cargo_config_enables_unstable_web_sys_bindings` guards the rustflags. A per-crate gate set can be entirely green while these fail, and a sandboxed reviewer's `just test` can die at the socket-bound `cranpose-services::peer` tests before ever reaching them: both happened at once, so a `/tmp/` literal reached main and four sessions then fixed the same line in parallel (#503, #504, #506, #507). Run `cargo test --profile ci -p desktop-app --test source_hygiene_aliases` alongside targeted runs, or the full `just test`, before pushing. And when a gate does go red, `gh pr list --search "<failing test name>"` before writing a patch: a break that is *in* main is not cleared by rebasing, and someone else's fix is usually already open.
- **A full disk reports itself as a compile or test failure, never as a disk error.** The signatures are `failed to build archive ...: failed to open object file (os error 2)`, `extern location for <crate> does not exist: .../deps/lib<crate>-*.rmeta`, and a test binary that `could not execute process ... (never executed)` — all of which read as a dependency or code problem. They are truncated artifacts from a build that was killed mid-write. Check `df -h /` first. Two follow-on traps: `cargo clean --profile <p>` can itself die partway on the corrupted tree and print `No such file or directory` while leaving it broken, so confirm the directory is actually gone or `mv` it aside and let the build recreate it; and at genuinely zero bytes every tool that writes a temp file fails, including the ones you would use to diagnose, so free space before anything else. On this workspace `target/` reaches ~200G across profiles and a review worktree adds ~60G more.
- **A parser that reads `cargo tree` output must strip ANSI, not just the box-drawing characters.** `.github/workflows/rust.yml` sets `CARGO_TERM_COLOR: always`, and cargo honours it even through a pipe, so a line that reads `│   └── name` locally arrives as `ESC[2m│ESC[0m   ESC[2m└──ESC[0m name` in CI — a `starts_with('│')` guard never fires and the name comes out carrying the drawing. It is invisible on every dev machine because a pipe auto-disables colour: `cargo tree | od -c` shows no `033` bytes, `CARGO_TERM_COLOR=always cargo tree | od -c` shows `033 [ 2 m` ahead of every prefix. Reproduce any CI-only text-parsing difference with `CARGO_TERM_COLOR=always <command>`, and fix it in both layers — strip ANSI in the parser so any colour source is handled, and pass `--color never` so it is never generated.
- **A CI waiter that greps for "pending" fires on an output gap.** `until ! gh pr checks <n> 2>/dev/null | grep -q pending` reads "the word is absent" as "the run finished", so one transient gh failure ends the wait exactly like completion — and the `2>/dev/null` hides the error that would have shown it. `gh pr checks` also exits 8 while checks are still queued, so a waiter keyed on its status reports failure while its own banner says complete. Require all three: gh exited 0, output non-empty, and every check terminal via `[.statusCheckRollup[] | (.status // .state // "UNKNOWN")]` rejecting `QUEUED|IN_PROGRESS|PENDING|WAITING|REQUESTED|UNKNOWN`. That still only sees *registered* checks, so a partial list reads as complete — assert the expected set by name; a cranpose PR carries seven (Android release build, architecture budgets, fmt + tests + clippy, iOS build, robot e2e, robot external captures, wasm build). Require *pass*, not "nothing failed" — a skipped or cancelled check is not green. Then check what the merge actually did rather than printing success after calling it: a `gh pr merge` refused for a conflict still returns to the next line, and its exit status must be read directly, never through a pipe (same trap as the `| tail` entry above). Finish by confirming the squash commit is an ancestor of `origin/main` with the expected subject — `state == MERGED` can be set by auto-merge, a merge queue or an admin action, so it never asserts that *your* merge is what landed.
- **Verify the verifier: a guard you have not watched reject a bad input is not a guard.** Every hardening step is itself a predicate nobody tested, and the test is the easy thing to get silently wrong. `timeout` does not exist on macOS, so `timeout 8 ./wait.sh && echo FIRED || echo held` prints "held" from `command not found` — a green result for a script that never ran, and AGENTS.md sends people to `macm3` over ssh where a `timeout`-based test copied from Linux fails exactly this way. Feed the guard the states it exists to catch and watch each one: for a CI gate that is empty rollup, partial-but-all-green, one QUEUED, one FAILURE, one CANCELLED, one SKIPPED, an unexpected extra check, and only then all-green. The partial case is not hypothetical — a PR was observed going "no checks reported" to 4 registered to 7, so a gate that trusts the registered list can pass before the slowest job is created.
- **An app's own background work is measured as framework frame cost.** Scroll on a 2018-class Android device read as 15-17 fps with `present` p50 29-33 ms; the app was in fact still running on-device inference, settled at 442% CPU before a finger touched the screen. Re-measured on the same scene with the app quiet at 0-3% CPU: 22-28 fps, `present` p50 13.5-17 ms — the headline numbers were inflated about 1.5x. Note what did *not* move: the ranked causes filed alongside them — render passes, isolated layers, and a 35% layer-cache hit rate re-measured at 34.6% with 1.78 MP of blurred offscreen per frame — reproduced on the quiet device, two of them worse than filed. Contamination inflates the headline figures you quote in the title; it does not necessarily touch the per-frame counters underneath, so re-verify rather than retracting wholesale. Before trusting any on-device frame number, poll `adb shell top` for the package until its CPU settles and print what it settled at; a run that does not report its settled CPU is not a measurement, and the same applies to any app doing background indexing or sync. Never A/B across two package names either — separate installs carry separate app data and settings, so the arms differ in more than the build (theirs disagreed on `vsync_period_ms`, 15.841 vs 16.632, which is how the contamination surfaced). Install both arms over the same package, or flip them inside one binary with a `debug.cranpose.*` property override — `debug.cranpose.a11y_sync` exists for exactly this, and `segment_surface` and `pipeline_disk_cache` use the same mechanism. `idle_iters` tells you whether a telemetry window was a real scroll window or mostly idle.
- **Carry an untouched stage as a within-run control before believing a perf regression.** On a Kirin 980, patched `update` p50 measured 4.63-7.53 ms against 4.38-4.57 unpatched — the patched *minimum* above the unpatched *maximum*, on precisely the stage the change touched, which reads as a ~2 ms regression and nearly blocked its own merge. Carrying `render`, which the change did not touch, as a control gave ratios of 0.89/0.90 unpatched and 0.88/0.95/1.00/1.04 patched: flat. The governor moves every CPU stage together between sessions, so a same-direction shift in an untouched stage means you measured the device, not the patch. Second tell that two arms are not in the same state: `vsync_period_ms` disagreeing between them (15.841 vs 16.638). Detail on #504.
- **A knowledge PR has a convergence budget, and good material is not sufficient reason to spend it.** #510 took nine pushes and sixteen cancelled runs over two and a half hours; `heavy (self-hosted)` never completed once, because each new entry superseded the run validating the last. It converged on the first cycle after the branch was frozen. The check rollup for the current head shows only that head's checks and says nothing about the runs already killed — `gh run list --branch <b>` does. When entries keep arriving, freeze and open a follow-up.
- **A cache key can name the right thing at the wrong granularity, and the obvious fix ships a stale frame.** `LayerRasterCacheKey::backdrop_effect` (raster_cache.rs) takes `local_bounds: Rect` and stores its x/y/w/h in `local_bounds_bits`, but `backdrop_effect_cache_key` (render_paths.rs) passes a screen-space `visible_rect` into that slot. Every scrolled frame moves y, so the key changes and every lookup misses — the 34.6% hit rate and 1.78 MP of blurred offscreen re-rendered per frame in #500. Do **not** make the key translation-invariant: a backdrop caches what is *behind* the card, so when the card moves that region is genuinely different pixels and an invariant key serves a stale backdrop — subtle enough on a slow scroll to ship. The key is right about what it identifies; the granularity is what needs work.
- **Closing a PR does not cancel its CI, and on a constrained pool that cost falls on everyone else.** A PR closed at 16:00Z had a run *start* at 19:00Z on a machine the whole board was queued behind; deleting the branch does not stop it either. Both robot jobs require `[self-hosted, Linux, cranpose-heavy]` and only samarch-1's two runners match — the Macs carry the label but are macOS — so with ~19 robot jobs queued, one dead run holds roughly half the throughput. Cancel the runs when you close or supersede a PR, and note that a cancel *request* is not a cancel: a self-hosted job keeps reporting `in_progress` after accepting one, so read the status back. Worth evaluating when the board is quiet: a concurrency group keyed on the ref with `cancel-in-progress`.
- **`gh pr merge`'s exit status carries no information in either direction, and `--delete-branch` is a no-op.** Six observations on this repo: exit 1 on merges that succeeded (#510, #511), exit 1 on a merge that genuinely failed with "the merge commit cannot be cleanly created" (#499), exit 0 with empty output on merges that succeeded (#508, #512), and — the dangerous one — **exit 0 on a merge that genuinely failed** with "Pull Request has merge conflicts" (#515), so a script trusting exit 0 reports success on a conflict. On #512 a `mergeable`/`mergeStateStatus` read immediately before the call returned `UNKNOWN`/`UNKNOWN` on a merge that then succeeded, so the pre-state is decoration too. Delete the branch explicitly with a separate push, and verify the merge by ancestry and content only. Trusting the exit status alone reports failure on success, exactly as trusting `state == MERGED` alone reports success on someone else's merge. Only the ancestry check settles what landed. `--delete-branch` also did not delete the branch; that needed a separate push.
- **Test the accept path: a predicate that only ever holds is indistinguishable from a working one until the day it needs to fire.** A CI gate ran `gh pr view <n> --json statusCheckRollup --jq -c '<filter>'`, but `--jq` takes exactly one argument, so `-c` became the filter and the real filter a second positional — `accepts at most 1 arg(s), received 2`. gh exited non-zero and the rollup was empty on every poll for hours, across two arms. It had been tested against seven synthetic *bad* inputs and held on all of them, which is why it was trusted; the one untested input was a genuinely green rollup, the only one that ever had to pass. Corollary on reporting: when that gate first ended with an empty rollup it was described as the guard correctly holding — a bug narrated as the system working as designed. Watch for that shape in your own summaries. Corollary on stderr: suppressing it converts a broken command into a plausible measurement. Both failures above were loud — `accepts at most 1 arg(s)` on every call — and silent only because `2>/dev/null` discarded the one line that named them. A dead command does not read as "no data"; it reads as evidence, and two people drew two different confident conclusions from one that never ran.
- **Never let absence be the success signal.** Four failures in one day shared one shape: a check that concluded something had *succeeded* because an expected symptom was *missing*. A waiter grepped for "pending" and read the word's absence as the run finishing, so a transient error ended the wait like completion. A rollup test accepted a partial list of registered checks because none of the four present were failing, while the slowest job had not been created yet. A merge was reported done because nothing objected, when the exit status said 1 and only the tree said otherwise. And a command that never ran returned nothing, which was taken as a measurement rather than as a broken command. State the success condition positively and require it to be present: a literal `true` from the predicate, every expected check named and accounted for, the commit reachable from `origin/main`, a non-empty parse with stderr left visible. Absence is what you get when the question was never asked.
- **A headless scroll harness that scrolls nowhere still hits every cache, and every counter assertion built on it lies green.** `LazyListState::dispatch_scroll_delta(+12.0)` in an `AppShell` test clamps silently at offset zero — content scrolls with *negative* deltas — so six "scrolled" frames were byte-identical stills: `blur=0, cache_miss=0`, pass counts steady, and a fixed-glass batching test was being written against a scene where nothing moved. Before trusting any scroll-driven counter in `capture_frame` harnesses, prove motion first: sum the captured frame's bytes across two frames and require the sums to differ (`backdrop_pass_batching.rs` grew that check as an eprintln probe; two lines, caught it immediately). The device-side twin of this trap is `idle_iters`; the headless twin is a pixel sum.
- **Fill-pixel counters rank GPU work; only time attribution sizes it.** A device profile showed cached shadow composites filling 4.9–5.2 MP a frame against a 2.4 MP screen — the largest single number on the page — and a full fill-reduction change (occluder banding, 35% fewer shadow pixels) was designed, tested and device-measured off that ranking. The ablation then showed all shadows together cost 2–5 fps, and the banding's fps delta drowned in run-to-run noise: on a tiler with AFBC, megapixels of flat translucent black are far cheaper per pixel than the counter implies. Counters say *what* is drawn; before writing a fix, get *time* — GPU timestamp queries per pass — or ablate. Two corollaries from the same afternoon: an ablation must not change the pass structure it measures (skipping shadow *encodes* left their items splitting batch passes, 13→25, and "shadows off" measured slower than on), and back-to-back device runs drift thermally, so alternate A/B/A/B and log the battery temperature into the same output as the fps.
- **The desktop demo's retained-feed replay bypasses the shadow-composite path entirely, so a desktop fps A/B of shadow work measures nothing.** A `shadowed_cards_scroll` perf-harness scenario of opaque elevated cards ran at 145 fps with `avg_composite_passes=0, avg_blur_passes=0, cache hits/misses 0` — 99.25% lazy reuse replayed retained commands and no shadow composite ever executed, while the same scene shape on Android runs 11–12 shadow composites a frame. Desktop iteration is excellent for pass-structure and fill contracts (headless `capture_frame` tests, seconds per cycle) but a desktop wall-clock number only transfers to Android for paths the desktop actually executes; check the counters before trusting the fps.

- The Mate 20 X keyguard is secure (face/PIN): `wm dismiss-keyguard`, MENU, and
  swipe all bounce, `screencap` returns 0 bytes against it, and an activity
  `am start`ed behind it comes up resumed but surfaceless — the process is
  alive, renders nothing, and prints no telemetry, which reads exactly like a
  broken build. Never end a device script with `KEYCODE_SLEEP`; it locks every
  later round out until a human unlocks the phone. Check
  `dumpsys window policy | grep showing` before trusting an empty logcat.

- **A `$var` inside `ssh host 'bash -lc "..."` belongs to the remote LOGIN
  shell, not to the script.** `for rev in a b; do git checkout $rev; ...` ran
  a three-commit bisect where every arm silently tested the SAME tree: the
  remote outer shell expanded `$rev` to empty before `bash -lc` ever parsed
  the loop, `git checkout -q` with no argument is a no-op that exits 0, and
  the loop's own `echo === $rev ===` printed `===  ===` — the tell was in the
  output and still easy to read past. Escape as `\$rev` (and `\$PATH`), or
  scp a script file. Related self-kill: `pkill -f <pattern>` where the
  pattern also appears in the calling shell's own command line (it always
  does when you just typed it) matches the parent `bash -lc` and kills the
  session with exit 255 and zero output.

- **samarch-1's real display (DISPLAY=:0) fails three robot tests that xvfb
  CI passes, on main itself** — robot_counter_button_release_external_visual
  ("Increment click did not update counter"), robot_regression_shader_visual
  _contract ("glass/blur overlap lost backdrop detail in the right half",
  byte-identical pixel counts across runs), and robot_markdown_full_demo_
  code_block_visual_contract (wheel input or blank code block, varies).
  Verified at origin/main f1b4ce58: all three FAIL on :0 and pass in CI's
  xvfb run of the same commits. Before attributing an X0 robot failure to a
  branch, run the same test at origin/main on :0 first; conversely a real-
  display-only failure is invisible to CI, so a green board does not clear
  it. The suites' authoritative environment is the one CI runs. And before
  blaming the display at all, rerun the same binary with `TZ=UTC`: cranscan's
  nightly suite-wide red turned out to be UTC-stored dates grouped against
  the LOCAL calendar day (every run between local midnight and UTC midnight
  grew an extra date header and shifted all content ~26 px), which reads
  exactly like an environment failure and follows the clock, not the box.
  Cheaper still than the rerun: the red window is exactly local midnight to
  UTC midnight (00:00-02:00 CEST), so the FIRST question on such a red is
  the failed run's wall-clock time — inside the window the branch is not
  the suspect, and the same commit goes green after 02:00 with no fix
  aboard (measured across four cranscan PRs in one night).

- **`std::thread::available_parallelism()` reports the calling thread's
  affinity mask, not the machine** — after `sched_setaffinity` restricted a
  thread to 4 of 8 cores, every later parallelism read on that thread (and
  its children, which inherit the mask) answered 4. On the Mate 20 X this
  silently flipped the ≥6-core present-thread class and the pipeline ran
  single-threaded; the affinity readback listing one frame thread where the
  unpinned arm had two was the only visible symptom, and it cost a full APK
  rebuild + device A/B round to discover. Read machine-topology facts
  before any affinity call, and treat any capacity decision made after a
  pin as suspect.

- **Heavy ssh builds on samarch-1 can kill a CI job running on the same
  box** — a `cargo test` over ssh while `samarch-1-cranpose` had a job
  broke the job's sccache server ("Server startup failed: Address in use"
  at job start, then "server shut down unexpectedly" + connection resets
  mid-job), failing the job with infra "could not compile" errors on
  crates.io deps that compile everywhere. The "validate on samarch before
  CI" advice needs a check first: if the runner is mid-job, wait or use the
  second checkout with its own SCCACHE_DIR/port, or skip sccache
  (`SCCACHE_DISABLE=1`/unset RUSTC_WRAPPER) for the validation build.

- **A recovery path keyed on a timeout is a guard that lies under load** —
  `scripts/ci/start_sccache.sh` reclaimed the sccache port
  (`--stop-server` then `--start-server`) when the server failed to answer
  within 30s. It was written for a host carrying two sccache versions
  fighting over port 4226; that second binary was retired the same evening
  and the reclaim outlived its reason. A shared server under concurrent
  load is precisely what misses a fixed timeout while being alive and
  mid-compile for another job, so the reclaim killed healthy servers and
  two builds died with "The server looks like it shut down unexpectedly,
  compiling locally instead". Nothing may stop a daemon it did not start:
  if a shared service will not answer, fail loudly rather than restarting
  it underneath whoever is using it.

- **Fixing the workflow in front of you is not the same as establishing the
  property you named** — #526 was titled "Let one sccache serve the whole
  host" but only rewired `heavy-selfhosted.yml`; `rust.yml` kept the bare
  `RUSTC_WRAPPER: sccache` and its own `--start-server`. Two provisioning
  regimes then ran concurrently on samarch-1, and only one of them could
  kill the server. When a change claims a host-wide invariant, grep the
  whole `.github/workflows` tree for the pattern before believing it holds.

- **Two frame instruments print lines that both start `total_ms=`, and they
  are not the same instrument** — `CRANPOSE_DESKTOP_FRAME_TELEMETRY_MS`
  (`cranpose/src/desktop.rs`) sits on the present path, so a headless run
  never reaches it and yields a couple of samples that read as "no slow
  frames" rather than "no instrument". `CRANPOSE_FRAME_STAGE_TELEMETRY_MS`
  (`cranpose-app-shell/src/shell_frame.rs`) is a different one, called from
  `process_frame_in_context` with no present involved and no headless check
  in the file; it fires headless. It does go through `log::warn!`, so it
  needs `RUST_LOG=warn` and a binary built with the `logging` feature —
  `robot-app` pulls it, a plain build does not. An earlier version of this
  entry claimed the second variable was the present-path one, on a
  measurement actually taken with the first; both print `total_ms=`, and
  nobody checked which function emitted the line. Confirm which module reads
  the variable before attributing a silence to it.

- **Aggregate structural-change counts hide the thing you are looking for** —
  chasing a scroll frame that re-lowered 51 layers, per-parent totals said only
  "node 60 is structurally dirty every frame", which reads like a busy list.
  Printing each record with its reason, parent AND child
  (`CRANPOSE_STRUCTURAL_DIAG=1`) showed the parent had exactly two distinct
  children over 82,420 frames, each cycled 11,067 times in a perfectly regular
  attach/detach pair: the same unchanged child, not many changing ones. The
  sequence was the evidence; no aggregate over it could have been. When a
  counter says "this fires constantly", print the identities before theorising
  about the cause.

- **A per-item diagnostic does not add a constant to a benchmark, it adds a
  term proportional to the item count — usually the very quantity the change
  reduces** — #530 was measured with `CRANPOSE_SCENE_UPDATE_DIAG=1` on both
  arms, which looks like a fair comparison and is not. That flag `eprintln!`s
  one line per built layer; the pre-fix arm built 50.8 layers per frame and
  the post-fix arm 11.3, so the instrument cost about 4.5x more on the arm it
  was meant to show as slower. Reported scene_ms p50 was 0.77 -> 0.25;
  re-measured with the flag off on both arms it is 0.040 -> 0.020. Real
  halving, right mechanism, magnitudes ~20x too large and the ratio
  exaggerated. An instrument like this does not merely add noise, it biases in
  the flattering direction, so a result measured under it is unsafe in exactly
  the case you most want to believe. Take the headline number with every
  diagnostic off, and use the diagnostics only for counts and identities —
  those (166,257 structural events -> 291, 50.8 -> 11.3 builds per frame) are
  immune to their own overhead.

- **Reasoning that is airtight about work avoided still has to be measured** —
  no-op attach/detach called `bubble_layout_dirty` and `bubble_measure_dirty`
  unconditionally, each walking to the root and marking every ancestor
  `needs_measure`, about 166,000 times per run for child lists that did not
  change. Guarding those bubbles is correct and the argument for it is exact.
  The measured effect on a desktop scroll is 0.01 ms, one quantum of the
  telemetry's resolution. Avoided work is not the same quantity as saved time:
  the ancestors were already marked by something else, so marking them again
  cost the walk and nothing more. Count the work you remove, then measure the
  time, and believe the second number.

- **logcat's ring evicts the head of a capture, and the surviving tail
  reads as a result** — a 24-frame remnant of a triple-telemetry round
  (three per-frame stage switches spamming the ring) said "scene stage
  unchanged post-fix"; the full 225-line single-switch rerun showed p50
  moved 2.0 -> 1.18ms. The bias is systematic, not noise: the ring keeps
  the END of the run, which over-samples whatever phase ran last. Check
  line counts against expected frame counts before believing any logcat
  capture, and keep per-frame switches to the one being read.

- **A pkill on a shared CI host kills CI, and a "free the host" order must
  name the exclusion** — freeing samarch-1 for a queued robot job,
  `pkill -f run_robot_test` also hit the CI job that had started six seconds
  earlier: the runner executes the same scripts by the same names from its
  own work directory. The job died with a bare signal 15 that got three
  confident wrong attributions inside ten minutes ("superseded run", "test
  failure", "host guard") — only the person who typed the pkill could know,
  because nothing in the artifacts said so. Scope the kill to your own
  checkout (`pkill -f '/home/s/robot-repro.*run_robot_test'`), and when
  ordering a host cleared, say "everything except the runner's _work
  directory" out loud — the receiver should not have to infer that the
  freeze's beneficiary runs the same binaries.

- **A half-instrumented invalidation scheme is a new bug, not a half-fix** —
  #538's first commit recorded geometry changes only in the node setters,
  but the engine writes ordinary children's geometry through a direct
  handle that bypasses them. The recorded ids switched frames onto the
  scoped scene path, and the full-rebuild fallback that had been silently
  rescuing every unrecorded node left with the redundancy: the branch was
  WORSE than main (robot_drag_selection: main PASS, branch FAIL). Partial
  coverage is worse than none. The durable fix made the fields private so
  an unrecorded write does not compile — prefer making the bypass
  impossible over enumerating the sites, and distrust any optimisation
  that is "correct for the cases we enumerated".

- **Two scene-test traps that make a green lie** — a moved Text sibling
  rescues itself (re-measuring a text re-shapes it and schedules its own
  draw repass, sneaking the row into the scene scope), so a pin for missing
  geometry records must move a solid box; and tiny scenes flood the scope
  through the retained-redraw sweep so everything self-heals, so pin with
  per-item lazy scopes and assert rebuilds == 0 against a harness that
  counts its internal fallback as a rebuild. Both greens looked like proof
  and proved nothing; both were caught only by severing the mechanism and
  demanding the red first.

- **Kirin 980's Vulkan driver grants no TIMESTAMP_QUERY, and the
  instrument only says so once** — `debug.cranpose.pass_timing` is dead
  on the Mate 20 X (Mali-G76, wgpu picks Vulkan): the adapter lacks the
  feature, so no `[GPU-PASS]` window ever prints. The warning that names
  the cause is logged once at renderer construction, so the standard
  capture pattern (launch, settle, `logcat -c`, capture) wipes it and
  the run reads as silently empty — one full three-arm device cycle was
  spent on captures that could never contain data. Prove the instrument
  is live before spending arms on it (capture startup WITHOUT clearing
  logcat and look for the seeding line AND the absence of the
  lacks-TIMESTAMP_QUERY warning). GPU attribution on this device class
  is arm-differencing only. Related trap in the same cycle: an
  env-gated renderer toggle is unreachable on Android unless it is in
  `PROPERTY_BACKED_ENV_VARS` (android_frame_telemetry.rs) — check the
  mirror list before planning a device A/B around any CRANPOSE_* switch
  (CRANPOSE_ENABLE_DIRECT_SCENE_RANGE_CACHE was readable in code and
  unreachable on device until 2c9deef9 mirrored it).

- **with_host_lock.sh around run_robot_test.sh is a self-deadlock, and it
  starves CI while it hangs** — run_robot_test.sh acquires the samarch-1
  host lock internally; hold it exclusively from outside and the script's
  own acquisition waits forever on your hold. The hang is silent (the
  suite just "runs long") while every queued CI job on the host waits
  behind the lock — a captures job sat 44:50 behind one wedged wrapper.
  Invoke the script bare; if you must kill a wedged holder, match PIDs by
  checkout path, never by process name (pgrep matches your own grep).

- **`[GPU f#N]` prints every 60th frame and its counters describe THAT
  frame only** — it is not a per-frame average, and the sampled frame can
  be the one atypical frame in the window. During the scroll campaign the
  sampled frames were ambient-step epoch frames where every backdrop
  cache misses at once, so the line read "miss=18" while the all-frames
  `CRANPOSE_LAYER_CACHE_DIAG` total over the same window averaged 1-2
  misses per frame — an order of magnitude apart, and it misdirected the
  attribution for about an hour. The counter answered a different
  question than the one asked. When a sampled instrument and a totals
  instrument disagree, believe the totals; better, never average from one
  sample.

- **"No caller in this repo" is not evidence an API is dead, and the check
  that says so silently excludes first-party consumers** — `9af4604b`
  deleted `Modifier::horizontal_scroll_guarded`/`vertical_scroll_guarded`
  as dead code on that reasoning. leetcodedaily called the horizontal one,
  passing `|| false` to stop the row's built-in drag from competing with
  its own drag-reorder `pointer_input`; migrating it to 0.1.106 forced
  `.horizontal_scroll(...)` just to compile, and the two gestures have
  been fighting since. The same commit added 280 lines of
  `draggable_guarded` tests, so `draggable_guarded` survived the identical
  pass — the asymmetry is the tell that the scope, not the judgement, was
  wrong. Note the implementation never died: `scroll_impl` still took the
  guard, still threaded it into `DragGesture`, still evaluated it per
  event. Only the public entry points went, leaving a live mechanism
  pinned to `None`, which is why nothing failed to compile.
  Same class as `just clippy` never linting the ~165 `[[example]]` targets
  behind `required-features = ["robot-app"]`: a check whose scope silently
  excludes the thing it is supposed to cover, reporting clean because it
  looked nowhere. Before deleting a public API, grep the downstream
  consumers too (`/Users/s/develop/consumer-bumps/`), and prefer a test
  that calls it — a test is an in-repo caller, and it is what kept
  `draggable_guarded` alive.

## Infinite transition "freeze" needs a spec change AND a reader remount

The showcase planets stopped rotating after details -> back -> Saved -> Explore.
Scripted Saved/Explore round trips (40+, timed randomly) and detail/back alone
never reproduced it, and neither did dirty/recomposition diagnostics: the draw
dirty set was identical frozen and moving (the layers re-rendered every frame),
so the renderer was a dead end. The value itself was stuck: a spec change
(`animateFloat` re-specced linear <-> stepped) captures a per-animation
play-time offset, and when every reader leaves (a `with_key` tab rebuild) the
transition loop breaks and, on restart, used to restart play time from zero,
so `saturating_sub(offset)` pinned that one animation at its initial value for
as long as the stale offset. Any other animation on the same transition kept
moving, which is what made it look like a rendering bug. Reproduce with the
exact user sequence, and when a value looks frozen log the *state value* in the
draw closure before touching the renderer.

## A "frozen backdrop" verdict from a shift search over a featureless lane

`robot_glass_backdrop_scroll_stability` went red on main at "Remove blur from
Receipts glass surfaces" and stayed red for two days, reading exactly like a
stale backdrop cache. It was not: the compositor re-captured the bar's
backdrop every frame (`CRANPOSE_BACKDROP_DIAG=1` showed a fresh `prepare
MISS copied=true` per frame) and a cold render of the same scroll position
matched the incrementally scrolled frame pixel for pixel. The test searched
for a one-pixel vertical shift inside a 32px lane of the bar; with the blur
gone that lane held only a card's diagonal gradient, whose vertical change
per pixel is below one colour step, so the best-fit shift was 0 and the test
called it "frozen". The blur had been smearing card edges and text into the
lane, which is the only reason it ever measured a shift. The lens optics that
followed compress vertical motion under the bar as well, so the "identity
lane" premise does not hold for this material at all. A shift heuristic on a
glass output cannot tell "frozen" from "no vertical texture"; the test now
compares every incremental step against a cold render of the same scroll
position (remount the tab, scroll straight there, capture), which is the
cache's actual contract and fails from step 2 when the backdrop cache key is
deliberately frozen. Reproduce such failures headlessly on the Mac first
(`robot_glass_backdrop_scroll_headless`): a samarch iteration was queued
10+ minutes behind CI's host lock, the Mac loop is 90 seconds.

## Ranking GPU cost on the Mate 20 X: no timestamps, so ablate the scene

`debug.cranpose.pass_timing` prints only "adapter lacks TIMESTAMP_QUERY" on
the Mate 20 X (Mali-G76, Vulkan), and logcat's chatty filter drops most of
the per-node `[layer-cache-diag]` lines, so neither a pass profile nor a
per-frame miss list is available there. What worked, at 2.5 minutes per
build-and-measure round on the showcase copy in the scratchpad: freeze or
remove one thing at a time and read `present p50`. Baseline 41.5 ms; cards
without glass 5.5 ms; starfield frozen 6.8 ms; planets frozen 35.8 ms; card
glass with a trivial shader 38.8 ms; header blur removed 41.2 ms; tab bar
blur off 43.5 ms. Read together: the glass cards cost 36 ms only while the
backdrop beneath them changes, and the shader is 3 ms of that. The rest is
five root backdrop captures plus five underlay bakes per frame, each a flush
of the pending composites and a copy out of the live composition target,
which on a tile-based GPU is a full tile store and reload of the 2.4 MP
target every time. The per-kind `miss_px_by_kind` field on the `[GPU f#]`
line is the cheap way to see which cache kind is churning without logcat.

## A fence-split pass profile ranks by latency, and one sampled GPU line lies

Two instruments cost an evening on the Mate 20 X. `debug.cranpose.gpu_fence_profile`
(submit and wait at every pass boundary, minus an empty round trip) charged
each full-screen composite pass about 11 ms and every small pass under 1 ms,
so seven root composites looked like 77 ms of a 45 ms frame. Removing six of
them changed present time by nothing: the profiler measures each pass's
latency in isolation, and a tiler hides most of a big target's write-back
and reload behind the next pass's work. Use the profile only to list passes
per frame and their targets, never to rank them; on this GPU the frame cost
tracks the pass count (about 0.4 ms of fixed cost each, 11 passes present in
5 ms, 37 in 30 ms) far more than the pixels. The second trap is the
`[GPU f#]` line: it is one sampled frame out of sixty, and on this screen
consecutive frames alternate between a star step (56 passes, 2 backdrop hits)
and a quiet frame (38 passes, 16 hits), so two runs of the same build read as
a regression or a fix depending on which frame the sampler landed on. Compare
several consecutive samples, or `pass_px` averaged over a run, before
believing a difference.

## `present` is not GPU time: a 50 ms animation period hides a 45 ms GPU frame

The Mate 20 X showcase reported present p50 of 6.8 ms with the stars frozen and
33 ms with them moving, at identical pass and cache counts, and an evening went
into "what makes a relowered scene 4x slower on the GPU". Nothing does. A
whole-frame fence (`debug.cranpose.gpu_fence_profile frame`) puts both at
50 ms of GPU per frame; the frozen variant only steps its planets, every 50 ms,
so the GPU finished each frame just before the next one was submitted and
neither acquire nor present ever blocked. Present and acquire block only by the
amount the GPU falls behind the app's own frame period, so on an
animation-paced screen they read as small right up until the GPU is slower
than the period, then jump. Measure GPU time with the whole-frame fence, or
compare `pass_px` and the Mac's `CRANPOSE_GPU_PASS_TIMING` occupancy, never
the present column.

## A fence cannot time the GPU here either; inject CPU delay and read the period

The whole-frame fence reports 43 ms per frame on a scene running at 52 fps
unfenced (cranorbit MEGA BOSS, 2026-09-04), so on this device it has a
round-trip floor larger than the frame and cannot rank anything. What does
discriminate is `debug.cranpose.encode_delay_ms`: a sleep on the present
thread between acquire and encode. If the period does not move, the GPU is
the frame and the CPU hides under it (showcase: +20 ms cost nothing, +40 ms
cost a frame, so the GPU frame is ~40 ms and present blocks on the previous
frame, not this one); if it moves one for one from zero, the present thread
is the frame and the present block is `gpu - encode` (cranorbit: GPU ~19 ms
behind an 8 ms present column). Two alternating rounds, temperature logged.

## An uber-shader's OFF features cost more than its ON ones on Mali

The liquid glass fragment program gates a dozen optional features on
uniforms (loupe, fold, morph shapes, wobble, touch, shadow, content mask,
optical blur, zoom...). The showcase cards use none of them, and the per-pass
fence still put the four card passes at 47 ms, ~33 ns per pixel for 19 taps.
Ablating the live features one at a time never explained it: removing the
chromatic channels saved 12 ms, removing reflection plus frost 16 ms, and an
early return before the tail saved 32 ms, none of it proportional to the taps
removed. Folding the thirteen inactive uniforms to constants brought the same
passes to 21 ms with byte-identical output. The dead branches were not free:
they held registers and their uniforms were fetched per fragment. Specialize
per material with `override` constants (`specialize_liquid_glass`,
`CRANPOSE_NO_SHADER_SPECIALIZATION` to compare); do not ablate live features
looking for the cost of dead ones.

## Whole-frame fence under DVFS hides a 26 ms GPU saving

Cutting 36 ms of isolated pass time from the showcase frame moved the
`gpu_fence_profile frame` reading from ~89 ms to 65-90 ms depending on the
run: the GPU governor drops its clock as the work shrinks, so wall time per
frame barely moves until the work is small enough to change the clock's
target. Judge a device change by scroll fps (`measure.sh ... scroll`, here
14.3 to 18.5) and by the per-pass inventory (`gpu_fence_profile 1`, stable to
within a millisecond across runs), not by the whole-frame number.

## A stacked bench on a tiler measures one layer

Drawing the same opaque full-screen fill eight times per frame to amplify a
shader difference above governor noise measured about one layer: Mali's
forward pixel kill discards fragments of earlier quads once a later opaque
quad covers them. Stack translucent layers, or compare single-layer runs
interleaved several times.

## Flat varyings fetch like uniform loads on Mali

Moving the gradient stops from a per-fragment loop over a dynamically indexed
uniform array into five flat varyings saved about 3 ms of the 11 ms a
full-screen three-stop radial costs, not the 11 ms hoped for: reading a flat
`vec4` varying per fragment costs about what one uniform load did, and a
gradient-free fragment (`vec4(t)`) ran at the solid-fill floor. The remaining
cost is the per-fragment data fetch itself; a big fill would need its
per-shape data in statically indexed uniforms (preloaded per draw) to reach
the floor.

## A surface configured for rendering only silently keeps the composition copy

Rendering the frame straight into the swapchain image needs that image to be
samplable and copyable, which the Mate 20 X swapchain supports; the parity
test passed on the Mac, the phone build still ran the output conversion pass
because the Android `SurfaceConfiguration` requested `RENDER_ATTACHMENT`
alone and the runtime check on `texture.usage()` correctly fell back. Check
the device's per-pass inventory for the pass you removed, and log the
configured usage next to the capabilities (`Android surface: ... configured
...`) so the fallback is visible.

## The Android toggle table compiles only for Android

`crates/cranpose/src/android_frame_telemetry.rs` declares its property-backed
toggle table with an explicit array length. Every host gate (`just test`,
`just clippy`, the mac CI job) passed with a 56-row table declared as 55 rows,
because that file is only compiled for the Android target; the first thing
that caught it was the phone APK build six seconds in. When you add a row,
bump the length in the same edit and build the APK before pushing: the
"Android release build" CI job is the only gate that compiles it.

## A pixel test's failure numbers name the mechanism it expects

`translated_text_wrapper_preserves_local_picture...` failed with 241
differing pixels against a bound of 240 and a max channel sum of 267 after
the renderer's direct path drew the text raster at the same device position
in both frames. That pair of numbers is a snapped raster read back through a
bilinear sample at a 0.35 px phase: 267 = 3 x 89 = 3 x 0.35 x 255. The test
was not asserting rigid motion, it was asserting the supersampled "motion
stable capture" surface the old planner composited at fractional offsets.
Before chasing a raster bug, decode the reported diff against the sampling
the assertion applies; when it reproduces a resample of a correct raster the
test encodes a removed mechanism and needs a new contract, not a fix.

## Deleting a test by `rfind("#[test]")` walks into the previous module

A script that removed tests by searching backwards for the nearest `#[test]`
from a matched string deleted the `FinishedRecording` struct and half of
`DrawScopeDefault` in `geometry.rs`: the match was in a doc comment above
the item, so the search jumped into an earlier test module. Bound the search
to the `#[cfg(test)] mod tests` slice and end an item at the first `\n}\n`
at column zero, never by counting braces through string literals, and keep
a pristine copy of the file (`git show HEAD:<path>`) to rebuild from.

## Pass count, not fill, is the frame cost on every tiler you ship to

The resolve-then-compose renderer halved the showcase's pass pixels and
still fell from 24 to 14 fps on the Mate 20 X: 65 small passes against 30.
Metal on the Mac charges the same shape of cost — the pass-timing report
showed ~0.5 ms for a 40x40 blur pass — so the Mac build is the instrument:
`CRANPOSE_GPU_PASS_TIMING=1 CRANPOSE_GPU_STATS=1 ./cranpose-showcase` prints
a per-label pass inventory (`[GPU-PASS] Shader Effect Pass 14ms x30 | ...`)
that the phone cannot (no timestamp queries, and its logcat drops the
`[GPU f#]` lines under scroll). Read that inventory before touching a
shader: the fix was batching captures and blurs into atlas passes and
drawing shader tails in the final pass, which no ALU work would have found.

## Attributing Mali frame time without GPU timestamps (2026-09-03)

The Mate 20 X exposes no timestamp queries, and reading the Mac pass
inventory for it misleads twice: Metal's per-pass "occupancy" sums pass
durations that overlap (the `span` is the real GPU time), and a pass that
is cheap on Apple silicon (an atlas-sized blur pair, a re-shaded glass
inside a capture) is bandwidth or ALU on Mali. What worked, in one APK
build: a temporary `CRANPOSE_ABLATE` toggle mapped to
`debug.cranpose.ablate`, read in frame.rs and draw_pass.rs, that drops one
thing per run (all backdrops, the blur pair, the capture draws, the shapes
or blits or shader composites inside captures, the shader-only children),
then `setprop` per variant and the frame telemetry's present p50. The
differences add up to the frame, so one build answers every "what does X
cost" question; three separate hypothesis builds before it answered none.
Take the toggle out before committing: it is a debug branch in the hot path.

## A rule that helps one path can hurt the other (2026-09-03)

The tail-recapture rule (a shader tail read by more than half its area
resolves into a texture) was written for shader-only children, where the
alternative is a two-pass surface. Applied to backdrop glasses it dropped
them out of the atlas into per-glass capture and effect passes, and the
Mac inventory grew four unbatched captures per frame. Any rule that decides
between "stay batched" and "resolve alone" must be measured on both kinds
of item before it ships.

## Reproduce a robot scroll failure headlessly before writing fixtures (2026-09-03)

Three robot runners failed on CI's X11 host. Two hours went into guessing
the failing element from diff crops and building four render-graph
fixtures (layer shadows, cutout shadows, a glass over a shadow) that never
turned red, then got deleted. What answered in ten minutes was composing
the real demo page in an `AppShell` over a headless renderer at the CI
density, driving its scroll state by one physical pixel and comparing
shifted frames -- `apps/desktop-demo/tests/liquid_scroll_phase.rs`. It
reproduced the exact 8 800-pixel flicker on the Mac, and every fix could be
checked in seconds instead of a queued samarch-1 run. The robot log's
first-diff pixel names a symptom, not the element: read-tail resolve
regions, gradient dither and shadow bands all showed up as "1 LSB on a
card". Start from the scene, not from the pixel.

## An ablation worktree keeps every earlier ablation (2026-09-03)

Device ablations were built from a scratch worktree so the live tree stayed
clean, and the worktree was "synced" by copying the files `git status`
listed in the live tree. After the live work was committed, `git status`
listed nothing for `frame.rs`, so the worktree kept the previous ablation's
`frame.rs` under every later build. A toggle APK meant to split a 22 ms
capture cost then measured its "none" variant at 32 ms against the live
tree's 46 ms, and two hours went into the wrong question (a build-location
confound) before a `diff` of the two trees showed the stale ablation. Sync
an ablation tree with `diff -rq` against the live tree, or rebuild it from
the commit plus one patch, and never trust a "none" variant that does not
reproduce the live baseline first.

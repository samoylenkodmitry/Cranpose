goal: make liquid ui in cranpose that matches the mac/ios liquid glass at ./example

## Project orientation (fresh checkout)

**cranpose** is a Jetpack-Compose-style declarative UI framework in Rust: `#[composable]` functions, `remember`/`mutableStateOf`/`State`, modifier chains, closed-form value-space spring animation, rendered by wgpu (Vulkan on this Linux/X11 host) with a software text raster. Targets: desktop (winit), wasm/WebGL2, android, ios. There is a headless **robot** test harness that drives real gestures and captures screenshots.

Crate map (`crates/`):
- `cranpose-core` — runtime, recompose scheduler, state/snapshot, frame clock.
- `cranpose-ui` — widgets, text (measure/layout/wrap), layout engine, modifiers, `basic_text_field.rs`, `text_selection.rs`, `text_field_modifier_node.rs`.
- `cranpose-ui-graphics` — `Color`, draw primitives, **`shaders/liquid_glass.wgsl`** (the glass shader; uniform accessors `get_float(N)`/`get_vec*`), `liquid_glass.rs`.
- `cranpose-liquid` — the **Liquid Glass** component library (`Glass` material, `dynamics.rs`, `widgets/{tab_bar,toggle,segmented,slider,menu,button,card,nav_bar,search_field}.rs`, `motion.rs`, `theme.rs`).
- `cranpose-render/{common,wgpu}`, `cranpose-app-shell`, `cranpose-foundation`, `cranpose-animation`, `cranpose-testing` (robot assertions/helpers), `cranpose-macros`.
- `cranpose` — top-level crate + platform entry points: `src/{desktop,web,android,ios}.rs`, `src/robot.rs` (`capture_keyframes`, `touch_down/move/up`, `find_button_bounds_exact`, `measure_text`).

Apps: `apps/desktop-demo` (the showcase + `robot-runners/` + `examples/`), `apps/android-demo`, `apps/desktop-demo-platform` (android/wasm exported libs).


## Gate commands (all were GREEN at `a50a6911`)

```sh
cargo test --workspace                       # 91 test binaries; also the half-state-language guard (no "migration"/"legacy" in source)
cargo clippy --workspace --all-targets       # MUST be zero warnings
cargo fmt --all
apps/desktop-demo/build-web.sh               # WASM
JAVA_HOME=/usr/lib/jvm/java-21-openjdk apps/android-demo/android/gradlew -p apps/android-demo/android :app:assembleRelease
CRANPOSE_HOST_MAX_TEMP_C=97 CRANPOSE_HOST_RESUME_TEMP_C=93 CRANPOSE_HOST_TEMP_MAX_WAIT_SECS=900 ./run_robot_test.sh --sequential   # 127/127
```

Single robot runner: `ROBOT_SHOT_DIR=<dir> cargo run -p desktop-app --example <name> --features desktop,robot-app`.

## Tailnet builder and live desktop-demo service

Use the extra Arch Linux x86_64 host for compilation and X11 robot work so
the workstation remains responsive. The builder is `s@100.103.151.60` on SSH
port `223`; its synchronized checkout is
`/home/s/.cache/cranpose/liquid-validation-HPCYtsG8`. Do not put its password
in the repository. Establish one multiplexed SSH connection and reuse it for
all rsync/build commands:

```sh
ssh -MNf -o ControlMaster=yes -o ControlPersist=12h \
  -S /tmp/cranpose-samarch-control.sock -p 223 s@100.103.151.60
```

Synchronize only the tracked files changed by the current work. `-R` preserves
their repository-relative paths and avoids copying target directories or the
user-owned reference recordings:

```sh
rsync -aR -e 'ssh -S /tmp/cranpose-samarch-control.sock -p 223' \
  ./crates/cranpose-liquid/src/material.rs \
  ./crates/cranpose-ui-graphics/shaders/liquid_glass.wgsl \
  s@100.103.151.60:/home/s/.cache/cranpose/liquid-validation-HPCYtsG8/
```

The builder can sustain 12 Cargo jobs. Disable sccache for reproducible
validation. X11 robots additionally need the builder's real display and
authority file:

```sh
ssh -S /tmp/cranpose-samarch-control.sock -p 223 s@100.103.151.60 \
  'cd /home/s/.cache/cranpose/liquid-validation-HPCYtsG8 && \
   CARGO_BUILD_JOBS=12 CRANPOSE_USE_SCCACHE=0 cargo test'

ssh -S /tmp/cranpose-samarch-control.sock -p 223 s@100.103.151.60 \
  'cd /home/s/.cache/cranpose/liquid-validation-HPCYtsG8 && \
   DISPLAY=:0 XAUTHORITY=/home/s/.Xauthority \
   CARGO_BUILD_JOBS=12 CRANPOSE_USE_SCCACHE=0 \
   CRANPOSE_HOST_MAX_TEMP_C=95 CRANPOSE_HOST_RESUME_TEMP_C=92 \
   CRANPOSE_HOST_COOLDOWN_SECS=2 CRANPOSE_HOST_MAX_WAIT_SECS=900 \
   ./run_robot_test.sh --sequential'
```

Use at most four Cargo jobs for a fallback workstation build. The workstation
has worse thermals; keep the 95/92 C guard instead of disabling it. Robot
processes must remain sequential because they share one X11 desktop.

The latest visually accepted optimized desktop build is always kept running
by the enabled user unit
`~/.config/systemd/user/cranpose-desktop-demo.service`. Its stable contract is:

```ini
[Service]
WorkingDirectory=/home/s/develop/projects/compose-rs-proposal
Environment=DISPLAY=:0
Environment=XAUTHORITY=/home/s/.Xauthority
Environment=WINIT_UNIX_BACKEND=x11
ExecStart=/home/s/develop/projects/compose-rs-proposal/target/release/desktop-app
Restart=always
RestartSec=2s
```

Keep the currently installed process alive while the next release compiles.
Build on the extra host, copy to a staging filename, atomically replace the
local executable, then restart and prove that the process runs those bytes:

```sh
ssh -S /tmp/cranpose-samarch-control.sock -p 223 s@100.103.151.60 \
  'cd /home/s/.cache/cranpose/liquid-validation-HPCYtsG8 && \
   CARGO_BUILD_JOBS=12 CRANPOSE_USE_SCCACHE=0 \
   cargo build --release -p desktop-app'

rsync -a -e 'ssh -S /tmp/cranpose-samarch-control.sock -p 223' \
  s@100.103.151.60:/home/s/.cache/cranpose/liquid-validation-HPCYtsG8/target/release/desktop-app \
  target/release/desktop-app.next
chmod 755 target/release/desktop-app.next
mv target/release/desktop-app.next target/release/desktop-app
systemctl --user restart cranpose-desktop-demo.service
systemctl --user is-active cranpose-desktop-demo.service
pid="$(systemctl --user show cranpose-desktop-demo.service -p MainPID --value)"
sha256sum target/release/desktop-app "/proc/$pid/exe"
```

The two hashes must match. A successful build without this install, restart,
and hash check does not count as the latest desktop demo being available to
the user.

## Release status

Workspace is at **0.1.59** (last published, `c5e9e733`). **This round's 18 commits are UNRELEASED on main.** Once the three judge loops (flight/#32/#33) converge and gates are green, the flow is the usual: bump `Cargo.toml` workspace version + intra-workspace pins + `Cargo.lock`, `Release X.Y.Z: …` commit on main, lightweight tag `git tag --no-sign vX.Y.Z` (global `tag.gpgsign=true`), push → `publish.yml`. Then integrate into cranscan (`~/develop/projects/ocr`, 7 cranpose pins) + release. Do NOT release with judge loops open unless the user says so.

## Constraints (AGENTS.md — non-negotiable)

Zero warnings everywhere; all tests pass ("never *not yours*"); no `git reset` (stash); no `rm -rf` (mv to `_old`); no "migration"/"legacy"/"deprecate" wording (there is a source guard test); fix root causes, failing test first for bugs; robot suite sequential with the thermal knobs above. Commit trailer:


Checkpoint gates:

- workspace `cargo test > 1.tmp`: pass, zero warnings;
- workspace `cargo clippy > 2.tmp`: pass, zero warnings;
- Android `:app:assembleRelease`: pass after moving Java source/target to 17,
  zero warnings;
- desktop-demo wasm build: pass;
- optimized desktop release build: pass, zero warnings;
- full real-X11 robot suite on Intel UHD 730: 128/128 pass;
- local NVIDIA/X11 smoke set: 7/7 pass, including liquid motion, menu, loupe,
  vertical selection grab, tab navigation, fused viewport, and external drag.

The verified release is installed at `target/release/desktop-app` and runs as
the user unit `cranpose-desktop-demo.service` with normal nice level 0. The
Liquid UI opens directly on the physical-profile playground.


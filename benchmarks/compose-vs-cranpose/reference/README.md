# GPUI workspace reference fixture

This directory preserves the source used for the GPUI workspace screenshots and
desktop scenario runs. It is based on [longbridge/gpui-fast](https://github.com/longbridge/gpui-fast)
at commit `7ab23f46f2ba3a040ceb27d387383a2896bc5ae1`, with the exact
[fixture patch](gpui-fixture.patch). [The manifest](gpui-fixture.json) records the
patch, input/output source hashes, font hashes and initial capture state.
It contains no performance results, binaries or font files.

## What the patch changes

- Loads device Roboto Regular, Medium and Bold and selects Roboto for the gallery.
- Uses the light theme and opens the workspace with its search field focused.
- Stops the unrelated gallery unread-count timer, keeping that badge at three.
- Adds the original local `TICKS` counter and automatic-run rate diagnostic.

The tick instrumentation is preserved exactly in `auto.rs` and `workspace.rs`.
The other two changed files are `mod.rs` and `theme.rs`. The workspace already
starts with streaming off and tick zero; no extra pause patch is needed.
The normal quote, scroll and hover drivers remain in the pinned source.

## Reconstruct the source

Use a new checkout. Keep any existing reference checkout, local edits and IDE
configuration intact. Run the following from the Cranpose repository root;
choose an unused absolute path for `GPUI_FIXTURE`.

```bash
export CRANPOSE_REFERENCE="$PWD/benchmarks/compose-vs-cranpose/reference"
export GPUI_FIXTURE=/absolute/path/to/new/gpui-reference
git clone --no-checkout https://github.com/longbridge/gpui-fast "$GPUI_FIXTURE"
git -C "$GPUI_FIXTURE" checkout --detach 7ab23f46f2ba3a040ceb27d387383a2896bc5ae1
git -C "$GPUI_FIXTURE" apply --check "$CRANPOSE_REFERENCE/gpui-fixture.patch"
git -C "$GPUI_FIXTURE" apply "$CRANPOSE_REFERENCE/gpui-fixture.patch"
python3 - "$CRANPOSE_REFERENCE" "$GPUI_FIXTURE" <<'PY'
import hashlib
import json
import pathlib
import subprocess
import sys

reference, source = map(pathlib.Path, sys.argv[1:])
manifest = json.loads((reference / "gpui-fixture.json").read_text())
digest = lambda data: hashlib.sha256(data).hexdigest()
assert digest((reference / manifest["patch"]).read_bytes()) == manifest["patch_sha256"]
for item in manifest["files"]:
    original = subprocess.check_output(
        ["git", "show", manifest["commit"] + ":" + item["path"]], cwd=source
    )
    assert digest(original) == item["base_sha256"], item["path"]
    rebuilt = (source / item["path"]).read_bytes()
    assert digest(rebuilt) == item["fixture_sha256"], item["path"]
    assert len(rebuilt) == item["fixture_bytes"], item["path"]
print("Patch and all four reconstructed files match.")
PY
```

The verification requires only Git and Python; it does not build or launch GPUI.
The patch is an archived fixture for this benchmark. Do not apply it to another
GPUI revision and assume the result is the same reference.

## Supply the external fonts

The captured reference used Roboto files from `/system/fonts` on a Huawei Mate
20 X running Android 10. Extract those files from the comparison device with
ADB, or reuse copies that match the manifest. Set `ANDROID_SERIAL` to that
device's serial. Font names alone are insufficient: files on another Android
version can differ.

The archived patch reads the following fixed directory. Keeping it unchanged
preserves the verified source hashes.

```bash
mkdir -p /tmp/compose-gpui-parity/fonts
for face in Regular Medium Bold; do
  adb -s "$ANDROID_SERIAL" pull "/system/fonts/Roboto-$face.ttf" /tmp/compose-gpui-parity/fonts/
done
python3 - "$CRANPOSE_REFERENCE/gpui-fixture.json" <<'PY'
import hashlib
import json
import pathlib
import sys

fonts = json.loads(pathlib.Path(sys.argv[1]).read_text())["fonts"]
for name, expected in fonts["sha256"].items():
    path = pathlib.Path(fonts["fixture_directory"]) / name
    assert hashlib.sha256(path.read_bytes()).hexdigest() == expected, path
print("All three reference fonts match.")
PY
```

## Build and run

The successful fixture build used macOS, Rust 1.98.1, the default `fast`
feature, the pinned lockfile and the repository's `.cargo/config.toml`.
A working Xcode Metal toolchain is required for the macOS backend. Run inside
the fixture so Cargo loads its configuration, and keep its build output
separate from existing reference executables.

```bash
(
  cd "$GPUI_FIXTURE"
  CARGO_TARGET_DIR="$GPUI_FIXTURE/target" cargo build --locked --release -p gpui_perf -j 2
)
"$GPUI_FIXTURE/target/release/gpui_perf"
```

With no arguments, the fixture opens the paused workspace at 1280 × 820 logical
client pixels, light theme, tick zero, stream off and automatic scrolling off.
The diagnostic footer can still update. Match display density and client size,
crop OS chrome, and compare to the Android apps' `workspace --ez still true`
state as described in the [benchmark guide](../README.md#workspace-reference-and-interaction-checks).
Record actual client bounds and scale; window managers may constrain the requested size.

The same binary retains the automatic desktop scenarios:

```bash
"$GPUI_FIXTURE/target/release/gpui_perf" --auto --only WorkspaceQuotes --retention on --frames 1200 --no-hold-clock
```

Replace `WorkspaceQuotes` with `WorkspaceScroll` or `WorkspaceHover`.
These runs enable quote streaming and use 30 warm-up frames. `--no-hold-clock`
disables GPUI's optional CPU clock-holding helper. Retain stdout and stderr:
the automatic table reports GPUI draw counts/rates; `TICKS` reports quote-stream
ticks per second. Neither counter measures physical display scanout.
CPU p50/p95 values come from main-thread CPU differences between frame callbacks;
they are not isolated draw timings. Treat counter definitions and workload
cadence separately when comparing another framework.

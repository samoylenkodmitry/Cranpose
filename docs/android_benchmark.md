# Android benchmark tooling

- Requires Python 3.11+, Pillow, adb, cargo-ndk and the app’s Android NDK; video capture also requires ffmpeg and screenrecord or scrcpy.
- Build immutable source archives and native provenance with `python3 scripts/android_benchmark_build.py --help`; keep reports outside the disposable build cache.
- Set `RUSTUP_TOOLCHAIN` to the framework's pinned toolchain for both builds. Compilation runs inside the frozen application, whose toolchain file can otherwise select a different compiler; pair validation rejects that mismatch before device measurement.
- Package one verified native ABI into an isolated signed APK with `python3 scripts/android_benchmark_package.py --help`; both revisions must share their application payload and build settings.
- Compile the device input helper with `./perf_android.sh build-route --android-jar SDK/android.jar --d8 BUILD_TOOLS/d8 --output OUTPUT`.
- Configure display size, density, input timing and distinct visible endpoints in a route JSON; examples live in [scripts/android/routes](../scripts/android/routes).
- Use `./perf_android.sh build-ocr --output OUTPUT` on macOS for image endpoint checks; restrict heading regions to exclude persistent navigation labels.
- Run `./perf_android.sh measure --serial SERIAL --route ROUTE --dex HELPER/classes.dex --a A/apk.json --b B/apk.json --output OUTPUT` for one locked ABAB BABA sequence.
- Add `--record --record-backend scrcpy` for comparison videos; recording runs and their nested windows are ineligible for FPS acceptance.
- Matching installed APK hashes reuse the on-device payload and skip redundant installation; differing payloads are transferred and verified before use.
- Each leg verifies the installed APK, first-gesture motion, both endpoints, foreground PID, input timing and device temperature; captures sit outside the measurement window.
- Reports preserve command output, failed legs and cleanup errors; diagnostic properties are restored before any performance result becomes eligible.
- `just gc` and `just gc-apply` include disposable benchmark snapshots under `${XDG_CACHE_HOME:-$HOME/.cache}/cranpose/benchmarks`; nested caches count once and recent descendants protect their parent.
- `just test-shell-helpers` covers archive integrity, provenance, sequence locking, route validation, cleanup, reporting and cache collection.
- The helper gate provisions pinned Pillow in `target/python-benchmark`; a global Pillow installation is unnecessary for CI.

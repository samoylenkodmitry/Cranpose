# Building Everbloom (Cranpose)

All commands run from this folder (`benchmarks/shopbench` of a Cranpose
checkout). The app is its own Cargo workspace and builds Cranpose from the
checkout it sits in.

## Toolchain

| Tool | Version |
| --- | --- |
| Rust | the checkout's `rust-toolchain.toml` (1.99.0), with the `aarch64-apple-ios` and `aarch64-apple-ios-sim` targets |
| Xcode | 27.2 (iOS SDK 27.2), for the SDK, `codesign` and the Simulator |

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```

## iPhone release build (benchmarked)

Release profile: `opt-level = 3`, `lto = true`, `codegen-units = 1`,
`strip = true`, `panic = "abort"`, the release profile of the Cranpose
workspace. Features: `cranpose` with `ios`, `renderer-wgpu` and
`image-loading`, no default features, the system font (SF Pro) at four
weights.

Signed for a device (an Apple Development identity and a provisioning profile
that covers `dev.whiskerrs.shopbench.cranpose`, such as the team's wildcard
profile):

```sh
CODESIGN_IDENTITY="Apple Development: <name> (<id>)" \
PROVISIONING_PROFILE=<path to the .mobileprovision> \
  ios/build-app.sh aarch64-apple-ios
```

Without them the bundle is ad-hoc signed (`codesign --sign -`), which builds
but does not install on an iPhone:

```sh
ios/build-app.sh aarch64-apple-ios
```

The app: `target/aarch64-apple-ios/release/Everbloom.app`.

## Install and launch on an iPhone

```sh
xcrun devicectl list devices
DEVICE=<identifier from the list>
xcrun devicectl device install app --device "$DEVICE" target/aarch64-apple-ios/release/Everbloom.app
xcrun devicectl device process launch --device "$DEVICE" --terminate-existing \
  --environment-variables '{"SHOP_API_URL":"http://192.168.1.20:8787/api"}' \
  dev.whiskerrs.shopbench.cranpose
```

Without `--environment-variables` the app reads
`https://apparel-large-catalog.mock.shop/api`.

## Simulator build

The same release profile for the Simulator:

```sh
ios/build-app.sh aarch64-apple-ios-sim
xcrun simctl install booted target/aarch64-apple-ios-sim/release/Everbloom.app
SIMCTL_CHILD_SHOP_API_URL=http://127.0.0.1:8787/api \
  xcrun simctl launch --terminate-running-process booted dev.whiskerrs.shopbench.cranpose
```

## Desktop

`cargo run --release` opens the app in a window of the phone's size on macOS,
Linux or Windows; `SHOP_API_URL` points it at a replay server.

## Measuring with the benchmark's tools

The benchmark's `tools/measure.py` reads each app from `~/shopbench-runs/`.
Add this app to its `APPS`:

```python
"cranpose": ("cranpose", RUNS / "cranpose/target/aarch64-apple-ios/release/Everbloom.app"),
```

and build it there, or pass `--app-path cranpose=<path to Everbloom.app>`.
The launch only, as DartNative ran it:

```sh
python3 tools/measure.py --udid <iPhone id> --host <the Mac's IP> --apps cranpose \
  --launch-runs 10 --journey-runs 0 --instruments ""
```

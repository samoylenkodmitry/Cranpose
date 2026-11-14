# Everbloom in Cranpose

The shop app of the [shop app benchmark](https://github.com/whiskerrs/benchmarks),
written with Cranpose. The benchmark builds the same small clothing shop with
many frameworks and measures each on an iPhone with the same tools: launch to
the first frame, launch to Home's content, app size, and a walk through the
app for memory, scrolling, CPU and energy.

The app follows the benchmark's prompt (`prompt/PROMPT.md` there, at commit
[`bf0fa20`](https://github.com/whiskerrs/benchmarks/tree/bf0fa20e4)): the same
screens, layout, data and requests, and the element names its XCUITest runner
looks for, set with `Modifier::test_tag`. [`NOTES.md`](NOTES.md) has the
decisions the prompt left open.

Results, the launch goal and the plan to reach it are in
[#2201](https://github.com/samoylenkodmitry/Cranpose/issues/2201).

## Attribution

The benchmark, its prompt, its design, its test runner and its measuring
tools are the work of its authors, under the MIT license in [`LICENSE`](LICENSE)
(Copyright (c) 2026 itome). The GraphQL documents in `src/api.rs` are copied
from its `prompt/storefront.graphql`, the colors and sizes in `src/theme.rs`
and the icon paths in `src/icons.rs` from its `design/`. The app's own code is
Cranpose's.

## The screens

| Module | Screen |
| --- | --- |
| `src/home.rs` | Home: collection chips, featured collection, collections carousel, best sellers |
| `src/collection.rs` | Collection: description, sort chips, a lazy two-column grid that loads page after page |
| `src/product.rs` | Product: image pager, options, description, recommendations, Add to cart |
| `src/search.rs` | Search: a field sent 300 ms after the typing stops, a lazy list of results |
| `src/cart.rs` | Cart: lines with quantity steppers, subtotal, Check out |
| `src/app.rs` | The three tabs: Home and Search keep a stack each (`NavHost`), Cart has one screen |

Each screen keeps its data in a view model (`viewModel`), so a screen covered
by another finds its data again.

## `SHOPBENCH home_ready`

The benchmark takes Home content as the moment the app writes the log line
`SHOPBENCH home_ready` to the iOS unified log. Its apps write it right after
Home's loaded data reaches the screen's views, before the frame that shows
them is drawn; SwiftUI writes it from `onAppear` of the loaded content.
`src/home_ready.rs` writes it at the same point, from a `SideEffect` of Home's
loaded content, once per process. It is the only line the app writes.

## Build for iOS

The app is its own Cargo workspace on the crates of the checkout it sits in.
`ios/build-app.sh` builds the Rust binary and assembles `Everbloom.app`
without an Xcode project: the iOS backend starts `UIApplicationMain` itself.

| Tool | Version |
| --- | --- |
| Rust | the checkout's `rust-toolchain.toml`, with the `aarch64-apple-ios` and `aarch64-apple-ios-sim` targets |
| Xcode | 27.2 (iOS SDK 27.2), for the SDK, `codesign` and the Simulator |

The build uses the release profile of the Cranpose workspace (`opt-level = 3`,
`lto = true`, `codegen-units = 1`, `strip = true`, `panic = "abort"`) and the
features `ios`, `renderer-wgpu` and `image-loading`, with the system font
(SF Pro) at four weights.

An iPhone build needs an Apple Development identity and a provisioning
profile that covers `dev.whiskerrs.shopbench.cranpose`, such as the team's
wildcard profile. From the checkout's root:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
CODESIGN_IDENTITY="Apple Development: <name> (<id>)" \
PROVISIONING_PROFILE=<path to the .mobileprovision> \
  just shopbench-ios
```

The app lands in `benchmarks/shopbench/target/aarch64-apple-ios/release/Everbloom.app`.
Without the two variables the bundle is ad-hoc signed, which builds but does
not install on an iPhone. `just shopbench-ios aarch64-apple-ios-sim` builds
the Simulator's bundle; `PROFILE=debug` builds without optimization.

To run it on an iPhone against a replay server:

```sh
xcrun devicectl device install app --device <device id> benchmarks/shopbench/target/aarch64-apple-ios/release/Everbloom.app
xcrun devicectl device process launch --device <device id> --terminate-existing \
  --environment-variables '{"SHOP_API_URL":"http://<the Mac's IP>:8787/api"}' \
  dev.whiskerrs.shopbench.cranpose
```

Without `SHOP_API_URL` the app reads `https://apparel-large-catalog.mock.shop/api`.
`cargo run --release` in this folder opens the app in a window of the phone's
size on macOS, Linux or Windows.

## Measure with the benchmark's tools

Clone [whiskerrs/benchmarks](https://github.com/whiskerrs/benchmarks) and check
out `bf0fa20`, the commit this app follows. Its `tools/README.md` describes
the tools; the steps for this app are:

1. Replay data. `python3 tools/replay/replay.py crawl` records the API
   answers and fetches every image once. `measure.py` then starts
   `replay.py serve` on port 8787 itself, and a request with no recording
   counts as a miss that invalidates the run.
2. The runner. Build `tools/runner` for the iPhone as its README says. Its
   XCUITest finds the app's elements by accessibility identifier, which
   Cranpose's iOS host sets from `Modifier::test_tag` once the test-tag
   change lands; until then the runner cannot find them.
3. Add the app to `APPS` in `tools/measure.py`:

   ```python
   "cranpose": ("cranpose", RUNS / "cranpose/target/aarch64-apple-ios/release/Everbloom.app"),
   ```

   and pass `--app-path cranpose=<path to Everbloom.app>` for a build kept
   elsewhere.
4. Run the protocol, for example the launch only:

   ```sh
   python3 tools/measure.py --udid <iPhone id> --host <the Mac's IP> --apps cranpose,swiftui \
     --launch-runs 10 --journey-runs 0 --instruments ""
   ```

The method used for the numbers in #2201, and why it differs from the
benchmark's per-launch recordings, is in that issue.

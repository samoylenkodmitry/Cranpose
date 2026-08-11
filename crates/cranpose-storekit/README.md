# cranpose-storekit

StoreKit 2 in-app purchases for [Cranpose](https://github.com/samoylenkodmitry/cranpose),
implementing the cross-platform [`cranpose_services::purchases`] API on iOS and
macOS.

```rust,no_run
use cranpose_services::purchases;

cranpose_storekit::register();
purchases::configure(&["com.example.app.pro"]);

// Read the snapshot from the frame loop — the store answers asynchronously.
let state = purchases::store_state();
// No store that will sell here: free in that case.
let unlocked = state.phase.cannot_sell() || state.owns("com.example.app.pro");
let price = state.display_price("com.example.app.pro").unwrap_or("");
```

On non-Apple targets `register()` is a no-op, the build script does nothing,
and the crate compiles away — so it can sit in an Android, desktop or web
dependency graph unchanged.

## How it works

StoreKit 2 is Swift-only: `Product`, `Transaction`, `Transaction.updates` and
the on-device JWS verification have no Objective-C surface. The build script
compiles `swift/storekit.swift` with `swiftc -emit-library -static` and links
the archive with `+whole-archive` so Swift's autolink records survive.

Two things about that link are worth knowing before changing it:

- **`cargo:rustc-link-arg` does not propagate from a dependency's build
  script.** An rlib is never linked, so a link-arg emitted here silently
  vanishes and the build stays green with the symbol missing. Only
  `rustc-link-lib` and `rustc-link-search` reach the final binary; the build
  script uses those exclusively.
- **Swift autolinking needs the toolchain path as well as the SDK path.**
  `libswiftCompatibility56.a` and `libswiftCompatibilityPacks.a` live in the
  Xcode toolchain, not the SDK. Without that search path the link fails in a
  way that reads as "autolinking is broken".

There is no `Frameworks/` copy step and no `@rpath`: the Swift runtime has
shipped in the OS since iOS 12.2, so the SDK's `.tbd` stubs are all the linker
needs.

## Requirements

- **`IPHONEOS_DEPLOYMENT_TARGET=15.0` (or `MACOSX_DEPLOYMENT_TARGET=12.0`) must
  be exported.** The build script refuses to run without it, on purpose.
  Swift's *concurrency* runtime has been in the OS only since iOS 15 / macOS
  12; link below that and `libswift_Concurrency` resolves against Xcode's
  Swift 5.5 back-deployment copy, whose install name is `@rpath/…`. The build
  stays green and the app dies at launch with "Library not loaded". rustc
  reads the same variable when it links the final binary, which is why the
  build script insists rather than defaulting.
- Xcode installed and selected (`xcode-select -p`), for `swiftc` and `xcrun`.

## Verifying a link

```bash
export IPHONEOS_DEPLOYMENT_TARGET=15.0
cargo build -p cranpose-storekit --example link_check --target aarch64-apple-ios
otool -L target/aarch64-apple-ios/debug/examples/link_check | grep swift
```

Every line must be an absolute `/usr/lib/swift/…` path. A single `@rpath/…`
entry means the deployment target slipped and the binary will not launch.
Measured on device, simulator and macOS host with Xcode 26.5: all absolute.

## License

Apache-2.0

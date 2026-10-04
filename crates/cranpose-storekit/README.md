# Cranpose StoreKit

`cranpose-storekit` connects Apple's StoreKit 2 purchase service to [`cranpose-services::purchases`](https://docs.rs/cranpose-services/latest/cranpose_services/purchases/index.html). The adapter reads products and transactions, verifies signed transactions on device, and publishes store state and purchase events to Cranpose.

## Setup

Enable `cranpose/storekit` in an iOS or macOS app. The iOS launcher registers the adapter; a macOS app calls `register` before product configuration. Direct users add `cranpose-storekit` and call `register` before product configuration. The build uses Xcode's `swiftc` to compile the StoreKit 2 shim.

```rust,no_run
use cranpose_services::purchases;

fn configure_purchases() {
    cranpose_storekit::register();
    purchases::configure(&["com.example.pro"]);
}

fn purchase_pro() {
    purchases::purchase("com.example.pro");
}
```

Read `purchases::store_state()` for product prices and owned entitlements. Use `rememberStoreState` and `rememberPurchaseEvents` inside composables to observe updates. Product IDs must match App Store Connect products; store replies arrive asynchronously.

## Apple build requirements

Build Apple targets with active Xcode from `xcode-select`. iOS 15 and macOS 12 are minimum targets and include Swift concurrency. The build script warns and assumes the minimum when `IPHONEOS_DEPLOYMENT_TARGET` or `MACOSX_DEPLOYMENT_TARGET` is absent. Set the same variable in the app build environment so `rustc` and `swiftc` use the same target version. The build script requires those minimum versions.

The build script compiles the Swift shim into a static archive and links the archive into the Rust app with Cargo link directives. Swift callbacks can arrive on any thread; the adapter protects shared state with a mutex before app code reads store snapshots. StoreKit 2 requires products configured in App Store Connect and a signed app for device purchase tests. Other target families compile the public `register` function as an empty adapter.

## Link check

The crate includes `examples/link_check.rs`. On a Mac with Xcode, build the example for a simulator or device target:

```sh
export IPHONEOS_DEPLOYMENT_TARGET=15.0
cargo build -p cranpose-storekit --example link_check --target aarch64-apple-ios
otool -L target/aarch64-apple-ios/debug/examples/link_check
```

## Links

- [API documentation](https://docs.rs/cranpose-storekit/latest/cranpose_storekit/)
- [Purchases service API](https://docs.rs/cranpose-services/latest/cranpose_services/purchases/index.html)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-storekit)
- [Apache-2.0 license](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose-storekit/LICENSE)
- [MIT license](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose-storekit/LICENSE-MIT)

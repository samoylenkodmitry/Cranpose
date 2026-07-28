//! Links every FFI entry point into an executable so `otool -L` can prove
//! which Swift runtime libraries the binary ended up depending on.
//!
//! The failure this guards against is silent: if the linker resolves
//! `libswift_Concurrency` against Xcode's Swift 5.5 back-deployment copy
//! instead of the one in the OS, the dependency is recorded as
//! `@rpath/libswift_Concurrency.dylib` and the app dies at launch with
//! "Library not loaded" — on a user's device, not on the build machine.
//!
//! ```text
//! cargo build -p cranpose-storekit --example link_check --target aarch64-apple-ios
//! otool -L target/aarch64-apple-ios/debug/examples/link_check | grep swift
//! ```
//!
//! Every `swift` line must be an absolute `/usr/lib/swift/...` path.

fn main() {
    cranpose_storekit::register();
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    {
        use cranpose_services::purchases::{self, Purchases};
        purchases::configure(&["com.example.link.check"]);
        let backend = cranpose_storekit::StoreKitPurchases;
        backend.purchase("com.example.link.check");
        backend.restore();
        println!("{:?}", purchases::store_state());
    }
}

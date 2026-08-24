#[cfg(target_os = "ios")]
fn main() {
    desktop_app::ios_entry_point();
}

// The `cranpose-ios` binary only runs on iOS. The `ios` feature can still be
// enabled on other targets (for example `cargo ... --all-features` checks),
// where this binary has no entry point to call.
#[cfg(not(target_os = "ios"))]
fn main() {}

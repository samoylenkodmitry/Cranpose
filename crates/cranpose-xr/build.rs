#![allow(
    clippy::disallowed_methods,
    clippy::print_stdout,
    reason = "a build script reads cargo's environment and prints cargo's directives"
)]

// Names the builds by what the `openxr` feature gives them, so that no module repeats the long
// conditions. `headset` is the OpenXR backend: a platform with a headset runtime. `desktop_run`
// is `cranpose_xr::run`: a desktop that opens a flat window where it has no headset.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(headset)");
    println!("cargo::rustc-check-cfg=cfg(desktop_run)");
    if std::env::var_os("CARGO_FEATURE_OPENXR").is_none() {
        return;
    }
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(target.as_str(), "android" | "linux" | "windows") {
        println!("cargo::rustc-cfg=headset");
    }
    if matches!(target.as_str(), "linux" | "windows" | "macos") {
        println!("cargo::rustc-cfg=desktop_run");
    }
}

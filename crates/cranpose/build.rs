#![allow(
    clippy::disallowed_methods,
    reason = "a build script reads cargo's environment"
)]

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    first_screen_pipelines();
    println!("cargo::rerun-if-changed=examples/localization/locales");
    println!("cargo::rerun-if-env-changed=CARGO_NDK_ANDROID_PLATFORM");
    println!("cargo::rerun-if-env-changed=ANDROID_PLATFORM");
}

/// Writes `OUT_DIR/first_screen_pipelines.bin`: the file
/// `CRANPOSE_FIRST_SCREEN_PIPELINES` names, the pipelines a recording run of
/// an earlier build of the app saw its first screen draw with, or nothing.
fn first_screen_pipelines() {
    println!("cargo::rerun-if-env-changed=CRANPOSE_FIRST_SCREEN_PIPELINES");
    let Some(out) = std::env::var_os("OUT_DIR") else {
        println!("cargo::error=OUT_DIR unset");
        return;
    };
    let records = match std::env::var_os("CRANPOSE_FIRST_SCREEN_PIPELINES") {
        Some(path) => {
            println!(
                "cargo::rerun-if-changed={}",
                std::path::Path::new(&path).display()
            );
            match std::fs::read(&path) {
                Ok(records) => records,
                Err(error) => {
                    println!(
                        "cargo::error=CRANPOSE_FIRST_SCREEN_PIPELINES {}: {error}",
                        std::path::Path::new(&path).display()
                    );
                    return;
                }
            }
        }
        None => Vec::new(),
    };
    if let Err(error) = std::fs::write(
        std::path::Path::new(&out).join("first_screen_pipelines.bin"),
        records,
    ) {
        println!("cargo::error=first_screen_pipelines.bin: {error}");
    }
}

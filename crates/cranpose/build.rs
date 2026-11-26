#![allow(
    clippy::disallowed_methods,
    reason = "a build script reads cargo's environment"
)]

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(cranpose_first_screen_glsl)");
    println!("cargo::rustc-check-cfg=cfg(cranpose_gpu_video)");
    emit_gpu_video_cfg();
    first_screen_pipelines();
    println!("cargo::rerun-if-changed=examples/localization/locales");
    println!("cargo::rerun-if-env-changed=CARGO_NDK_ANDROID_PLATFORM");
    println!("cargo::rerun-if-env-changed=ANDROID_PLATFORM");
}

fn emit_gpu_video_cfg() {
    let has = |name: &str| std::env::var_os(name).is_some();
    if !has("CARGO_FEATURE_VIDEO") || !has("CARGO_FEATURE_RENDERER_WGPU") {
        return;
    }
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let supported = matches!(os.as_str(), "macos" | "ios" | "windows" | "linux")
        || (os == "android" && has("CARGO_FEATURE_ANDROID"))
        || (arch == "wasm32" && os == "unknown" && has("CARGO_FEATURE_WEB"));
    if supported {
        println!("cargo::rustc-cfg=cranpose_gpu_video");
    }
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
    // A page with records also fetches the exact GLSL of its first screen's
    // WebGL stages from beside its wasm (`src/web/first_screen_glsl.rs`).
    if !records.is_empty() {
        println!("cargo::rustc-cfg=cranpose_first_screen_glsl");
    }
    if let Err(error) = std::fs::write(
        std::path::Path::new(&out).join("first_screen_pipelines.bin"),
        records,
    ) {
        println!("cargo::error=first_screen_pipelines.bin: {error}");
    }
}

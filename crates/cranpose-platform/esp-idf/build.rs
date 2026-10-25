//! Stops a build whose ESP-IDF links the per-frame code script of another
//! copy of this crate. esp-idf-sys configures its CMake project once and
//! runs it again only when its own settings change, so an application that
//! moves to another Cranpose revision keeps the components of the revision
//! it first built: the old IRAM function list and C code.

#![allow(
    clippy::disallowed_methods,
    reason = "a build script reads cargo's environment"
)]

use std::{env, path::Path, process::ExitCode};

fn main() -> ExitCode {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=DEP_ESP_IDF_EMBUILD_LINK_ARGS");
    let (Some(args), Some(manifest_dir)) = (
        env::var_os("DEP_ESP_IDF_EMBUILD_LINK_ARGS"),
        env::var_os("CARGO_MANIFEST_DIR"),
    ) else {
        return ExitCode::SUCCESS;
    };
    let args = args.to_string_lossy();
    let marker = Path::new("components").join("cranpose_hot_code");
    let own = Path::new(&manifest_dir).join(&marker);
    let marker = marker.to_string_lossy();
    if !args.contains(marker.as_ref()) || args.contains(own.to_string_lossy().as_ref()) {
        return ExitCode::SUCCESS;
    }
    let linked = args
        .split(['\'', '"', ' '])
        .find(|arg| arg.contains(marker.as_ref()))
        .unwrap_or_default();
    let profile = match env::var("PROFILE").as_deref() {
        Ok("release") => " --release",
        _ => "",
    };
    let target = env::var("TARGET").unwrap_or_default();
    eprintln!(
        "ESP-IDF was built with the components of another copy of cranpose-platform-esp-idf \
         ({linked}): esp-idf-sys keeps the components it first built. \
         Run `cargo clean -p esp-idf-sys{profile} --target {target}` and build again."
    );
    ExitCode::FAILURE
}

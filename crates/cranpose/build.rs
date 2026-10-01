fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CARGO_NDK_ANDROID_PLATFORM");
    println!("cargo::rerun-if-env-changed=ANDROID_PLATFORM");
}

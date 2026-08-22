//! Compiles `swift/storekit.swift` into a static archive and tells Cargo to
//! link it — using **only** directives that propagate from a dependency's
//! build script all the way to the final binary link.
//!
//! The hazard: `cargo:rustc-link-arg` applies only to the crate that emitted
//! it, and an rlib is never linked, so a link-arg emitted here would silently
//! vanish — the build stays green and the symbol is simply gone. Measured: a
//! dependency emitting `cargo:rustc-link-arg=-lNO_SUCH_LIB` produces a binary
//! that links and runs fine, while the same bogus name via `rustc-link-lib`
//! fails the link loudly. `rustc-link-lib` and `rustc-link-search` *do*
//! propagate, so everything below is expressed with those two.

use std::env;
use std::path::PathBuf;
use std::process::Command;

const LIB_NAME: &str = "cranpose_storekit_swift";

fn main() {
    println!("cargo:rerun-if-changed=swift/storekit.swift");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=IPHONEOS_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    // Non-Apple targets: no-op. The crate's Rust side is cfg'd to match, so
    // Linux/Android/Windows/wasm see an empty crate and an empty build script.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "ios" && target_os != "macos" {
        return;
    }

    // Device vs simulator is NOT visible in `CARGO_CFG_TARGET_OS` — both iOS
    // targets report "ios". Read the full triple: a `-sim` suffix means
    // simulator, and `x86_64-apple-ios` is simulator-only (there is no Intel
    // iPhone).
    let target = env::var("TARGET").expect("TARGET");
    let arch = match env::var("CARGO_CFG_TARGET_ARCH")
        .unwrap_or_default()
        .as_str()
    {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => {
            println!(
                "cargo:warning=cranpose-storekit: unsupported Apple arch {other}, skipping Swift"
            );
            return;
        }
    };
    let simulator = target.ends_with("-sim") || target == "x86_64-apple-ios";

    // (xcrun --sdk name, swiftc -target suffix, toolchain lib subdir)
    let (sdk_name, swift_triple, platform_dir) = if target_os == "ios" {
        let deployment = deployment_target("IPHONEOS_DEPLOYMENT_TARGET", (15, 0), "iOS 15");
        if simulator {
            (
                "iphonesimulator",
                format!("{arch}-apple-ios{deployment}-simulator"),
                "iphonesimulator",
            )
        } else {
            (
                "iphoneos",
                format!("{arch}-apple-ios{deployment}"),
                "iphoneos",
            )
        }
    } else {
        let deployment = deployment_target("MACOSX_DEPLOYMENT_TARGET", (12, 0), "macOS 12");
        (
            "macosx",
            format!("{arch}-apple-macosx{deployment}"),
            "macosx",
        )
    };

    let sdk_path = xcrun(&["--sdk", sdk_name, "--show-sdk-path"])
        .expect("xcrun --show-sdk-path (is Xcode installed and selected?)");
    let toolchain_lib_swift = xcrun(&["--find", "swiftc"])
        .map(PathBuf::from)
        // .../XcodeDefault.xctoolchain/usr/bin/swiftc -> .../usr/lib/swift
        .and_then(|p| Some(p.parent()?.parent()?.join("lib").join("swift")))
        .expect("locate the Swift toolchain lib directory");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let source = manifest.join("swift").join("storekit.swift");
    let archive = out_dir.join(format!("lib{LIB_NAME}.a"));

    // `-emit-library -static` => a .a; `-wmo -O` => whole-module optimization,
    // the shape a release Swift library ships in.
    let status = Command::new("swiftc")
        .args(["-emit-library", "-static", "-O", "-wmo"])
        .args(["-target", &swift_triple])
        .args(["-sdk", &sdk_path])
        .args(["-module-name", "CranposeStoreKit"])
        .arg("-o")
        .arg(&archive)
        .arg(&source)
        .status()
        .expect("run swiftc");
    assert!(status.success(), "swiftc failed for {swift_triple}");

    // --- the only directives that reach the final binary --------------------
    //
    // 1. Where our archive lives.
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    // 2. Link it whole, so every object — and therefore every
    //    LC_LINKER_OPTION autolink record swiftc embedded (-lswiftCore,
    //    -lswift_Concurrency, -framework StoreKit, …) — is seen by the linker.
    //    Without `+whole-archive` the linker drops objects nothing references
    //    yet, taking their autolink records with them.
    println!("cargo:rustc-link-lib=static:+whole-archive={LIB_NAME}");
    // 3. Where those autolinked Swift runtime stubs resolve. The SDK holds
    //    libswiftCore.tbd & friends (install name
    //    /usr/lib/swift/libswiftCore.dylib — in-OS since iOS 12.2, so no
    //    `Frameworks/` copy and no @rpath are needed).
    println!("cargo:rustc-link-search=native={sdk_path}/usr/lib/swift");
    // 4. The toolchain's back-deployment compatibility archives
    //    (libswiftCompatibility56.a, libswiftCompatibilityPacks.a). These live
    //    in the *toolchain*, not the SDK, and are in no published recipe —
    //    without this path the link fails in a way that reads as "autolinking
    //    is broken".
    let toolchain_platform = toolchain_lib_swift.join(platform_dir);
    if toolchain_platform.is_dir() {
        println!(
            "cargo:rustc-link-search=native={}",
            toolchain_platform.display()
        );
    }

    // This crate's OWN test and example binaries only. rustc defaults
    // `aarch64-apple-darwin` to macOS 11 — below the in-OS concurrency floor —
    // so without an rpath `cargo test -p cranpose-storekit` aborts at start-up
    // on a machine that is perfectly capable of running it.
    //
    // This is deliberately NOT the mechanism apps rely on: `rustc-link-arg`
    // does not propagate out of a dependency's build script, so it cannot
    // reach a downstream binary. Apps export the deployment target instead.
    if target_os == "macos" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }

    // Note: on Mach-O there is no `swiftrt.o` to fold in — that is an
    // ELF/COFF artifact. The Swift runtime finds its metadata through Mach-O
    // section headers, so the archive ships as swiftc emitted it.
}

/// The deployment target for this platform.
///
/// This is load-bearing, and its failure mode is nasty: Swift's **concurrency**
/// runtime has shipped in the OS only since iOS 15 / macOS 12. Link below that
/// and `libswift_Concurrency` resolves against Xcode's Swift 5.5
/// back-deployment copy in `usr/lib/swift-5.5/<platform>/`, whose install name
/// is `@rpath/libswift_Concurrency.dylib`. Nothing warns — the build is green
/// and the app dies at launch with "Library not loaded".
///
/// The same variable is what *rustc* reads when it links the final binary, so
/// setting it here only fixes `swiftc`'s half. It is the **app's** build that
/// must export it, which is why an unset variable is a warning aimed at the
/// app author rather than a value this crate can fix. An explicitly wrong
/// value is a different thing and does fail the build.
///
/// Verify a real binary with the `link_check` example: every `swift` line from
/// `otool -L` must be an absolute `/usr/lib/swift/…` path.
fn deployment_target(var: &str, floor: (u32, u32), floor_label: &str) -> String {
    let floor_text = format!("{}.{}", floor.0, floor.1);
    let Ok(value) = env::var(var) else {
        println!(
            "cargo:warning=cranpose-storekit: {var} is not set; assuming {floor_text}. \
             Export {var}={floor_text} in the app's build so rustc links with the same \
             deployment target, otherwise libswift_Concurrency binds to @rpath and the app \
             crashes at launch."
        );
        return floor_text;
    };
    let mut parts = value.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    assert!(
        (major, minor) >= floor,
        "cranpose-storekit: {var}={value} is below {floor_label}, the StoreKit 2 floor. \
         Swift's concurrency runtime is only in the OS from {floor_label} on; linking below it \
         produces an @rpath dependency and a launch-time crash."
    );
    value
}

fn xcrun(args: &[&str]) -> Option<String> {
    let out = Command::new("xcrun").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

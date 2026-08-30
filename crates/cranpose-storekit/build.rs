use std::{env, path::PathBuf, process::Command};

const LIB_NAME: &str = "cranpose_storekit_swift";

fn main() {
    println!("cargo:rerun-if-changed=swift/storekit.swift");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=IPHONEOS_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "ios" && target_os != "macos" {
        return;
    }

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
        .and_then(|p| Some(p.parent()?.parent()?.join("lib").join("swift")))
        .expect("locate the Swift toolchain lib directory");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let source = manifest.join("swift").join("storekit.swift");
    let archive = out_dir.join(format!("lib{LIB_NAME}.a"));

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

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static:+whole-archive={LIB_NAME}");
    println!("cargo:rustc-link-search=native={sdk_path}/usr/lib/swift");
    let toolchain_platform = toolchain_lib_swift.join(platform_dir);
    if toolchain_platform.is_dir() {
        println!(
            "cargo:rustc-link-search=native={}",
            toolchain_platform.display()
        );
    }

    if target_os == "macos" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}

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

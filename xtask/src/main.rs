use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(());
    };

    match command {
        "bundle-macos" if args[1..].iter().any(|arg| arg == "--help" || arg == "-h") => {
            print_bundle_usage();
            Ok(())
        }
        "bundle-macos" => bundle_macos(BundleMacosOptions::parse(&args[1..])?),
        "binary-size" if args[1..].iter().any(|arg| arg == "--help" || arg == "-h") => {
            print_binary_size_usage();
            Ok(())
        }
        "binary-size" => report_binary_size(BinarySizeOptions::parse(&args[1..])?),
        "dist-min" if args[1..].iter().any(|arg| arg == "--help" || arg == "-h") => {
            print_dist_min_usage();
            Ok(())
        }
        "dist-min" => build_dist_min(DistMinOptions::parse(&args[1..])?),
        "dependency-budget" if args[1..].iter().any(|arg| arg == "--help" || arg == "-h") => {
            print_dependency_budget_usage();
            Ok(())
        }
        "dependency-budget" => check_dependency_budget(DependencyBudgetOptions::parse(&args[1..])?),
        "help" | "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        other => Err(format!("unknown xtask command `{other}`")),
    }
}

fn print_usage() {
    eprintln!(
        "usage: cargo xtask <command> [options]\n\
         \n\
         commands:\n\
           bundle-macos        Build a macOS .app bundle\n\
           binary-size         Build or inspect a binary and print its file size\n\
           dist-min            Build the smallest binary (nightly build-std + immediate-abort)\n\
           dependency-budget   Fail if new duplicate dependency families appear\n\
         \n\
         bundle-macos options:\n\
           --package <name>       Cargo package to build [desktop-app]\n\
           --bin <name>           Cargo binary to bundle [desktop-app]\n\
           --profile <name>       Cargo profile to build [release]\n\
           --app-name <name>      macOS app bundle name [Cranpose Demo]\n\
           --bundle-id <id>       CFBundleIdentifier [io.cranpose.demo]\n\
           --out-dir <path>       Bundle output directory [target/macos-bundles]\n\
           --resources <path>     Directory copied into Contents/Resources\n\
           --target <triple>      Cargo target triple\n\
           --no-build             Bundle an already built binary\n\
           --sign-identity <id>   codesign identity to seal the bundle [ad-hoc \"-\"]"
    );
}

/// Every target triple the project ships, from the release workflow and the
/// `ios-sim`/`ios-device`/`android`/`web` recipes.
///
/// The dependency budget resolves the graph for all of them at once, so its
/// verdict is identical on every host. Without the pins `cargo tree` filters by
/// host platform, and a budget that is green on a Linux CI runner can be red on
/// macOS. Per-architecture entries matter as much as per-OS ones: families like
/// `windows_x86_64_msvc` are architecture-specific, so dropping an Android ABI
/// or the iOS simulator here would let a split hide. Resolving all of them
/// costs no measurable time over resolving one.
///
/// Adding a shipped target means adding it here.
const SHIPPED_TARGETS: &[&str] = &[
    "aarch64-apple-darwin",
    "aarch64-apple-ios",
    "aarch64-apple-ios-sim",
    "aarch64-linux-android",
    "armv7-linux-androideabi",
    "i686-linux-android",
    "wasm32-unknown-unknown",
    "x86_64-linux-android",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
];

/// A duplicate dependency version family the workspace knowingly carries.
#[derive(Debug, PartialEq, Eq)]
struct DuplicateDebt {
    family: &'static str,
    reason: &'static str,
}

/// Duplicate dependency version families in the default-feature graph.
///
/// Every entry is upstream debt: the crate pinning the old family is at its
/// latest published release, so the split cannot be collapsed from this side.
/// Each reason names the concrete upstream event that clears it. The budget
/// fails on any family missing from this table, and also on any entry whose
/// split no longer exists, so the table shrinks the moment that event lands.
///
/// These are not a licence to add duplicates. A split this workspace can
/// collapse itself -- by aligning a version, dropping a dependency, or
/// patching a crate to an upstream rev the way `gpu-descriptor` is patched --
/// must be collapsed instead of recorded here.
const WORKSPACE_DUPLICATE_DEBT: &[DuplicateDebt] = &[
    DuplicateDebt {
        family: "jni-sys",
        reason: "ndk 0.9.0 and ndk-sys 0.6.0 (latest) pin jni-sys ^0.3 while jni 0.22 is on ^0.4",
    },
    DuplicateDebt {
        family: "objc2",
        reason: "winit 0.31 is still a beta; accesskit_macos 0.26.3 (latest) holds objc2 0.5 on purpose until winit 0.31 ships stable (AccessKit/accesskit#616), while winit-appkit 0.31.0-beta.2 is already on 0.6",
    },
    DuplicateDebt {
        family: "objc2-app-kit",
        reason: "follows the objc2 split via accesskit_macos 0.26.3",
    },
    DuplicateDebt {
        family: "objc2-foundation",
        reason: "follows the objc2 split via accesskit_macos 0.26.3",
    },
    DuplicateDebt {
        family: "thiserror",
        reason: "ndk 0.9.0 (latest) pins thiserror ^1 while the workspace is on 2",
    },
    DuplicateDebt {
        family: "thiserror-impl",
        reason: "follows the thiserror split via ndk 0.9.0",
    },
    DuplicateDebt {
        family: "windows-sys",
        reason: "winit-win32 0.31.0-beta.2 pins ^0.59 and arboard 3.6.1 pins <0.61 while the rest of the graph is on 0.61",
    },
    DuplicateDebt {
        family: "windows-targets",
        reason: "follows the windows-sys split",
    },
    DuplicateDebt {
        family: "windows_x86_64_msvc",
        reason: "follows the windows-targets split",
    },
];

/// Additional duplicate families that only appear with `--all-features`.
const ALL_FEATURES_EXTRA_DUPLICATE_DEBT: &[DuplicateDebt] = &[DuplicateDebt {
    family: "env_filter",
    reason: "android_logger 0.15.1 (latest) pins env_filter ^0.1 while env_logger 0.11 is past 1.0",
}];

const RENDERER_PIXELS_FORBIDDEN_PACKAGES: &[&str] =
    &["pixels", "wgpu", "wgpu-core", "wgpu-hal", "naga"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DependencyBudgetScope {
    Workspace,
    AllFeatures,
}

impl DependencyBudgetScope {
    fn label(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::AllFeatures => "workspace all-features",
        }
    }

    fn cargo_tree_args(self) -> Vec<String> {
        let mut args: Vec<String> = ["tree", "--duplicates", "--workspace", "--color", "never"]
            .map(str::to_owned)
            .into();
        if let Self::AllFeatures = self {
            args.push("--all-features".to_owned());
        }
        args.extend(shipped_target_args());
        args
    }

    fn recorded_debt(self) -> Vec<&'static DuplicateDebt> {
        match self {
            Self::Workspace => WORKSPACE_DUPLICATE_DEBT.iter().collect(),
            Self::AllFeatures => WORKSPACE_DUPLICATE_DEBT
                .iter()
                .chain(ALL_FEATURES_EXTRA_DUPLICATE_DEBT)
                .collect(),
        }
    }
}

fn shipped_target_args() -> impl Iterator<Item = String> {
    SHIPPED_TARGETS
        .iter()
        .flat_map(|target| ["--target".to_owned(), (*target).to_owned()])
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DependencyBudgetOptions {
    scopes: Vec<DependencyBudgetScope>,
    explain: bool,
}

impl DependencyBudgetOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut scopes = Vec::new();
        let mut explain = false;

        for arg in args {
            match arg.as_str() {
                "--explain" => explain = true,
                "--workspace-only" => {
                    scopes.clear();
                    scopes.push(DependencyBudgetScope::Workspace);
                }
                "--all-features-only" => {
                    scopes.clear();
                    scopes.push(DependencyBudgetScope::AllFeatures);
                }
                other => return Err(format!("unknown dependency-budget option `{other}`")),
            }
        }

        if scopes.is_empty() {
            scopes.push(DependencyBudgetScope::Workspace);
            scopes.push(DependencyBudgetScope::AllFeatures);
        }

        Ok(Self { scopes, explain })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CargoBinaryOptions {
    package: String,
    bin: String,
    profile: String,
    target: Option<String>,
    manifest_path: Option<PathBuf>,
    patch_workspace_cranpose: bool,
}

impl Default for CargoBinaryOptions {
    fn default() -> Self {
        Self {
            package: "desktop-app".to_owned(),
            bin: "desktop-app".to_owned(),
            profile: "release".to_owned(),
            target: None,
            manifest_path: None,
            patch_workspace_cranpose: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BundleMacosOptions {
    package: String,
    bin: String,
    profile: String,
    app_name: String,
    bundle_id: String,
    out_dir: PathBuf,
    resources: Option<PathBuf>,
    target: Option<String>,
    build: bool,
    sign_identity: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BinarySizeOptions {
    binary: CargoBinaryOptions,
    build: bool,
    max_bytes: Option<u64>,
}

impl BinarySizeOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            binary: CargoBinaryOptions::default(),
            build: true,
            max_bytes: None,
        };

        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--package" => {
                    options.binary.package = required_value(args, &mut index, "--package")?
                }
                "--bin" => options.binary.bin = required_value(args, &mut index, "--bin")?,
                "--profile" => {
                    options.binary.profile = required_value(args, &mut index, "--profile")?
                }
                "--target" => {
                    options.binary.target = Some(required_value(args, &mut index, "--target")?)
                }
                "--manifest-path" => {
                    options.binary.manifest_path = Some(PathBuf::from(required_value(
                        args,
                        &mut index,
                        "--manifest-path",
                    )?))
                }
                "--max-bytes" => {
                    let value = required_value(args, &mut index, "--max-bytes")?;
                    options.max_bytes = Some(parse_u64_option("--max-bytes", &value)?);
                }
                "--patch-workspace-cranpose" => {
                    options.binary.patch_workspace_cranpose = true;
                }
                "--no-build" => options.build = false,
                other => return Err(format!("unknown binary-size option `{other}`")),
            }
            index += 1;
        }

        validate_non_empty("package", &options.binary.package)?;
        validate_non_empty("bin", &options.binary.bin)?;
        validate_non_empty("profile", &options.binary.profile)?;

        Ok(options)
    }
}

impl BundleMacosOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            package: "desktop-app".to_owned(),
            bin: "desktop-app".to_owned(),
            profile: "release".to_owned(),
            app_name: "Cranpose Demo".to_owned(),
            bundle_id: "io.cranpose.demo".to_owned(),
            out_dir: PathBuf::from("target/macos-bundles"),
            resources: None,
            target: None,
            build: true,
            sign_identity: None,
        };

        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--package" => options.package = required_value(args, &mut index, "--package")?,
                "--bin" => options.bin = required_value(args, &mut index, "--bin")?,
                "--profile" => options.profile = required_value(args, &mut index, "--profile")?,
                "--app-name" => options.app_name = required_value(args, &mut index, "--app-name")?,
                "--bundle-id" => {
                    options.bundle_id = required_value(args, &mut index, "--bundle-id")?
                }
                "--out-dir" => {
                    options.out_dir = PathBuf::from(required_value(args, &mut index, "--out-dir")?)
                }
                "--resources" => {
                    options.resources = Some(PathBuf::from(required_value(
                        args,
                        &mut index,
                        "--resources",
                    )?))
                }
                "--target" => options.target = Some(required_value(args, &mut index, "--target")?),
                "--no-build" => options.build = false,
                "--sign-identity" => {
                    options.sign_identity =
                        Some(required_value(args, &mut index, "--sign-identity")?)
                }
                other => return Err(format!("unknown bundle-macos option `{other}`")),
            }
            index += 1;
        }

        validate_bundle_id(&options.bundle_id)?;
        validate_non_empty("package", &options.package)?;
        validate_non_empty("bin", &options.bin)?;
        validate_non_empty("profile", &options.profile)?;
        validate_non_empty("app-name", &options.app_name)?;

        Ok(options)
    }
}

fn print_bundle_usage() {
    eprintln!("usage: cargo xtask bundle-macos [--package desktop-app] [--bin desktop-app]");
}

fn print_binary_size_usage() {
    eprintln!(
        "usage: cargo xtask binary-size [--manifest-path path] [--package desktop-app] [--bin desktop-app] [--profile release] [--max-bytes N] [--patch-workspace-cranpose] [--no-build]"
    );
}

fn print_dist_min_usage() {
    eprintln!(
        "usage: cargo xtask dist-min [--manifest-path path] [--package desktop-app] [--bin desktop-app] [--profile release-small] [--target triple] [--max-bytes N] [--patch-workspace-cranpose] [--features list] [--no-default-features]\n\
         \n\
         Builds the smallest possible binary: nightly cargo with -Zbuild-std,\n\
         size-tuned std, and immediate-abort panics. Requires the nightly\n\
         toolchain with the rust-src component."
    );
}

fn print_dependency_budget_usage() {
    eprintln!(
        "usage: cargo xtask dependency-budget [--workspace-only | --all-features-only] [--explain]"
    );
}

fn required_value(args: &[String], index: &mut usize, name: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("{name} requires a value"))
}

fn validate_non_empty(name: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{name} must not be empty"))
    } else {
        Ok(())
    }
}

fn parse_u64_option(name: &str, value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|error| format!("{name} must be an unsigned integer: {error}"))
}

fn validate_bundle_id(bundle_id: &str) -> Result<(), String> {
    validate_non_empty("bundle-id", bundle_id)?;
    if bundle_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
    {
        Ok(())
    } else {
        Err(format!(
            "bundle-id `{bundle_id}` contains unsupported characters"
        ))
    }
}

fn bundle_macos(options: BundleMacosOptions) -> Result<(), String> {
    let workspace = workspace_root()?;
    let binary_options = bundle_binary_options(&options);
    if options.build {
        build_binary(&workspace, &binary_options)?;
    }

    let binary = built_binary_path(&workspace, &binary_options);
    if !binary.exists() {
        return Err(format!(
            "built binary `{}` does not exist; run without --no-build or check --profile/--target",
            binary.display()
        ));
    }

    let bundle = create_bundle(&workspace, &options, &binary)?;
    // Always seal the bundle. The Rust linker ad-hoc signs the inner Mach-O with
    // a bundle-style signature that expects `_CodeSignature/CodeResources`, but
    // assembling the `.app` afterwards leaves that seal missing. A downloaded
    // (quarantined) bundle in that state fails Gatekeeper with "is damaged and
    // should be moved to the Trash". Re-signing the assembled bundle binds the
    // Info.plist and writes a valid seal; ad-hoc (`-`) is the default when no
    // developer identity is supplied.
    sign_bundle(
        &bundle,
        bundle_sign_identity(options.sign_identity.as_deref()),
    )?;

    println!("macOS app bundle: {}", bundle.display());
    Ok(())
}

fn report_binary_size(options: BinarySizeOptions) -> Result<(), String> {
    let workspace = workspace_root()?;
    let binary = stage_patched_package(&workspace, &options.binary)?;
    if options.build {
        build_binary(&workspace, &binary)?;
    }
    report_built_binary(&workspace, &binary, options.max_bytes)
}

fn report_built_binary(
    workspace: &Path,
    binary_options: &CargoBinaryOptions,
    max_bytes: Option<u64>,
) -> Result<(), String> {
    let binary = built_binary_path(workspace, binary_options);
    let metadata = fs::metadata(&binary)
        .map_err(|error| format!("failed to inspect `{}`: {error}", binary.display()))?;
    let bytes = metadata.len();
    println!(
        "{} {}:{} {} bytes ({:.2} MiB)",
        binary_options.profile,
        binary_options.package,
        binary_options.bin,
        bytes,
        bytes as f64 / (1024.0 * 1024.0)
    );
    if let Some(max_bytes) = max_bytes {
        if bytes > max_bytes {
            return Err(format!(
                "{} {}:{} size {} bytes exceeds max {} bytes",
                binary_options.profile,
                binary_options.package,
                binary_options.bin,
                bytes,
                max_bytes
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DistMinOptions {
    binary: CargoBinaryOptions,
    max_bytes: Option<u64>,
    features: Option<String>,
    no_default_features: bool,
}

impl DistMinOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            binary: CargoBinaryOptions {
                profile: "release-small".to_owned(),
                ..CargoBinaryOptions::default()
            },
            max_bytes: None,
            features: None,
            no_default_features: false,
        };

        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--package" => {
                    options.binary.package = required_value(args, &mut index, "--package")?
                }
                "--bin" => options.binary.bin = required_value(args, &mut index, "--bin")?,
                "--profile" => {
                    options.binary.profile = required_value(args, &mut index, "--profile")?
                }
                "--target" => {
                    options.binary.target = Some(required_value(args, &mut index, "--target")?)
                }
                "--manifest-path" => {
                    options.binary.manifest_path = Some(PathBuf::from(required_value(
                        args,
                        &mut index,
                        "--manifest-path",
                    )?))
                }
                "--max-bytes" => {
                    let value = required_value(args, &mut index, "--max-bytes")?;
                    options.max_bytes = Some(parse_u64_option("--max-bytes", &value)?);
                }
                "--patch-workspace-cranpose" => {
                    options.binary.patch_workspace_cranpose = true;
                }
                "--features" => {
                    options.features = Some(required_value(args, &mut index, "--features")?)
                }
                "--no-default-features" => options.no_default_features = true,
                other => return Err(format!("unknown dist-min option `{other}`")),
            }
            index += 1;
        }

        validate_non_empty("package", &options.binary.package)?;
        validate_non_empty("bin", &options.binary.bin)?;
        validate_non_empty("profile", &options.binary.profile)?;

        Ok(options)
    }
}

/// Extra rustc flags for the smallest distribution build: immediate-abort
/// panics remove the unwind/panic-message machinery from every crate
/// (including the rebuilt std), `location-detail=none` drops panic-site
/// file/line path strings, `fmt-debug=none` compiles `Debug` formatting to
/// no-ops, and lld's `--icf=all` folds identical functions (e.g. the
/// per-call-site composable shims). The trade: dist-min binaries abort
/// without a message and log `{:?}` payloads as empty — acceptable for
/// shipped builds, not for debugging. Requires `lld` on PATH.
const DIST_MIN_RUSTFLAGS: &str = "-Cpanic=immediate-abort -Zunstable-options \
     -Zlocation-detail=none -Zfmt-debug=none";

/// Linker extras that only apply to ELF/lld targets (Linux, Android):
/// `--icf=all` folds identical functions; macOS ld64 and MSVC link.exe have
/// no such flags (MSVC's /OPT:ICF is already on in release).
const DIST_MIN_LLD_RUSTFLAGS: &str = " -Clink-arg=-fuse-ld=lld -Clink-arg=-Wl,--icf=all";

fn dist_min_rustflags_for_target(target: &str) -> String {
    let mut flags = DIST_MIN_RUSTFLAGS.to_owned();
    // MSVC targets use SEH exceptions and REQUIRE unwind tables; rustc
    // hard-errors on force-unwind-tables=no there.
    if !target.contains("msvc") {
        flags.push_str(" -Cforce-unwind-tables=no");
    }
    if target.contains("linux") || target.contains("android") {
        flags.push_str(DIST_MIN_LLD_RUSTFLAGS);
    }
    flags
}

fn build_dist_min(mut options: DistMinOptions) -> Result<(), String> {
    let workspace = workspace_root()?;
    // -Zbuild-std requires an explicit --target.
    if options.binary.target.is_none() {
        options.binary.target = Some(host_triple()?);
    }
    options.binary = stage_patched_package(&workspace, &options.binary)?;

    // `cargo +nightly` only works through the rustup shim; inside `cargo
    // xtask` the PATH cargo is the toolchain binary itself, so go through
    // rustup explicitly. The outer cargo also exports its own toolchain via
    // CARGO/RUSTC/RUSTDOC, which must not leak into the nightly child.
    let nightly = pinned_nightly_channel(&workspace)?;
    let mut command = Command::new("rustup");
    command.args(["run", nightly.as_str(), "cargo", "build"]);
    command.env_remove("CARGO");
    command.env_remove("RUSTC");
    command.env_remove("RUSTDOC");
    if let Some(manifest_path) = options.binary.manifest_path.as_deref() {
        command.arg("--manifest-path").arg(manifest_path);
    }
    if options.binary.patch_workspace_cranpose {
        add_workspace_cranpose_patches(&mut command, &workspace);
    }
    command.args([
        "-p",
        options.binary.package.as_str(),
        "--bin",
        options.binary.bin.as_str(),
        "--profile",
        options.binary.profile.as_str(),
        "--target",
        options.binary.target.as_deref().expect("target set above"),
        "-Zunstable-options",
        "-Zbuild-std=std,panic_abort",
        "-Zbuild-std-features=optimize_for_size",
    ]);
    if options.no_default_features {
        command.arg("--no-default-features");
    }
    if let Some(features) = options.features.as_deref() {
        command.args(["--features", features]);
    }

    let mut rustflags = std::env::var("RUSTFLAGS").unwrap_or_default();
    if !rustflags.is_empty() {
        rustflags.push(' ');
    }
    rustflags.push_str(&dist_min_rustflags_for_target(
        options.binary.target.as_deref().expect("target set above"),
    ));
    command.env("RUSTFLAGS", rustflags);

    let status = command
        .status()
        .map_err(|error| format!("failed to run nightly cargo build: {error}"))?;
    if !status.success() {
        return Err(format!(
            "dist-min build failed with status {status}; it requires the \
             {nightly} toolchain with the rust-src component \
             (rustup toolchain install {nightly} --component rust-src) and \
             the lld linker on PATH"
        ));
    }

    report_built_binary(&workspace, &options.binary, options.max_bytes)
}

fn host_triple() -> Result<String, String> {
    let output = Command::new("rustc")
        .args(["-vV"])
        .output()
        .map_err(|error| format!("failed to run rustc -vV: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("rustc -vV returned invalid UTF-8: {error}"))?;
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV did not report a host triple".to_owned())
}

fn bundle_binary_options(options: &BundleMacosOptions) -> CargoBinaryOptions {
    CargoBinaryOptions {
        package: options.package.clone(),
        bin: options.bin.clone(),
        profile: options.profile.clone(),
        target: options.target.clone(),
        manifest_path: None,
        patch_workspace_cranpose: false,
    }
}

/// The nightly toolchain named by `rust-toolchain-nightly.toml`.
///
/// `dist-min` needs nightly cargo for `-Zbuild-std`, but spelling the channel
/// `nightly` would build release artifacts against whatever nightly the host
/// happens to hold that day. Reading the pin keeps a tag reproducible.
fn pinned_nightly_channel(workspace: &Path) -> Result<String, String> {
    let path = workspace.join("rust-toolchain-nightly.toml");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("channel"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .filter_map(|rest| rest.trim().strip_prefix('"'))
        .filter_map(|rest| rest.strip_suffix('"'))
        .next()
        .map(str::to_owned)
        .ok_or_else(|| format!("no `channel = \"...\"` entry in {}", path.display()))
}

fn workspace_root() -> Result<PathBuf, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|error| format!("failed to run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo metadata returned invalid UTF-8: {error}"))?;
    parse_workspace_root(&stdout)
        .ok_or_else(|| "cargo metadata did not include workspace_root".to_owned())
}

fn parse_workspace_root(metadata_json: &str) -> Option<PathBuf> {
    let key = "\"workspace_root\":\"";
    let start = metadata_json.find(key)? + key.len();
    let tail = &metadata_json[start..];
    let end = tail.find('"')?;
    Some(PathBuf::from(unescape_json_string(&tail[..end])))
}

fn unescape_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn build_binary(workspace: &Path, options: &CargoBinaryOptions) -> Result<(), String> {
    let mut command = Command::new("cargo");
    command.arg("build");
    if let Some(manifest_path) = options.manifest_path.as_deref() {
        command.arg("--manifest-path").arg(manifest_path);
    }
    if options.patch_workspace_cranpose {
        add_workspace_cranpose_patches(&mut command, workspace);
    }
    command.args([
        "-p",
        options.package.as_str(),
        "--bin",
        options.bin.as_str(),
        "--profile",
        options.profile.as_str(),
    ]);
    if let Some(target) = options.target.as_deref() {
        command.args(["--target", target]);
    }

    let status = command
        .status()
        .map_err(|error| format!("failed to run cargo build: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build failed with status {status}"))
    }
}

const WORKSPACE_CRANPOSE_PATCHES: &[(&str, &str)] = &[
    ("cranpose", "crates/cranpose"),
    ("cranpose-animation", "crates/cranpose-animation"),
    ("cranpose-app-shell", "crates/cranpose-app-shell"),
    ("cranpose-core", "crates/cranpose-core"),
    ("cranpose-foundation", "crates/cranpose-foundation"),
    ("cranpose-macros", "crates/cranpose-macros"),
    (
        "cranpose-platform-android",
        "crates/cranpose-platform/android",
    ),
    (
        "cranpose-platform-desktop-winit",
        "crates/cranpose-platform/desktop-winit",
    ),
    ("cranpose-platform-web", "crates/cranpose-platform/web"),
    ("cranpose-render-common", "crates/cranpose-render/common"),
    ("cranpose-render-pixels", "crates/cranpose-render/pixels"),
    ("cranpose-render-wgpu", "crates/cranpose-render/wgpu"),
    ("cranpose-runtime-std", "crates/cranpose-runtime-std"),
    ("cranpose-services", "crates/cranpose-services"),
    ("cranpose-ui", "crates/cranpose-ui"),
    ("cranpose-ui-graphics", "crates/cranpose-ui-graphics"),
    ("cranpose-ui-layout", "crates/cranpose-ui-layout"),
];

fn add_workspace_cranpose_patches(command: &mut Command, workspace: &Path) {
    for (package, relative_path) in WORKSPACE_CRANPOSE_PATCHES {
        let package_path = workspace.join(relative_path);
        command.arg("--config").arg(format!(
            "patch.crates-io.{package}.path=\"{}\"",
            escape_toml_string(&package_path.display().to_string())
        ));
    }
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Staging root for out-of-workspace packages built with local Cranpose
/// patches. It lives under the workspace `target/`, which is ignored, so the
/// lockfile cargo rewrites there is a build artifact like any other.
const PATCHED_PACKAGE_STAGE: &str = "target/patched-packages";

/// The package files a staged build needs beside its sources.
const STAGED_PACKAGE_FILES: &[&str] = &["Cargo.toml", "Cargo.lock"];

/// Redirect a patched build at a staged copy of the package it measures.
///
/// `--patch-workspace-cranpose` hands cargo a `patch.crates-io.*.path` for
/// every Cranpose crate. Cargo re-resolves under those patches and rewrites the
/// package's `Cargo.lock`, dropping the `source` and `checksum` of every
/// patched crate. `apps/isolated-demo` tracks its lockfile on purpose -- it is
/// the canary proving a release resolves from crates.io -- so measuring a
/// binary must not rewrite it. Building a staged copy keeps that lockfile
/// untouched while the patched resolution lands in `target/`.
///
/// A workspace build needs no staging: with no `--manifest-path` cargo resolves
/// the workspace's own lockfile, which already holds the Cranpose crates as
/// workspace members.
fn stage_patched_package(
    workspace: &Path,
    options: &CargoBinaryOptions,
) -> Result<CargoBinaryOptions, String> {
    let Some(manifest_path) = options.manifest_path.as_deref() else {
        return Ok(options.clone());
    };
    if !options.patch_workspace_cranpose {
        return Ok(options.clone());
    }

    let manifest_path = absolute_path(workspace, manifest_path);
    let package_dir = manifest_path
        .parent()
        .ok_or_else(|| format!("`{}` has no package directory", manifest_path.display()))?;
    let package_dir_name = package_dir
        .file_name()
        .ok_or_else(|| format!("`{}` has no package directory name", package_dir.display()))?;
    let staged = workspace.join(PATCHED_PACKAGE_STAGE).join(package_dir_name);

    let source_roots = package_source_roots(&manifest_path)?;
    // Cargo reports canonical target paths, so the package directory has to be
    // canonical as well for them to strip against it.
    let package_dir = fs::canonicalize(package_dir)
        .map_err(|error| format!("failed to resolve `{}`: {error}", package_dir.display()))?;
    stage_package(&package_dir, &staged, &source_roots)?;

    Ok(CargoBinaryOptions {
        manifest_path: Some(staged.join("Cargo.toml")),
        ..options.clone()
    })
}

/// Mirror the files cargo compiles -- the manifest, the lockfile and every
/// directory holding a declared target -- into `staged`.
///
/// Mirroring the whole package directory instead would drag in whatever other
/// build systems left inside it (`android/app/build`, `ios/build`, `pkg`), so
/// the set comes from what cargo itself reports as this package's targets.
fn stage_package(
    package_dir: &Path,
    staged: &Path,
    source_roots: &[PathBuf],
) -> Result<(), String> {
    fs::create_dir_all(staged)
        .map_err(|error| format!("failed to create `{}`: {error}", staged.display()))?;

    for name in STAGED_PACKAGE_FILES {
        mirror_optional_file(&package_dir.join(name), &staged.join(name))?;
    }

    for root in source_roots {
        let relative = root.strip_prefix(package_dir).map_err(|_| {
            format!(
                "cargo target directory `{}` lies outside package `{}`; a patched \
                 build stages one package, not a workspace",
                root.display(),
                package_dir.display()
            )
        })?;
        mirror_dir(root, &staged.join(relative))?;
    }

    Ok(())
}

/// The directories holding the cargo targets `manifest_path` declares.
///
/// `--no-deps` reports the manifest's own packages without resolving
/// dependencies, so asking is free and -- unlike a build -- leaves the
/// package's lockfile alone.
fn package_source_roots(manifest_path: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(manifest_path)
        .output()
        .map_err(|error| format!("failed to run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo metadata returned invalid UTF-8: {error}"))?;
    let roots = parse_target_source_roots(&stdout);
    if roots.is_empty() {
        return Err(format!(
            "cargo metadata reported no targets for `{}`",
            manifest_path.display()
        ));
    }
    Ok(roots)
}

fn parse_target_source_roots(metadata_json: &str) -> Vec<PathBuf> {
    const KEY: &str = "\"src_path\":\"";
    let mut roots = Vec::new();
    let mut rest = metadata_json;

    while let Some(start) = rest.find(KEY) {
        rest = &rest[start + KEY.len()..];
        let Some(end) = rest.find('"') else { break };
        let src_path = PathBuf::from(unescape_json_string(&rest[..end]));
        rest = &rest[end..];
        let Some(root) = src_path.parent() else {
            continue;
        };
        if !roots.iter().any(|known| known == root) {
            roots.push(root.to_path_buf());
        }
    }

    roots
}

/// Mirror `source` onto `destination`, copying only the files whose bytes
/// differ and dropping whatever `source` no longer holds.
///
/// Copying unconditionally would restamp every mtime and make cargo rebuild the
/// staged package from scratch on every run; comparing contents first keeps the
/// measurement incremental.
fn mirror_dir(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("failed to create `{}`: {error}", destination.display()))?;

    let mut mirrored = BTreeSet::<OsString>::new();
    for entry in read_dir_entries(source)? {
        let name = entry.file_name();
        let source_entry = source.join(&name);
        let destination_entry = destination.join(&name);
        if source_entry.is_dir() {
            mirror_dir(&source_entry, &destination_entry)?;
        } else {
            mirror_file(&source_entry, &destination_entry)?;
        }
        mirrored.insert(name);
    }

    for entry in read_dir_entries(destination)? {
        let name = entry.file_name();
        if mirrored.contains(&name) {
            continue;
        }
        let stale = destination.join(&name);
        let removed = if stale.is_dir() {
            fs::remove_dir_all(&stale)
        } else {
            fs::remove_file(&stale)
        };
        removed.map_err(|error| format!("failed to remove `{}`: {error}", stale.display()))?;
    }

    Ok(())
}

/// Copy `source` onto `destination` unless their bytes already match.
fn mirror_file(source: &Path, destination: &Path) -> Result<(), String> {
    let bytes = fs::read(source)
        .map_err(|error| format!("failed to read `{}`: {error}", source.display()))?;
    if fs::read(destination).is_ok_and(|mirrored| mirrored == bytes) {
        return Ok(());
    }
    // `fs::copy` rather than `fs::write`: it carries the permission bits over,
    // so a staged script stays executable.
    fs::copy(source, destination).map_err(|error| {
        format!(
            "failed to copy `{}` to `{}`: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

/// Mirror `source` when it exists, and drop a `destination` left over from a
/// run where it did.
fn mirror_optional_file(source: &Path, destination: &Path) -> Result<(), String> {
    if source.exists() {
        return mirror_file(source, destination);
    }
    match fs::remove_file(destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to remove `{}`: {error}",
            destination.display()
        )),
    }
}

fn read_dir_entries(path: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let read = |error| format!("failed to read `{}`: {error}", path.display());
    fs::read_dir(path)
        .map_err(read)?
        .collect::<io::Result<Vec<_>>>()
        .map_err(read)
}

fn built_binary_path(workspace: &Path, options: &CargoBinaryOptions) -> PathBuf {
    let mut path = cargo_target_root(workspace, options);
    if let Some(target) = options.target.as_deref() {
        path.push(target);
    }
    path.push(cargo_profile_dir(&options.profile));
    path.push(binary_file_name(&options.bin));
    path
}

fn cargo_target_root(workspace: &Path, options: &CargoBinaryOptions) -> PathBuf {
    let Some(manifest_path) = options.manifest_path.as_deref() else {
        return workspace.join("target");
    };
    absolute_path(workspace, manifest_path)
        .parent()
        .map(|parent| parent.join("target"))
        .unwrap_or_else(|| workspace.join("target"))
}

fn absolute_path(workspace: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    }
}

fn cargo_profile_dir(profile: &str) -> &str {
    match profile {
        "dev" => "debug",
        "release" => "release",
        other => other,
    }
}

fn binary_file_name(bin: &str) -> String {
    #[cfg(windows)]
    {
        format!("{bin}.exe")
    }
    #[cfg(not(windows))]
    {
        bin.to_owned()
    }
}

fn create_bundle(
    workspace: &Path,
    options: &BundleMacosOptions,
    binary: &Path,
) -> Result<PathBuf, String> {
    let app_dir = workspace
        .join(&options.out_dir)
        .join(format!("{}.app", options.app_name));
    let contents = app_dir.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");

    fs::create_dir_all(&macos)
        .map_err(|error| format!("failed to create `{}`: {error}", macos.display()))?;
    fs::create_dir_all(&resources)
        .map_err(|error| format!("failed to create `{}`: {error}", resources.display()))?;

    let executable_name = executable_name(&options.app_name);
    copy_file(binary, &macos.join(&executable_name))?;
    write_info_plist(
        &contents.join("Info.plist"),
        &options.app_name,
        &executable_name,
        &options.bundle_id,
    )?;

    if let Some(source_resources) = options.resources.as_deref() {
        copy_dir_recursive(source_resources, &resources)?;
    }

    Ok(app_dir)
}

fn executable_name(app_name: &str) -> String {
    app_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

fn copy_file(source: &Path, dest: &Path) -> Result<(), String> {
    fs::copy(source, dest).map_err(|error| {
        format!(
            "failed to copy `{}` to `{}`: {error}",
            source.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err(format!(
            "resources path `{}` is not a directory",
            source.display()
        ));
    }

    for entry in fs::read_dir(source)
        .map_err(|error| format!("failed to read `{}`: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("failed to read resource entry: {error}"))?;
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect `{}`: {error}", source_path.display()))?;
        if file_type.is_dir() {
            fs::create_dir_all(&dest_path)
                .map_err(|error| format!("failed to create `{}`: {error}", dest_path.display()))?;
            copy_dir_recursive(&source_path, &dest_path)?;
        } else if file_type.is_file() {
            copy_file(&source_path, &dest_path)?;
        }
    }
    Ok(())
}

fn write_info_plist(
    path: &Path,
    app_name: &str,
    executable_name: &str,
    bundle_id: &str,
) -> Result<(), String> {
    let plist = info_plist(app_name, executable_name, bundle_id);
    fs::write(path, plist).map_err(|error| format!("failed to write `{}`: {error}", path.display()))
}

fn info_plist(app_name: &str, executable_name: &str, bundle_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>{}</string>
  <key>CFBundleExecutable</key>
  <string>{}</string>
  <key>CFBundleIdentifier</key>
  <string>{}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>{}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
"#,
        escape_xml(app_name),
        escape_xml(executable_name),
        escape_xml(bundle_id),
        escape_xml(app_name)
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Selects the codesign identity used to seal a macOS bundle.
///
/// Falls back to ad-hoc (`-`) when no developer identity is supplied so the
/// `.app` always carries a valid signature seal.
fn bundle_sign_identity(requested: Option<&str>) -> &str {
    requested.unwrap_or("-")
}

fn sign_bundle(bundle: &Path, identity: &str) -> Result<(), String> {
    let status = Command::new("codesign")
        .args(["--force", "--deep", "--sign", identity])
        .arg(bundle)
        .status()
        .map_err(|error| format!("failed to run codesign: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("codesign failed with status {status}"))
    }
}

fn check_dependency_budget(options: DependencyBudgetOptions) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(error) = check_renderer_pixels_feature_boundary(options.explain) {
        errors.push(error);
    }
    for scope in options.scopes {
        if let Err(error) = check_dependency_budget_scope(scope, options.explain) {
            errors.push(error);
        }
    }

    dependency_budget_result(errors)
}

fn check_renderer_pixels_feature_boundary(explain: bool) -> Result<(), String> {
    let output = Command::new("cargo")
        .args([
            "tree",
            "-p",
            "cranpose",
            "--no-default-features",
            "--features",
            "renderer-pixels",
            "--color",
            "never",
        ])
        .args(shipped_target_args())
        .output()
        .map_err(|error| format!("failed to run cargo tree for renderer-pixels: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo tree returned invalid UTF-8: {error}"))?;
    renderer_pixels_feature_boundary_violation(&stdout)?;
    if explain {
        println!(
            "renderer-pixels feature boundary ok: no external pixels, wgpu, wgpu-core, wgpu-hal, or naga packages"
        );
    }
    Ok(())
}

fn renderer_pixels_feature_boundary_violation(cargo_tree: &str) -> Result<(), String> {
    let packages = package_names_in_cargo_tree(cargo_tree);
    let leaked = RENDERER_PIXELS_FORBIDDEN_PACKAGES
        .iter()
        .filter(|package| packages.iter().any(|name| name == **package))
        .copied()
        .collect::<Vec<_>>();

    if leaked.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "cranpose/renderer-pixels must not pull external renderer packages: {}",
            leaked.join(", ")
        ))
    }
}

fn dependency_budget_result(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn check_dependency_budget_scope(
    scope: DependencyBudgetScope,
    explain: bool,
) -> Result<(), String> {
    let output = Command::new("cargo")
        .args(scope.cargo_tree_args())
        .output()
        .map_err(|error| format!("failed to run cargo tree: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo tree returned invalid UTF-8: {error}"))?;
    let duplicate_details = duplicate_package_details(&stdout);
    let duplicate_version_families = duplicate_version_package_families(&duplicate_details);
    let recorded_debt = scope.recorded_debt();

    let violation = duplicate_budget_violation(scope, &duplicate_version_families, &recorded_debt);
    if violation.is_none() {
        println!(
            "duplicate dependency version budget ok ({}): {} recorded families",
            scope.label(),
            recorded_debt.len()
        );
    }
    if explain {
        print_duplicate_package_details(scope, &duplicate_details, &duplicate_version_families);
        print_recorded_duplicate_debt(scope, &recorded_debt);
    }
    match violation {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn duplicate_budget_violation(
    scope: DependencyBudgetScope,
    duplicate_version_families: &[String],
    recorded_debt: &[&DuplicateDebt],
) -> Option<String> {
    let unexpected = duplicate_version_families
        .iter()
        .filter(|family| !recorded_debt.iter().any(|debt| debt.family == **family))
        .cloned()
        .collect::<Vec<_>>();
    let stale = recorded_debt
        .iter()
        .filter(|debt| {
            !duplicate_version_families
                .iter()
                .any(|family| family == debt.family)
        })
        .map(|debt| debt.family.to_owned())
        .collect::<Vec<_>>();

    let mut errors = Vec::new();
    if !unexpected.is_empty() {
        errors.push(format!(
            "unexpected duplicate dependency version families for {}: {}; collapse the split or record it as upstream debt",
            scope.label(),
            unexpected.join(", ")
        ));
    }
    if !stale.is_empty() {
        errors.push(format!(
            "stale duplicate dependency debt for {}: {}; the split is gone, remove the entries",
            scope.label(),
            stale.join(", ")
        ));
    }
    if errors.is_empty() {
        None
    } else {
        Some(errors.join("\n"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DuplicatePackageFamily {
    name: String,
    roots: Vec<DuplicatePackageRoot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DuplicatePackageRoot {
    name: String,
    version: String,
    root: String,
    direct_dependents: Vec<String>,
}

/// Removes ANSI escape sequences from one `cargo tree` line.
///
/// `cargo tree` colours its output whenever `CARGO_TERM_COLOR=always`, which
/// the CI workflow sets globally. A nested line then arrives as
/// `ESC[2m│ESC[0m   ESC[2m└──ESC[0m thiserror v1.0.69`, which does not start
/// with a box-drawing character, so the root check waves it through and the
/// package "name" ends up carrying the tree drawing with it. Strip the escapes
/// before parsing so the budget reads the same graph however the environment
/// is configured.
fn strip_ansi_escapes(line: &str) -> String {
    let mut plain = String::with_capacity(line.len());
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
        // A control sequence is `ESC [ <parameters> <final>`, where the final
        // byte is in `@..=~`. Any other escape is two characters long.
        if let Some('[') = characters.next() {
            for character in characters.by_ref() {
                if ('@'..='~').contains(&character) {
                    break;
                }
            }
        }
    }
    plain
}

fn duplicate_package_details(cargo_tree: &str) -> Vec<DuplicatePackageFamily> {
    let mut roots_by_package = BTreeMap::<String, Vec<DuplicatePackageRoot>>::new();
    let mut current = None::<DuplicatePackageRoot>;

    for line in cargo_tree.lines().map(strip_ansi_escapes) {
        let line = line.as_str();
        if let Some(root) = root_duplicate_package_entry(line) {
            if let Some(root) = current.take() {
                roots_by_package
                    .entry(root.name.clone())
                    .or_default()
                    .push(root);
            }
            current = Some(root);
            continue;
        }

        if line.trim().is_empty() {
            if let Some(root) = current.take() {
                roots_by_package
                    .entry(root.name.clone())
                    .or_default()
                    .push(root);
            }
            continue;
        }

        if let Some(dependent) = direct_duplicate_dependent(line) {
            if let Some(root) = current.as_mut() {
                root.direct_dependents.push(dependent);
            }
        }
    }

    if let Some(root) = current {
        roots_by_package
            .entry(root.name.clone())
            .or_default()
            .push(root);
    }

    roots_by_package
        .into_iter()
        .filter(|(_, roots)| roots.len() > 1)
        .map(|(name, roots)| DuplicatePackageFamily { name, roots })
        .collect()
}

fn duplicate_version_package_families(details: &[DuplicatePackageFamily]) -> Vec<String> {
    let mut families = details
        .iter()
        .filter(|family| {
            let mut versions = family
                .roots
                .iter()
                .map(|root| root.version.as_str())
                .collect::<Vec<_>>();
            versions.sort_unstable();
            versions.dedup();
            versions.len() > 1
        })
        .map(|family| family.name.clone())
        .collect::<Vec<_>>();
    families.sort();
    families
}

fn package_names_in_cargo_tree(cargo_tree: &str) -> Vec<String> {
    let mut packages = cargo_tree
        .lines()
        .filter_map(|line| package_name_from_cargo_tree_line(&strip_ansi_escapes(line)))
        .collect::<Vec<_>>();
    packages.sort();
    packages.dedup();
    packages
}

fn package_name_from_cargo_tree_line(line: &str) -> Option<String> {
    let entry = line
        .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '│' | '├' | '└' | '─'));
    let version_index = entry.find(" v")?;
    Some(entry[..version_index].to_owned())
}

fn root_duplicate_package_entry(line: &str) -> Option<DuplicatePackageRoot> {
    if line.starts_with(char::is_whitespace)
        || line.starts_with('├')
        || line.starts_with('└')
        || line.starts_with('│')
        || line.trim().is_empty()
    {
        return None;
    }

    let version_index = line.find(" v")?;
    let version_start = version_index + 2;
    let version_end = line[version_start..]
        .find(char::is_whitespace)
        .map(|offset| version_start + offset)
        .unwrap_or(line.len());
    Some(DuplicatePackageRoot {
        name: line[..version_index].to_owned(),
        version: line[version_start..version_end].to_owned(),
        root: line.trim().to_owned(),
        direct_dependents: Vec::new(),
    })
}

fn direct_duplicate_dependent(line: &str) -> Option<String> {
    let dependent = line
        .strip_prefix("├── ")
        .or_else(|| line.strip_prefix("└── "))?;
    Some(dependent.trim().to_owned())
}

fn print_duplicate_package_details(
    scope: DependencyBudgetScope,
    details: &[DuplicatePackageFamily],
    budget_families: &[String],
) {
    println!("duplicate dependency roots ({}):", scope.label());
    if budget_families.is_empty() {
        println!("  none");
        return;
    }

    for family_name in budget_families {
        let Some(family) = details.iter().find(|family| &family.name == family_name) else {
            continue;
        };
        println!("  {}:", family.name);
        for root in &family.roots {
            if root.direct_dependents.is_empty() {
                println!("    {}", root.root);
            } else {
                println!("    {} <- {}", root.root, root.direct_dependents.join(", "));
            }
        }
    }
}

fn print_recorded_duplicate_debt(scope: DependencyBudgetScope, recorded_debt: &[&DuplicateDebt]) {
    println!("recorded duplicate dependency debt ({}):", scope.label());
    if recorded_debt.is_empty() {
        println!("  none");
        return;
    }
    for debt in recorded_debt {
        println!("  {}: {}", debt.family, debt.reason);
    }
}

#[cfg(all(test, unix))]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[cfg(all(test, not(unix)))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn parse_bundle_defaults() {
        let options = BundleMacosOptions::parse(&[]).expect("default options parse");

        assert_eq!(options.package, "desktop-app");
        assert_eq!(options.bin, "desktop-app");
        assert_eq!(options.profile, "release");
        assert_eq!(options.app_name, "Cranpose Demo");
        assert_eq!(options.bundle_id, "io.cranpose.demo");
        assert!(options.build);
    }

    #[test]
    fn parse_bundle_options() {
        let options = BundleMacosOptions::parse(&[
            "--package".into(),
            "isolated-demo".into(),
            "--bin".into(),
            "isolated-demo".into(),
            "--profile".into(),
            "release-small".into(),
            "--app-name".into(),
            "Cranpose Isolated".into(),
            "--bundle-id".into(),
            "io.cranpose.isolated".into(),
            "--out-dir".into(),
            "target/custom-bundles".into(),
            "--target".into(),
            "aarch64-apple-darwin".into(),
            "--no-build".into(),
            "--sign-identity".into(),
            "Developer ID Application: Example".into(),
        ])
        .expect("custom options parse");

        assert_eq!(options.package, "isolated-demo");
        assert_eq!(options.profile, "release-small");
        assert_eq!(options.target.as_deref(), Some("aarch64-apple-darwin"));
        assert!(!options.build);
        assert_eq!(
            options.sign_identity.as_deref(),
            Some("Developer ID Application: Example")
        );
    }

    #[test]
    fn parse_binary_size_options() {
        let options = BinarySizeOptions::parse(&[
            "--package".into(),
            "desktop-app".into(),
            "--bin".into(),
            "desktop-app".into(),
            "--profile".into(),
            "dev".into(),
            "--target".into(),
            "x86_64-unknown-linux-gnu".into(),
            "--manifest-path".into(),
            "apps/isolated-demo/Cargo.toml".into(),
            "--max-bytes".into(),
            "29360128".into(),
            "--patch-workspace-cranpose".into(),
            "--no-build".into(),
        ])
        .expect("binary-size options parse");

        assert_eq!(options.binary.package, "desktop-app");
        assert_eq!(options.binary.profile, "dev");
        assert_eq!(
            options.binary.target.as_deref(),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            options.binary.manifest_path.as_deref(),
            Some(Path::new("apps/isolated-demo/Cargo.toml"))
        );
        assert_eq!(options.max_bytes, Some(29_360_128));
        assert!(options.binary.patch_workspace_cranpose);
        assert!(!options.build);
    }

    #[test]
    fn parse_dist_min_feature_flags_and_target_rustflags() {
        let options = DistMinOptions::parse(&[
            "--features".into(),
            "desktop,renderer-wgpu".into(),
            "--no-default-features".into(),
        ])
        .expect("dist-min feature options parse");
        assert_eq!(options.features.as_deref(), Some("desktop,renderer-wgpu"));
        assert!(options.no_default_features);

        let linux = dist_min_rustflags_for_target("x86_64-unknown-linux-gnu");
        assert!(linux.contains("--icf=all"), "linux gets lld icf: {linux}");
        let android = dist_min_rustflags_for_target("aarch64-linux-android");
        assert!(android.contains("--icf=all"), "android gets lld icf");
        let mac = dist_min_rustflags_for_target("aarch64-apple-darwin");
        assert!(!mac.contains("lld"), "ld64 targets skip lld flags: {mac}");
        let win = dist_min_rustflags_for_target("x86_64-pc-windows-msvc");
        assert!(!win.contains("icf"), "msvc has /OPT:ICF already: {win}");
        assert!(
            !win.contains("force-unwind-tables"),
            "msvc requires unwind tables: {win}"
        );
        assert!(linux.contains("-Cforce-unwind-tables=no"));
    }

    #[test]
    fn parse_dist_min_options() {
        let options = DistMinOptions::parse(&[
            "--package".into(),
            "isolated-demo".into(),
            "--bin".into(),
            "isolated-demo".into(),
            "--target".into(),
            "x86_64-unknown-linux-gnu".into(),
            "--manifest-path".into(),
            "apps/isolated-demo/Cargo.toml".into(),
            "--max-bytes".into(),
            "6291456".into(),
            "--patch-workspace-cranpose".into(),
        ])
        .expect("dist-min options parse");

        assert_eq!(options.binary.package, "isolated-demo");
        assert_eq!(options.binary.bin, "isolated-demo");
        assert_eq!(options.binary.profile, "release-small");
        assert_eq!(
            options.binary.target.as_deref(),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            options.binary.manifest_path.as_deref(),
            Some(Path::new("apps/isolated-demo/Cargo.toml"))
        );
        assert_eq!(options.max_bytes, Some(6_291_456));
        assert!(options.binary.patch_workspace_cranpose);
    }

    #[test]
    fn parse_dist_min_rejects_unknown_option() {
        let error = DistMinOptions::parse(&["--no-build".into()])
            .expect_err("dist-min should reject binary-size-only flags");
        assert!(error.contains("--no-build"));
    }

    #[test]
    fn parse_binary_size_rejects_invalid_budget() {
        let error = BinarySizeOptions::parse(&["--max-bytes".into(), "not-a-number".into()])
            .expect_err("invalid max bytes should fail");

        assert!(error.contains("--max-bytes must be an unsigned integer"));
    }

    #[test]
    fn parse_dependency_budget_defaults_to_workspace_and_all_features() {
        let options = DependencyBudgetOptions::parse(&[]).expect("dependency-budget options parse");

        assert_eq!(
            options.scopes,
            vec![
                DependencyBudgetScope::Workspace,
                DependencyBudgetScope::AllFeatures
            ]
        );
        assert!(!options.explain);
    }

    #[test]
    fn parse_dependency_budget_accepts_single_scope_options() {
        let workspace = DependencyBudgetOptions::parse(&["--workspace-only".into()])
            .expect("workspace-only options parse");
        assert_eq!(workspace.scopes, vec![DependencyBudgetScope::Workspace]);
        assert!(!workspace.explain);

        let all_features = DependencyBudgetOptions::parse(&["--all-features-only".into()])
            .expect("all-features-only options parse");
        assert_eq!(
            all_features.scopes,
            vec![DependencyBudgetScope::AllFeatures]
        );
        assert!(!all_features.explain);
    }

    #[test]
    fn parse_dependency_budget_accepts_explain_with_scope() {
        let options =
            DependencyBudgetOptions::parse(&["--workspace-only".into(), "--explain".into()])
                .expect("dependency-budget explain options parse");

        assert_eq!(options.scopes, vec![DependencyBudgetScope::Workspace]);
        assert!(options.explain);
    }

    #[test]
    fn parse_dependency_budget_rejects_unknown_option() {
        let error = DependencyBudgetOptions::parse(&["--unexpected".into()])
            .expect_err("unknown option should fail");

        assert!(error.contains("unknown dependency-budget option"));
    }

    #[test]
    fn plist_escapes_bundle_fields() {
        let plist = info_plist("Cranpose & Demo", "Cranpose<Demo>", "io.cranpose.demo");

        assert!(plist.contains("Cranpose &amp; Demo"));
        assert!(plist.contains("Cranpose&lt;Demo&gt;"));
        assert!(plist.contains("io.cranpose.demo"));
    }

    #[test]
    fn duplicate_budget_parser_returns_root_package_families() {
        let tree = "\
hashbrown v0.15.5
└── gpu-descriptor v0.3.2

hashbrown v0.16.1
└── naga v29.0.3

tiny-skia v0.12.0 (*)
";

        let details = duplicate_package_details(tree);
        assert_eq!(
            duplicate_version_package_families(&details),
            vec!["hashbrown".to_owned()]
        );
    }

    #[test]
    fn dependency_budget_cargo_tree_args_pin_shipped_targets() {
        let shipped_targets = [
            "aarch64-apple-darwin",
            "aarch64-apple-ios",
            "aarch64-apple-ios-sim",
            "aarch64-linux-android",
            "armv7-linux-androideabi",
            "i686-linux-android",
            "wasm32-unknown-unknown",
            "x86_64-linux-android",
            "x86_64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu",
        ];

        for scope in [
            DependencyBudgetScope::Workspace,
            DependencyBudgetScope::AllFeatures,
        ] {
            let args = scope.cargo_tree_args();
            for target in shipped_targets {
                assert!(
                    args.windows(2)
                        .any(|pair| pair[0] == "--target" && pair[1] == target),
                    "budget cargo tree args for {} must pin --target {target}",
                    scope.label()
                );
            }
        }
    }

    /// CI sets `CARGO_TERM_COLOR=always`, so every `cargo tree` line arrives
    /// wrapped in ANSI escapes. A nested line then starts with `ESC[` rather
    /// than with a box-drawing character, which is exactly what the root check
    /// looks for.
    #[test]
    fn duplicate_budget_parser_ignores_coloured_nested_lines() {
        let tree = concat!(
            "\u{1b}[2m│\u{1b}[0m   \u{1b}[2m└──\u{1b}[0m thiserror v1.0.69\n",
            "\u{1b}[2m│\u{1b}[0m   \u{1b}[2m└──\u{1b}[0m thiserror v2.0.18\n",
        );

        let details = duplicate_package_details(tree);

        assert!(
            duplicate_version_package_families(&details).is_empty(),
            "nested tree lines are dependents, not roots, coloured or not"
        );
    }

    #[test]
    fn duplicate_budget_parser_reads_coloured_root_lines() {
        let tree = concat!(
            "\u{1b}[1mthiserror\u{1b}[0m v1.0.69\n",
            "\u{1b}[2m└──\u{1b}[0m ndk v0.9.0\n",
            "\n",
            "\u{1b}[1mthiserror\u{1b}[0m v2.0.18\n",
            "\u{1b}[2m└──\u{1b}[0m cranpose v0.1.101\n",
        );

        let details = duplicate_package_details(tree);

        assert_eq!(
            duplicate_version_package_families(&details),
            vec!["thiserror".to_owned()],
            "a coloured root line names the same family as an uncoloured one"
        );
    }

    #[test]
    fn cargo_tree_package_parser_strips_colour() {
        let tree = "\u{1b}[1mcranpose-render-pixels\u{1b}[0m v0.1.101\n";

        assert_eq!(
            package_names_in_cargo_tree(tree),
            vec!["cranpose-render-pixels".to_owned()]
        );
    }

    #[test]
    fn duplicate_budget_violation_rejects_unrecorded_families() {
        let families = vec!["foldhash".to_owned(), "hashbrown".to_owned()];

        let violation =
            duplicate_budget_violation(DependencyBudgetScope::Workspace, &families, &[])
                .expect("unrecorded duplicate-version families should fail the budget");

        assert!(violation.contains(
            "unexpected duplicate dependency version families for workspace: foldhash, hashbrown;"
        ));
    }

    #[test]
    fn duplicate_budget_violation_rejects_stale_recorded_debt() {
        let debt = DuplicateDebt {
            family: "hashbrown",
            reason: "an upstream split that no longer exists",
        };

        let violation = duplicate_budget_violation(DependencyBudgetScope::Workspace, &[], &[&debt])
            .expect("stale recorded debt should fail the budget");

        assert!(violation.contains("stale duplicate dependency debt for workspace: hashbrown;"));
    }

    #[test]
    fn duplicate_budget_violation_reports_unexpected_and_stale_together() {
        let debt = DuplicateDebt {
            family: "hashbrown",
            reason: "an upstream split that no longer exists",
        };
        let families = vec!["new-family".to_owned()];

        let violation =
            duplicate_budget_violation(DependencyBudgetScope::Workspace, &families, &[&debt])
                .expect("unexpected and stale families should both fail the budget");

        assert!(violation.contains("unexpected duplicate dependency version families"));
        assert!(violation.contains("new-family"));
        assert!(violation.contains("stale duplicate dependency debt"));
        assert!(violation.contains("hashbrown"));
    }

    #[test]
    fn duplicate_budget_violation_accepts_exact_debt_match() {
        let debt = DuplicateDebt {
            family: "hashbrown",
            reason: "an upstream pin",
        };
        let families = vec!["hashbrown".to_owned()];

        assert_eq!(
            duplicate_budget_violation(DependencyBudgetScope::Workspace, &families, &[&debt]),
            None
        );
    }

    #[test]
    fn duplicate_budget_violation_accepts_empty_tree_and_empty_debt() {
        assert_eq!(
            duplicate_budget_violation(DependencyBudgetScope::Workspace, &[], &[]),
            None
        );
    }

    #[test]
    fn recorded_debt_families_are_unique_per_scope() {
        for scope in [
            DependencyBudgetScope::Workspace,
            DependencyBudgetScope::AllFeatures,
        ] {
            let mut families = scope
                .recorded_debt()
                .iter()
                .map(|debt| debt.family)
                .collect::<Vec<_>>();
            families.sort_unstable();
            let mut deduped = families.clone();
            deduped.dedup();
            assert_eq!(families, deduped, "duplicate debt entry for {:?}", scope);
        }
    }

    #[test]
    fn dependency_budget_result_aggregates_scope_errors() {
        let error = dependency_budget_result(vec![
            "unexpected duplicate dependency version families for workspace: hashbrown".to_owned(),
            "unexpected duplicate dependency version families for workspace all-features: roxmltree"
                .to_owned(),
        ])
        .expect_err("scope errors should be aggregated");

        assert!(error.contains("workspace: hashbrown"));
        assert!(error.contains("workspace all-features: roxmltree"));
        assert_eq!(error.lines().count(), 2);
    }

    #[test]
    fn dependency_budget_result_accepts_no_scope_errors() {
        assert_eq!(dependency_budget_result(Vec::new()), Ok(()));
    }

    #[test]
    fn duplicate_details_printer_accepts_empty_focused_family_list() {
        print_duplicate_package_details(DependencyBudgetScope::Workspace, &[], &[]);
    }

    #[test]
    fn duplicate_version_families_ignore_repeated_same_version_roots() {
        let details = vec![
            DuplicatePackageFamily {
                name: "hashbrown".to_owned(),
                roots: vec![
                    DuplicatePackageRoot {
                        name: "hashbrown".to_owned(),
                        version: "0.15.5".to_owned(),
                        root: "hashbrown v0.15.5".to_owned(),
                        direct_dependents: Vec::new(),
                    },
                    DuplicatePackageRoot {
                        name: "hashbrown".to_owned(),
                        version: "0.16.1".to_owned(),
                        root: "hashbrown v0.16.1".to_owned(),
                        direct_dependents: Vec::new(),
                    },
                ],
            },
            DuplicatePackageFamily {
                name: "serde".to_owned(),
                roots: vec![
                    DuplicatePackageRoot {
                        name: "serde".to_owned(),
                        version: "1.0.228".to_owned(),
                        root: "serde v1.0.228".to_owned(),
                        direct_dependents: Vec::new(),
                    },
                    DuplicatePackageRoot {
                        name: "serde".to_owned(),
                        version: "1.0.228".to_owned(),
                        root: "serde v1.0.228".to_owned(),
                        direct_dependents: Vec::new(),
                    },
                ],
            },
        ];

        assert_eq!(
            duplicate_version_package_families(&details),
            vec!["hashbrown".to_owned()]
        );
    }

    #[test]
    fn duplicate_budget_parser_returns_root_versions_and_direct_dependents() {
        let tree = "\
hashbrown v0.15.5
└── gpu-descriptor v0.3.2
    └── wgpu-hal v29.0.3

hashbrown v0.16.1
├── gpu-allocator v0.28.0
│   └── wgpu-hal v29.0.3
├── lru v0.16.4
└── naga v29.0.3

serde v1.0.228
└── bincode v1.3.3

serde v1.0.228
├── zbus_names v4.3.2
└── zvariant v5.11.0

tiny-skia v0.12.0 (*)
";

        let details = duplicate_package_details(tree);

        assert_eq!(
            details,
            vec![
                DuplicatePackageFamily {
                    name: "hashbrown".to_owned(),
                    roots: vec![
                        DuplicatePackageRoot {
                            name: "hashbrown".to_owned(),
                            version: "0.15.5".to_owned(),
                            root: "hashbrown v0.15.5".to_owned(),
                            direct_dependents: vec!["gpu-descriptor v0.3.2".to_owned()],
                        },
                        DuplicatePackageRoot {
                            name: "hashbrown".to_owned(),
                            version: "0.16.1".to_owned(),
                            root: "hashbrown v0.16.1".to_owned(),
                            direct_dependents: vec![
                                "gpu-allocator v0.28.0".to_owned(),
                                "lru v0.16.4".to_owned(),
                                "naga v29.0.3".to_owned(),
                            ],
                        },
                    ],
                },
                DuplicatePackageFamily {
                    name: "serde".to_owned(),
                    roots: vec![
                        DuplicatePackageRoot {
                            name: "serde".to_owned(),
                            version: "1.0.228".to_owned(),
                            root: "serde v1.0.228".to_owned(),
                            direct_dependents: vec!["bincode v1.3.3".to_owned()],
                        },
                        DuplicatePackageRoot {
                            name: "serde".to_owned(),
                            version: "1.0.228".to_owned(),
                            root: "serde v1.0.228".to_owned(),
                            direct_dependents: vec![
                                "zbus_names v4.3.2".to_owned(),
                                "zvariant v5.11.0".to_owned(),
                            ],
                        },
                    ],
                },
            ]
        );
    }

    #[test]
    fn cargo_tree_package_parser_distinguishes_pixels_facade_from_external_pixels() {
        let tree = "\
cranpose v0.1.0
└── cranpose-render-pixels v0.1.0
    ├── cranpose-render-common v0.1.0
    └── ab_glyph v0.2.32
";

        assert_eq!(
            package_names_in_cargo_tree(tree),
            vec![
                "ab_glyph".to_owned(),
                "cranpose".to_owned(),
                "cranpose-render-common".to_owned(),
                "cranpose-render-pixels".to_owned()
            ]
        );
    }

    #[test]
    fn renderer_pixels_feature_boundary_accepts_in_tree_renderer() {
        let tree = "\
cranpose v0.1.0
└── cranpose-render-pixels v0.1.0
    └── cranpose-render-common v0.1.0
";

        assert_eq!(renderer_pixels_feature_boundary_violation(tree), Ok(()));
    }

    #[test]
    fn renderer_pixels_feature_boundary_rejects_external_pixels_stack() {
        let tree = "\
cranpose v0.1.0
├── cranpose-render-pixels v0.1.0
└── pixels v0.17.0
    └── wgpu v29.0.3
        ├── naga v29.0.3
        └── wgpu-hal v29.0.3
";

        let error = renderer_pixels_feature_boundary_violation(tree)
            .expect_err("external pixels stack should be rejected");

        assert!(error.contains("pixels"));
        assert!(error.contains("wgpu"));
        assert!(error.contains("wgpu-hal"));
        assert!(!error.contains("cranpose-render-pixels"));
    }

    #[test]
    fn create_bundle_writes_expected_layout() {
        let workspace = unique_temp_dir();
        let binary = workspace.join("target/release/desktop-app");
        fs::create_dir_all(binary.parent().expect("binary parent")).expect("create binary parent");
        fs::write(&binary, b"demo").expect("write test binary");
        make_executable(&binary).expect("make binary executable");

        let resources = workspace.join("resources");
        fs::create_dir_all(resources.join("nested")).expect("create resources");
        fs::write(resources.join("nested/data.txt"), b"resource").expect("write resource");

        let options = BundleMacosOptions {
            package: "desktop-app".to_owned(),
            bin: "desktop-app".to_owned(),
            profile: "release".to_owned(),
            app_name: "Cranpose Demo".to_owned(),
            bundle_id: "io.cranpose.demo".to_owned(),
            out_dir: PathBuf::from("bundles"),
            resources: Some(resources),
            target: None,
            build: false,
            sign_identity: None,
        };

        let bundle = create_bundle(&workspace, &options, &binary).expect("create bundle");

        assert!(bundle.join("Contents/Info.plist").exists());
        assert!(bundle.join("Contents/MacOS/Cranpose-Demo").exists());
        assert!(bundle.join("Contents/Resources/nested/data.txt").exists());
    }

    #[test]
    fn bundle_defaults_to_adhoc_signing() {
        // Without a developer identity the bundle must still be sealed ad-hoc.
        // An unsealed `.app` is reported "damaged" by Gatekeeper once downloaded.
        assert_eq!(bundle_sign_identity(None), "-");
    }

    #[test]
    fn bundle_uses_supplied_signing_identity() {
        assert_eq!(
            bundle_sign_identity(Some("Developer ID Application: Example")),
            "Developer ID Application: Example"
        );
    }

    #[test]
    fn patched_isolated_build_never_touches_the_tracked_package() {
        // `--patch-workspace-cranpose` makes cargo re-resolve and rewrite the
        // package's Cargo.lock. apps/isolated-demo tracks its lockfile as the
        // proof that a release resolves from crates.io, so the patched build
        // has to compile a staged copy instead.
        let workspace = unique_temp_dir();
        let package_dir = workspace.join("apps/isolated-demo");
        fs::create_dir_all(package_dir.join("src")).expect("create package");
        fs::write(package_dir.join("Cargo.toml"), MINIMAL_MANIFEST).expect("write manifest");
        fs::write(package_dir.join("Cargo.lock"), PUBLISHED_LOCKFILE).expect("write lockfile");
        fs::write(package_dir.join("src/main.rs"), b"fn main() {}").expect("write source");
        // Another build system's output next to the sources must not be staged.
        fs::create_dir_all(package_dir.join("android/app/build")).expect("create gradle output");
        fs::write(package_dir.join("android/app/build/huge.apk"), b"output").expect("write output");

        let options = CargoBinaryOptions {
            package: "isolated-demo".to_owned(),
            bin: "isolated-demo".to_owned(),
            profile: "release-small".to_owned(),
            target: None,
            manifest_path: Some(PathBuf::from("apps/isolated-demo/Cargo.toml")),
            patch_workspace_cranpose: true,
        };

        let staged = stage_patched_package(&workspace, &options).expect("stage patched package");

        let staged_dir = workspace.join(PATCHED_PACKAGE_STAGE).join("isolated-demo");
        assert_eq!(
            staged.manifest_path,
            Some(staged_dir.join("Cargo.toml")),
            "a patched build must not compile the tracked manifest"
        );
        assert_eq!(
            fs::read_to_string(package_dir.join("Cargo.lock")).expect("read tracked lockfile"),
            PUBLISHED_LOCKFILE,
            "the tracked lockfile must survive staging byte for byte"
        );
        assert_eq!(
            fs::read_to_string(staged_dir.join("Cargo.lock")).expect("read staged lockfile"),
            PUBLISHED_LOCKFILE
        );
        assert!(staged_dir.join("src/main.rs").exists());
        assert!(
            !staged_dir.join("android").exists(),
            "staging must copy cargo targets, not another build system's output"
        );
        assert_eq!(
            built_binary_path(&workspace, &staged),
            staged_dir.join("target/release-small/isolated-demo"),
            "the measured binary must come from the staged target directory"
        );
    }

    #[test]
    fn staging_is_incremental_and_prunes_removed_sources() {
        let package_dir = unique_temp_dir().join("package");
        let source = package_dir.join("src");
        fs::create_dir_all(source.join("nested")).expect("create sources");
        fs::write(package_dir.join("Cargo.toml"), b"[package]").expect("write manifest");
        fs::write(source.join("main.rs"), b"fn main() {}").expect("write source");
        fs::write(source.join("nested/gone.rs"), b"mod gone;").expect("write source");

        let staged = package_dir.join("staged");
        let roots = [source.clone()];
        stage_package(&package_dir, &staged, &roots).expect("stage package");
        let first = fs::metadata(staged.join("src/main.rs"))
            .and_then(|metadata| metadata.modified())
            .expect("staged mtime");

        // Unchanged sources must keep their timestamps, or every run would
        // rebuild the package from scratch.
        stage_package(&package_dir, &staged, &roots).expect("restage package");
        let second = fs::metadata(staged.join("src/main.rs"))
            .and_then(|metadata| metadata.modified())
            .expect("restaged mtime");
        assert_eq!(first, second, "an unchanged source must not be recopied");

        fs::remove_dir_all(source.join("nested")).expect("remove sources");
        fs::remove_file(package_dir.join("Cargo.toml")).expect("remove manifest");
        stage_package(&package_dir, &staged, &roots).expect("restage pruned package");
        assert!(
            !staged.join("src/nested").exists(),
            "staging must drop sources the package no longer has"
        );
        assert!(!staged.join("Cargo.toml").exists());
    }

    #[test]
    fn unpatched_builds_compile_the_manifest_they_were_given() {
        let workspace = unique_temp_dir();
        let options = CargoBinaryOptions {
            manifest_path: Some(PathBuf::from("apps/isolated-demo/Cargo.toml")),
            ..CargoBinaryOptions::default()
        };

        let staged = stage_patched_package(&workspace, &options).expect("stage unpatched package");

        assert_eq!(staged, options);
    }

    #[test]
    fn workspace_builds_need_no_staging() {
        let workspace = unique_temp_dir();
        let options = CargoBinaryOptions {
            patch_workspace_cranpose: true,
            ..CargoBinaryOptions::default()
        };

        let staged = stage_patched_package(&workspace, &options).expect("stage workspace build");

        assert_eq!(staged, options);
    }

    #[test]
    fn target_source_roots_dedupe_the_directories_cargo_reports() {
        let metadata = r#"{"packages":[{"targets":[
            {"name":"isolated_demo","src_path":"/w/apps/isolated-demo/src/lib.rs"},
            {"name":"isolated-demo","src_path":"/w/apps/isolated-demo/src/main.rs"},
            {"name":"build-script-build","src_path":"/w/apps/isolated-demo/build.rs"}
        ]}]}"#;

        assert_eq!(
            parse_target_source_roots(metadata),
            vec![
                PathBuf::from("/w/apps/isolated-demo/src"),
                PathBuf::from("/w/apps/isolated-demo"),
            ]
        );
    }

    #[test]
    fn staging_rejects_targets_outside_the_package() {
        let root = unique_temp_dir();
        let package_dir = root.join("package");
        fs::create_dir_all(&package_dir).expect("create package");

        let error = stage_package(
            &package_dir,
            &root.join("staged"),
            &[root.join("elsewhere")],
        )
        .expect_err("a target outside the package must not be staged silently");

        assert!(error.contains("lies outside package"), "{error}");
    }

    /// Staging asks cargo which directories hold the package's targets, so the
    /// fixture manifest has to be one cargo accepts.
    const MINIMAL_MANIFEST: &str = "\
[package]
name = \"isolated-demo\"
version = \"0.1.0\"
edition = \"2021\"

[[bin]]
name = \"isolated-demo\"
path = \"src/main.rs\"

[workspace]
";

    const PUBLISHED_LOCKFILE: &str = "\
version = 4

[[package]]
name = \"isolated-demo\"
version = \"0.1.0\"
";

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-output/xtask")
            .join(format!("cranpose-xtask-test-{nanos}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}

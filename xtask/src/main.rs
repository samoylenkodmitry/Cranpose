use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::LazyLock,
};

use regex::{Captures, Regex};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    // `crate-published` alone needs a three-way exit code (0 = published,
    // 1 = not yet, 2 = error) to preserve the shell `crate_exists`/
    // `publish_if_needed` contract in publish.yml, so it bypasses the
    // ordinary `Result<(), String>` dispatch below.
    if args.first().map(String::as_str) == Some("crate-published") {
        return crate_published_command(&args[1..]);
    }
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// One `cargo xtask <name>` subcommand: its handler and its `--help` text.
///
/// A table, not a growing `match`, deliberately: every arm used to repeat
/// its own `if args[1..].iter().any(|arg| arg == "--help" ...)` guard, which
/// made `run`'s own cyclomatic complexity grow by two branches per command
/// forever -- the diff-scoped complexity gate (`just complexity-gate`) is
/// what caught it, at 63 against a limit of 20. `run` now does exactly one
/// generic lookup and one generic help-check; each command's own parsing
/// and logic lives in its own (separately measured, separately testable)
/// function.
struct XtaskCommand {
    name: &'static str,
    run: fn(&[String]) -> Result<(), String>,
    print_usage: fn(),
}

const COMMANDS: &[XtaskCommand] = &[
    XtaskCommand {
        name: "bundle-macos",
        run: |args| bundle_macos(BundleMacosOptions::parse(args)?),
        print_usage: print_bundle_usage,
    },
    XtaskCommand {
        name: "binary-size",
        run: |args| report_binary_size(BinarySizeOptions::parse(args)?),
        print_usage: print_binary_size_usage,
    },
    XtaskCommand {
        name: "dist-min",
        run: |args| build_dist_min(DistMinOptions::parse(args)?),
        print_usage: print_dist_min_usage,
    },
    XtaskCommand {
        name: "dependency-budget",
        run: |args| check_dependency_budget(DependencyBudgetOptions::parse(args)?),
        print_usage: print_dependency_budget_usage,
    },
    XtaskCommand {
        name: "versions",
        run: |args| {
            if let Some(extra) = args.first() {
                return Err(format!("unknown versions option `{extra}`"));
            }
            check_versions()
        },
        print_usage: print_versions_usage,
    },
    XtaskCommand {
        name: "sync-isolated-demo",
        run: |args| sync_isolated_demo(SyncIsolatedDemoOptions::parse(args)?),
        print_usage: print_sync_isolated_demo_usage,
    },
    XtaskCommand {
        name: "bump-release-version",
        run: |args| bump_release_version(&single_positional_arg("bump-release-version", args)?),
        print_usage: print_bump_release_version_usage,
    },
    XtaskCommand {
        name: "verify-tag",
        run: |args| verify_tag(&single_positional_arg("verify-tag", args)?),
        print_usage: print_verify_tag_usage,
    },
    XtaskCommand {
        name: "publish-order",
        run: |args| {
            if let Some(extra) = args.first() {
                return Err(format!("unknown publish-order option `{extra}`"));
            }
            publish_order()
        },
        print_usage: print_publish_order_usage,
    },
    XtaskCommand {
        name: "wait-for-crates-io",
        run: |args| wait_for_crates_io(&single_positional_arg("wait-for-crates-io", args)?),
        print_usage: print_wait_for_crates_io_usage,
    },
    XtaskCommand {
        name: "complexity-gate",
        run: |args| run_complexity_gate(GateOptions::parse(args, "complexity-gate")?),
        print_usage: print_complexity_gate_usage,
    },
    XtaskCommand {
        name: "duplication-gate",
        run: |args| run_duplication_gate(GateOptions::parse(args, "duplication-gate")?),
        print_usage: print_duplication_gate_usage,
    },
    XtaskCommand {
        name: "robot-suite-partition",
        run: |args| {
            if let Some(extra) = args.first() {
                return Err(format!("unknown robot-suite-partition option `{extra}`"));
            }
            robot_suite_partition::run_at(&workspace_root()?)
        },
        print_usage: print_robot_suite_partition_usage,
    },
    XtaskCommand {
        name: "ci-gate-reachability",
        run: |args| {
            if let Some(extra) = args.first() {
                return Err(format!("unknown ci-gate-reachability option `{extra}`"));
            }
            ci_gate_reachability::run_at(&workspace_root()?)
        },
        print_usage: print_ci_gate_reachability_usage,
    },
];

fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command_name) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(());
    };
    if matches!(command_name, "help" | "-h" | "--help") {
        print_usage();
        return Ok(());
    }

    let Some(command) = COMMANDS.iter().find(|entry| entry.name == command_name) else {
        return Err(format!("unknown xtask command `{command_name}`"));
    };

    if args[1..].iter().any(|arg| arg == "--help" || arg == "-h") {
        (command.print_usage)();
        return Ok(());
    }
    (command.run)(&args[1..])
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
           versions            Check that workspace, lockfile and isolated-demo versions agree\n\
           sync-isolated-demo  Point apps/isolated-demo at a published cranpose version\n\
           bump-release-version Bump Cargo.toml and Cargo.lock to a release tag's version\n\
           verify-tag           Check a release tag against the workspace version\n\
           publish-order        Print/write the cranpose crate publish order\n\
           wait-for-crates-io   Poll crates.io until a release is live\n\
           crate-published      Exit 0/1/2: published, not yet, or error\n\
           complexity-gate       Diff-scoped cyclomatic complexity ceiling\n\
           duplication-gate      Diff-scoped copy-paste budget\n\
           ci-gate-reachability  Every `just` recipe CI runs must be reachable from `ci`/`ci-full`\n\
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

fn print_versions_usage() {
    eprintln!(
        "usage: cargo xtask versions\n\
         \n\
         Checks that the workspace version, Cargo.lock, and apps/isolated-demo\n\
         (its manifest and its own lockfile) all agree on the same cranpose\n\
         package version."
    );
}

fn print_sync_isolated_demo_usage() {
    eprintln!(
        "usage: cargo xtask sync-isolated-demo [version]\n\
         \n\
         Points apps/isolated-demo/Cargo.toml at a published cranpose version.\n\
         With no argument, syncs to the current workspace version. A leading\n\
         'v' (as in a release tag) is stripped. Run `cargo update --manifest-path\n\
         apps/isolated-demo/Cargo.toml -p cranpose -p cranpose-core` afterwards\n\
         to move the demo's lockfile."
    );
}

/// A command that takes exactly one bare positional argument (no flags).
fn single_positional_arg(command: &str, args: &[String]) -> Result<String, String> {
    match args {
        [value] if !value.starts_with('-') => Ok(value.clone()),
        [] => Err(format!("{command} requires an argument")),
        _ => Err(format!(
            "{command} takes exactly one argument, got {args:?}"
        )),
    }
}

fn print_bump_release_version_usage() {
    eprintln!(
        "usage: cargo xtask bump-release-version <tag>\n\
         \n\
         Bumps Cargo.toml's workspace.package.version and every cranpose\n\
         workspace.dependencies entry, plus every cranpose package's version\n\
         in Cargo.lock, to <tag> (a leading 'v' is required and stripped).\n\
         Rewrites the files in place with line-level text edits, so unrelated\n\
         formatting and comments are left untouched."
    );
}

fn print_verify_tag_usage() {
    eprintln!(
        "usage: cargo xtask verify-tag <tag>\n\
         \n\
         Checks that <tag> (a leading 'v' is required and stripped) matches\n\
         Cargo.toml's workspace version, and that every cranpose workspace\n\
         dependency matches it too. Does not check Cargo.lock or\n\
         apps/isolated-demo -- this runs between `sync_versions` and\n\
         `bump_isolated_demo` in publish.yml, before the isolated demo is\n\
         allowed to move."
    );
}

fn print_publish_order_usage() {
    eprintln!(
        "usage: cargo xtask publish-order\n\
         \n\
         Resolves the workspace's cranpose crates into a publish order (a\n\
         crate never precedes one of its own non-dev dependencies), writes\n\
         it to ./publish-order.txt (one crate per line), and prints it."
    );
}

fn print_wait_for_crates_io_usage() {
    eprintln!(
        "usage: cargo xtask wait-for-crates-io <version>\n\
         \n\
         Polls crates.io for `cranpose` and `cranpose-core` at <version>,\n\
         up to 30 times 10 seconds apart per crate, and fails if either\n\
         never appears."
    );
}

fn print_crate_published_usage() {
    eprintln!(
        "usage: cargo xtask crate-published <crate> <version>\n\
         \n\
         Queries crates.io for whether <crate>@<version> is published.\n\
         Exit code 0 = published, 1 = not published (404), 2 = error --\n\
         this three-way contract is deliberate, matching a shell\n\
         `if crate_exists ...; then ... else ...; fi` caller."
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GateOptions {
    base: String,
    config: Option<PathBuf>,
}

impl GateOptions {
    fn parse(args: &[String], command_name: &str) -> Result<Self, String> {
        let mut base = "origin/main".to_owned();
        let mut config = None;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--base" => base = required_value(args, &mut index, "--base")?,
                "--config" => {
                    config = Some(PathBuf::from(required_value(args, &mut index, "--config")?))
                }
                other => return Err(format!("unknown {command_name} option `{other}`")),
            }
            index += 1;
        }
        Ok(Self { base, config })
    }
}

fn default_code_quality_gates_config(root: &Path) -> PathBuf {
    root.join("scripts/ci/code_quality_gates.toml")
}

fn violations_message(header: String, violations: &[String]) -> String {
    let mut message = header;
    for violation in violations {
        message.push_str(&format!("\n  {violation}"));
    }
    message
}

fn run_complexity_gate(options: GateOptions) -> Result<(), String> {
    let root = workspace_root()?;
    let config_path = options
        .config
        .unwrap_or_else(|| default_code_quality_gates_config(&root));
    complexity_gate::run_at(&root, &options.base, &config_path)
}

fn run_duplication_gate(options: GateOptions) -> Result<(), String> {
    let root = workspace_root()?;
    let config_path = options
        .config
        .unwrap_or_else(|| default_code_quality_gates_config(&root));
    duplication_gate::run_at(&root, &options.base, &config_path)
}

fn print_complexity_gate_usage() {
    eprintln!(
        "usage: cargo xtask complexity-gate [--base origin/main] [--config path]\n\
         \n\
         Fails if a function the diff adds or modifies has a higher cyclomatic\n\
         complexity than before (or, for new code, above the configured limit).\n\
         Installs rust-code-analysis-cli via `cargo install` if it is missing."
    );
}

fn print_duplication_gate_usage() {
    eprintln!(
        "usage: cargo xtask duplication-gate [--base origin/main] [--config path]\n\
         \n\
         Fails if the diff introduces a cloned block of code, comparing jscpd's\n\
         report before and after so pre-existing duplication is not penalized.\n\
         Installs jscpd via `cargo install` if it is missing."
    );
}

fn print_robot_suite_partition_usage() {
    eprint!(
        "usage: cargo xtask robot-suite-partition\n\
         \n\
         Every robot example `robot-gpu` skips must be one `robot-captures`\n\
         runs, and the reverse. CI splits the suite across those two halves;\n\
         an example dropped from one and not added to the other stops running\n\
         anywhere, in silence.\n"
    );
}

fn print_ci_gate_reachability_usage() {
    eprintln!(
        "usage: cargo xtask ci-gate-reachability\n\
         \n\
         Fails if `.github/workflows/rust.yml` invokes a `just` recipe that\n\
         `ci` and `ci-full` cannot reach in the justfile's dependency graph --\n\
         a gate CI judges every pull request on but a developer cannot run\n\
         before pushing is worse than no gate at all (see #593). Also fails\n\
         loudly, rather than passing, if it parses zero `just` invocations\n\
         from rust.yml: that means the parser itself broke."
    );
}

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

#[derive(Debug, PartialEq, Eq)]
struct DuplicateDebt {
    family: &'static str,
    reason: &'static str,
}

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

const DIST_MIN_RUSTFLAGS: &str = "-Cpanic=immediate-abort -Zunstable-options \
     -Zlocation-detail=none -Zfmt-debug=none";

const DIST_MIN_LLD_RUSTFLAGS: &str = " -Clink-arg=-fuse-ld=lld -Clink-arg=-Wl,--icf=all";

fn dist_min_rustflags_for_target(target: &str) -> String {
    let mut flags = DIST_MIN_RUSTFLAGS.to_owned();
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
    if options.binary.target.is_none() {
        options.binary.target = Some(host_triple()?);
    }
    options.binary = stage_patched_package(&workspace, &options.binary)?;

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

const PATCHED_PACKAGE_STAGE: &str = "target/patched-packages";

const STAGED_PACKAGE_FILES: &[&str] = &["Cargo.toml", "Cargo.lock"];

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
    let package_dir = fs::canonicalize(package_dir)
        .map_err(|error| format!("failed to resolve `{}`: {error}", package_dir.display()))?;
    stage_package(&package_dir, &staged, &source_roots)?;

    Ok(CargoBinaryOptions {
        manifest_path: Some(staged.join("Cargo.toml")),
        ..options.clone()
    })
}

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

fn mirror_file(source: &Path, destination: &Path) -> Result<(), String> {
    let bytes = fs::read(source)
        .map_err(|error| format!("failed to read `{}`: {error}", source.display()))?;
    if fs::read(destination).is_ok_and(|mirrored| mirrored == bytes) {
        return Ok(());
    }
    fs::copy(source, destination).map_err(|error| {
        format!(
            "failed to copy `{}` to `{}`: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

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

fn strip_ansi_escapes(line: &str) -> String {
    let mut plain = String::with_capacity(line.len());
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
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

/// What a Cargo.lock records for a crate resolved from crates.io. A lockfile
/// entry without it was resolved from somewhere else -- a `[patch]`, a path
/// dependency, a git dependency.
const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn load_toml(path: &Path) -> Result<toml::Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    toml::from_str(&text).map_err(|error| format!("failed to parse `{}`: {error}", path.display()))
}

fn workspace_package_version(root: &Path) -> Result<String, String> {
    let manifest_path = root.join("Cargo.toml");
    let manifest = load_toml(&manifest_path)?;
    manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("package"))
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "{} has no workspace.package.version",
                manifest_path.display()
            )
        })
}

fn dependency_version(spec: &toml::Value) -> Option<String> {
    match spec {
        toml::Value::String(version) => Some(version.clone()),
        toml::Value::Table(table) => table
            .get("version")
            .and_then(toml::Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}

fn display_version(version: &Option<String>) -> &str {
    version.as_deref().unwrap_or("None")
}

/// One `[[package]]` entry from a Cargo.lock, filtered to cranpose crates.
struct LockPackage {
    name: String,
    version: Option<String>,
    source: Option<String>,
    checksum: Option<String>,
}

fn cranpose_lock_packages(lockfile: &toml::Value) -> Vec<LockPackage> {
    let Some(packages) = lockfile.get("package").and_then(toml::Value::as_array) else {
        return Vec::new();
    };

    packages
        .iter()
        .filter_map(toml::Value::as_table)
        .filter_map(|package| {
            let name = package.get("name")?.as_str()?;
            if !name.starts_with("cranpose") {
                return None;
            }
            Some(LockPackage {
                name: name.to_owned(),
                version: package
                    .get("version")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
                source: package
                    .get("source")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
                checksum: package
                    .get("checksum")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn lock_versions(packages: &[LockPackage]) -> BTreeMap<String, BTreeSet<String>> {
    let mut versions = BTreeMap::<String, BTreeSet<String>>::new();
    for package in packages {
        if let Some(version) = &package.version {
            versions
                .entry(package.name.clone())
                .or_default()
                .insert(version.clone());
        }
    }
    versions
}

/// `[dependencies]` plus every `[target.'cfg(..)'.dependencies]` table in a
/// manifest -- everywhere a crate dependency version can be declared.
fn dependency_tables(manifest: &toml::Value) -> Vec<&toml::Table> {
    let mut tables = Vec::new();
    if let Some(dependencies) = manifest.get("dependencies").and_then(toml::Value::as_table) {
        tables.push(dependencies);
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            if let Some(target_dependencies) =
                target.get("dependencies").and_then(toml::Value::as_table)
            {
                tables.push(target_dependencies);
            }
        }
    }
    tables
}

fn sorted_cranpose_dependencies(table: &toml::Table) -> Vec<(&String, &toml::Value)> {
    let mut entries = table
        .iter()
        .filter(|(name, _)| name.starts_with("cranpose"))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(name, _)| name.as_str());
    entries
}

/// Assert a lockfile resolves every Cranpose crate from crates.io.
///
/// `apps/isolated-demo` is the canary that proves a release is consumable by
/// an outside project, so its lockfile has to pin the *published* crates. A
/// local `[patch]` -- the one `cargo xtask binary-size
/// --patch-workspace-cranpose` applies -- makes cargo drop the `source` and
/// `checksum` lines, which silently turns the canary into a path build that
/// verifies nothing.
fn check_published_lock(
    path: &Path,
    relative: &str,
    workspace_version: &str,
    failures: &mut Vec<String>,
) -> Result<(), String> {
    let lockfile = load_toml(path)?;
    let packages = cranpose_lock_packages(&lockfile);
    if packages.is_empty() {
        failures.push(format!("{relative} locks no cranpose packages"));
        return Ok(());
    }

    for package in &packages {
        let name = &package.name;
        if package.source.as_deref() != Some(CRATES_IO_SOURCE) {
            let origin = package.source.as_deref().unwrap_or("a local path");
            failures.push(format!(
                "{relative} resolves {name} from {origin}, expected the published crate at {CRATES_IO_SOURCE}"
            ));
        } else if package.checksum.is_none() {
            failures.push(format!("{relative} package {name} has no checksum"));
        }
        if package.version.as_deref() != Some(workspace_version) {
            failures.push(format!(
                "{relative} package {name} is {}, expected {workspace_version}",
                display_version(&package.version)
            ));
        }
    }
    Ok(())
}

/// `just versions`: workspace, lockfile and `apps/isolated-demo` must all
/// agree on the same cranpose package version.
///
/// Ported from the former `scripts/check_cranpose_versions.py` -- Python's
/// `tomllib` is 3.11+, so the same commit passed or failed this gate
/// depending on which `python3` happened to be first on a runner's PATH.
/// Rust and `toml` (already a build dependency of this workspace) are on
/// every runner that can build the crate at all, so this check can no longer
/// depend on ambient interpreter state.
fn check_versions() -> Result<(), String> {
    let root = workspace_root()?;
    check_versions_at(&root)
}

/// The body of `check_versions`, taking the workspace root explicitly so
/// tests can point it at a fixture directory instead of shelling out to
/// `cargo metadata` for the real workspace.
fn check_versions_at(root: &Path) -> Result<(), String> {
    let mut failures = Vec::new();

    let root_manifest = load_toml(&root.join("Cargo.toml"))?;
    let workspace = root_manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "Cargo.toml has no [workspace] table".to_owned())?;
    let workspace_version = workspace
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "Cargo.toml has no workspace.package.version".to_owned())?
        .to_owned();

    let expected_package_names =
        check_workspace_dependency_versions(workspace, &workspace_version, &mut failures);
    check_root_lock_versions(
        root,
        &expected_package_names,
        &workspace_version,
        &mut failures,
    )?;
    check_isolated_demo_manifest_versions(root, &workspace_version, &mut failures)?;
    check_published_lock(
        &root.join("apps/isolated-demo/Cargo.lock"),
        "apps/isolated-demo/Cargo.lock",
        &workspace_version,
        &mut failures,
    )?;

    if failures.is_empty() {
        println!("cranpose package versions are aligned at {workspace_version}");
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// Every cranpose `workspace.dependencies` entry must pin `workspace_version`.
/// Returns the set of package names a root `Cargo.lock` is then expected to
/// carry.
fn check_workspace_dependency_versions(
    workspace: &toml::Table,
    workspace_version: &str,
    failures: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut expected_package_names = BTreeSet::new();
    let Some(dependencies) = workspace
        .get("dependencies")
        .and_then(toml::Value::as_table)
    else {
        return expected_package_names;
    };
    for (name, spec) in sorted_cranpose_dependencies(dependencies) {
        expected_package_names.insert(name.clone());
        let version = dependency_version(spec);
        if version.as_deref() != Some(workspace_version) {
            failures.push(format!(
                "workspace dependency {name} is {}, expected {workspace_version}",
                display_version(&version)
            ));
        }
    }
    expected_package_names
}

/// The root `Cargo.lock` must carry every expected workspace package, each
/// at exactly `workspace_version`.
fn check_root_lock_versions(
    root: &Path,
    expected_package_names: &BTreeSet<String>,
    workspace_version: &str,
    failures: &mut Vec<String>,
) -> Result<(), String> {
    let root_lock = load_toml(&root.join("Cargo.lock"))?;
    let root_versions = lock_versions(&cranpose_lock_packages(&root_lock));
    let root_lock_names: BTreeSet<String> = root_versions.keys().cloned().collect();
    for name in expected_package_names.difference(&root_lock_names) {
        failures.push(format!("Cargo.lock is missing workspace package {name}"));
    }
    for (name, versions) in &root_versions {
        if !(versions.len() == 1 && versions.contains(workspace_version)) {
            let found = versions.iter().cloned().collect::<Vec<_>>().join(", ");
            failures.push(format!(
                "Cargo.lock package {name} has {found}, expected {workspace_version}"
            ));
        }
    }
    Ok(())
}

/// `apps/isolated-demo`'s manifest must also pin `workspace_version` for
/// every cranpose dependency, in `[dependencies]` and any
/// `[target.'cfg(..)'.dependencies]`.
fn check_isolated_demo_manifest_versions(
    root: &Path,
    workspace_version: &str,
    failures: &mut Vec<String>,
) -> Result<(), String> {
    let isolated_manifest = load_toml(&root.join("apps/isolated-demo/Cargo.toml"))?;
    for table in dependency_tables(&isolated_manifest) {
        for (name, spec) in sorted_cranpose_dependencies(table) {
            let version = dependency_version(spec);
            if version.as_deref() != Some(workspace_version) {
                failures.push(format!(
                    "apps/isolated-demo dependency {name} is {}, expected {workspace_version}",
                    display_version(&version)
                ));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SyncIsolatedDemoOptions {
    version: Option<String>,
}

impl SyncIsolatedDemoOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut version = None;
        for arg in args {
            if arg.starts_with('-') {
                return Err(format!("unknown sync-isolated-demo option `{arg}`"));
            }
            if version.is_some() {
                return Err(format!("unexpected argument `{arg}`"));
            }
            version = Some(arg.clone());
        }
        Ok(Self { version })
    }
}

/// `cranpose[-foo] = { ..., version = "x", ... }` (inline table) in
/// `[dependencies]` or any `[target.'cfg(..)'.dependencies]`.
static INLINE_TABLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)(?P<head>^[ \t]*cranpose[\w-]*[ \t]*=[ \t]*\{[^}\n]*?version[ \t]*=[ \t]*")(?P<version>[^"]+)(?P<tail>")"#,
    )
    .expect("INLINE_TABLE_RE is a valid pattern")
});

/// `cranpose[-foo] = "x"` (bare string) in the same places.
static BARE_STRING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?P<head>^[ \t]*cranpose[\w-]*[ \t]*=[ \t]*")(?P<version>[^"]+)(?P<tail>")"#)
        .expect("BARE_STRING_RE is a valid pattern")
});

static SEMVER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d+\.\d+\.\d+([-+][0-9A-Za-z.\-]+)?$").expect("SEMVER_RE is a valid pattern")
});

/// Rewrite every `cranpose*` dependency line `pattern` matches to `target_version`,
/// recording `"name old -> new"` for each line whose version actually changes.
///
/// Matches whose version already equals `target_version` are rewritten to the
/// same text (a no-op) and are not recorded, mirroring the Python original,
/// which only reported changed lines.
fn rewrite_cranpose_versions(
    pattern: &Regex,
    text: &str,
    target_version: &str,
    changed: &mut Vec<String>,
) -> String {
    pattern
        .replace_all(text, |captures: &Captures| {
            let whole = captures.get(0).expect("group 0 always matches").as_str();
            let name = whole.split_once('=').map_or(whole, |(name, _)| name).trim();
            let old_version = &captures["version"];
            if old_version != target_version {
                changed.push(format!("{name} {old_version} -> {target_version}"));
            }
            format!("{}{target_version}{}", &captures["head"], &captures["tail"])
        })
        .into_owned()
}

/// `cargo xtask sync-isolated-demo`: point `apps/isolated-demo/Cargo.toml` at
/// a published cranpose version.
///
/// Ported from the former `scripts/sync_isolated_demo.py`. The isolated demo
/// deliberately consumes cranpose from crates.io (it is the canary that
/// proves a release is usable by a real downstream crate), so it can only be
/// moved onto a release *after* that release is on the registry --
/// `publish.yml`'s `sync_versions` job defers the bump for exactly that
/// reason, and `bump_isolated_demo` runs this once the crates are live.
///
/// Rewrites the demo's manifest with regex substitution rather than a
/// round-tripped TOML re-serialization, so comments and formatting elsewhere
/// in the file survive untouched. Run `cargo update --manifest-path
/// apps/isolated-demo/Cargo.toml -p cranpose -p cranpose-core` afterwards to
/// move the demo's lockfile (that step needs the version to be resolvable).
fn sync_isolated_demo(options: SyncIsolatedDemoOptions) -> Result<(), String> {
    let root = workspace_root()?;
    sync_isolated_demo_at(&root, options)
}

/// The body of `sync_isolated_demo`, taking the workspace root explicitly so
/// tests can point it at a fixture directory instead of shelling out to
/// `cargo metadata` for the real workspace.
fn sync_isolated_demo_at(root: &Path, options: SyncIsolatedDemoOptions) -> Result<(), String> {
    let version = match options.version {
        Some(version) => version,
        None => workspace_package_version(root)?,
    };
    let version = version.strip_prefix('v').unwrap_or(&version);
    if !SEMVER_RE.is_match(version) {
        return Err(format!("Not a semver version: '{version}'"));
    }

    let manifest_path = root.join("apps/isolated-demo/Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("failed to read `{}`: {error}", manifest_path.display()))?;

    let mut changed = Vec::new();
    let after_inline_tables =
        rewrite_cranpose_versions(&INLINE_TABLE_RE, &manifest, version, &mut changed);
    let rewritten =
        rewrite_cranpose_versions(&BARE_STRING_RE, &after_inline_tables, version, &mut changed);

    if changed.is_empty() {
        println!("apps/isolated-demo already depends on cranpose {version}.");
        return Ok(());
    }

    fs::write(&manifest_path, rewritten)
        .map_err(|error| format!("failed to write `{}`: {error}", manifest_path.display()))?;
    for line in &changed {
        println!("apps/isolated-demo: {line}");
    }
    Ok(())
}

/// Splits `text` on `\n`, keeping each line's terminator attached (matching
/// Python's `str.splitlines(keepends=True)` for the plain-`\n` files this
/// module edits -- Cargo.toml and Cargo.lock never carry `\r\n` in this
/// repository).
fn split_keepends(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            lines.push(text[start..=index].to_owned());
            start = index + 1;
        }
    }
    if start < text.len() {
        lines.push(text[start..].to_owned());
    }
    lines
}

fn find_section_header_index(lines: &[String], section: &str) -> Option<usize> {
    let header = format!("[{section}]");
    lines.iter().position(|line| line.trim() == header)
}

/// The line range of `[section]`'s body: `(first line after the header,
/// first line of the next top-level-or-nested `[...]` header, or the end of
/// the file)`.
fn find_section_bounds(lines: &[String], section: &str) -> Result<(usize, usize), String> {
    let header_index = find_section_header_index(lines, section)
        .ok_or_else(|| format!("Missing [{section}] in Cargo.toml"))?;
    let start = header_index + 1;
    let mut end = lines.len();
    for (offset, line) in lines[start..].iter().enumerate() {
        let stripped = line.trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            end = start + offset;
            break;
        }
    }
    Ok((start, end))
}

static WORKSPACE_VERSION_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*version\s*=").expect("WORKSPACE_VERSION_LINE_RE is valid"));
static WORKSPACE_VERSION_DOTTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*version\.").expect("WORKSPACE_VERSION_DOTTED_RE is valid"));
static LEADING_WHITESPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*").expect("LEADING_WHITESPACE_RE is valid"));
static CRANPOSE_DEP_TABLE_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*cranpose[\w-]*\s*=\s*\{").expect("CRANPOSE_DEP_TABLE_LINE_RE is valid")
});
static VERSION_KV_PRESENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"version\s*=\s*"[^"]+""#).expect("VERSION_KV_PRESENT_RE is valid")
});
static VERSION_KV_REPLACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(version\s*=\s*")[^"]+("\s*)"#).expect("VERSION_KV_REPLACE_RE is valid")
});

fn leading_whitespace(line: &str) -> &str {
    LEADING_WHITESPACE_RE
        .find(line)
        .map(|m| m.as_str())
        .unwrap_or("")
}

/// Inserts a `[workspace.package]` section (with the same defaults the
/// former `scripts/check_cranpose_versions.py`'s sibling release script
/// used) right before the end of `[workspace]`, if one is not already
/// present. A workspace this repository ships always has one already; this
/// exists only because the release script it replaces defended against its
/// absence.
fn ensure_workspace_package_section(lines: &mut Vec<String>, version: &str) -> Result<(), String> {
    if find_section_header_index(lines, "workspace.package").is_some() {
        return Ok(());
    }

    let (_, workspace_end) = find_section_bounds(lines, "workspace")?;
    let insert_at = workspace_end;

    let mut block = Vec::new();
    if insert_at > 0 && !lines[insert_at - 1].trim().is_empty() {
        block.push("\n".to_owned());
    }
    block.extend([
        "[workspace.package]\n".to_owned(),
        format!("version = \"{version}\"\n"),
        "edition = \"2021\"\n".to_owned(),
        "license = \"Apache-2.0\"\n".to_owned(),
        "repository = \"https://github.com/samoylenkodmitry/cranpose\"\n".to_owned(),
        "homepage = \"https://samoylenkodmitry.github.io/cranpose/\"\n".to_owned(),
        "\n".to_owned(),
    ]);
    lines.splice(insert_at..insert_at, block);
    Ok(())
}

/// Rewrites `[workspace.package]`'s `version` to `version`, dropping any
/// other `version = ...` or dotted `version.xxx = ...` line in the section
/// and leaving everything else untouched.
fn update_workspace_package_section(lines: &mut Vec<String>, version: &str) -> Result<(), String> {
    ensure_workspace_package_section(lines, version)?;
    let (start, end) = find_section_bounds(lines, "workspace.package")?;

    let mut new_lines = Vec::new();
    let mut version_written = false;
    for line in &lines[start..end] {
        if WORKSPACE_VERSION_LINE_RE.is_match(line) {
            if !version_written {
                let indent = leading_whitespace(line);
                new_lines.push(format!("{indent}version = \"{version}\"\n"));
                version_written = true;
            }
            continue;
        }
        if WORKSPACE_VERSION_DOTTED_RE.is_match(line) {
            continue;
        }
        new_lines.push(line.clone());
    }
    if !version_written {
        new_lines.insert(0, format!("version = \"{version}\"\n"));
    }
    lines.splice(start..end, new_lines);
    Ok(())
}

/// Rewrites every `cranpose[-foo] = { ... version = "x" ... }` line in
/// `[workspace.dependencies]` to `version`, leaving the rest of each line
/// (path, default-features, ...) untouched. A dependency table with no
/// `version` key at all is left alone and reported as a mismatch, matching
/// the release script this replaces -- a workspace dependency published to
/// crates.io must always carry an explicit version.
fn update_workspace_dependencies_section(
    lines: &mut [String],
    version: &str,
) -> Result<(), String> {
    let (start, end) = find_section_bounds(lines, "workspace.dependencies")?;
    let escaped_version = regex::escape(version);
    let version_matches_target =
        Regex::new(&format!(r#"version\s*=\s*"{escaped_version}""#)).expect("valid pattern");

    let mut mismatches = Vec::new();
    for line in &mut lines[start..end] {
        if !CRANPOSE_DEP_TABLE_LINE_RE.is_match(line) {
            continue;
        }
        if !VERSION_KV_PRESENT_RE.is_match(line) {
            mismatches.push(line.trim().to_owned());
            continue;
        }
        let replaced = VERSION_KV_REPLACE_RE
            .replace_all(line, format!("${{1}}{version}${{2}}"))
            .into_owned();
        if !version_matches_target.is_match(&replaced) {
            mismatches.push(replaced.trim().to_owned());
        }
        *line = replaced;
    }

    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Some cranpose workspace dependencies were not updated:\n{}",
            mismatches.join("\n")
        ))
    }
}

/// Bumps `path` (a `Cargo.toml`) to `version`: `workspace.package.version`
/// and every cranpose `workspace.dependencies` entry. Rewrites the file in
/// place via line-level text edits -- not a TOML parse/reserialize -- so
/// comments, ordering and formatting elsewhere in the manifest survive
/// untouched. Writes only if the content actually changed.
fn bump_cargo_toml_version(path: &Path, version: &str) -> Result<(), String> {
    let original = fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    let mut lines = split_keepends(&original);

    update_workspace_package_section(&mut lines, version)?;
    update_workspace_dependencies_section(&mut lines, version)?;

    let updated = lines.concat();
    if updated != original {
        fs::write(path, updated)
            .map_err(|error| format!("failed to write `{}`: {error}", path.display()))?;
    }
    Ok(())
}

/// Bumps every `[[package]]` in `path` (a `Cargo.lock`) whose `name` starts
/// with `cranpose` to `version`, leaving `source`, `checksum` and
/// `dependencies` lines alone. Always rewrites the file (matching the
/// release script this replaces), even when no line actually changed.
fn bump_cargo_lock_version(path: &Path, version: &str) -> Result<(), String> {
    if !path.exists() {
        return Err("Cargo.lock not found".to_owned());
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    let mut lines = split_keepends(&text);

    let mut in_package = false;
    let mut update_version = false;
    for line in &mut lines {
        let stripped = line.trim();
        if stripped == "[[package]]" {
            in_package = true;
            update_version = false;
            continue;
        }
        if in_package && stripped.starts_with('[') && stripped.ends_with(']') {
            in_package = false;
            update_version = false;
            continue;
        }
        if in_package && stripped.starts_with("name = ") {
            let name = stripped["name = ".len()..].trim().trim_matches('"');
            update_version = name.starts_with("cranpose");
            continue;
        }
        if in_package && update_version && stripped.starts_with("version = ") {
            let indent = leading_whitespace(line).to_owned();
            *line = format!("{indent}version = \"{version}\"\n");
            update_version = false;
        }
    }

    fs::write(path, lines.concat())
        .map_err(|error| format!("failed to write `{}`: {error}", path.display()))
}

/// `cargo xtask bump-release-version`: ported from the former inline
/// `python3` heredocs in `publish.yml`'s `sync_versions` job. Neither
/// heredoc imported `tomllib`, so this pair was not the release-breaking
/// half of the ambient-Python defect -- but it is still ambient Python in
/// the same release path, and the fix for one instance is the fix for all
/// of them.
fn bump_release_version(tag: &str) -> Result<(), String> {
    let root = workspace_root()?;
    bump_release_version_at(&root, tag)
}

fn bump_release_version_at(root: &Path, tag: &str) -> Result<(), String> {
    let version = tag
        .strip_prefix('v')
        .ok_or_else(|| format!("Expected tag starting with 'v', got '{tag}'"))?;
    bump_cargo_toml_version(&root.join("Cargo.toml"), version)?;
    bump_cargo_lock_version(&root.join("Cargo.lock"), version)?;
    Ok(())
}

/// `cargo xtask verify-tag`: ported from the `import tomllib` heredoc in
/// `publish.yml`'s `publish` job -- the release-breaking half of the
/// ambient-Python defect. Deliberately checks only the tag and the
/// workspace dependency versions, not Cargo.lock or apps/isolated-demo:
/// this runs after `sync_versions` but before `bump_isolated_demo`, the
/// window where apps/isolated-demo still points at the *previous* release
/// on purpose.
fn verify_tag(tag: &str) -> Result<(), String> {
    let root = workspace_root()?;
    verify_tag_at(&root, tag)
}

fn verify_tag_at(root: &Path, tag: &str) -> Result<(), String> {
    let tag_version = tag
        .strip_prefix('v')
        .ok_or_else(|| format!("Expected tag starting with 'v', got '{tag}'"))?;

    let manifest = load_toml(&root.join("Cargo.toml"))?;
    let workspace = manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "Cargo.toml has no [workspace] table".to_owned())?;
    let workspace_version = workspace
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "Cargo.toml has no workspace.package.version".to_owned())?;

    if tag_version != workspace_version {
        return Err(format!(
            "Tag version v{tag_version} does not match workspace version {workspace_version}"
        ));
    }

    let mut mismatches = Vec::new();
    if let Some(dependencies) = workspace
        .get("dependencies")
        .and_then(toml::Value::as_table)
    {
        for (name, spec) in sorted_cranpose_dependencies(dependencies) {
            let dep_version = dependency_version(spec);
            if dep_version.as_deref() != Some(workspace_version) {
                mismatches.push(format!("{name} => {}", display_version(&dep_version)));
            }
        }
    }
    if !mismatches.is_empty() {
        return Err(format!(
            "Workspace dependency versions must match workspace version:\n{}",
            mismatches.join("\n")
        ));
    }

    println!("Tag v{tag_version} matches workspace version {workspace_version}");
    Ok(())
}

/// Runs `cargo metadata --no-deps --format-version 1`, optionally scoped to
/// `manifest_path`, and returns its stdout. The one place every xtask
/// command that needs cargo's own view of the workspace shells out from.
fn cargo_metadata_json(manifest_path: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new("cargo");
    command.args(["metadata", "--no-deps", "--format-version", "1"]);
    if let Some(manifest_path) = manifest_path {
        command.arg("--manifest-path").arg(manifest_path);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo metadata returned invalid UTF-8: {error}"))
}

/// `cargo xtask publish-order`: ported from the `json`-only (no `tomllib`)
/// heredoc in `publish.yml`'s `publish` job. Resolves every cranpose
/// workspace crate's non-dev dependency edges into a publish order and
/// writes it to `publish-order.txt`, one crate per line, for the shell's
/// `while IFS= read -r crate; do ...; done < publish-order.txt` loop.
fn publish_order() -> Result<(), String> {
    let metadata_json = cargo_metadata_json(None)?;
    let order = resolve_publish_order(&metadata_json)?;

    let mut file_contents = order.join("\n");
    file_contents.push('\n');
    fs::write("publish-order.txt", file_contents)
        .map_err(|error| format!("failed to write `publish-order.txt`: {error}"))?;

    println!("Resolved publish order:");
    for name in &order {
        println!("  {name}");
    }
    Ok(())
}

/// The topological sort at the heart of `publish-order`, pulled out as a
/// pure function of `cargo metadata`'s JSON so it can be tested against a
/// fixture instead of a real `cargo metadata` invocation. Split from the
/// graph-building half (`cranpose_publish_graph`): together they were 27
/// deep, over the diff-scoped complexity gate's limit of 20.
fn resolve_publish_order(metadata_json: &str) -> Result<Vec<String>, String> {
    let metadata: serde_json::Value = serde_json::from_str(metadata_json)
        .map_err(|error| format!("cargo metadata returned invalid JSON: {error}"))?;
    let (package_order, remaining) = cranpose_publish_graph(&metadata)?;
    topological_publish_order(&package_order, remaining)
}

/// A cranpose workspace package's position among `workspace_members` (the
/// tie-breaker `topological_publish_order` sorts by), keyed by name, paired
/// with which other cranpose packages each one depends on non-dev.
type PublishGraph = (BTreeMap<String, usize>, BTreeMap<String, BTreeSet<String>>);

/// Every cranpose workspace package, its position among `workspace_members`
/// (the tie-breaker `topological_publish_order` sorts by), and which other
/// cranpose packages it depends on non-dev.
fn cranpose_publish_graph(metadata: &serde_json::Value) -> Result<PublishGraph, String> {
    let workspace_member_ids = metadata
        .get("workspace_members")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "cargo metadata output has no workspace_members array".to_owned())?;
    let member_order: BTreeMap<&str, usize> = workspace_member_ids
        .iter()
        .enumerate()
        .filter_map(|(index, id)| id.as_str().map(|id| (id, index)))
        .collect();

    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "cargo metadata output has no packages array".to_owned())?;

    let mut package_order = BTreeMap::<String, usize>::new();
    let mut package_deps = BTreeMap::<String, BTreeSet<String>>::new();
    for package in packages {
        let Some((name, order, dependencies)) = cranpose_package_entry(package, &member_order)?
        else {
            continue;
        };
        package_order.insert(name.clone(), order);
        package_deps.insert(name, dependencies);
    }

    // Only internal (cranpose-to-cranpose) edges gate the order.
    let remaining = package_deps
        .into_iter()
        .map(|(name, deps)| {
            let internal = deps
                .into_iter()
                .filter(|dep| package_order.contains_key(dep))
                .collect();
            (name, internal)
        })
        .collect();

    Ok((package_order, remaining))
}

/// A workspace package's publish-graph entry, or `None` when `package` is
/// not a cranpose workspace member (an external dependency, or a workspace
/// member this release does not publish).
fn cranpose_package_entry(
    package: &serde_json::Value,
    member_order: &BTreeMap<&str, usize>,
) -> Result<Option<(String, usize, BTreeSet<String>)>, String> {
    let id = package
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "cargo metadata package is missing an id".to_owned())?;
    let Some(&order) = member_order.get(id) else {
        return Ok(None);
    };
    let name = package
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "cargo metadata package is missing a name".to_owned())?;
    if !name.starts_with("cranpose") {
        return Ok(None);
    }

    // Dev-deps never gate publish order (path-only dev-deps are stripped
    // from published manifests); counting them creates false cycles like
    // liquid -> testing -> cranpose -> liquid.
    let dependencies = package
        .get("dependencies")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|dependency| {
            dependency.get("kind").and_then(serde_json::Value::as_str) != Some("dev")
        })
        .filter_map(|dependency| {
            dependency
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();

    Ok(Some((name.to_owned(), order, dependencies)))
}

/// Kahn's algorithm over `remaining` (package name -> its still-unpublished
/// cranpose dependencies): repeatedly publish whatever has none left,
/// breaking ties by `package_order` (declaration order in the workspace)
/// then by name, for a deterministic result.
fn topological_publish_order(
    package_order: &BTreeMap<String, usize>,
    mut remaining: BTreeMap<String, BTreeSet<String>>,
) -> Result<Vec<String>, String> {
    let mut order = Vec::new();
    while !remaining.is_empty() {
        let mut ready: Vec<String> = remaining
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(name, _)| name.clone())
            .collect();
        if ready.is_empty() {
            let cycle = remaining.keys().cloned().collect::<Vec<_>>().join(", ");
            return Err(format!("Cyclic cranpose publish dependencies: {cycle}"));
        }
        ready.sort_by_key(|name| (package_order[name], name.clone()));

        for name in &ready {
            order.push(name.clone());
            remaining.remove(name);
        }
        for deps in remaining.values_mut() {
            for name in &ready {
                deps.remove(name);
            }
        }
    }

    Ok(order)
}

/// The outcome of asking crates.io whether a crate version is published.
enum CrateLookup {
    Published,
    NotPublished,
    /// An HTTP response other than 200/404 -- fatal, never retried.
    UnexpectedStatus(String),
    /// A transport failure (DNS, connection, timeout) -- retried by pollers.
    QueryFailed(String),
}

/// crates.io rejects requests with a blank or generic `User-Agent` (curl's
/// own default included) with a 403, and asks API clients to identify
/// themselves -- see <https://crates.io/data-access#api>. `reqwest::blocking::get`
/// sends no `User-Agent` at all, so every lookup here needs a client built
/// with one instead of the bare shortcut function.
fn crates_io_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("cranpose-xtask (https://github.com/samoylenkodmitry/Cranpose)")
        .build()
        .map_err(|error| format!("failed to build an HTTP client: {error}"))
}

fn lookup_crate_version(
    client: &reqwest::blocking::Client,
    crate_name: &str,
    version: &str,
) -> CrateLookup {
    let url = format!("https://crates.io/api/v1/crates/{crate_name}/{version}");
    match client.get(&url).send() {
        Ok(response) => match response.status().as_u16() {
            200 => CrateLookup::Published,
            404 => CrateLookup::NotPublished,
            _ => CrateLookup::UnexpectedStatus(response.status().to_string()),
        },
        Err(error) => CrateLookup::QueryFailed(error.to_string()),
    }
}

/// `cargo xtask crate-published`: ported from the `crate_exists()` bash
/// function's `python3` heredoc in `publish.yml`'s `publish` job. Exits 0
/// (published), 1 (not published, HTTP 404) or 2 (error) -- that three-way
/// contract is what `publish_if_needed`'s `if crate_exists ...; then ...
/// else local rc=$?; if [ "$rc" -ne 1 ]; then ...` reads, so it bypasses
/// the ordinary `Result<(), String>` command dispatch in `main`.
fn crate_published_command(args: &[String]) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_crate_published_usage();
        return ExitCode::SUCCESS;
    }
    let (crate_name, version) = match args {
        [crate_name, version] => (crate_name, version),
        _ => {
            eprintln!("usage: cargo xtask crate-published <crate> <version>");
            return ExitCode::from(2);
        }
    };
    let client = match crates_io_client() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match lookup_crate_version(&client, crate_name, version) {
        CrateLookup::Published => ExitCode::SUCCESS,
        CrateLookup::NotPublished => ExitCode::FAILURE,
        CrateLookup::UnexpectedStatus(status) => {
            eprintln!("Unexpected response for {crate_name} {version}: {status}");
            ExitCode::from(2)
        }
        CrateLookup::QueryFailed(error) => {
            eprintln!("Failed to query crates.io for {crate_name} {version}: {error}");
            ExitCode::from(2)
        }
    }
}

/// The cranpose crates `bump_isolated_demo` waits on before pointing the
/// isolated demo at a release.
const RELEASE_WAIT_CRATES: &[&str] = &["cranpose", "cranpose-core"];
const RELEASE_WAIT_ATTEMPTS: u32 = 30;
const RELEASE_WAIT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// `cargo xtask wait-for-crates-io`: ported from the `python3` heredoc in
/// `publish.yml`'s `bump_isolated_demo` job. Polls each of
/// `RELEASE_WAIT_CRATES` up to `RELEASE_WAIT_ATTEMPTS` times,
/// `RELEASE_WAIT_INTERVAL` apart, before giving up on that crate.
fn wait_for_crates_io(tag_or_version: &str) -> Result<(), String> {
    let version = tag_or_version.strip_prefix('v').unwrap_or(tag_or_version);
    let client = crates_io_client()?;
    for crate_name in RELEASE_WAIT_CRATES {
        let mut published = false;
        for _ in 0..RELEASE_WAIT_ATTEMPTS {
            match lookup_crate_version(&client, crate_name, version) {
                CrateLookup::Published => {
                    println!("{crate_name} {version} is on crates.io.");
                    published = true;
                    break;
                }
                CrateLookup::NotPublished => {}
                CrateLookup::UnexpectedStatus(status) => {
                    return Err(format!(
                        "Unexpected response for {crate_name} {version}: {status}"
                    ));
                }
                CrateLookup::QueryFailed(error) => {
                    println!("query for {crate_name} failed ({error}); retrying");
                }
            }
            std::thread::sleep(RELEASE_WAIT_INTERVAL);
        }
        if !published {
            return Err(format!(
                "{crate_name} {version} never appeared on crates.io"
            ));
        }
    }
    Ok(())
}

mod gate_diff {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::LazyLock,
    };

    use regex::Regex;

    pub(crate) type Span = (usize, usize);
    pub(crate) type HunkSpans = BTreeMap<String, Vec<(Option<Span>, Option<Span>)>>;
    pub(crate) type RangesByFile = BTreeMap<String, Vec<Span>>;

    pub(crate) fn ensure_ref(root: &Path, base: &str) -> Result<(), String> {
        if let Some(branch) = base.strip_prefix("origin/") {
            let _ = Command::new("git")
                .args(["fetch", "--quiet", "origin", branch])
                .current_dir(root)
                .status();
        }
        let resolved = Command::new("git")
            .args(["rev-parse", "--verify", "-q", base])
            .current_dir(root)
            .output()
            .map_err(|error| format!("gate_diff: failed to run git rev-parse: {error}"))?;
        if !resolved.status.success() {
            return Err(format!(
                "gate_diff: cannot resolve base ref `{base}`: it fetched nothing and no local ref by that name exists"
            ));
        }
        Ok(())
    }

    pub(crate) fn is_shallow_repository(root: &Path) -> Result<bool, String> {
        let output = Command::new("git")
            .args(["rev-parse", "--is-shallow-repository"])
            .current_dir(root)
            .output()
            .map_err(|error| format!("gate_diff: failed to run git rev-parse: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
    }

    pub(crate) fn merge_base(root: &Path, base: &str) -> Result<String, String> {
        ensure_ref(root, base)?;

        let first_attempt = Command::new("git")
            .args(["merge-base", base, "HEAD"])
            .current_dir(root)
            .output()
            .map_err(|error| format!("gate_diff: failed to run git merge-base: {error}"))?;
        if first_attempt.status.success() {
            return Ok(String::from_utf8_lossy(&first_attempt.stdout)
                .trim()
                .to_owned());
        }

        if !is_shallow_repository(root)? {
            return Err(format!(
                "gate_diff: no merge base between `{base}` and HEAD, and the checkout \
                 already has full history -- these two branches share no common ancestor, \
                 so the diff cannot be scoped"
            ));
        }

        let deepened = Command::new("git")
            .args(["fetch", "--quiet", "--unshallow", "origin"])
            .current_dir(root)
            .status()
            .map_err(|error| format!("gate_diff: failed to run git fetch: {error}"))?;
        if !deepened.success() {
            return Err(format!(
                "gate_diff: the checkout is shallow and `git fetch --unshallow origin` \
                 failed -- cannot deepen history enough to find a merge base with `{base}`"
            ));
        }

        let second_attempt = Command::new("git")
            .args(["merge-base", base, "HEAD"])
            .current_dir(root)
            .output()
            .map_err(|error| format!("gate_diff: failed to run git merge-base: {error}"))?;
        if !second_attempt.status.success() {
            return Err(format!(
                "gate_diff: no merge base between `{base}` and HEAD even after \
                 `git fetch --unshallow` -- cannot scope the diff safely"
            ));
        }
        Ok(String::from_utf8_lossy(&second_attempt.stdout)
            .trim()
            .to_owned())
    }

    static HUNK_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")
            .expect("HUNK_HEADER_RE is a valid pattern")
    });

    pub(crate) fn parse_hunk_spans(diff_text: &str) -> HunkSpans {
        let mut hunks: HunkSpans = BTreeMap::new();
        let mut current_file: Option<String> = None;
        for line in diff_text.split('\n') {
            if let Some(path) = line.strip_prefix("+++ ") {
                current_file = if path == "/dev/null" {
                    None
                } else {
                    Some(path.to_owned())
                };
                continue;
            }
            if !line.starts_with("@@ ") {
                continue;
            }
            let Some(file) = current_file.as_ref() else {
                continue;
            };
            let Some(captures) = HUNK_HEADER_RE.captures(line) else {
                continue;
            };
            let old_start: usize = captures[1].parse().expect("hunk header has digits");
            let old_count: usize = captures
                .get(2)
                .map_or(1, |m| m.as_str().parse().expect("hunk header has digits"));
            let new_start: usize = captures[3].parse().expect("hunk header has digits");
            let new_count: usize = captures
                .get(4)
                .map_or(1, |m| m.as_str().parse().expect("hunk header has digits"));
            let old_span = (old_count != 0).then(|| (old_start, old_start + old_count - 1));
            let new_span = (new_count != 0).then(|| (new_start, new_start + new_count - 1));
            hunks
                .entry(file.clone())
                .or_default()
                .push((old_span, new_span));
        }
        hunks
    }

    fn is_ident_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_'
    }

    fn is_hex_digit(ch: Option<&char>) -> bool {
        matches!(ch, Some(ch) if ch.is_ascii_hexdigit())
    }

    fn raw_string_prefix_end(chars: &[char], i: usize) -> Option<(usize, usize)> {
        let mut j = i;
        if chars.get(j) == Some(&'b') {
            j += 1;
        }
        if chars.get(j) != Some(&'r') {
            return None;
        }
        j += 1;
        let hashes_start = j;
        while chars.get(j) == Some(&'#') && j - hashes_start < 255 {
            j += 1;
        }
        let hashes = j - hashes_start;
        if chars.get(j) != Some(&'"') {
            return None;
        }
        Some((hashes, j + 1))
    }

    fn find_char_subsequence(haystack: &[char], needle: &[char], from: usize) -> Option<usize> {
        if needle.is_empty() {
            return Some(from);
        }
        if from + needle.len() > haystack.len() {
            return None;
        }
        (from..=haystack.len() - needle.len())
            .find(|&start| haystack[start..start + needle.len()] == *needle)
    }

    fn match_hex_escape(chars: &[char], after_backslash: usize) -> Option<usize> {
        if chars.get(after_backslash) == Some(&'x')
            && is_hex_digit(chars.get(after_backslash + 1))
            && is_hex_digit(chars.get(after_backslash + 2))
        {
            Some(after_backslash + 3)
        } else {
            None
        }
    }

    fn match_unicode_escape(chars: &[char], after_backslash: usize) -> Option<usize> {
        if chars.get(after_backslash) != Some(&'u') || chars.get(after_backslash + 1) != Some(&'{')
        {
            return None;
        }
        let hex_start = after_backslash + 2;
        let mut k = hex_start;
        while is_hex_digit(chars.get(k)) && k - hex_start < 6 {
            k += 1;
        }
        (k > hex_start && chars.get(k) == Some(&'}')).then_some(k + 1)
    }

    fn match_any_escape(chars: &[char], after_backslash: usize) -> Option<usize> {
        match chars.get(after_backslash) {
            Some(&c) if c != '\n' => Some(after_backslash + 1),
            _ => None,
        }
    }

    fn match_char_literal_escape(chars: &[char], after_quote: usize) -> Option<usize> {
        let after_backslash = after_quote + 1;
        match_hex_escape(chars, after_backslash)
            .or_else(|| match_unicode_escape(chars, after_backslash))
            .or_else(|| match_any_escape(chars, after_backslash))
    }

    fn match_char_literal_body(chars: &[char], after_quote: usize) -> Option<usize> {
        if chars.get(after_quote) == Some(&'\\') {
            return match_char_literal_escape(chars, after_quote);
        }
        match chars.get(after_quote) {
            Some(&c) if c != '\'' && c != '\\' && c != '\n' => Some(after_quote + 1),
            _ => None,
        }
    }

    fn match_char_literal(chars: &[char], i: usize) -> Option<usize> {
        if chars.get(i) != Some(&'\'') {
            return None;
        }
        let after_body = match_char_literal_body(chars, i + 1)?;
        (chars.get(after_body) == Some(&'\'')).then_some(after_body + 1)
    }

    struct Scanned {
        token: String,
        next_i: usize,
        newlines: usize,
    }

    fn advance_in_block_comment(chars: &[char], i: usize) -> (usize, i32) {
        if chars.get(i) == Some(&'/') && chars.get(i + 1) == Some(&'*') {
            (i + 2, 1)
        } else if chars.get(i) == Some(&'*') && chars.get(i + 1) == Some(&'/') {
            (i + 2, -1)
        } else {
            (i + 1, 0)
        }
    }

    fn skip_line_comment(chars: &[char], i: usize) -> Option<usize> {
        if chars.get(i) != Some(&'/') || chars.get(i + 1) != Some(&'/') {
            return None;
        }
        Some(
            chars[i..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(chars.len(), |p| p + i),
        )
    }

    fn starts_block_comment(chars: &[char], i: usize) -> bool {
        chars.get(i) == Some(&'/') && chars.get(i + 1) == Some(&'*')
    }

    fn scan_raw_string(chars: &[char], i: usize) -> Option<Scanned> {
        let (hashes, open_end) = raw_string_prefix_end(chars, i)?;
        let mut end_marker = vec!['"'];
        end_marker.extend(std::iter::repeat_n('#', hashes));
        Some(match find_char_subsequence(chars, &end_marker, open_end) {
            None => Scanned {
                token: chars[i..].iter().collect(),
                next_i: chars.len(),
                newlines: 0,
            },
            Some(end) => {
                let span_end = end + end_marker.len();
                let token: String = chars[i..span_end].iter().collect();
                let newlines = token.matches('\n').count();
                Scanned {
                    token,
                    next_i: span_end,
                    newlines,
                }
            }
        })
    }

    fn scan_double_quoted_string(chars: &[char], i: usize) -> Scanned {
        let n = chars.len();
        let mut j = i + 1;
        while j < n {
            let c = chars[j];
            if c == '\\' && j + 1 < n {
                j += 2;
                continue;
            }
            j += 1;
            if c == '"' {
                break;
            }
        }
        let token: String = chars[i..j].iter().collect();
        let newlines = token.matches('\n').count();
        Scanned {
            token,
            next_i: j,
            newlines,
        }
    }

    fn scan_identifier(chars: &[char], i: usize) -> Scanned {
        let n = chars.len();
        let mut j = i;
        while j < n && is_ident_char(chars[j]) {
            j += 1;
        }
        Scanned {
            token: chars[i..j].iter().collect(),
            next_i: j,
            newlines: 0,
        }
    }

    fn push_scanned(tokens: &mut [Vec<String>], line_idx: &mut usize, scanned: Scanned) -> usize {
        tokens[*line_idx].push(scanned.token);
        *line_idx += scanned.newlines;
        scanned.next_i
    }

    pub(crate) fn code_tokens_by_line(text: &str) -> Vec<Vec<String>> {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let line_count = text.matches('\n').count() + 1;
        let mut tokens: Vec<Vec<String>> = vec![Vec::new(); line_count];
        let mut line_idx = 0usize;
        let mut i = 0usize;
        let mut block_depth = 0u32;

        while i < n {
            let ch = chars[i];

            if ch == '\n' {
                line_idx += 1;
                i += 1;
                continue;
            }

            if block_depth > 0 {
                let (next_i, delta) = advance_in_block_comment(&chars, i);
                block_depth = block_depth.saturating_add_signed(delta);
                i = next_i;
                continue;
            }

            if ch == ' ' || ch == '\t' || ch == '\r' {
                i += 1;
                continue;
            }

            if let Some(next_i) = skip_line_comment(&chars, i) {
                i = next_i;
                continue;
            }

            if starts_block_comment(&chars, i) {
                block_depth = 1;
                i += 2;
                continue;
            }

            let prev_is_ident = i > 0 && is_ident_char(chars[i - 1]);
            if !prev_is_ident {
                if let Some(scanned) = scan_raw_string(&chars, i) {
                    i = push_scanned(&mut tokens, &mut line_idx, scanned);
                    continue;
                }
            }

            if ch == '"' {
                let scanned = scan_double_quoted_string(&chars, i);
                i = push_scanned(&mut tokens, &mut line_idx, scanned);
                continue;
            }

            if ch == '\'' {
                match match_char_literal(&chars, i) {
                    Some(end) => {
                        tokens[line_idx].push(chars[i..end].iter().collect());
                        i = end;
                    }
                    None => {
                        tokens[line_idx].push(ch.to_string());
                        i += 1;
                    }
                }
                continue;
            }

            if is_ident_char(ch) {
                let scanned = scan_identifier(&chars, i);
                i = push_scanned(&mut tokens, &mut line_idx, scanned);
                continue;
            }

            tokens[line_idx].push(ch.to_string());
            i += 1;
        }

        tokens
    }

    fn tokens_in_span(tokens_by_line: &[Vec<String>], span: Option<Span>) -> Vec<String> {
        let Some((start, end)) = span else {
            return Vec::new();
        };
        let upper = end.min(tokens_by_line.len());
        let mut result = Vec::new();
        for line in &tokens_by_line[start.saturating_sub(1).min(upper)..upper] {
            result.extend(line.iter().cloned());
        }
        result
    }

    const CLOSING_DELIMITERS: [&str; 3] = [")", "]", "}"];

    pub(crate) fn drop_trailing_commas(tokens: &[String]) -> Vec<String> {
        tokens
            .iter()
            .enumerate()
            .filter(|(idx, tok)| {
                !(tok.as_str() == ","
                    && idx + 1 < tokens.len()
                    && CLOSING_DELIMITERS.contains(&tokens[idx + 1].as_str()))
            })
            .map(|(_, tok)| tok.clone())
            .collect()
    }

    pub(crate) fn semantic_ranges(
        hunk_spans: &HunkSpans,
        mut read_old: impl FnMut(&str) -> String,
        mut read_new: impl FnMut(&str) -> String,
    ) -> RangesByFile {
        let mut ranges: RangesByFile = BTreeMap::new();
        for (file, spans) in hunk_spans {
            let new_tokens_by_line = code_tokens_by_line(&read_new(file));
            let old_tokens_by_line = code_tokens_by_line(&read_old(file));
            let mut kept: Vec<Span> = Vec::new();
            for (old_span, new_span) in spans {
                let Some(new_span) = *new_span else {
                    continue;
                };
                let old_tokens =
                    drop_trailing_commas(&tokens_in_span(&old_tokens_by_line, *old_span));
                let new_tokens =
                    drop_trailing_commas(&tokens_in_span(&new_tokens_by_line, Some(new_span)));
                if old_tokens == new_tokens {
                    continue;
                }
                kept.push(new_span);
            }
            if !kept.is_empty() {
                ranges.insert(file.clone(), kept);
            }
        }
        ranges
    }

    pub(crate) fn changed_ranges(
        root: &Path,
        base: &str,
        pathspec: &str,
    ) -> Result<RangesByFile, String> {
        let base_sha = merge_base(root, base)?;
        let output = Command::new("git")
            .args([
                "diff",
                "--unified=0",
                "--no-prefix",
                &base_sha,
                "--",
                pathspec,
            ])
            .current_dir(root)
            .output()
            .map_err(|error| format!("gate_diff: failed to run git diff: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let diff_text = String::from_utf8_lossy(&output.stdout).into_owned();
        let hunk_spans = parse_hunk_spans(&diff_text);

        let read_new =
            |file: &str| -> String { fs::read_to_string(root.join(file)).unwrap_or_default() };
        let read_old = |file: &str| -> String {
            let blob = Command::new("git")
                .args(["show", &format!("{base_sha}:{file}")])
                .current_dir(root)
                .output();
            match blob {
                Ok(blob) if blob.status.success() => {
                    String::from_utf8_lossy(&blob.stdout).into_owned()
                }
                _ => String::new(),
            }
        };

        Ok(semantic_ranges(&hunk_spans, read_old, read_new))
    }

    pub(crate) fn write_old_blobs(
        root: &Path,
        base_sha: &str,
        files: &[String],
        dest_root: &Path,
    ) -> Result<Vec<String>, String> {
        let mut written = Vec::new();
        for file in files {
            let blob = Command::new("git")
                .args(["show", &format!("{base_sha}:{file}")])
                .current_dir(root)
                .output()
                .map_err(|error| format!("gate_diff: failed to run git show: {error}"))?;
            if !blob.status.success() {
                continue;
            }
            let dest = dest_root.join(file);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "gate_diff: failed to create `{}`: {error}",
                        parent.display()
                    )
                })?;
            }
            fs::write(&dest, &blob.stdout).map_err(|error| {
                format!("gate_diff: failed to write `{}`: {error}", dest.display())
            })?;
            written.push(file.clone());
        }
        Ok(written)
    }

    fn home_dir() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    fn expand_user(path: &Path, home: &Path) -> PathBuf {
        match path.to_str() {
            Some("~") => home.to_path_buf(),
            Some(s) if s.starts_with("~/") => home.join(&s[2..]),
            _ => path.to_path_buf(),
        }
    }

    pub(crate) fn cargo_bin_dir() -> PathBuf {
        cargo_bin_dir_for(std::env::var_os("CARGO_HOME"), home_dir())
    }

    pub(crate) fn cargo_bin_dir_for(
        cargo_home: Option<std::ffi::OsString>,
        home: PathBuf,
    ) -> PathBuf {
        match cargo_home {
            Some(cargo_home) => expand_user(Path::new(&cargo_home), &home).join("bin"),
            None => home.join(".cargo").join("bin"),
        }
    }

    #[cfg(unix)]
    fn is_executable_file(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        path.is_file()
            && fs::metadata(path)
                .map(|meta| meta.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }

    #[cfg(not(unix))]
    fn is_executable_file(path: &Path) -> bool {
        path.is_file()
    }

    fn install_cargo_tool(root: &Path, name: &str) {
        let _ = Command::new("cargo")
            .args(["install", name, "--locked"])
            .current_dir(root)
            .status();
    }

    pub(crate) fn resolve_cargo_tool(
        root: &Path,
        name: &str,
        on_install_failure_hint: &str,
    ) -> Result<PathBuf, String> {
        resolve_cargo_tool_in(
            root,
            &cargo_bin_dir(),
            name,
            on_install_failure_hint,
            install_cargo_tool,
        )
    }

    pub(crate) fn resolve_cargo_tool_in(
        root: &Path,
        bin_dir: &Path,
        name: &str,
        on_install_failure_hint: &str,
        install: impl FnOnce(&Path, &str),
    ) -> Result<PathBuf, String> {
        let binary = bin_dir.join(name);
        if !is_executable_file(&binary) {
            install(root, name);
        }
        if !is_executable_file(&binary) {
            let hint = if on_install_failure_hint.is_empty() {
                String::new()
            } else {
                format!(" {on_install_failure_hint}")
            };
            return Err(format!(
                "gate_diff: {name} is not at {} and `cargo install {name} --locked` did not put it there.{hint}",
                binary.display()
            ));
        }
        Ok(binary)
    }

    pub(crate) fn intersects(a: Span, b: Span) -> bool {
        a.0 <= b.1 && b.0 <= a.1
    }

    pub(crate) fn any_intersect(ranges: &[Span], span: Span) -> bool {
        ranges.iter().any(|&range| intersects(range, span))
    }
}
mod complexity_gate {
    use std::{collections::BTreeMap, fs, path::Path, process::Command};

    use serde_json::Value;

    use super::gate_diff;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct FunctionMetric {
        pub(crate) name: String,
        pub(crate) start: Option<usize>,
        pub(crate) end: Option<usize>,
        pub(crate) cyclomatic: Option<i64>,
    }

    const ANONYMOUS: &str = "<anonymous>";

    pub(crate) fn load_max_cyclomatic(config_path: &Path) -> Result<i64, String> {
        let config = super::load_toml(config_path)?;
        config
            .get("complexity")
            .and_then(toml::Value::as_table)
            .and_then(|table| table.get("max_cyclomatic"))
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| {
                format!(
                    "{}: missing complexity.max_cyclomatic",
                    config_path.display()
                )
            })
    }

    pub(crate) fn functions_in(
        space: &Value,
        out: &mut Vec<FunctionMetric>,
        inside_function: bool,
    ) {
        let is_function = space.get("kind").and_then(Value::as_str) == Some("function");
        let raw_name = space.get("name").and_then(Value::as_str);
        let is_named = raw_name.is_some_and(|name| !name.is_empty() && name != ANONYMOUS);
        if is_function && (is_named || !inside_function) {
            let cyclomatic = space
                .get("metrics")
                .and_then(|metrics| metrics.get("cyclomatic"))
                .and_then(|cyclomatic| cyclomatic.get("sum"))
                .and_then(Value::as_f64)
                .map(|sum| sum as i64);
            out.push(FunctionMetric {
                name: if is_named {
                    raw_name
                        .expect("is_named implies raw_name is Some")
                        .to_owned()
                } else {
                    ANONYMOUS.to_owned()
                },
                start: space
                    .get("start_line")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize),
                end: space
                    .get("end_line")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize),
                cyclomatic,
            });
        }
        if let Some(children) = space.get("spaces").and_then(Value::as_array) {
            for child in children {
                functions_in(child, out, inside_function || is_function);
            }
        }
    }

    fn find_json_files_recursive(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        let Ok(entries) = fs::read_dir(dir) else {
            return found;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(find_json_files_recursive(&path));
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                found.push(path);
            }
        }
        found
    }

    pub(crate) fn analyze_files(
        rust_code_analysis_cli: &Path,
        files: &[String],
        out_dir: &Path,
        cwd: &Path,
    ) -> Result<BTreeMap<String, Vec<FunctionMetric>>, String> {
        let mut command = Command::new(rust_code_analysis_cli);
        command
            .arg("-m")
            .arg("-O")
            .arg("json")
            .arg("-o")
            .arg(out_dir)
            .current_dir(cwd);
        for file in files {
            command.arg("-p").arg(file);
        }
        let output = command.output().map_err(|error| {
            format!("complexity-gate: failed to run rust-code-analysis-cli: {error}")
        })?;

        let produced = find_json_files_recursive(out_dir);
        if !output.status.success() && produced.is_empty() {
            return Err(format!(
                "complexity-gate: rust-code-analysis-cli failed with no output\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim());
        }

        let mut by_file = BTreeMap::new();
        for file in files {
            let json_path = out_dir.join(format!("{file}.json"));
            if !json_path.exists() {
                eprintln!("complexity-gate: no analysis produced for {file}, skipping");
                continue;
            }
            let text = match fs::read_to_string(&json_path) {
                Ok(text) => text,
                Err(error) => {
                    eprintln!("complexity-gate: could not read analysis for {file}: {error}");
                    continue;
                }
            };
            let data: Value = match serde_json::from_str(&text) {
                Ok(data) => data,
                Err(error) => {
                    eprintln!("complexity-gate: could not parse analysis for {file}: {error}");
                    continue;
                }
            };
            let mut functions = Vec::new();
            functions_in(&data, &mut functions, false);
            by_file.insert(file.clone(), functions);
        }
        Ok(by_file)
    }

    pub(crate) fn old_complexity_by_name(
        functions: &[FunctionMetric],
    ) -> BTreeMap<String, Vec<i64>> {
        let mut by_name: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for func in functions {
            if func.name == ANONYMOUS {
                continue;
            }
            let Some(cyclomatic) = func.cyclomatic else {
                continue;
            };
            by_name
                .entry(func.name.clone())
                .or_default()
                .push(cyclomatic);
        }
        by_name
    }

    pub(crate) fn find_violations(
        ranges: &gate_diff::RangesByFile,
        new_functions_by_file: &BTreeMap<String, Vec<FunctionMetric>>,
        old_functions_by_file: &BTreeMap<String, Vec<FunctionMetric>>,
        max_cyclomatic: i64,
    ) -> Vec<String> {
        let mut violations = Vec::new();
        for (file, functions) in new_functions_by_file {
            let file_ranges = ranges.get(file).map(Vec::as_slice).unwrap_or(&[]);
            let old_by_name = old_complexity_by_name(
                old_functions_by_file
                    .get(file)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            );
            let mut cursor: BTreeMap<String, usize> = BTreeMap::new();
            for func in functions {
                let occurrence = *cursor.get(&func.name).unwrap_or(&0);
                cursor.insert(func.name.clone(), occurrence + 1);

                let (Some(start), Some(end), Some(new_cyclomatic)) =
                    (func.start, func.end, func.cyclomatic)
                else {
                    continue;
                };
                if !gate_diff::any_intersect(file_ranges, (start, end)) {
                    continue;
                }
                if new_cyclomatic <= max_cyclomatic {
                    continue;
                }
                let old_cyclomatic = old_by_name
                    .get(&func.name)
                    .and_then(|values| values.get(occurrence))
                    .copied();
                if let Some(old_cyclomatic) = old_cyclomatic {
                    if new_cyclomatic <= old_cyclomatic {
                        continue;
                    }
                }
                let reason = match old_cyclomatic {
                    Some(old) => format!("was {old}, is now {new_cyclomatic}"),
                    None => format!("is new at {new_cyclomatic}"),
                };
                violations.push(format!(
                    "{file}:{start}-{end} {} {reason} (limit {max_cyclomatic})",
                    func.name
                ));
            }
        }
        violations
    }

    fn old_functions_for(
        root: &Path,
        base_sha: &str,
        files: &[String],
        tmp_dir: &Path,
        rust_code_analysis_cli: &Path,
    ) -> Result<BTreeMap<String, Vec<FunctionMetric>>, String> {
        let old_src = tmp_dir.join("old-src");
        fs::create_dir(&old_src).map_err(|error| {
            format!(
                "complexity-gate: failed to create `{}`: {error}",
                old_src.display()
            )
        })?;
        let old_files = gate_diff::write_old_blobs(root, base_sha, files, &old_src)?;
        if old_files.is_empty() {
            return Ok(BTreeMap::new());
        }
        let old_out = tmp_dir.join("old-out");
        fs::create_dir(&old_out).map_err(|error| {
            format!(
                "complexity-gate: failed to create `{}`: {error}",
                old_out.display()
            )
        })?;
        analyze_files(rust_code_analysis_cli, &old_files, &old_out, &old_src)
    }

    pub(crate) fn run_at(root: &Path, base: &str, config_path: &Path) -> Result<(), String> {
        let max_cyclomatic = load_max_cyclomatic(config_path)?;
        let ranges = gate_diff::changed_ranges(root, base, "*.rs")?;
        if ranges.is_empty() {
            println!("complexity-gate: no changed Rust files against {base}");
            return Ok(());
        }

        let rust_code_analysis_cli =
            gate_diff::resolve_cargo_tool(root, "rust-code-analysis-cli", "")?;
        let base_sha = gate_diff::merge_base(root, base)?;

        let tmp = tempfile::tempdir()
            .map_err(|error| format!("complexity-gate: failed to create a temp dir: {error}"))?;

        let new_out = tmp.path().join("new-out");
        fs::create_dir(&new_out).map_err(|error| {
            format!(
                "complexity-gate: failed to create `{}`: {error}",
                new_out.display()
            )
        })?;
        let files: Vec<String> = ranges.keys().cloned().collect();
        let new_functions_by_file = analyze_files(&rust_code_analysis_cli, &files, &new_out, root)?;
        let old_functions_by_file =
            old_functions_for(root, &base_sha, &files, tmp.path(), &rust_code_analysis_cli)?;

        let violations = find_violations(
            &ranges,
            &new_functions_by_file,
            &old_functions_by_file,
            max_cyclomatic,
        );
        if !violations.is_empty() {
            return Err(super::violations_message(
                format!(
                    "complexity-gate: {} function(s) over the limit:",
                    violations.len()
                ),
                &violations,
            ));
        }

        let total_ranges: usize = ranges.values().map(Vec::len).sum();
        println!("complexity-gate: {total_ranges} changed line range(s) clean");
        Ok(())
    }
}
mod duplication_gate {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::Path,
        process::Command,
    };

    use serde_json::Value;

    use super::{gate_diff, gate_diff::Span};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct DuplicateSide {
        pub(crate) name: String,
        pub(crate) start: usize,
        pub(crate) end: usize,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Duplicate {
        pub(crate) first_file: DuplicateSide,
        pub(crate) second_file: DuplicateSide,
        pub(crate) lines: usize,
    }

    pub(crate) struct DuplicationConfig {
        pub(crate) min_lines: u64,
        pub(crate) min_tokens: u64,
        pub(crate) ignore_globs: Vec<String>,
    }

    pub(crate) fn load_duplication_config(config_path: &Path) -> Result<DuplicationConfig, String> {
        let config = super::load_toml(config_path)?;
        let table = config
            .get("duplication")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| format!("{}: missing [duplication] table", config_path.display()))?;
        let min_lines = table
            .get("min_lines")
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| format!("{}: missing duplication.min_lines", config_path.display()))?
            as u64;
        let min_tokens = table
            .get("min_tokens")
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| format!("{}: missing duplication.min_tokens", config_path.display()))?
            as u64;
        let ignore_globs = table
            .get("ignore_globs")
            .and_then(toml::Value::as_array)
            .map(|globs| {
                globs
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(DuplicationConfig {
            min_lines,
            min_tokens,
            ignore_globs,
        })
    }

    fn duplicate_side_from_json(value: &Value) -> Option<DuplicateSide> {
        Some(DuplicateSide {
            name: value.get("name")?.as_str()?.to_owned(),
            start: value.get("start")?.as_u64()? as usize,
            end: value.get("end")?.as_u64()? as usize,
        })
    }

    fn duplicate_from_json(value: &Value) -> Option<Duplicate> {
        Some(Duplicate {
            first_file: duplicate_side_from_json(value.get("firstFile")?)?,
            second_file: duplicate_side_from_json(value.get("secondFile")?)?,
            lines: value.get("lines")?.as_u64()? as usize,
        })
    }

    pub(crate) fn parse_duplicates(report: &Value) -> Vec<Duplicate> {
        report
            .get("duplicates")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(duplicate_from_json)
            .collect()
    }

    pub(crate) fn run_jscpd(
        jscpd: &Path,
        min_lines: u64,
        min_tokens: u64,
        ignore_globs: &[String],
        out_dir: &Path,
        cwd: &Path,
    ) -> Result<Vec<Duplicate>, String> {
        let mut command = Command::new(jscpd);
        command
            .arg("--min-lines")
            .arg(min_lines.to_string())
            .arg("--min-tokens")
            .arg(min_tokens.to_string())
            .arg("-f")
            .arg("rust")
            .arg("-r")
            .arg("json")
            .arg("-o")
            .arg(out_dir)
            .current_dir(cwd);
        if !ignore_globs.is_empty() {
            command.arg("--ignore").arg(ignore_globs.join(","));
        }
        command.arg(".");
        let output = command
            .output()
            .map_err(|error| format!("duplication-gate: failed to run jscpd: {error}"))?;

        let report_path = out_dir.join("jscpd-report.json");
        if !report_path.exists() {
            return Err(format!(
                "duplication-gate: jscpd produced no report\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let text = fs::read_to_string(&report_path)
            .map_err(|error| format!("duplication-gate: failed to read jscpd report: {error}"))?;
        let report: Value = serde_json::from_str(&text)
            .map_err(|error| format!("duplication-gate: could not parse jscpd report: {error}"))?;
        Ok(parse_duplicates(&report))
    }

    pub(crate) fn side_span(side: &DuplicateSide) -> (&str, Span) {
        (&side.name, (side.start, side.end))
    }

    pub(crate) fn file_pair(dup: &Duplicate) -> (String, String) {
        let mut names = [dup.first_file.name.clone(), dup.second_file.name.clone()];
        names.sort();
        let [a, b] = names;
        (a, b)
    }

    pub(crate) fn touches_diff(dup: &Duplicate, ranges: &gate_diff::RangesByFile) -> bool {
        let (first_file, first_span) = side_span(&dup.first_file);
        let (second_file, second_span) = side_span(&dup.second_file);
        gate_diff::any_intersect(
            ranges.get(first_file).map(Vec::as_slice).unwrap_or(&[]),
            first_span,
        ) || gate_diff::any_intersect(
            ranges.get(second_file).map(Vec::as_slice).unwrap_or(&[]),
            second_span,
        )
    }

    pub(crate) fn already_duplicated_before(
        candidate: &Duplicate,
        old_duplicates: &[Duplicate],
    ) -> bool {
        let candidate_pair = file_pair(candidate);
        for old_dup in old_duplicates {
            if file_pair(old_dup) != candidate_pair {
                continue;
            }
            if old_dup.lines == 0 || candidate.lines == 0 {
                continue;
            }
            let ratio = old_dup.lines as f64 / candidate.lines as f64;
            if (0.5..=2.0).contains(&ratio) {
                return true;
            }
        }
        false
    }

    pub(crate) fn find_violations(
        new_duplicates: &[Duplicate],
        old_duplicates: &[Duplicate],
        ranges: &gate_diff::RangesByFile,
    ) -> Vec<String> {
        let mut old_by_pair: BTreeMap<(String, String), Vec<Duplicate>> = BTreeMap::new();
        for old_dup in old_duplicates {
            old_by_pair
                .entry(file_pair(old_dup))
                .or_default()
                .push(old_dup.clone());
        }

        let mut violations = Vec::new();
        for dup in new_duplicates {
            if !touches_diff(dup, ranges) {
                continue;
            }
            let comparable = old_by_pair
                .get(&file_pair(dup))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if already_duplicated_before(dup, comparable) {
                continue;
            }
            let (first_file, first_span) = side_span(&dup.first_file);
            let (second_file, second_span) = side_span(&dup.second_file);
            let first_hit = gate_diff::any_intersect(
                ranges.get(first_file).map(Vec::as_slice).unwrap_or(&[]),
                first_span,
            );
            let second_hit = gate_diff::any_intersect(
                ranges.get(second_file).map(Vec::as_slice).unwrap_or(&[]),
                second_span,
            );
            violations.push(format!(
                "{first_file}:{}-{}{} duplicates {second_file}:{}-{}{} ({} lines, introduced by this diff)",
                first_span.0,
                first_span.1,
                if first_hit { " (new)" } else { "" },
                second_span.0,
                second_span.1,
                if second_hit { " (new)" } else { "" },
                dup.lines,
            ));
        }
        violations
    }

    const JSCPD_INSTALL_HINT: &str = "jscpd also ships an npm package that wraps a prebuilt copy \
        of the same Rust binary, but this project does not require Node.js and will not start \
        requiring it here -- if `cargo install jscpd --locked` cannot run, fix that (network, \
        registry, offline mirror), do not swap in `npm install -g jscpd`.";

    pub(crate) fn involved_files(candidates: &[Duplicate]) -> Vec<String> {
        let mut files: BTreeSet<String> = BTreeSet::new();
        for dup in candidates {
            files.insert(dup.first_file.name.clone());
            files.insert(dup.second_file.name.clone());
        }
        files.into_iter().collect()
    }

    fn old_duplicates_for(
        root: &Path,
        base_sha: &str,
        involved_files: &[String],
        tmp_dir: &Path,
        jscpd: &Path,
        config: &DuplicationConfig,
    ) -> Result<Vec<Duplicate>, String> {
        if involved_files.is_empty() {
            return Ok(Vec::new());
        }
        let old_src = tmp_dir.join("old-src");
        fs::create_dir(&old_src).map_err(|error| {
            format!(
                "duplication-gate: failed to create `{}`: {error}",
                old_src.display()
            )
        })?;
        let old_files = gate_diff::write_old_blobs(root, base_sha, involved_files, &old_src)?;
        if old_files.is_empty() {
            return Ok(Vec::new());
        }
        let old_out = tmp_dir.join("old-out");
        fs::create_dir(&old_out).map_err(|error| {
            format!(
                "duplication-gate: failed to create `{}`: {error}",
                old_out.display()
            )
        })?;
        run_jscpd(
            jscpd,
            config.min_lines,
            config.min_tokens,
            &config.ignore_globs,
            &old_out,
            &old_src,
        )
    }

    pub(crate) fn run_at(root: &Path, base: &str, config_path: &Path) -> Result<(), String> {
        let config = load_duplication_config(config_path)?;
        let ranges = gate_diff::changed_ranges(root, base, "*.rs")?;
        if ranges.is_empty() {
            println!("duplication-gate: no changed Rust files against {base}");
            return Ok(());
        }

        let jscpd = gate_diff::resolve_cargo_tool(root, "jscpd", JSCPD_INSTALL_HINT)?;
        let base_sha = gate_diff::merge_base(root, base)?;

        let tmp = tempfile::tempdir()
            .map_err(|error| format!("duplication-gate: failed to create a temp dir: {error}"))?;

        let new_out = tmp.path().join("new-out");
        fs::create_dir(&new_out).map_err(|error| {
            format!(
                "duplication-gate: failed to create `{}`: {error}",
                new_out.display()
            )
        })?;
        let duplicates = run_jscpd(
            &jscpd,
            config.min_lines,
            config.min_tokens,
            &config.ignore_globs,
            &new_out,
            root,
        )?;

        let candidates: Vec<Duplicate> = duplicates
            .iter()
            .filter(|dup| touches_diff(dup, &ranges))
            .cloned()
            .collect();
        let files_to_check = involved_files(&candidates);
        let old_duplicates = old_duplicates_for(
            root,
            &base_sha,
            &files_to_check,
            tmp.path(),
            &jscpd,
            &config,
        )?;

        let violations = find_violations(&duplicates, &old_duplicates, &ranges);
        if !violations.is_empty() {
            return Err(super::violations_message(
                format!(
                    "duplication-gate: {} clone(s) introduced by this diff:",
                    violations.len()
                ),
                &violations,
            ));
        }

        println!(
            "duplication-gate: {} clone(s) in the repo, {} touch the diff, none introduced by it",
            duplicates.len(),
            candidates.len()
        );
        Ok(())
    }
}

/// Checks a property, not a list: every `just <recipe>` that
/// `.github/workflows/rust.yml` invokes must be reachable from the
/// justfile's `ci` or `ci-full` aggregate. `clippy-wasm` broke this exact
/// way (#593) -- `wasm-build` gated every pull request on it, but `ci` (what
/// "run this before pushing" documents as the pre-push check) did not
/// contain it, so the only way to discover the break was to push and wait
/// for CI. A hand-maintained list drifting from what it stands for is the
/// same defect shape `clippy-android`, `clippy-svg`, `clippy-hyphenation`
/// and `clippy-robot` were each added to close, one at a time, after each
/// went unlinted until something noticed by hand. This gate makes the next
/// one impossible instead of noticing it later.
/// CI runs the robot suite as two halves: `robot-gpu` under a hardware
/// swapchain with a handful of examples skipped, and `robot-captures` under
/// software present running exactly those. Nothing enforced that the two
/// lists agree, so an example removed from one half and not added to the
/// other would stop running anywhere without a gate going red -- the same
/// shape as a hand-maintained list standing in for a property.
mod robot_suite_partition {
    use std::{collections::BTreeSet, fs, path::Path};

    /// Collects the arguments of every `--<flag> <value>` occurrence inside a
    /// recipe's body, where the recipe runs from `name:` at column zero to the
    /// next line at column zero.
    pub(crate) fn recipe_flag_values(
        justfile_text: &str,
        recipe: &str,
        flag: &str,
    ) -> BTreeSet<String> {
        let header = format!("{recipe}:");
        let mut values = BTreeSet::new();
        let mut inside = false;
        for line in justfile_text.split('\n') {
            let indented = line.starts_with(' ') || line.starts_with('\t');
            if !indented {
                if inside {
                    break;
                }
                inside = line.trim_end() == header;
                continue;
            }
            if !inside {
                continue;
            }
            let mut tokens = line.split_whitespace().peekable();
            while let Some(token) = tokens.next() {
                if token == flag {
                    if let Some(value) = tokens.peek() {
                        values.insert((*value).to_owned());
                    }
                }
            }
        }
        values
    }

    pub(crate) fn find_violations(justfile_text: &str) -> Vec<String> {
        let skipped = recipe_flag_values(justfile_text, "robot-gpu", "--skip");
        let captured = recipe_flag_values(justfile_text, "robot-captures", "--example");
        let mut violations = Vec::new();

        // Non-vacuity: two empty sets are trivially equal, and would let this
        // gate pass on a justfile it failed to parse at all.
        if skipped.is_empty() {
            violations.push(
                "`robot-gpu` declares no `--skip` examples; either the split is gone or this \
                 gate failed to parse the recipe"
                    .to_owned(),
            );
        }
        if captured.is_empty() {
            violations.push(
                "`robot-captures` declares no `--example` entries; either the split is gone or \
                 this gate failed to parse the recipe"
                    .to_owned(),
            );
        }
        for name in skipped.difference(&captured) {
            violations.push(format!(
                "`{name}` is skipped by `robot-gpu` but not run by `robot-captures`, so it runs \
                 nowhere"
            ));
        }
        for name in captured.difference(&skipped) {
            violations.push(format!(
                "`{name}` is run by `robot-captures` but not skipped by `robot-gpu`, so it runs \
                 twice"
            ));
        }
        violations
    }

    pub(crate) fn run_at(root: &Path) -> Result<(), String> {
        let path = root.join("justfile");
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("robot-suite-partition: failed to read {path:?}: {error}"))?;
        let violations = find_violations(&text);
        if violations.is_empty() {
            let count = recipe_flag_values(&text, "robot-gpu", "--skip").len();
            println!(
                "robot-suite-partition: the {count} example(s) `robot-gpu` skips are exactly \
                 those `robot-captures` runs"
            );
            return Ok(());
        }
        let mut message = format!(
            "robot-suite-partition: {} problem(s) with the robot suite split:",
            violations.len()
        );
        for violation in &violations {
            message.push_str("\n  ");
            message.push_str(violation);
        }
        Err(message)
    }
}

mod ci_gate_reachability {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::Path,
        sync::LazyLock,
    };

    use regex::Regex;

    /// `just`'s own recipe-name grammar: a letter or underscore, then
    /// letters, digits, underscores or hyphens. Requiring that first
    /// character keeps `cargo install just --locked` from reading as an
    /// invocation of a recipe named `--locked` -- `-` is a valid character
    /// inside a recipe name (`test-shell-helpers`) but never starts one.
    static JUST_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\bjust\s+([A-Za-z_][A-Za-z0-9_-]*)").expect("JUST_CALL_RE is a valid pattern")
    });

    /// Every `just <recipe>` name mentioned in `line`, ignoring anything
    /// after the name itself -- `just robot-one my_example` yields
    /// `robot-one`, so a recipe invoked with arguments is still found.
    fn just_recipe_calls(line: &str) -> impl Iterator<Item = String> + '_ {
        JUST_CALL_RE.captures_iter(line).map(|c| c[1].to_owned())
    }

    /// One `just <recipe>` call found in a workflow's `run:` step, kept with
    /// enough context to name where it came from in a failure message.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct WorkflowInvocation {
        pub(crate) recipe: String,
        pub(crate) line_number: usize,
        pub(crate) line_text: String,
    }

    fn leading_spaces(line: &str) -> usize {
        line.chars().take_while(|ch| *ch == ' ').count()
    }

    fn collect_workflow_calls(
        line: &str,
        line_number: usize,
        invocations: &mut Vec<WorkflowInvocation>,
    ) {
        for recipe in just_recipe_calls(line) {
            invocations.push(WorkflowInvocation {
                recipe,
                line_number,
                line_text: line.trim().to_owned(),
            });
        }
    }

    /// Every `just <recipe>` call a GitHub Actions workflow's `run:` steps
    /// make, whether written inline (`run: just fmt-check`) or inside a
    /// block scalar (`run: |` followed by indented shell lines). Only
    /// `run:` step bodies are scanned, not comments or doc-prose that merely
    /// mentions a recipe in backticks, by tracking each `run:` line's own
    /// indentation and following YAML's block-scalar rule: the block's
    /// content ends at the first non-blank line back at or above that
    /// indentation (blank lines inside the block do not end it).
    pub(crate) fn parse_workflow_just_invocations(workflow_text: &str) -> Vec<WorkflowInvocation> {
        let lines: Vec<&str> = workflow_text.split('\n').collect();
        let mut invocations = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index];
            let indent = leading_spaces(line);
            // `- run: just x` and `run: just x` are the same step to YAML.
            // Matching only the second would make this gate's coverage depend
            // on a formatting convention nothing enforces: one step written
            // with the leading dash would be skipped in silence, and a gate
            // that silently sees nothing reports every repository clean.
            let bare = line.trim_start();
            let bare = bare.strip_prefix("- ").map_or(bare, str::trim_start);
            let Some(rest) = bare.strip_prefix("run:") else {
                index += 1;
                continue;
            };
            let rest = rest.trim();
            let is_block_scalar = rest.is_empty() || rest.starts_with('|') || rest.starts_with('>');
            if !is_block_scalar {
                collect_workflow_calls(rest, index + 1, &mut invocations);
                index += 1;
                continue;
            }
            let mut cursor = index + 1;
            while cursor < lines.len() {
                let body_line = lines[cursor];
                if body_line.trim().is_empty() {
                    cursor += 1;
                    continue;
                }
                if leading_spaces(body_line) <= indent {
                    break;
                }
                collect_workflow_calls(body_line, cursor + 1, &mut invocations);
                cursor += 1;
            }
            index = cursor;
        }
        invocations
    }

    /// The justfile's own recipe dependency graph: every recipe name it
    /// defines, and for each one, the recipes it reaches directly -- both
    /// declared prerequisites (`ci: fmt-check typos ...`) and recipes it
    /// shells out to by name from its own body. `budgets` runs `just
    /// budgets-here` from its body rather than declaring it as a
    /// prerequisite, so a reading of only recipe headers would miss that
    /// edge and wrongly report `budgets-here`'s dependencies as unreachable.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub(crate) struct JustfileGraph {
        pub(crate) recipes: BTreeSet<String>,
        pub(crate) edges: BTreeMap<String, BTreeSet<String>>,
    }

    /// Splits a recipe header into its name-and-parameters part and its
    /// dependency-list part, or `None` if `trimmed` is not a recipe header
    /// at all. `nightly := \`sed ...\`` is a variable assignment, not a
    /// recipe -- its `:=` is what tells the two apart, since no recipe
    /// header can contain that sequence (parameter defaults use `="..."`,
    /// never a colon).
    fn recipe_header(trimmed: &str) -> Option<(&str, &str)> {
        if trimmed.contains(":=") {
            return None;
        }
        let colon = trimmed.find(':')?;
        Some((&trimmed[..colon], &trimmed[colon + 1..]))
    }

    /// Parses a justfile into its recipe dependency graph. This is not a
    /// general justfile parser -- it understands exactly the constructs
    /// this repository's justfile uses (bare recipe headers, parameters
    /// with quoted defaults, `#` doc-comments and variable assignments at
    /// column zero, indented recipe bodies) and nothing more exotic
    /// (recipe attributes, `import`, dependency arguments).
    pub(crate) fn parse_justfile(justfile_text: &str) -> JustfileGraph {
        let mut graph = JustfileGraph::default();
        let mut current_recipe: Option<String> = None;
        for line in justfile_text.split('\n') {
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some(recipe) = &current_recipe {
                    graph
                        .edges
                        .entry(recipe.clone())
                        .or_default()
                        .extend(just_recipe_calls(line));
                }
                continue;
            }
            current_recipe = None;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
                continue;
            }
            let Some((header, deps_part)) = recipe_header(trimmed) else {
                continue;
            };
            let Some(name) = header.split_whitespace().next() else {
                continue;
            };
            let deps: BTreeSet<String> = deps_part
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            graph.recipes.insert(name.to_owned());
            graph.edges.entry(name.to_owned()).or_default().extend(deps);
            current_recipe = Some(name.to_owned());
        }
        graph
    }

    /// Every recipe `root` reaches, directly or transitively, through
    /// `edges` -- a `root` the graph has no entry for simply reaches
    /// nothing.
    pub(crate) fn transitive_closure(
        edges: &BTreeMap<String, BTreeSet<String>>,
        root: &str,
    ) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![root.to_owned()];
        while let Some(name) = stack.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if let Some(deps) = edges.get(&name) {
                stack.extend(deps.iter().cloned());
            }
        }
        seen
    }

    /// For every recipe CI actually invokes (deduplicated, first occurrence
    /// wins), either it is missing from the justfile entirely or it exists
    /// but `ci`/`ci-full` cannot reach it -- reported as two distinct
    /// failure shapes, as the property requires.
    pub(crate) fn find_violations(
        invocations: &[WorkflowInvocation],
        graph: &JustfileGraph,
    ) -> Vec<String> {
        let reachable: BTreeSet<String> = transitive_closure(&graph.edges, "ci")
            .into_iter()
            .chain(transitive_closure(&graph.edges, "ci-full"))
            .collect();

        let mut seen_recipes = BTreeSet::new();
        let mut violations = Vec::new();
        for invocation in invocations {
            if !seen_recipes.insert(invocation.recipe.clone()) {
                continue;
            }
            if !graph.recipes.contains(&invocation.recipe) {
                violations.push(format!(
                    "`{}` -- invoked at rust.yml:{} (`{}`) but no such recipe exists in the justfile",
                    invocation.recipe, invocation.line_number, invocation.line_text
                ));
            } else if !reachable.contains(&invocation.recipe) {
                violations.push(format!(
                    "`{}` -- invoked at rust.yml:{} (`{}`) but is not reachable from `ci` or `ci-full`",
                    invocation.recipe, invocation.line_number, invocation.line_text
                ));
            }
        }
        violations
    }

    pub(crate) fn run_at(root: &Path) -> Result<(), String> {
        let workflow_path = root.join(".github/workflows/rust.yml");
        let workflow_text = fs::read_to_string(&workflow_path).map_err(|error| {
            format!(
                "ci-gate-reachability: failed to read {}: {error}",
                workflow_path.display()
            )
        })?;
        let justfile_path = root.join("justfile");
        let justfile_text = fs::read_to_string(&justfile_path).map_err(|error| {
            format!(
                "ci-gate-reachability: failed to read {}: {error}",
                justfile_path.display()
            )
        })?;

        let invocations = parse_workflow_just_invocations(&workflow_text);
        if invocations.is_empty() {
            return Err(format!(
                "ci-gate-reachability: parsed zero `just` invocations out of {} -- the parser \
                 broke, or the workflow stopped calling `just` entirely, and either way this \
                 gate would silently be checking nothing rather than failing loudly",
                workflow_path.display()
            ));
        }

        let graph = parse_justfile(&justfile_text);
        if graph.recipes.is_empty() {
            return Err(format!(
                "ci-gate-reachability: parsed zero recipes out of {} -- the parser broke",
                justfile_path.display()
            ));
        }

        let violations = find_violations(&invocations, &graph);
        if !violations.is_empty() {
            return Err(super::violations_message(
                format!(
                    "ci-gate-reachability: {} recipe(s) CI runs are not reachable from `ci`/`ci-full`:",
                    violations.len()
                ),
                &violations,
            ));
        }

        println!(
            "ci-gate-reachability: {} `just` invocation(s) in rust.yml, all reachable from `ci`/`ci-full`",
            invocations.len()
        );
        Ok(())
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
mod robot_suite_partition_tests {
    use crate::robot_suite_partition::{find_violations, recipe_flag_values};

    fn justfile(gpu_skips: &[&str], capture_examples: &[&str]) -> String {
        let mut text =
            String::from("# a comment\nrobot-gpu:\n    xvfb-run ./run_robot_test.sh \\\n");
        for name in gpu_skips {
            text.push_str(&format!("      --skip {name} \\\n"));
        }
        text.push_str("\nrobot-captures:\n    xvfb-run ./run_robot_test.sh \\\n");
        for name in capture_examples {
            text.push_str(&format!("      --example {name} \\\n"));
        }
        text.push_str("\nunrelated:\n    --skip not_in_either\n");
        text
    }

    #[test]
    fn matching_halves_are_clean() {
        let text = justfile(&["alpha", "beta"], &["beta", "alpha"]);
        assert!(
            find_violations(&text).is_empty(),
            "{:?}",
            find_violations(&text)
        );
    }

    #[test]
    fn an_example_skipped_but_never_captured_runs_nowhere() {
        let text = justfile(&["alpha", "beta"], &["alpha"]);
        let violations = find_violations(&text);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("beta") && violations[0].contains("nowhere"),
            "{violations:?}"
        );
    }

    #[test]
    fn an_example_captured_but_not_skipped_runs_twice() {
        let text = justfile(&["alpha"], &["alpha", "beta"]);
        let violations = find_violations(&text);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("beta") && violations[0].contains("twice"),
            "{violations:?}"
        );
    }

    #[test]
    fn an_empty_half_fails_rather_than_passing_vacuously() {
        // Two empty sets are equal. Without the emptiness check this gate
        // would pass on a justfile it could not parse.
        let text = justfile(&[], &[]);
        let violations = find_violations(&text);
        assert_eq!(
            violations.len(),
            2,
            "both halves must be reported: {violations:?}"
        );
    }

    #[test]
    fn a_recipe_body_ends_at_the_next_column_zero_line() {
        // `unrelated` also contains `--skip`; it must not leak into robot-gpu.
        let text = justfile(&["alpha"], &["alpha"]);
        let skipped = recipe_flag_values(&text, "robot-gpu", "--skip");
        assert!(
            !skipped.contains("not_in_either"),
            "leaked from a later recipe: {skipped:?}"
        );
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn the_real_justfile_halves_agree_and_are_not_empty() {
        let root = crate::workspace_root().expect("workspace root");
        let text = std::fs::read_to_string(root.join("justfile")).expect("justfile is readable");
        let skipped = recipe_flag_values(&text, "robot-gpu", "--skip");
        assert!(!skipped.is_empty(), "the gate must find the real skip list");
        assert!(
            find_violations(&text).is_empty(),
            "{:?}",
            find_violations(&text)
        );
    }
}

#[cfg(test)]
mod ci_gate_reachability_tests {
    use crate::ci_gate_reachability::{
        find_violations, parse_justfile, parse_workflow_just_invocations, transitive_closure,
    };

    const JUSTFILE: &str = "\
# a doc comment mentioning `just orphaned` must not create an edge
alpha:
    cargo alpha

beta:
    cargo beta

deep:
    cargo deep

middle: deep
    cargo middle

platform-only:
    cargo platform

takes-args target=\"x\":
    cargo run {{target}}

body-caller:
    just beta

ci: alpha middle body-caller takes-args
ci-full: ci platform-only
";

    fn workflow(body: &str) -> String {
        format!("jobs:\n  check:\n    steps:\n{body}")
    }

    #[test]
    fn a_recipe_reachable_only_through_ci_is_not_a_violation() {
        let calls = parse_workflow_just_invocations(&workflow("      - run: just alpha\n"));
        assert_eq!(calls.len(), 1, "the workflow declares exactly one call");
        assert!(find_violations(&calls, &parse_justfile(JUSTFILE)).is_empty());
    }

    #[test]
    fn a_recipe_reachable_only_through_ci_full_is_not_a_violation() {
        let calls = parse_workflow_just_invocations(&workflow("      - run: just platform-only\n"));
        assert!(find_violations(&calls, &parse_justfile(JUSTFILE)).is_empty());
    }

    #[test]
    fn a_recipe_reached_through_a_dependency_chain_is_not_a_violation() {
        // ci -> middle -> deep. Only the transitive step makes `deep` reachable.
        let graph = parse_justfile(JUSTFILE);
        assert!(transitive_closure(&graph.edges, "ci").contains("deep"));
        let calls = parse_workflow_just_invocations(&workflow("      - run: just deep\n"));
        assert!(find_violations(&calls, &graph).is_empty());
    }

    #[test]
    fn a_recipe_a_body_shells_out_to_is_reachable() {
        // `body-caller` runs `just beta` from its body rather than declaring
        // it. Reading only recipe headers would report `beta` unreachable.
        let calls = parse_workflow_just_invocations(&workflow("      - run: just beta\n"));
        assert!(find_violations(&calls, &parse_justfile(JUSTFILE)).is_empty());
    }

    #[test]
    fn a_recipe_in_neither_aggregate_is_a_violation() {
        let calls = parse_workflow_just_invocations(&workflow("      - run: just orphaned\n"));
        let violations = find_violations(&calls, &parse_justfile(JUSTFILE));
        assert_eq!(violations.len(), 1, "got {violations:?}");
        assert!(violations[0].contains("orphaned"), "got {violations:?}");
    }

    #[test]
    fn a_recipe_absent_from_the_justfile_reports_distinctly() {
        let defined = parse_workflow_just_invocations(&workflow("      - run: just beta\n"));
        let mut undefined = parse_workflow_just_invocations(&workflow("      - run: just nope\n"));
        undefined.extend(defined);
        let violations = find_violations(&undefined, &parse_justfile(JUSTFILE));
        assert_eq!(
            violations.len(),
            1,
            "only the undefined one fails: {violations:?}"
        );
        assert!(
            violations[0].contains("no such recipe exists"),
            "an undefined recipe must not read as merely unreachable: {violations:?}"
        );
    }

    #[test]
    fn a_recipe_invoked_with_arguments_is_matched_by_name() {
        let calls =
            parse_workflow_just_invocations(&workflow("      - run: just takes-args value\n"));
        assert_eq!(calls[0].recipe, "takes-args");
        assert!(find_violations(&calls, &parse_justfile(JUSTFILE)).is_empty());
    }

    #[test]
    fn calls_inside_a_block_scalar_are_found_and_prose_is_not() {
        let text = workflow(
            "      # just orphaned in a comment is prose, not a call\n\
             \x20     - run: |\n\
             \x20         just alpha\n\
             \x20\n\
             \x20         just beta\n\
             \x20     - name: mentions `just orphaned` in prose\n",
        );
        let found: Vec<String> = parse_workflow_just_invocations(&text)
            .into_iter()
            .map(|call| call.recipe)
            .collect();
        assert_eq!(
            found,
            vec!["alpha".to_owned(), "beta".to_owned()],
            "got {found:?}"
        );
    }

    #[test]
    fn the_real_workflow_parses_to_a_non_zero_number_of_calls() {
        // Without this the whole gate is vacuous: a parser that silently
        // finds nothing reports every repository clean.
        let root = crate::workspace_root().expect("workspace root");
        let text = std::fs::read_to_string(root.join(".github/workflows/rust.yml"))
            .expect("rust.yml is readable");
        let calls = parse_workflow_just_invocations(&text);
        assert!(
            calls.len() >= 10,
            "rust.yml must parse to a meaningful number of `just` calls, got {}",
            calls.len()
        );
    }
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
        let workspace = unique_temp_dir();
        let package_dir = workspace.join("apps/isolated-demo");
        fs::create_dir_all(package_dir.join("src")).expect("create package");
        fs::write(package_dir.join("Cargo.toml"), MINIMAL_MANIFEST).expect("write manifest");
        fs::write(package_dir.join("Cargo.lock"), PUBLISHED_LOCKFILE).expect("write lockfile");
        fs::write(package_dir.join("src/main.rs"), b"fn main() {}").expect("write source");
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

    const FIXTURE_VERSION: &str = "0.1.105";

    fn write_root_manifest(root: &Path, workspace_version: &str, dependency_version: &str) {
        let manifest = format!(
            "[workspace]\n\
             members = [\"crates/cranpose\"]\n\
             \n\
             [workspace.package]\n\
             version = \"{workspace_version}\"\n\
             \n\
             [workspace.dependencies]\n\
             cranpose = {{ path = \"crates/cranpose\", version = \"{dependency_version}\" }}\n"
        );
        fs::write(root.join("Cargo.toml"), manifest).expect("write root manifest");
    }

    /// A root `Cargo.lock` entry for a workspace member: no `source` or
    /// `checksum`, exactly like a path dependency the workspace resolves
    /// itself (see the real `Cargo.lock`, which carries neither for its own
    /// `cranpose` packages).
    fn write_root_lock(root: &Path, version: &str) {
        let lock =
            format!("version = 4\n\n[[package]]\nname = \"cranpose\"\nversion = \"{version}\"\n");
        fs::write(root.join("Cargo.lock"), lock).expect("write root lock");
    }

    fn write_root_lock_without_cranpose(root: &Path) {
        let lock = "version = 4\n\n[[package]]\nname = \"log\"\nversion = \"0.4.0\"\n";
        fs::write(root.join("Cargo.lock"), lock).expect("write root lock");
    }

    fn write_isolated_manifest(root: &Path, dependency_version: &str) {
        let dir = root.join("apps/isolated-demo");
        fs::create_dir_all(&dir).expect("create apps/isolated-demo");
        let manifest = format!(
            "[package]\n\
             name = \"isolated-demo\"\n\
             version = \"0.1.0\"\n\
             \n\
             [dependencies]\n\
             cranpose = {{ version = \"{dependency_version}\" }}\n"
        );
        fs::write(dir.join("Cargo.toml"), manifest).expect("write isolated-demo manifest");
    }

    fn write_isolated_lock(
        root: &Path,
        version: &str,
        source: Option<&str>,
        checksum: Option<&str>,
    ) {
        let dir = root.join("apps/isolated-demo");
        fs::create_dir_all(&dir).expect("create apps/isolated-demo");
        let mut lock =
            format!("version = 4\n\n[[package]]\nname = \"cranpose\"\nversion = \"{version}\"\n");
        if let Some(source) = source {
            lock.push_str(&format!("source = \"{source}\"\n"));
        }
        if let Some(checksum) = checksum {
            lock.push_str(&format!("checksum = \"{checksum}\"\n"));
        }
        fs::write(dir.join("Cargo.lock"), lock).expect("write isolated-demo lock");
    }

    fn write_isolated_lock_without_cranpose(root: &Path) {
        let dir = root.join("apps/isolated-demo");
        fs::create_dir_all(&dir).expect("create apps/isolated-demo");
        let lock = "version = 4\n\n[[package]]\nname = \"log\"\nversion = \"0.4.0\"\n";
        fs::write(dir.join("Cargo.lock"), lock).expect("write isolated-demo lock");
    }

    /// Every file `check_versions_at` reads, all agreeing at `FIXTURE_VERSION`.
    fn write_aligned_versions_fixture(root: &Path) {
        write_root_manifest(root, FIXTURE_VERSION, FIXTURE_VERSION);
        write_root_lock(root, FIXTURE_VERSION);
        write_isolated_manifest(root, FIXTURE_VERSION);
        write_isolated_lock(
            root,
            FIXTURE_VERSION,
            Some(CRATES_IO_SOURCE),
            Some("deadbeef"),
        );
    }

    #[test]
    fn versions_pass_when_everything_aligned() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);

        check_versions_at(&root).expect("aligned versions must pass");
    }

    /// Fixtures prove the logic; they cannot prove the *real* Cargo.toml
    /// still parses the way the fixtures assume it does. A fixture-only
    /// suite can stay green while the actual manifest drifts into a shape
    /// this parser does not handle (a renamed section, a moved key) and the
    /// gate would silently stop meaning anything on the one file it exists
    /// to check.
    fn real_workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .canonicalize()
            .expect("resolve the real workspace root from xtask's own manifest dir")
    }

    #[test]
    fn check_versions_passes_against_the_real_workspace() {
        check_versions_at(&real_workspace_root())
            .expect("the real workspace's own versions must already be aligned");
    }

    /// The root manifest's `[workspace]` table.
    fn workspace_table(root: &Path) -> toml::value::Table {
        load_toml(&root.join("Cargo.toml"))
            .expect("parse the workspace manifest")
            .get("workspace")
            .and_then(toml::Value::as_table)
            .cloned()
            .expect("the root manifest has a [workspace] table")
    }

    /// Every path listed in `workspace.members`.
    fn workspace_members(workspace: &toml::value::Table) -> Vec<String> {
        workspace
            .get("members")
            .and_then(toml::Value::as_array)
            .expect("the workspace manifest lists its members")
            .iter()
            .map(|member| {
                member
                    .as_str()
                    .expect("every workspace member is a path string")
                    .to_owned()
            })
            .collect()
    }

    /// The level `[workspace.lints.rust]` sets for `name`, if any.
    fn workspace_rust_lint_level(workspace: &toml::value::Table, name: &str) -> Option<String> {
        workspace
            .get("lints")
            .and_then(toml::Value::as_table)
            .and_then(|lints| lints.get("rust"))
            .and_then(toml::Value::as_table)
            .and_then(|rust| rust.get(name))
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    }

    /// The rules AGENTS.md hands to the compiler live in the root manifest's
    /// `[workspace.lints]` tables, and cargo applies them to a member only
    /// when that member's own manifest says `[lints] workspace = true`.
    /// Nothing requires that opt-in, so a crate added without it builds with
    /// `unsafe_code`, `todo`, `dbg_macro` and `linker_messages` all back at
    /// their default levels while every gate stays green.
    #[test]
    fn every_workspace_member_inherits_the_workspace_lints() {
        let root = real_workspace_root();
        let missing: Vec<String> = workspace_members(&workspace_table(&root))
            .into_iter()
            .filter(|member| {
                let manifest = load_toml(&root.join(member).join("Cargo.toml"))
                    .expect("parse a workspace member manifest");
                manifest
                    .get("lints")
                    .and_then(toml::Value::as_table)
                    .and_then(|lints| lints.get("workspace"))
                    .and_then(toml::Value::as_bool)
                    != Some(true)
            })
            .collect();

        assert!(
            missing.is_empty(),
            "these workspace members do not carry `workspace = true` under `[lints]`, so the \
             workspace lint table does not reach them: {missing:?}"
        );
    }

    /// `linker_messages` is the one warning class a `-D warnings` run cannot
    /// reach, because clippy stops at metadata and never links. Denying it
    /// in the workspace lint table is what fails a build on a linker
    /// warning; at its default level a message such as macOS ld's
    /// `__eh_frame section too large` prints under `cargo build`/`cargo
    /// test` and turns nothing red.
    #[test]
    fn the_workspace_lints_deny_linker_messages() {
        assert_eq!(
            workspace_rust_lint_level(&workspace_table(&real_workspace_root()), "linker_messages")
                .as_deref(),
            Some("deny"),
            "a linker warning must fail the build; shrink what is being linked instead of \
             relaxing this level -- TIME_WASTERS.md records how to attribute an oversized \
             `__eh_frame` back to the objects that filled it"
        );
    }

    #[test]
    fn verify_tag_passes_against_the_real_workspace_version() {
        let root = real_workspace_root();
        let workspace_version =
            workspace_package_version(&root).expect("read the real workspace version");

        verify_tag_at(&root, &format!("v{workspace_version}"))
            .expect("verify-tag must accept the real workspace's own version as a matching tag");
    }

    #[test]
    fn versions_fail_when_workspace_dependency_diverges() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_root_manifest(&root, FIXTURE_VERSION, "0.1.104");

        let error = check_versions_at(&root).expect_err("a stale workspace dependency must fail");

        assert!(
            error.contains("workspace dependency cranpose is 0.1.104, expected 0.1.105"),
            "{error}"
        );
    }

    #[test]
    fn versions_fail_when_lockfile_version_diverges() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_root_lock(&root, "0.1.104");

        let error = check_versions_at(&root).expect_err("a stale Cargo.lock entry must fail");

        assert!(
            error.contains("Cargo.lock package cranpose has 0.1.104, expected 0.1.105"),
            "{error}"
        );
    }

    #[test]
    fn versions_fail_when_lockfile_is_missing_a_workspace_package() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_root_lock_without_cranpose(&root);

        let error = check_versions_at(&root).expect_err("a missing lock entry must fail");

        assert!(
            error.contains("Cargo.lock is missing workspace package cranpose"),
            "{error}"
        );
    }

    #[test]
    fn versions_fail_when_isolated_demo_manifest_diverges() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_isolated_manifest(&root, "0.1.104");

        let error = check_versions_at(&root).expect_err("a stale isolated-demo manifest must fail");

        assert!(
            error.contains("apps/isolated-demo dependency cranpose is 0.1.104, expected 0.1.105"),
            "{error}"
        );
    }

    #[test]
    fn versions_fail_when_isolated_lock_has_no_cranpose_packages() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_isolated_lock_without_cranpose(&root);

        let error = check_versions_at(&root)
            .expect_err("a canary lockfile with no cranpose crates must fail");

        assert!(
            error.contains("apps/isolated-demo/Cargo.lock locks no cranpose packages"),
            "{error}"
        );
    }

    #[test]
    fn versions_fail_when_isolated_lock_resolves_from_a_local_path() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_isolated_lock(&root, FIXTURE_VERSION, None, None);

        let error = check_versions_at(&root)
            .expect_err("a patched/path-resolved canary lockfile must fail");

        assert!(
            error.contains(
                "apps/isolated-demo/Cargo.lock resolves cranpose from a local path, expected \
                 the published crate at registry+https://github.com/rust-lang/crates.io-index"
            ),
            "{error}"
        );
    }

    #[test]
    fn versions_fail_when_isolated_lock_resolves_from_a_patch() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_isolated_lock(
            &root,
            FIXTURE_VERSION,
            Some("git+https://example.com/cranpose"),
            None,
        );

        let error = check_versions_at(&root).expect_err("a git-resolved canary lockfile must fail");

        assert!(
            error.contains("resolves cranpose from git+https://example.com/cranpose, expected"),
            "{error}"
        );
    }

    #[test]
    fn versions_fail_when_isolated_lock_has_no_checksum() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_isolated_lock(&root, FIXTURE_VERSION, Some(CRATES_IO_SOURCE), None);

        let error =
            check_versions_at(&root).expect_err("a canary lockfile missing a checksum must fail");

        assert!(
            error.contains("apps/isolated-demo/Cargo.lock package cranpose has no checksum"),
            "{error}"
        );
    }

    #[test]
    fn versions_fail_when_isolated_lock_version_diverges() {
        let root = unique_temp_dir();
        write_aligned_versions_fixture(&root);
        write_isolated_lock(&root, "0.1.104", Some(CRATES_IO_SOURCE), Some("deadbeef"));

        let error =
            check_versions_at(&root).expect_err("a stale canary lockfile version must fail");

        assert!(
            error.contains(
                "apps/isolated-demo/Cargo.lock package cranpose is 0.1.104, expected 0.1.105"
            ),
            "{error}"
        );
    }

    #[test]
    fn dependency_version_reads_bare_strings_and_inline_tables() {
        let bare: toml::Value = toml::from_str("v = \"1.2.3\"").expect("parse bare string");
        let table: toml::Value =
            toml::from_str("v = { path = \"crates/cranpose\", version = \"1.2.3\" }")
                .expect("parse inline table");
        let no_version: toml::Value = toml::from_str("v = { path = \"crates/cranpose\" }")
            .expect("parse table without version");

        assert_eq!(
            dependency_version(bare.get("v").expect("v present")),
            Some("1.2.3".to_owned())
        );
        assert_eq!(
            dependency_version(table.get("v").expect("v present")),
            Some("1.2.3".to_owned())
        );
        assert_eq!(
            dependency_version(no_version.get("v").expect("v present")),
            None
        );
    }

    #[test]
    fn lock_versions_groups_by_package_name() {
        let lock: toml::Value = toml::from_str(
            "[[package]]\n\
             name = \"cranpose\"\n\
             version = \"0.1.0\"\n\
             \n\
             [[package]]\n\
             name = \"cranpose\"\n\
             version = \"0.1.1\"\n\
             \n\
             [[package]]\n\
             name = \"log\"\n\
             version = \"0.4.0\"\n",
        )
        .expect("parse lockfile");

        let versions = lock_versions(&cranpose_lock_packages(&lock));

        assert_eq!(versions.len(), 1, "only cranpose-prefixed packages count");
        assert_eq!(
            versions.get("cranpose"),
            Some(&BTreeSet::from(["0.1.0".to_owned(), "0.1.1".to_owned()]))
        );
    }

    fn sync_isolated_demo_to(root: &Path, version: &str) {
        sync_isolated_demo_at(
            root,
            SyncIsolatedDemoOptions {
                version: Some(version.to_owned()),
            },
        )
        .expect("sync must succeed");
    }

    #[test]
    fn sync_isolated_demo_rewrites_inline_and_bare_dependencies() {
        let root = unique_temp_dir();
        write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
        let manifest_dir = root.join("apps/isolated-demo");
        fs::create_dir_all(&manifest_dir).expect("create apps/isolated-demo");
        fs::write(
            manifest_dir.join("Cargo.toml"),
            "[dependencies]\n\
             cranpose = { version = \"0.1.104\" }\n\
             cranpose-core = \"0.1.104\"\n\
             log = \"0.4\"\n\
             \n\
             [target.'cfg(target_arch = \"wasm32\")'.dependencies]\n\
             cranpose-platform-web = \"0.1.104\"\n",
        )
        .expect("write isolated-demo manifest");

        sync_isolated_demo_to(&root, "0.1.105");

        let updated = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read manifest");
        assert!(
            updated.contains("cranpose = { version = \"0.1.105\" }"),
            "{updated}"
        );
        assert!(updated.contains("cranpose-core = \"0.1.105\""), "{updated}");
        assert!(
            updated.contains("cranpose-platform-web = \"0.1.105\""),
            "target-cfg dependencies must be rewritten too: {updated}"
        );
        assert!(
            updated.contains("log = \"0.4\""),
            "non-cranpose dependencies must be left alone: {updated}"
        );
    }

    #[test]
    fn sync_isolated_demo_is_a_noop_when_already_synced() {
        let root = unique_temp_dir();
        write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
        let manifest_dir = root.join("apps/isolated-demo");
        fs::create_dir_all(&manifest_dir).expect("create apps/isolated-demo");
        let manifest_text = "[dependencies]\ncranpose = { version = \"0.1.105\" }\n";
        fs::write(manifest_dir.join("Cargo.toml"), manifest_text).expect("write manifest");

        sync_isolated_demo_to(&root, "0.1.105");

        let updated = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read manifest");
        assert_eq!(
            updated, manifest_text,
            "an already-synced manifest must be left byte-for-byte alone"
        );
    }

    #[test]
    fn sync_isolated_demo_defaults_to_the_workspace_version() {
        let root = unique_temp_dir();
        write_root_manifest(&root, "0.1.107", "0.1.107");
        let manifest_dir = root.join("apps/isolated-demo");
        fs::create_dir_all(&manifest_dir).expect("create apps/isolated-demo");
        fs::write(
            manifest_dir.join("Cargo.toml"),
            "[dependencies]\ncranpose = { version = \"0.1.104\" }\n",
        )
        .expect("write manifest");

        sync_isolated_demo_at(&root, SyncIsolatedDemoOptions { version: None })
            .expect("sync must succeed");

        let updated = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read manifest");
        assert!(updated.contains("0.1.107"), "{updated}");
    }

    #[test]
    fn sync_isolated_demo_strips_a_leading_v() {
        let root = unique_temp_dir();
        write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
        let manifest_dir = root.join("apps/isolated-demo");
        fs::create_dir_all(&manifest_dir).expect("create apps/isolated-demo");
        fs::write(
            manifest_dir.join("Cargo.toml"),
            "[dependencies]\ncranpose = { version = \"0.1.104\" }\n",
        )
        .expect("write manifest");

        sync_isolated_demo_at(
            &root,
            SyncIsolatedDemoOptions {
                version: Some("v0.2.0".to_owned()),
            },
        )
        .expect("sync must succeed");

        let updated = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read manifest");
        assert!(updated.contains("0.2.0"), "{updated}");
        assert!(
            !updated.contains("v0.2.0"),
            "the leading v must be stripped: {updated}"
        );
    }

    #[test]
    fn sync_isolated_demo_rejects_a_non_semver_version() {
        let root = unique_temp_dir();
        write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
        write_isolated_manifest(&root, FIXTURE_VERSION);

        let error = sync_isolated_demo_at(
            &root,
            SyncIsolatedDemoOptions {
                version: Some("not-a-version".to_owned()),
            },
        )
        .expect_err("a non-semver version must be rejected");

        assert!(error.contains("Not a semver version"), "{error}");
    }

    #[test]
    fn semver_regex_accepts_prerelease_and_build_metadata() {
        assert!(SEMVER_RE.is_match("1.2.3"));
        assert!(SEMVER_RE.is_match("1.2.3-alpha.1"));
        assert!(SEMVER_RE.is_match("1.2.3+build.5"));
        assert!(!SEMVER_RE.is_match("1.2"));
        assert!(!SEMVER_RE.is_match("v1.2.3"));
        assert!(!SEMVER_RE.is_match("not-a-version"));
    }

    #[test]
    fn parse_sync_isolated_demo_options() {
        let none = SyncIsolatedDemoOptions::parse(&[]).expect("no args parse");
        assert_eq!(none.version, None);

        let with_version =
            SyncIsolatedDemoOptions::parse(&["0.1.73".to_owned()]).expect("one arg parses");
        assert_eq!(with_version.version.as_deref(), Some("0.1.73"));

        SyncIsolatedDemoOptions::parse(&["0.1.73".to_owned(), "0.1.74".to_owned()])
            .expect_err("two positional arguments must be rejected");
        SyncIsolatedDemoOptions::parse(&["--bogus".to_owned()])
            .expect_err("unknown flags must be rejected");
    }

    fn write_root_lock_with_multiple_packages(root: &Path, cranpose_version: &str) {
        let lock = format!(
            "version = 4\n\
             \n\
             [[package]]\n\
             name = \"cranpose\"\n\
             version = \"{cranpose_version}\"\n\
             dependencies = [\n\
             \x20\"log\",\n\
             ]\n\
             \n\
             [[package]]\n\
             name = \"cranpose-core\"\n\
             version = \"{cranpose_version}\"\n\
             \n\
             [[package]]\n\
             name = \"log\"\n\
             version = \"0.4.0\"\n"
        );
        fs::write(root.join("Cargo.lock"), lock).expect("write root lock");
    }

    #[test]
    fn bump_release_version_rewrites_toml_and_lock() {
        let root = unique_temp_dir();
        write_root_manifest(&root, "0.1.104", "0.1.104");
        write_root_lock_with_multiple_packages(&root, "0.1.104");

        bump_release_version_at(&root, "v0.1.105").expect("bump must succeed");

        let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
        assert!(
            manifest.contains("version = \"0.1.105\""),
            "workspace.package.version must be bumped: {manifest}"
        );
        assert!(
            manifest.contains("cranpose = { path = \"crates/cranpose\", version = \"0.1.105\" }"),
            "workspace dependency must be bumped: {manifest}"
        );

        let lock = fs::read_to_string(root.join("Cargo.lock")).expect("read lock");
        assert!(
            lock.contains("name = \"cranpose\"\nversion = \"0.1.105\""),
            "{lock}"
        );
        assert!(
            lock.contains("name = \"cranpose-core\"\nversion = \"0.1.105\""),
            "{lock}"
        );
        assert!(
            lock.contains("name = \"log\"\nversion = \"0.4.0\""),
            "a non-cranpose package must be left alone: {lock}"
        );
    }

    #[test]
    fn bump_release_version_rejects_a_tag_without_a_leading_v() {
        let root = unique_temp_dir();
        write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
        write_root_lock(&root, FIXTURE_VERSION);

        let error = bump_release_version_at(&root, "0.1.105")
            .expect_err("a tag without a leading v must be rejected");

        assert_eq!(error, "Expected tag starting with 'v', got '0.1.105'");
    }

    #[test]
    fn bump_release_version_leaves_cargo_toml_untouched_on_a_dependency_mismatch() {
        let root = unique_temp_dir();
        let manifest = "[workspace]\n\
             members = [\"crates/cranpose\"]\n\
             \n\
             [workspace.package]\n\
             version = \"0.1.104\"\n\
             \n\
             [workspace.dependencies]\n\
             cranpose = { path = \"crates/cranpose\" }\n";
        fs::write(root.join("Cargo.toml"), manifest).expect("write manifest");
        write_root_lock(&root, "0.1.104");

        let error = bump_release_version_at(&root, "v0.1.105")
            .expect_err("a dependency with no version key must be reported, not silently kept");

        assert!(
            error.contains("Some cranpose workspace dependencies were not updated"),
            "{error}"
        );
        let unchanged = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
        assert_eq!(
            unchanged, manifest,
            "Cargo.toml must not be written when the dependency pass fails"
        );
    }

    #[test]
    fn bump_release_version_creates_workspace_package_when_missing() {
        let root = unique_temp_dir();
        let manifest = "[workspace]\n\
             members = [\"crates/cranpose\"]\n\
             \n\
             [workspace.dependencies]\n\
             cranpose = { path = \"crates/cranpose\", version = \"0.1.104\" }\n";
        fs::write(root.join("Cargo.toml"), manifest).expect("write manifest");
        write_root_lock(&root, "0.1.104");

        bump_release_version_at(&root, "v0.1.105").expect("bump must succeed");

        let updated = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
        assert!(updated.contains("[workspace.package]"), "{updated}");
        assert!(updated.contains("version = \"0.1.105\""), "{updated}");
        assert!(
            updated.contains("cranpose = { path = \"crates/cranpose\", version = \"0.1.105\" }"),
            "{updated}"
        );
    }

    #[test]
    fn verify_tag_passes_without_touching_the_lockfile_or_isolated_demo() {
        let root = unique_temp_dir();
        write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
        // Deliberately no Cargo.lock and no apps/isolated-demo: this runs
        // between `sync_versions` and `bump_isolated_demo`, where the demo
        // still points at the previous release on purpose. If verify-tag
        // read either, this test would fail with a missing-file error.

        verify_tag_at(&root, "v0.1.105").expect("verify-tag must not need Cargo.lock or the demo");
    }

    #[test]
    fn verify_tag_rejects_a_tag_without_a_leading_v() {
        let root = unique_temp_dir();
        write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);

        let error =
            verify_tag_at(&root, "0.1.105").expect_err("a tag without a leading v must fail");

        assert_eq!(error, "Expected tag starting with 'v', got '0.1.105'");
    }

    #[test]
    fn verify_tag_fails_when_tag_does_not_match_workspace_version() {
        let root = unique_temp_dir();
        write_root_manifest(&root, "0.1.105", "0.1.105");

        let error = verify_tag_at(&root, "v0.1.106")
            .expect_err("a tag ahead of the workspace version must fail");

        assert_eq!(
            error,
            "Tag version v0.1.106 does not match workspace version 0.1.105"
        );
    }

    #[test]
    fn verify_tag_fails_when_a_workspace_dependency_diverges() {
        let root = unique_temp_dir();
        write_root_manifest(&root, "0.1.105", "0.1.104");

        let error =
            verify_tag_at(&root, "v0.1.105").expect_err("a stale workspace dependency must fail");

        assert_eq!(
            error,
            "Workspace dependency versions must match workspace version:\ncranpose => 0.1.104"
        );
    }

    const PUBLISH_ORDER_METADATA_TEMPLATE: &str = r#"{
        "workspace_members": ["cranpose-core 0.1.0", "cranpose 0.1.0", "cranpose-ui 0.1.0"],
        "packages": [
            {
                "id": "cranpose-core 0.1.0",
                "name": "cranpose-core",
                "dependencies": []
            },
            {
                "id": "cranpose 0.1.0",
                "name": "cranpose",
                "dependencies": [
                    {"name": "cranpose-core", "kind": null},
                    {"name": "cranpose-ui", "kind": null}
                ]
            },
            {
                "id": "cranpose-ui 0.1.0",
                "name": "cranpose-ui",
                "dependencies": [
                    {"name": "cranpose-core", "kind": null}
                ]
            },
            {
                "id": "log 0.4.0",
                "name": "log",
                "dependencies": []
            }
        ]
    }"#;

    #[test]
    fn resolve_publish_order_orders_dependencies_before_dependents() {
        let order = resolve_publish_order(PUBLISH_ORDER_METADATA_TEMPLATE)
            .expect("a valid dependency graph must resolve");

        assert_eq!(order, vec!["cranpose-core", "cranpose-ui", "cranpose"]);
    }

    #[test]
    fn resolve_publish_order_ignores_dev_dependencies() {
        // liquid -> testing (dev) -> cranpose -> liquid would be a cycle if
        // dev-deps gated the order; they must not.
        let metadata = r#"{
            "workspace_members": ["cranpose-liquid 0.1.0", "cranpose-testing 0.1.0", "cranpose 0.1.0"],
            "packages": [
                {
                    "id": "cranpose-liquid 0.1.0",
                    "name": "cranpose-liquid",
                    "dependencies": [
                        {"name": "cranpose-testing", "kind": "dev"}
                    ]
                },
                {
                    "id": "cranpose-testing 0.1.0",
                    "name": "cranpose-testing",
                    "dependencies": [
                        {"name": "cranpose", "kind": null}
                    ]
                },
                {
                    "id": "cranpose 0.1.0",
                    "name": "cranpose",
                    "dependencies": [
                        {"name": "cranpose-liquid", "kind": "dev"}
                    ]
                }
            ]
        }"#;

        let order = resolve_publish_order(metadata).expect("dev-only cycles must not block");

        assert_eq!(order.len(), 3);
        assert!(
            order.iter().position(|n| n == "cranpose").unwrap()
                < order.iter().position(|n| n == "cranpose-testing").unwrap()
        );
    }

    #[test]
    fn resolve_publish_order_rejects_a_real_cycle() {
        let metadata = r#"{
            "workspace_members": ["cranpose-a 0.1.0", "cranpose-b 0.1.0"],
            "packages": [
                {
                    "id": "cranpose-a 0.1.0",
                    "name": "cranpose-a",
                    "dependencies": [{"name": "cranpose-b", "kind": null}]
                },
                {
                    "id": "cranpose-b 0.1.0",
                    "name": "cranpose-b",
                    "dependencies": [{"name": "cranpose-a", "kind": null}]
                }
            ]
        }"#;

        let error = resolve_publish_order(metadata).expect_err("a real cycle must be rejected");

        assert_eq!(
            error,
            "Cyclic cranpose publish dependencies: cranpose-a, cranpose-b"
        );
    }

    #[test]
    fn resolve_publish_order_ignores_non_workspace_and_non_cranpose_packages() {
        let metadata = r#"{
            "workspace_members": ["cranpose 0.1.0"],
            "packages": [
                {"id": "cranpose 0.1.0", "name": "cranpose", "dependencies": [
                    {"name": "log", "kind": null}
                ]},
                {"id": "log 0.4.0 (registry+https://x)", "name": "log", "dependencies": []}
            ]
        }"#;

        let order = resolve_publish_order(metadata).expect("must resolve");

        assert_eq!(order, vec!["cranpose"]);
    }

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

    fn strs(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    fn token_lines(lines: &[&[&str]]) -> Vec<Vec<String>> {
        lines.iter().map(|line| strs(line)).collect()
    }

    #[test]
    fn parse_hunk_spans_single_hunk_modified_file() {
        let diff = [
            "diff --git a/src/lib.rs b/src/lib.rs",
            "--- src/lib.rs",
            "+++ src/lib.rs",
            "@@ -10,2 +10,3 @@",
            "+one",
            "+two",
            "+three",
        ]
        .join("\n");
        assert_eq!(
            gate_diff::parse_hunk_spans(&diff),
            BTreeMap::from([(
                "src/lib.rs".to_owned(),
                vec![(Some((10, 11)), Some((10, 12)))]
            )])
        );
    }

    #[test]
    fn parse_hunk_spans_single_line_hunk_omits_count() {
        let diff = [
            "--- src/lib.rs",
            "+++ src/lib.rs",
            "@@ -5 +5 @@",
            "-old",
            "+new",
        ]
        .join("\n");
        assert_eq!(
            gate_diff::parse_hunk_spans(&diff),
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((5, 5)), Some((5, 5)))])])
        );
    }

    #[test]
    fn parse_hunk_spans_pure_deletion_hunk_has_no_new_side_span() {
        let diff = [
            "--- src/lib.rs",
            "+++ src/lib.rs",
            "@@ -20,3 +19,0 @@",
            "-gone",
            "-gone too",
            "-and this",
        ]
        .join("\n");
        assert_eq!(
            gate_diff::parse_hunk_spans(&diff),
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((20, 22)), None)])])
        );
    }

    #[test]
    fn parse_hunk_spans_pure_addition_hunk_has_no_old_side_span() {
        let diff = [
            "--- src/lib.rs",
            "+++ src/lib.rs",
            "@@ -50,0 +52,1 @@",
            "+a3",
        ]
        .join("\n");
        assert_eq!(
            gate_diff::parse_hunk_spans(&diff),
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(None, Some((52, 52)))])])
        );
    }

    #[test]
    fn parse_hunk_spans_deleted_file_is_skipped() {
        let diff = [
            "--- src/dead.rs",
            "+++ /dev/null",
            "@@ -1,3 +0,0 @@",
            "-a",
            "-b",
            "-c",
        ]
        .join("\n");
        assert_eq!(gate_diff::parse_hunk_spans(&diff), BTreeMap::new());
    }

    #[test]
    fn parse_hunk_spans_multiple_hunks_and_files() {
        let diff = [
            "--- src/a.rs",
            "+++ src/a.rs",
            "@@ -1,0 +1,2 @@",
            "+a1",
            "+a2",
            "@@ -50,0 +52,1 @@",
            "+a3",
            "--- src/b.rs",
            "+++ src/b.rs",
            "@@ -3,1 +3,1 @@",
            "-old",
            "+new",
        ]
        .join("\n");
        assert_eq!(
            gate_diff::parse_hunk_spans(&diff),
            BTreeMap::from([
                (
                    "src/a.rs".to_owned(),
                    vec![(None, Some((1, 2))), (None, Some((52, 52)))]
                ),
                ("src/b.rs".to_owned(), vec![(Some((3, 3)), Some((3, 3)))]),
            ])
        );
    }

    #[test]
    fn code_tokens_plain_code_line() {
        assert_eq!(
            gate_diff::code_tokens_by_line("let x = 1;"),
            token_lines(&[&["let", "x", "=", "1", ";"]])
        );
    }

    #[test]
    fn code_tokens_blank_line() {
        assert_eq!(gate_diff::code_tokens_by_line(""), token_lines(&[&[]]));
        assert_eq!(
            gate_diff::code_tokens_by_line("   \t  "),
            token_lines(&[&[]])
        );
    }

    #[test]
    fn code_tokens_pure_line_comment() {
        assert_eq!(
            gate_diff::code_tokens_by_line("// just a note"),
            token_lines(&[&[]])
        );
    }

    #[test]
    fn code_tokens_trailing_comment_yields_only_the_codes_tokens() {
        assert_eq!(
            gate_diff::code_tokens_by_line("let x = 1; // trailing"),
            token_lines(&[&["let", "x", "=", "1", ";"]])
        );
    }

    #[test]
    fn code_tokens_slash_slash_inside_a_string_is_one_string_token_not_a_comment() {
        let text = "let url = \"https://example.com\";";
        assert_eq!(
            gate_diff::code_tokens_by_line(text),
            token_lines(&[&["let", "url", "=", "\"https://example.com\"", ";"]])
        );
    }

    #[test]
    fn code_tokens_string_spanning_a_naive_comment_check_does_not_start_one() {
        let text = ["let s = \"a // b\";", "// this really is a comment"].join("\n");
        assert_eq!(
            gate_diff::code_tokens_by_line(&text),
            token_lines(&[&["let", "s", "=", "\"a // b\"", ";"], &[]])
        );
    }

    #[test]
    fn code_tokens_single_line_block_comment() {
        assert_eq!(
            gate_diff::code_tokens_by_line("/* note */"),
            token_lines(&[&[]])
        );
    }

    #[test]
    fn code_tokens_single_line_block_comment_with_trailing_code() {
        assert_eq!(
            gate_diff::code_tokens_by_line("/* note */ let x = 1;"),
            token_lines(&[&["let", "x", "=", "1", ";"]])
        );
    }

    #[test]
    fn code_tokens_multi_line_block_comment_is_all_blank() {
        let text = ["/* start", "middle line", "end */"].join("\n");
        assert_eq!(
            gate_diff::code_tokens_by_line(&text),
            token_lines(&[&[], &[], &[]])
        );
    }

    #[test]
    fn code_tokens_multi_line_block_comment_with_code_before_and_after() {
        let text = ["let a = 1; /* start", "middle line", "end */ let b = 2;"].join("\n");
        assert_eq!(
            gate_diff::code_tokens_by_line(&text),
            token_lines(&[
                &["let", "a", "=", "1", ";"],
                &[],
                &["let", "b", "=", "2", ";"]
            ])
        );
    }

    #[test]
    fn code_tokens_nested_block_comments() {
        let text = ["/* outer /* inner */ still commented", "*/ let x = 1;"].join("\n");
        assert_eq!(
            gate_diff::code_tokens_by_line(&text),
            token_lines(&[&[], &["let", "x", "=", "1", ";"]])
        );
    }

    #[test]
    fn code_tokens_raw_string_containing_slashes_and_quotes_is_one_token() {
        let text = "let s = r#\"// not a comment, and \"quoted\" too\"#;";
        let raw_token = "r#\"// not a comment, and \"quoted\" too\"#";
        assert_eq!(
            gate_diff::code_tokens_by_line(text),
            token_lines(&[&["let", "s", "=", raw_token, ";"]])
        );
    }

    #[test]
    fn code_tokens_raw_string_spanning_lines_is_one_token_on_its_start_line() {
        let text = "let s = r\"line one\n// still string content\nline three\";";
        let raw_token = "r\"line one\n// still string content\nline three\"";
        assert_eq!(
            gate_diff::code_tokens_by_line(text),
            token_lines(&[&["let", "s", "=", raw_token], &[], &[";"]])
        );
    }

    #[test]
    fn code_tokens_raw_string_prefix_not_misdetected_mid_identifier() {
        let text = "let bar = 1; let s = \"text\";";
        assert_eq!(
            gate_diff::code_tokens_by_line(text),
            token_lines(&[&["let", "bar", "=", "1", ";", "let", "s", "=", "\"text\"", ";"]])
        );
    }

    #[test]
    fn code_tokens_char_literal_does_not_start_a_comment() {
        let text = "let c = '/';";
        assert_eq!(
            gate_diff::code_tokens_by_line(text),
            token_lines(&[&["let", "c", "=", "'/'", ";"]])
        );
    }

    #[test]
    fn code_tokens_escaped_char_literal() {
        let text = "let c = '\\n';";
        assert_eq!(
            gate_diff::code_tokens_by_line(text),
            token_lines(&[&["let", "c", "=", "'\\n'", ";"]])
        );
    }

    #[test]
    fn code_tokens_lifetime_is_not_treated_as_an_unterminated_char_literal() {
        let text = "fn f<'a>(x: &'a str) -> &'a str { x }\n// a real comment";
        let tokens = gate_diff::code_tokens_by_line(text);
        assert!(
            !tokens[0].is_empty(),
            "the code line must yield at least one token"
        );
        assert_eq!(tokens[1], Vec::<String>::new());
    }

    #[test]
    fn code_tokens_string_containing_an_escaped_quote_is_one_token() {
        let text = "let s = \"she said \\\"hi\\\"\";";
        let string_token = "\"she said \\\"hi\\\"\"";
        assert_eq!(
            gate_diff::code_tokens_by_line(text),
            token_lines(&[&["let", "s", "=", string_token, ";"]])
        );
    }

    #[test]
    fn drop_trailing_commas_comma_before_closing_paren_is_dropped() {
        assert_eq!(
            gate_diff::drop_trailing_commas(&strs(&["f", "(", "a", ",", ")"])),
            strs(&["f", "(", "a", ")"])
        );
    }

    #[test]
    fn drop_trailing_commas_comma_before_closing_bracket_is_dropped() {
        assert_eq!(
            gate_diff::drop_trailing_commas(&strs(&["[", "1", ",", "]"])),
            strs(&["[", "1", "]"])
        );
    }

    #[test]
    fn drop_trailing_commas_comma_before_closing_brace_is_dropped() {
        assert_eq!(
            gate_diff::drop_trailing_commas(&strs(&["{", "x", ":", "1", ",", "}"])),
            strs(&["{", "x", ":", "1", "}"])
        );
    }

    #[test]
    fn drop_trailing_commas_comma_between_arguments_is_kept() {
        assert_eq!(
            gate_diff::drop_trailing_commas(&strs(&["f", "(", "a", ",", "b", ")"])),
            strs(&["f", "(", "a", ",", "b", ")"])
        );
    }

    #[test]
    fn drop_trailing_commas_trailing_comma_at_the_very_end_of_the_list_is_kept() {
        assert_eq!(
            gate_diff::drop_trailing_commas(&strs(&["a", ","])),
            strs(&["a", ","])
        );
    }

    fn semantic_ranges_with(
        hunk_spans: &gate_diff::HunkSpans,
        old: &BTreeMap<String, String>,
        new: &BTreeMap<String, String>,
    ) -> gate_diff::RangesByFile {
        gate_diff::semantic_ranges(
            hunk_spans,
            |file: &str| old.get(file).cloned().unwrap_or_default(),
            |file: &str| new.get(file).cloned().unwrap_or_default(),
        )
    }

    #[test]
    fn semantic_ranges_comment_only_edit_is_dropped() {
        let hunk_spans =
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((2, 2)), Some((2, 2)))])]);
        let old = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "fn f() {\n    let x = 1;  // set x\n}\n".to_owned(),
        )]);
        let new = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "fn f() {\n    let x = 1;\n}\n".to_owned(),
        )]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::new()
        );
    }

    #[test]
    fn semantic_ranges_hunk_with_a_real_code_change_is_kept_in_full() {
        let hunk_spans =
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((2, 3)), Some((2, 3)))])]);
        let old = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "fn f() {\n    // a comment\n    let x = 0;\n}\n".to_owned(),
        )]);
        let new = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "fn f() {\n    // updated comment\n    let x = 1;\n}\n".to_owned(),
        )]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(2, 3)])])
        );
    }

    #[test]
    fn semantic_ranges_only_the_semantically_unchanged_hunk_is_dropped_others_survive() {
        let hunk_spans = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![(Some((2, 2)), Some((2, 2))), (Some((4, 4)), Some((4, 4)))],
        )]);
        let old = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "fn f() {\n    // comment\n    let x = 1;\n    let y = 1;\n}\n".to_owned(),
        )]);
        let new = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "fn f() {\n    // different comment\n    let x = 1;\n    let y = 2;\n}\n".to_owned(),
        )]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(4, 4)])])
        );
    }

    #[test]
    fn semantic_ranges_file_with_no_surviving_ranges_is_dropped_entirely() {
        let hunk_spans = BTreeMap::from([(
            "src/only_comments.rs".to_owned(),
            vec![(Some((1, 1)), Some((1, 1)))],
        )]);
        let old = BTreeMap::from([(
            "src/only_comments.rs".to_owned(),
            "// nothing but this\n".to_owned(),
        )]);
        let new = BTreeMap::from([(
            "src/only_comments.rs".to_owned(),
            "// nothing but this, reworded\n".to_owned(),
        )]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::new()
        );
    }

    #[test]
    fn semantic_ranges_pure_deletion_hunk_contributes_no_range_regardless_of_content() {
        let hunk_spans = BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((5, 7)), None)])]);
        let old = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "fn f() {\n    let x = 1;\n    let y = 2;\n    let z = 3;\n}\n".to_owned(),
        )]);
        let new = BTreeMap::from([("src/lib.rs".to_owned(), "fn f() {\n}\n".to_owned())]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::new()
        );
    }

    #[test]
    fn semantic_ranges_comment_removal_that_collapses_a_block_onto_one_line_is_dropped() {
        let hunk_spans =
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((2, 4)), Some((2, 2)))])]);
        let old = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            [
                "fn f(cond: bool) {",
                "    if cond {",
                "        // Found something",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        )]);
        let new = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            ["fn f(cond: bool) {", "    if cond {}", "}", ""].join("\n"),
        )]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::new()
        );
    }

    #[test]
    fn semantic_ranges_whitespace_reorder_bundled_with_a_real_token_change_is_kept() {
        let hunk_spans =
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((1, 1)), Some((1, 2)))])]);
        let old = BTreeMap::from([("src/lib.rs".to_owned(), "let x = 1;\n".to_owned())]);
        let new = BTreeMap::from([("src/lib.rs".to_owned(), "let   x =\n    2;\n".to_owned())]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 2)])])
        );
    }

    #[test]
    fn semantic_ranges_string_literal_content_is_compared_not_discarded() {
        let hunk_spans =
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((1, 1)), Some((1, 1)))])]);
        let old = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "let s = \"// keep me A\";\n".to_owned(),
        )]);
        let new = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "let s = \"// keep me B\";\n".to_owned(),
        )]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 1)])])
        );
    }

    #[test]
    fn semantic_ranges_call_reformatted_onto_fewer_lines_drops_only_its_trailing_comma() {
        let hunk_spans =
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((1, 5)), Some((1, 1)))])]);
        let old = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            [
                "f(",
                "    // pick the material",
                "    material(),",
                "    move || dynamics(),",
                ");",
                "",
            ]
            .join("\n"),
        )]);
        let new = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            "f(material(), move || dynamics());\n".to_owned(),
        )]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::new()
        );
    }

    #[test]
    fn semantic_ranges_trailing_comma_change_bundled_with_a_real_edit_is_still_kept() {
        let hunk_spans =
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((1, 1)), Some((1, 1)))])]);
        let old = BTreeMap::from([("src/lib.rs".to_owned(), "f(a, b,);\n".to_owned())]);
        let new = BTreeMap::from([("src/lib.rs".to_owned(), "f(a, c);\n".to_owned())]);
        assert_eq!(
            semantic_ranges_with(&hunk_spans, &old, &new),
            BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 1)])])
        );
    }

    #[test]
    fn intersects_overlapping_ranges() {
        assert!(gate_diff::intersects((10, 20), (15, 25)));
        assert!(gate_diff::intersects((15, 25), (10, 20)));
    }

    #[test]
    fn intersects_touching_at_a_single_line_counts_as_overlap() {
        assert!(gate_diff::intersects((10, 20), (20, 30)));
    }

    #[test]
    fn intersects_disjoint_ranges() {
        assert!(!gate_diff::intersects((10, 20), (21, 30)));
    }

    #[test]
    fn intersects_one_range_contains_the_other() {
        assert!(gate_diff::intersects((1, 100), (40, 41)));
    }

    #[test]
    fn any_intersect_true_only_when_one_range_matches() {
        let ranges = vec![(1, 5), (50, 60)];
        assert!(gate_diff::any_intersect(&ranges, (55, 58)));
        assert!(!gate_diff::any_intersect(&ranges, (10, 20)));
        assert!(!gate_diff::any_intersect(&[], (1, 1000)));
    }

    #[test]
    fn cargo_bin_dir_honors_cargo_home() {
        let dir = gate_diff::cargo_bin_dir_for(
            Some(std::ffi::OsString::from("/scratch/cargo")),
            PathBuf::from("/unused"),
        );
        assert_eq!(dir, PathBuf::from("/scratch/cargo/bin"));
    }

    #[test]
    fn cargo_bin_dir_falls_back_to_home_cargo() {
        let dir = gate_diff::cargo_bin_dir_for(None, PathBuf::from("/home/test"));
        assert_eq!(dir, PathBuf::from("/home/test/.cargo/bin"));
    }

    #[test]
    fn resolve_cargo_tool_never_installs_when_already_at_the_pinned_location() {
        let root = unique_temp_dir();
        let bin_dir = unique_temp_dir();
        let fake_tool = bin_dir.join("some-tool");
        fs::write(&fake_tool, "#!/bin/sh\n").expect("write fake tool");
        make_executable(&fake_tool).expect("chmod fake tool");

        let installed = std::cell::Cell::new(false);
        let resolved =
            gate_diff::resolve_cargo_tool_in(&root, &bin_dir, "some-tool", "", |_, _| {
                installed.set(true);
            })
            .expect("resolve succeeds");

        assert!(!installed.get());
        assert_eq!(resolved, fake_tool);
    }

    #[test]
    fn resolve_cargo_tool_installs_when_missing_then_resolves_to_the_pinned_location() {
        let root = unique_temp_dir();
        let bin_dir = unique_temp_dir();
        let fake_tool = bin_dir.join("some-tool");

        let install_count = std::cell::Cell::new(0u32);
        let resolved =
            gate_diff::resolve_cargo_tool_in(&root, &bin_dir, "some-tool", "", |_, _| {
                install_count.set(install_count.get() + 1);
                fs::write(&fake_tool, "#!/bin/sh\n").expect("write fake tool");
                make_executable(&fake_tool).expect("chmod fake tool");
            })
            .expect("resolve succeeds");

        assert_eq!(install_count.get(), 1);
        assert_eq!(resolved, fake_tool);
    }

    #[test]
    fn resolve_cargo_tool_raises_with_the_hint_when_install_does_not_produce_the_binary() {
        let root = unique_temp_dir();
        let bin_dir = unique_temp_dir();
        let error = gate_diff::resolve_cargo_tool_in(
            &root,
            &bin_dir,
            "some-tool",
            "do not substitute npm",
            |_, _| {},
        )
        .expect_err("install never produces the binary");
        assert!(error.contains("do not substitute npm"));
    }

    fn git(args: &[&str], cwd: &Path) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git")
    }

    fn git_ok(args: &[&str], cwd: &Path) -> String {
        let output = git(args, cwd);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn is_shallow(repo: &Path) -> bool {
        git_ok(&["rev-parse", "--is-shallow-repository"], repo) == "true"
    }

    fn commit_file(repo: &Path, message: &str) -> String {
        fs::write(repo.join("file.txt"), message).expect("write file");
        git_ok(&["add", "."], repo);
        git_ok(&["commit", "--quiet", "-m", message], repo);
        git_ok(&["rev-parse", "HEAD"], repo)
    }

    fn init_repo(repo: &Path, initial_branch: &str) {
        fs::create_dir_all(repo).expect("create repo dir");
        git_ok(&["init", "--quiet", "-b", initial_branch], repo);
        git_ok(&["config", "user.email", "test@example.com"], repo);
        git_ok(&["config", "user.name", "Test"], repo);
    }

    fn shallow_checkout_of_branch_tip(origin: &Path, branch: &str, work: &Path) {
        fs::create_dir_all(work).expect("create work dir");
        git_ok(&["init", "--quiet"], work);
        git_ok(
            &[
                "remote",
                "add",
                "origin",
                &format!("file://{}", origin.display()),
            ],
            work,
        );
        git_ok(
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
            work,
        );
        let tip = git_ok(&["rev-parse", branch], origin);
        git_ok(
            &[
                "fetch",
                "--quiet",
                "--depth=1",
                "origin",
                &format!("+{tip}:refs/remotes/origin/{branch}"),
            ],
            work,
        );
        git_ok(
            &[
                "checkout",
                "--quiet",
                "-b",
                branch,
                &format!("origin/{branch}"),
            ],
            work,
        );
    }

    #[test]
    fn merge_base_deepens_shallow_history_to_find_the_merge_base() {
        let tmp = unique_temp_dir();
        let origin = tmp.join("origin");
        let work = tmp.join("work");
        init_repo(&origin, "main");
        let shared_ancestor = commit_file(&origin, "shared ancestor");
        git_ok(&["checkout", "--quiet", "-b", "feature"], &origin);
        commit_file(&origin, "feature work");
        git_ok(&["checkout", "--quiet", "main"], &origin);
        commit_file(&origin, "main moved on without the feature branch");

        shallow_checkout_of_branch_tip(&origin, "feature", &work);
        assert!(is_shallow(&work));

        assert_eq!(
            gate_diff::merge_base(&work, "origin/main").expect("merge base found"),
            shared_ancestor
        );
        assert!(!is_shallow(&work));
    }

    #[test]
    fn merge_base_raises_when_histories_truly_share_no_ancestor() {
        let tmp = unique_temp_dir();
        let origin_main = tmp.join("origin_main");
        let origin_feature = tmp.join("origin_feature");
        let work = tmp.join("work");
        init_repo(&origin_main, "main");
        commit_file(&origin_main, "main's own unrelated root");
        init_repo(&origin_feature, "feature");
        commit_file(&origin_feature, "feature's own unrelated root");

        fs::create_dir_all(&work).expect("create work dir");
        git_ok(&["init", "--quiet"], &work);
        git_ok(
            &[
                "remote",
                "add",
                "origin",
                &format!("file://{}", origin_main.display()),
            ],
            &work,
        );
        git_ok(&["fetch", "--quiet", "origin", "main"], &work);
        git_ok(
            &[
                "remote",
                "add",
                "elsewhere",
                &format!("file://{}", origin_feature.display()),
            ],
            &work,
        );
        git_ok(&["fetch", "--quiet", "elsewhere", "feature"], &work);
        git_ok(
            &["checkout", "--quiet", "-b", "feature", "elsewhere/feature"],
            &work,
        );
        assert!(!is_shallow(&work));

        let error = gate_diff::merge_base(&work, "origin/main").expect_err("no shared history");
        assert!(error.contains("share no common ancestor"));
    }

    const OVER_LIMIT_FUNCTION: &str = "fn deeply_branching(x: i32) -> i32 {\n    if x == 0 { return 0; }\n    if x == 1 { return 1; }\n    if x == 2 { return 2; }\n    if x == 3 { return 3; }\n    if x == 4 { return 4; }\n    if x == 5 { return 5; }\n    if x == 6 { return 6; }\n    if x == 7 { return 7; }\n    if x == 8 { return 8; }\n    if x == 9 { return 9; }\n    x\n}";

    fn init_repo_with_base_commit(repo: &Path) {
        fs::create_dir_all(repo.join("src")).expect("create src dir");
        git_ok(&["init", "--quiet", "-b", "main"], repo);
        git_ok(&["config", "user.email", "test@example.com"], repo);
        git_ok(&["config", "user.name", "Test"], repo);
        fs::write(repo.join("src/lib.rs"), format!("{OVER_LIMIT_FUNCTION}\n"))
            .expect("write lib.rs");
        git_ok(&["add", "."], repo);
        git_ok(&["commit", "--quiet", "-m", "base"], repo);
    }

    fn write_and_commit(repo: &Path, contents: &str, message: &str) {
        fs::write(repo.join("src/lib.rs"), contents).expect("write lib.rs");
        git_ok(&["add", "."], repo);
        git_ok(&["commit", "--quiet", "-m", message], repo);
    }

    #[test]
    fn changed_ranges_removing_a_comment_inside_the_function_does_not_touch_it() {
        let tmp = unique_temp_dir();
        let repo = tmp.join("repo");
        init_repo_with_base_commit(&repo);

        let with_comment = OVER_LIMIT_FUNCTION.replace(
            "    x\n}",
            "    // fall through for anything else\n    x\n}",
        );
        write_and_commit(&repo, &format!("{with_comment}\n"), "add a comment");

        let comment_removed = with_comment.replace("    // fall through for anything else\n", "");
        write_and_commit(
            &repo,
            &format!("{comment_removed}\n"),
            "remove only the comment",
        );

        let ranges =
            gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
        assert_eq!(
            ranges,
            BTreeMap::new(),
            "a hunk that only deleted a comment line must not touch anything"
        );
    }

    #[test]
    fn changed_ranges_a_genuine_logic_edit_in_the_same_function_still_touches_it() {
        let tmp = unique_temp_dir();
        let repo = tmp.join("repo");
        init_repo_with_base_commit(&repo);

        let edited = OVER_LIMIT_FUNCTION.replace(
            "    if x == 9 { return 9; }",
            "    if x == 9 { return 90; }",
        );
        write_and_commit(&repo, &format!("{edited}\n"), "change a return value");

        let ranges =
            gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
        let touched = ranges.get("src/lib.rs").expect("src/lib.rs touched");
        assert!(
            gate_diff::any_intersect(touched, (1, 13)),
            "a real logic edit must still land inside the function's span, got {touched:?}"
        );
    }

    #[test]
    fn changed_ranges_mixed_hunk_of_comment_and_logic_still_touches_it() {
        let tmp = unique_temp_dir();
        let repo = tmp.join("repo");
        init_repo_with_base_commit(&repo);

        let edited = OVER_LIMIT_FUNCTION.replace(
            "    x\n}",
            "    // fall through for anything else\n    x + 1\n}",
        );
        write_and_commit(
            &repo,
            &format!("{edited}\n"),
            "comment plus a real edit, one hunk",
        );

        let ranges =
            gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
        assert!(gate_diff::any_intersect(
            ranges.get("src/lib.rs").expect("touched"),
            (1, 13)
        ));
    }

    #[test]
    fn changed_ranges_comment_deletion_that_collapses_a_branch_onto_one_line_does_not_touch_it() {
        let tmp = unique_temp_dir();
        let repo = tmp.join("repo");
        init_repo_with_base_commit(&repo);

        let with_comment_block = OVER_LIMIT_FUNCTION.replace(
            "    if x == 9 { return 9; }",
            "    if x == 9 {\n        // nothing special about nine\n    }",
        );
        write_and_commit(
            &repo,
            &format!("{with_comment_block}\n"),
            "expand nine's branch",
        );

        let collapsed =
            OVER_LIMIT_FUNCTION.replace("    if x == 9 { return 9; }", "    if x == 9 {}");
        write_and_commit(
            &repo,
            &format!("{collapsed}\n"),
            "delete the comment, collapse the block",
        );

        let ranges =
            gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
        assert_eq!(
            ranges,
            BTreeMap::new(),
            "deleting a comment and collapsing its now-empty block is not a logic edit, got {ranges:?}"
        );
    }

    #[test]
    fn changed_ranges_reformatting_a_line_while_also_changing_it_still_touches_it() {
        let tmp = unique_temp_dir();
        let repo = tmp.join("repo");
        init_repo_with_base_commit(&repo);

        let edited = OVER_LIMIT_FUNCTION.replace(
            "    if x == 9 { return 9; }",
            "    if x == 9 {\n        return 90;\n    }",
        );
        write_and_commit(
            &repo,
            &format!("{edited}\n"),
            "reformat the branch onto three lines and change its value",
        );

        let ranges =
            gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
        let touched = ranges.get("src/lib.rs").expect("src/lib.rs touched");
        assert!(
            gate_diff::any_intersect(touched, (1, 13)),
            "a real value change bundled with a reformat must not be normalized away, got {touched:?}"
        );
    }

    fn func_space(
        name: Option<&str>,
        start: u64,
        end: u64,
        cyclomatic: f64,
        children: Vec<serde_json::Value>,
    ) -> serde_json::Value {
        serde_json::json!({
            "kind": "function",
            "name": name,
            "start_line": start,
            "end_line": end,
            "metrics": {"cyclomatic": {"sum": cyclomatic}},
            "spaces": children,
        })
    }

    #[test]
    fn functions_in_named_function_with_no_closures_is_reported_once() {
        let mut out = Vec::new();
        complexity_gate::functions_in(&func_space(Some("f"), 1, 10, 5.0, vec![]), &mut out, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "f");
    }

    #[test]
    fn functions_in_closure_nested_in_a_named_function_is_not_reported_separately() {
        let closure = func_space(None, 2, 9, 25.0, vec![]);
        let outer = func_space(Some("main"), 1, 10, 27.0, vec![closure]);
        let mut out = Vec::new();
        complexity_gate::functions_in(&outer, &mut out, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "main");
        assert_eq!(out[0].cyclomatic, Some(27));
    }

    #[test]
    fn functions_in_closure_nested_in_a_closure_still_collapses_to_one_report() {
        let inner_closure = func_space(None, 3, 8, 10.0, vec![]);
        let outer_closure = func_space(None, 2, 9, 15.0, vec![inner_closure]);
        let outer = func_space(Some("run"), 1, 10, 20.0, vec![outer_closure]);
        let mut out = Vec::new();
        complexity_gate::functions_in(&outer, &mut out, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "run");
    }

    #[test]
    fn functions_in_named_function_nested_inside_a_closure_is_still_reported() {
        let nested_fn = func_space(Some("helper"), 3, 5, 8.0, vec![]);
        let closure = func_space(None, 2, 6, 9.0, vec![nested_fn]);
        let outer = func_space(Some("run"), 1, 7, 12.0, vec![closure]);
        let mut out = Vec::new();
        complexity_gate::functions_in(&outer, &mut out, false);
        let names: BTreeSet<_> = out.iter().map(|f| f.name.clone()).collect();
        assert_eq!(
            names,
            BTreeSet::from(["run".to_owned(), "helper".to_owned()])
        );
    }

    #[test]
    fn functions_in_top_level_closure_with_no_enclosing_function_is_still_reported() {
        let top_level_closure = func_space(None, 1, 5, 12.0, vec![]);
        let mut out = Vec::new();
        complexity_gate::functions_in(&top_level_closure, &mut out, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "<anonymous>");
    }

    #[test]
    fn functions_in_rust_code_analysis_cli_names_a_closure_the_literal_string_anonymous() {
        let closure = func_space(Some("<anonymous>"), 2, 9, 25.0, vec![]);
        let outer = func_space(Some("main"), 1, 10, 27.0, vec![closure]);
        let mut out = Vec::new();
        complexity_gate::functions_in(&outer, &mut out, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "main");
    }

    #[test]
    fn functions_in_sibling_functions_are_both_reported() {
        let root = serde_json::json!({
            "kind": "file",
            "spaces": [func_space(Some("a"), 1, 5, 5.0, vec![]), func_space(Some("b"), 10, 15, 5.0, vec![])],
        });
        let mut out = Vec::new();
        complexity_gate::functions_in(&root, &mut out, false);
        let names: BTreeSet<_> = out.iter().map(|f| f.name.clone()).collect();
        assert_eq!(names, BTreeSet::from(["a".to_owned(), "b".to_owned()]));
    }

    fn function_metric(
        name: &str,
        start: usize,
        end: usize,
        cyclomatic: i64,
    ) -> complexity_gate::FunctionMetric {
        complexity_gate::FunctionMetric {
            name: name.to_owned(),
            start: Some(start),
            end: Some(end),
            cyclomatic: Some(cyclomatic),
        }
    }

    #[test]
    fn complexity_find_violations_flags_only_functions_the_diff_touches() {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(10, 15)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![
                function_metric("touched_and_complex", 10, 15, 25),
                function_metric("untouched_and_complex", 100, 120, 99),
                function_metric("touched_but_simple", 12, 13, 3),
            ],
        )]);
        let violations =
            complexity_gate::find_violations(&ranges, &new_functions, &BTreeMap::new(), 20);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("touched_and_complex"));
        assert!(violations[0].contains("is new at 25"));
    }

    #[test]
    fn complexity_find_violations_no_violations_when_nothing_over_the_limit() {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 100)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("fine", 1, 10, 5)],
        )]);
        assert_eq!(
            complexity_gate::find_violations(&ranges, &new_functions, &BTreeMap::new(), 20),
            Vec::<String>::new()
        );
    }

    #[test]
    fn complexity_find_violations_untouched_file_contributes_no_violations() {
        let ranges = BTreeMap::from([("src/other.rs".to_owned(), vec![(1, 5)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("huge", 1, 500, 500)],
        )]);
        assert_eq!(
            complexity_gate::find_violations(&ranges, &new_functions, &BTreeMap::new(), 20),
            Vec::<String>::new()
        );
    }

    #[test]
    fn complexity_find_violations_already_over_limit_function_untouched_by_a_real_edit_passes() {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 300)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("run", 1, 300, 174)],
        )]);
        let old_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("run", 1, 305, 174)],
        )]);
        assert_eq!(
            complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20),
            Vec::<String>::new()
        );
    }

    #[test]
    fn complexity_find_violations_already_over_limit_function_made_simpler_passes() {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 300)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("run", 1, 300, 150)],
        )]);
        let old_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("run", 1, 305, 174)],
        )]);
        assert_eq!(
            complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20),
            Vec::<String>::new()
        );
    }

    #[test]
    fn complexity_find_violations_already_over_limit_function_made_worse_still_trips_the_gate() {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 300)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("run", 1, 300, 180)],
        )]);
        let old_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("run", 1, 305, 174)],
        )]);
        let violations =
            complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("was 174, is now 180"));
    }

    #[test]
    fn complexity_find_violations_under_the_limit_before_and_over_after_still_trips_the_gate() {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 20)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("f", 1, 20, 25)],
        )]);
        let old_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("f", 1, 18, 18)],
        )]);
        let violations =
            complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("was 18, is now 25"));
    }

    #[test]
    fn complexity_find_violations_new_function_with_no_old_counterpart_is_judged_against_the_limit()
    {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 20)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("brand_new", 1, 20, 25)],
        )]);
        let violations =
            complexity_gate::find_violations(&ranges, &new_functions, &BTreeMap::new(), 20);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("is new at 25"));
    }

    #[test]
    fn complexity_find_violations_same_named_functions_are_matched_by_occurrence_order() {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 5), (10, 15)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![
                function_metric("new", 1, 5, 22),
                function_metric("new", 10, 15, 30),
            ],
        )]);
        let old_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![
                function_metric("new", 1, 5, 22),
                function_metric("new", 9, 14, 18),
            ],
        )]);
        let violations =
            complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains(":10-15"));
        assert!(violations[0].contains("was 18, is now 30"));
    }

    #[test]
    fn complexity_find_violations_anonymous_function_has_no_old_counterpart_even_if_old_side_has_one(
    ) {
        let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 5)])]);
        let new_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("<anonymous>", 1, 5, 25)],
        )]);
        let old_functions = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![function_metric("<anonymous>", 1, 5, 99)],
        )]);
        let violations =
            complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("is new at 25"));
    }

    #[test]
    fn load_max_cyclomatic_reads_the_configured_limit() {
        let dir = unique_temp_dir();
        let config_path = dir.join("code_quality_gates.toml");
        fs::write(&config_path, "[complexity]\nmax_cyclomatic = 20\n").expect("write config");
        assert_eq!(
            complexity_gate::load_max_cyclomatic(&config_path).expect("load succeeds"),
            20
        );
    }

    #[test]
    fn load_max_cyclomatic_fails_when_the_key_is_missing() {
        let dir = unique_temp_dir();
        let config_path = dir.join("code_quality_gates.toml");
        fs::write(&config_path, "[complexity]\n").expect("write config");
        assert!(complexity_gate::load_max_cyclomatic(&config_path).is_err());
    }

    fn duplicate(
        first: (&str, usize, usize),
        second: (&str, usize, usize),
        lines: usize,
    ) -> duplication_gate::Duplicate {
        duplication_gate::Duplicate {
            first_file: duplication_gate::DuplicateSide {
                name: first.0.to_owned(),
                start: first.1,
                end: first.2,
            },
            second_file: duplication_gate::DuplicateSide {
                name: second.0.to_owned(),
                start: second.1,
                end: second.2,
            },
            lines,
        }
    }

    #[test]
    fn duplication_find_violations_new_code_duplicating_old_code_fails() {
        let ranges = BTreeMap::from([("src/new.rs".to_owned(), vec![(1, 20)])]);
        let dup = duplicate(("src/new.rs", 5, 16), ("src/old.rs", 100, 111), 12);
        let violations = duplication_gate::find_violations(&[dup], &[], &ranges);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("src/new.rs:5-16 (new)"));
        assert!(!violations[0].contains("src/old.rs:100-111 (new)"));
    }

    #[test]
    fn duplication_find_violations_two_untouched_clones_are_not_flagged() {
        let ranges = BTreeMap::from([("src/elsewhere.rs".to_owned(), vec![(1, 5)])]);
        let dup = duplicate(("src/old_a.rs", 1, 12), ("src/old_b.rs", 1, 12), 12);
        assert_eq!(
            duplication_gate::find_violations(&[dup], &[], &ranges),
            Vec::<String>::new()
        );
    }

    #[test]
    fn duplication_find_violations_new_code_duplicating_itself_flags_both_sides() {
        let ranges = BTreeMap::from([("src/new.rs".to_owned(), vec![(1, 50)])]);
        let dup = duplicate(("src/new.rs", 1, 12), ("src/new.rs", 20, 31), 12);
        let violations = duplication_gate::find_violations(&[dup], &[], &ranges);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("(new)"));
    }

    #[test]
    fn duplication_find_violations_touched_clone_already_duplicated_before_passes() {
        let ranges = BTreeMap::from([("src/a.rs".to_owned(), vec![(10, 12)])]);
        let new_dup = duplicate(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), 12);
        let old_dup = duplicate(("src/a.rs", 9, 20), ("src/b.rs", 38, 49), 12);
        assert_eq!(
            duplication_gate::find_violations(&[new_dup], &[old_dup], &ranges),
            Vec::<String>::new()
        );
    }

    #[test]
    fn duplication_find_violations_touched_clone_with_no_old_counterpart_still_fails() {
        let ranges = BTreeMap::from([("src/a.rs".to_owned(), vec![(10, 12)])]);
        let new_dup = duplicate(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), 12);
        let violations = duplication_gate::find_violations(&[new_dup], &[], &ranges);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("introduced by this diff"));
    }

    #[test]
    fn duplication_find_violations_unrelated_old_clone_between_the_same_files_grants_no_amnesty() {
        let ranges = BTreeMap::from([("src/a.rs".to_owned(), vec![(10, 12)])]);
        let new_dup = duplicate(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), 12);
        let unrelated_old_dup = duplicate(("src/a.rs", 200, 299), ("src/b.rs", 300, 399), 100);
        let violations =
            duplication_gate::find_violations(&[new_dup], &[unrelated_old_dup], &ranges);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn duplication_find_violations_reformatted_clone_within_size_tolerance_still_matches() {
        let ranges = BTreeMap::from([("src/a.rs".to_owned(), vec![(10, 12)])]);
        let new_dup = duplicate(("src/a.rs", 10, 19), ("src/b.rs", 40, 49), 10);
        let old_dup = duplicate(("src/a.rs", 9, 20), ("src/b.rs", 38, 49), 12);
        assert_eq!(
            duplication_gate::find_violations(&[new_dup], &[old_dup], &ranges),
            Vec::<String>::new()
        );
    }

    #[test]
    fn file_pair_order_independent() {
        let a = duplicate(("x.rs", 0, 0), ("y.rs", 0, 0), 0);
        let b = duplicate(("y.rs", 0, 0), ("x.rs", 0, 0), 0);
        assert_eq!(
            duplication_gate::file_pair(&a),
            duplication_gate::file_pair(&b)
        );
    }

    #[test]
    fn already_duplicated_before_no_old_duplicates_at_all() {
        let candidate = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 12);
        assert!(!duplication_gate::already_duplicated_before(
            &candidate,
            &[]
        ));
    }

    #[test]
    fn already_duplicated_before_matching_pair_within_size_tolerance() {
        let candidate = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 12);
        let old = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 10);
        assert!(duplication_gate::already_duplicated_before(
            &candidate,
            &[old]
        ));
    }

    #[test]
    fn already_duplicated_before_matching_pair_outside_size_tolerance_does_not_count() {
        let candidate = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 12);
        let old = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 100);
        assert!(!duplication_gate::already_duplicated_before(
            &candidate,
            &[old]
        ));
    }

    #[test]
    fn already_duplicated_before_different_pair_does_not_count() {
        let candidate = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 12);
        let old = duplicate(("a.rs", 1, 1), ("c.rs", 1, 1), 12);
        assert!(!duplication_gate::already_duplicated_before(
            &candidate,
            &[old]
        ));
    }

    #[test]
    fn load_duplication_config_reads_lines_tokens_and_ignore_globs() {
        let dir = unique_temp_dir();
        let config_path = dir.join("code_quality_gates.toml");
        fs::write(
            &config_path,
            "[duplication]\nmin_lines = 10\nmin_tokens = 50\nignore_globs = [\"**/target/**\"]\n",
        )
        .expect("write config");
        let config =
            duplication_gate::load_duplication_config(&config_path).expect("load succeeds");
        assert_eq!(config.min_lines, 10);
        assert_eq!(config.min_tokens, 50);
        assert_eq!(config.ignore_globs, vec!["**/target/**".to_owned()]);
    }

    #[test]
    fn load_duplication_config_defaults_ignore_globs_to_empty() {
        let dir = unique_temp_dir();
        let config_path = dir.join("code_quality_gates.toml");
        fs::write(
            &config_path,
            "[duplication]\nmin_lines = 10\nmin_tokens = 50\n",
        )
        .expect("write config");
        let config =
            duplication_gate::load_duplication_config(&config_path).expect("load succeeds");
        assert_eq!(config.ignore_globs, Vec::<String>::new());
    }

    #[test]
    fn parse_duplicates_reads_first_and_second_file_spans() {
        let report = serde_json::json!({
            "duplicates": [
                {
                    "firstFile": {"name": "a.rs", "start": 1, "end": 12},
                    "secondFile": {"name": "b.rs", "start": 5, "end": 16},
                    "lines": 12,
                }
            ]
        });
        let duplicates = duplication_gate::parse_duplicates(&report);
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].first_file.name, "a.rs");
        assert_eq!(duplicates[0].lines, 12);
    }

    #[test]
    fn parse_duplicates_skips_entries_missing_required_fields() {
        let report = serde_json::json!({"duplicates": [{"firstFile": {"name": "a.rs", "start": 1, "end": 12}}]});
        assert_eq!(
            duplication_gate::parse_duplicates(&report),
            Vec::<duplication_gate::Duplicate>::new()
        );
    }

    #[test]
    fn parse_duplicates_defaults_to_empty_when_the_key_is_absent() {
        let report = serde_json::json!({});
        assert_eq!(
            duplication_gate::parse_duplicates(&report),
            Vec::<duplication_gate::Duplicate>::new()
        );
    }

    #[test]
    fn duplication_involved_files_collects_both_sides_of_every_candidate() {
        let a = duplicate(("a.rs", 1, 5), ("b.rs", 1, 5), 5);
        let b = duplicate(("b.rs", 10, 15), ("c.rs", 10, 15), 5);
        assert_eq!(
            duplication_gate::involved_files(&[a, b]),
            vec!["a.rs".to_owned(), "b.rs".to_owned(), "c.rs".to_owned()]
        );
    }

    #[test]
    fn parse_gate_options_defaults() {
        let options = GateOptions::parse(&[], "complexity-gate").expect("parse succeeds");
        assert_eq!(options.base, "origin/main");
        assert_eq!(options.config, None);
    }

    #[test]
    fn parse_gate_options_explicit_base_and_config() {
        let options = GateOptions::parse(
            &[
                "--base".into(),
                "main".into(),
                "--config".into(),
                "gates.toml".into(),
            ],
            "duplication-gate",
        )
        .expect("parse succeeds");
        assert_eq!(options.base, "main");
        assert_eq!(options.config, Some(PathBuf::from("gates.toml")));
    }

    #[test]
    fn parse_gate_options_rejects_unknown_option() {
        let error = GateOptions::parse(&["--nope".into()], "complexity-gate")
            .expect_err("rejects unknown option");
        assert!(error.contains("complexity-gate"));
        assert!(error.contains("--nope"));
    }

    #[test]
    fn violations_message_lists_each_violation_indented() {
        let message = violations_message(
            "complexity-gate: 2 function(s) over the limit:".to_owned(),
            &[
                "a.rs:1-2 f is new at 21 (limit 20)".to_owned(),
                "b.rs:3-4 g is new at 30 (limit 20)".to_owned(),
            ],
        );
        assert_eq!(
            message,
            "complexity-gate: 2 function(s) over the limit:\n  a.rs:1-2 f is new at 21 (limit 20)\n  b.rs:3-4 g is new at 30 (limit 20)"
        );
    }

    /// A fresh, never-reused scratch directory for a single test.
    ///
    /// The wall-clock timestamp alone is not a reliable uniqueness key: `cargo
    /// test` runs test functions on a thread pool, and a clock whose tick is
    /// coarser than the time between two threads calling `SystemTime::now()`
    /// hands them the same nanosecond count. That collision is rare enough to
    /// hide in a small suite but turned up routinely once enough tests here
    /// used this helper -- two tests silently shared one directory and
    /// stomped on each other's fixture files, failing whichever ran second,
    /// nondeterministically and on whichever test happened to lose the race.
    /// A monotonic in-process counter guarantees uniqueness regardless of
    /// clock resolution; the timestamp stays only to make directories sort
    /// and read chronologically.
    fn unique_temp_dir() -> PathBuf {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-output/xtask")
            .join(format!("cranpose-xtask-test-{nanos}-{sequence}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}

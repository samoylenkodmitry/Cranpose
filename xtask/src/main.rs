#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports on stdout and stderr"
)]

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
        run: |args| no_options("versions", args).and_then(|()| check_versions()),
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
        run: |args| no_options("publish-order", args).and_then(|()| publish_order()),
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
        run: |args| workspace_check("robot-suite-partition", args, robot_suite_partition::run_at),
        print_usage: print_robot_suite_partition_usage,
    },
    XtaskCommand {
        name: "ci-gate-reachability",
        run: |args| workspace_check("ci-gate-reachability", args, ci_gate_reachability::run_at),
        print_usage: print_ci_gate_reachability_usage,
    },
    XtaskCommand {
        name: "test-layout",
        run: |args| workspace_check("test-layout", args, test_layout::run_at),
        print_usage: print_test_layout_usage,
    },
    XtaskCommand {
        name: "thread-local-budget",
        run: |args| workspace_check("thread-local-budget", args, thread_local_budget::run_at),
        print_usage: print_thread_local_budget_usage,
    },
    XtaskCommand {
        name: "platform-contracts",
        run: |args| workspace_check("platform-contracts", args, platform_contracts),
        print_usage: print_platform_contracts_usage,
    },
];

fn no_options(name: &str, args: &[String]) -> Result<(), String> {
    match args.first() {
        Some(extra) => Err(format!("unknown {name} option `{extra}`")),
        None => Ok(()),
    }
}

// A check that takes no option runs on the workspace root.
fn workspace_check(
    name: &str,
    args: &[String],
    check: fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    no_options(name, args)?;
    check(&workspace_root()?)
}

fn platform_contracts(root: &Path) -> Result<(), String> {
    let mut problems = workflow_contracts::problems(root)?;
    problems.extend(android_contracts::problems(root)?);
    if problems.is_empty() {
        println!("platform-contracts: the workflows and the Android sources keep their contracts");
        return Ok(());
    }
    Err(format!("platform-contracts:\n  {}", problems.join("\n  ")))
}

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
           complexity-gate       Diff-scoped complexity and function length ceilings\n\
           duplication-gate      Diff-scoped copy-paste budget\n\
           ci-gate-reachability  Every `just` recipe CI runs must be reachable from `ci`/`ci-full`\n\
           test-layout          Every integration test file and robot runner must be linked into its binary\n\
           thread-local-budget  Framework `thread_local!` statics must stay at their budget\n\
           platform-contracts   CI workflows and Android sources keep the contracts past failures set\n\
         \n\
         bundle-macos options:\n\
           --package <name>       Cargo package to build [desktop-app]\n\
           --bin <name>           Cargo binary to bundle [desktop-app]\n\
           --profile <name>       Cargo profile to build [release]\n\
           --app-name <name>      macOS app bundle name [Cranpose Demo]\n\
           --bundle-id <id>       CFBundleIdentifier [io.cranpose.demo]\n\
           --out-dir <path>       Bundle output directory [target/macos-bundles]\n\
           --resources <path>     Directory copied into Contents/Resources\n\
           --icon <path>          .icns file shown as the app icon [demo icon]\n\
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
         in Cargo.lock, to <tag>. The tag must have the shape\n\
         v<major>.<minor>.<patch>, as in v0.1.132; the 'v' is stripped and\n\
         any other shape is refused. Rewrites the files in place with\n\
         line-level text edits, so unrelated formatting and comments are\n\
         left untouched."
    );
}

fn print_verify_tag_usage() {
    eprintln!(
        "usage: cargo xtask verify-tag <tag>\n\
         \n\
         Checks that <tag> has the shape v<major>.<minor>.<patch>, as in\n\
         v0.1.132, that it matches\n\
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
                    config = Some(PathBuf::from(required_value(args, &mut index, "--config")?));
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
         Fails if a function the diff adds or modifies is over the configured\n\
         cyclomatic complexity or lines of code and grew past its old value\n\
         (new code is judged against the limit alone).\n\
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

fn print_platform_contracts_usage() {
    eprintln!(
        "usage: cargo xtask platform-contracts\n\
         \n\
         Parses the workflows under .github/workflows and the Android manifests, and\n\
         reads the Java bridges, for the rules that each guard a past failure."
    );
}

fn print_thread_local_budget_usage() {
    eprintln!(
        "usage: cargo xtask thread-local-budget\n\
         \n\
         Counts the statics declared in `thread_local!` blocks in the sources of\n\
         the workspace crates under crates/, tests excluded, and fails when the\n\
         total differs from the budget. A name declared twice in one file, as\n\
         cfg alternatives are, counts once. On Android every such static takes\n\
         one of the process's pthread keys for the rest of its life."
    );
}

fn print_test_layout_usage() {
    eprintln!(
        "usage: cargo xtask test-layout\n\
         \n\
         Fails if a crate that links its integration tests into one binary\n\
         (`autotests = false`) has a `tests/*.rs` file that is not a module of\n\
         its `tests/integration.rs`, or a `tests/integration.rs` that is not a\n\
         `[[test]]` target of its Cargo.toml: cargo would compile and run\n\
         nothing for it."
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
        family: "bitflags",
        reason: "Wry 0.57 requires webkit2gtk 2 and javascriptcore-rs 1, which use bitflags 1 while the workspace uses 2",
    },
    DuplicateDebt {
        family: "heck",
        reason: "Wry's GTK 3 stack uses glib-macros 0.18 with heck 0.4 while the workspace uses 0.5",
    },
    DuplicateDebt {
        family: "proc-macro-crate",
        reason: "Wry's GTK 3 stack requires gtk3-macros 0.18 and glib-macros 0.18, which use proc-macro-crate 1 and 2 while the workspace uses 3",
    },
    DuplicateDebt {
        family: "serde_spanned",
        reason: "Wry's GTK 3 system-deps 6 requires toml 0.8 and serde_spanned 0.6 while the workspace uses toml 1 and serde_spanned 1",
    },
    DuplicateDebt {
        family: "toml",
        reason: "Wry's GTK 3 sys crates require system-deps 6 with toml 0.8 while the workspace uses toml 1",
    },
    DuplicateDebt {
        family: "toml_datetime",
        reason: "Follows the GTK 3 toml 0.8 and proc-macro-crate 1/2 split",
    },
    DuplicateDebt {
        family: "toml_edit",
        reason: "GTK 3's proc-macro-crate 1/2 and toml 0.8 require toml_edit 0.19/0.20 while proc-macro-crate 3 uses 0.25",
    },
    DuplicateDebt {
        family: "winnow",
        reason: "Follows the GTK 3 toml_edit 0.19/0.20 split via winnow 0.5",
    },
    DuplicateDebt {
        family: "hashbrown",
        reason: "gpu-allocator 0.28.0 (latest, and on upstream main) holds ^0.16 for wgpu-hal 30 while wgpu, AccessKit and indexmap are on ^0.17",
    },
    DuplicateDebt {
        family: "jni-sys",
        reason: "ndk 0.9.0 and ndk-sys 0.6.0 (latest) pin jni-sys ^0.3 while jni 0.22 is on ^0.4",
    },
    DuplicateDebt {
        family: "miniz_oxide",
        reason: "flate2 1.1.10 moved to ^0.9 while png 0.18.1 (latest) is still on ^0.8",
    },
    DuplicateDebt {
        family: "objc2",
        reason: "accesskit_macos 0.27 holds objc2 0.5 while winit-appkit 0.31.0-beta.3 is on 0.6 (AccessKit/accesskit#616)",
    },
    DuplicateDebt {
        family: "objc2-app-kit",
        reason: "follows the objc2 split via accesskit_macos 0.27",
    },
    DuplicateDebt {
        family: "objc2-foundation",
        reason: "follows the objc2 split via accesskit_macos 0.27",
    },
    DuplicateDebt {
        family: "syn",
        reason: "serde_derive, thiserror-impl 2, bytemuck_derive, wasm-bindgen-macro and async-recursion use syn 3 while jni-macros, num_enum_derive and zerocopy-derive use syn 2",
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
        family: "rustc-hash",
        reason: "fluent-bundle 0.16.0 and type-map 0.5.1 require rustc-hash 2.x (Askama 0.16 in UniFFI's all-features graph also requires 2.x), while wgpu-core 30.0.7, naga 30.0.1, and naga-types 30.0.1 require 1.1.x; unifying these incompatible major-version constraints would require changing upstream dependencies",
    },
];

const ALL_FEATURES_EXTRA_DUPLICATE_DEBT: &[DuplicateDebt] = &[DuplicateDebt {
    family: "env_filter",
    reason: "android_logger 0.15.1 (latest) pins env_filter ^0.1 while env_logger 0.11 is past 1.0",
}];

/// Upstream wgpu and Cranpose's fork of it (forks/README.md) alike.
const RENDERER_PIXELS_FORBIDDEN_PACKAGES: &[&str] = &[
    "pixels",
    "wgpu",
    "wgpu-core",
    "wgpu-hal",
    "cranpose-wgpu",
    "cranpose-wgpu-core",
    "cranpose-wgpu-hal",
    "naga",
];

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
    icon: PathBuf,
    target: Option<String>,
    build: bool,
    sign_identity: Option<String>,
}

/// The demo's macOS icon, rendered from its SVG by `scripts/dev/render_app_icons.mjs`.
const DEMO_MACOS_ICON: &str = "apps/desktop-demo/assets/icon/CranposeDemo.icns";

/// The name the icon takes inside `Contents/Resources`, without its extension
/// as `CFBundleIconFile` names it.
const BUNDLE_ICON_NAME: &str = "AppIcon";

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
                    options.binary.package = required_value(args, &mut index, "--package")?;
                }
                "--bin" => options.binary.bin = required_value(args, &mut index, "--bin")?,
                "--profile" => {
                    options.binary.profile = required_value(args, &mut index, "--profile")?;
                }
                "--target" => {
                    options.binary.target = Some(required_value(args, &mut index, "--target")?);
                }
                "--manifest-path" => {
                    options.binary.manifest_path = Some(PathBuf::from(required_value(
                        args,
                        &mut index,
                        "--manifest-path",
                    )?));
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
            icon: PathBuf::from(DEMO_MACOS_ICON),
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
                    options.bundle_id = required_value(args, &mut index, "--bundle-id")?;
                }
                "--out-dir" => {
                    options.out_dir = PathBuf::from(required_value(args, &mut index, "--out-dir")?);
                }
                "--resources" => {
                    options.resources = Some(PathBuf::from(required_value(
                        args,
                        &mut index,
                        "--resources",
                    )?));
                }
                "--icon" => {
                    options.icon = PathBuf::from(required_value(args, &mut index, "--icon")?);
                }
                "--target" => options.target = Some(required_value(args, &mut index, "--target")?),
                "--no-build" => options.build = false,
                "--sign-identity" => {
                    options.sign_identity =
                        Some(required_value(args, &mut index, "--sign-identity")?);
                }
                other => return Err(format!("unknown bundle-macos option `{other}`")),
            }
            index += 1;
        }

        options.validate()?;
        Ok(options)
    }

    fn validate(&self) -> Result<(), String> {
        validate_bundle_id(&self.bundle_id)?;
        validate_non_empty("package", &self.package)?;
        validate_non_empty("bin", &self.bin)?;
        validate_non_empty("profile", &self.profile)?;
        validate_non_empty("app-name", &self.app_name)?;
        if self
            .icon
            .extension()
            .is_some_and(|extension| extension == "icns")
        {
            Ok(())
        } else {
            Err(format!(
                "icon `{}` is not an .icns file",
                self.icon.display()
            ))
        }
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

    let binary = built_binary_path(&workspace, &binary_options)?;
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
    let binary = built_binary_path(workspace, binary_options)?;
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
                    options.binary.package = required_value(args, &mut index, "--package")?;
                }
                "--bin" => options.binary.bin = required_value(args, &mut index, "--bin")?,
                "--profile" => {
                    options.binary.profile = required_value(args, &mut index, "--profile")?;
                }
                "--target" => {
                    options.binary.target = Some(required_value(args, &mut index, "--target")?);
                }
                "--manifest-path" => {
                    options.binary.manifest_path = Some(PathBuf::from(required_value(
                        args,
                        &mut index,
                        "--manifest-path",
                    )?));
                }
                "--max-bytes" => {
                    let value = required_value(args, &mut index, "--max-bytes")?;
                    options.max_bytes = Some(parse_u64_option("--max-bytes", &value)?);
                }
                "--patch-workspace-cranpose" => {
                    options.binary.patch_workspace_cranpose = true;
                }
                "--features" => {
                    options.features = Some(required_value(args, &mut index, "--features")?);
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
        add_workspace_cranpose_patches(&mut command, &workspace)?;
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

    let mut rustflags = env::var("RUSTFLAGS").unwrap_or_default();
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
        add_workspace_cranpose_patches(&mut command, workspace)?;
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

fn add_workspace_cranpose_patches(command: &mut Command, workspace: &Path) -> Result<(), String> {
    let manifest = load_toml(&workspace.join("Cargo.toml"))?;
    let dependencies = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "workspace manifest has no [workspace.dependencies] table".to_owned())?;
    // A renamed dependency (`wgpu = { package = "cranpose-wgpu", ... }`)
    // patches its package. The workspace's own patches, such as the wgpu
    // fork's, carry over as they are.
    let path_dependencies = dependencies.iter().map(|(dependency, spec)| {
        let package = spec
            .get("package")
            .and_then(toml::Value::as_str)
            .unwrap_or(dependency);
        (package, spec)
    });
    let workspace_patches = manifest
        .get("patch")
        .and_then(|patch| patch.get("crates-io"))
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .map(|(package, spec)| (package.as_str(), spec));

    for (package, spec) in path_dependencies.chain(workspace_patches) {
        let Some(relative_path) = spec.get("path").and_then(toml::Value::as_str) else {
            continue;
        };
        let package_path = workspace.join(relative_path);
        command.arg("--config").arg(format!(
            "patch.crates-io.{package}.path=\"{}\"",
            escape_toml_string(&package_path.display().to_string())
        ));
    }
    Ok(())
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

fn built_binary_path(workspace: &Path, options: &CargoBinaryOptions) -> Result<PathBuf, String> {
    let mut path = cargo_target_root(workspace, options)?;
    if let Some(target) = options.target.as_deref() {
        path.push(target);
    }
    path.push(cargo_profile_dir(&options.profile));
    path.push(binary_file_name(&options.bin));
    Ok(path)
}

fn cargo_target_root(workspace: &Path, options: &CargoBinaryOptions) -> Result<PathBuf, String> {
    let manifest = match options.manifest_path.as_deref() {
        Some(path) => absolute_path(
            &env::current_dir()
                .map_err(|error| format!("failed to resolve working directory: {error}"))?,
            path,
        ),
        None => workspace.join("Cargo.toml"),
    };
    let metadata = cargo_metadata_json(Some(&manifest))?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata)
        .map_err(|error| format!("cargo metadata returned invalid JSON: {error}"))?;
    metadata["target_directory"]
        .as_str()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata did not include target_directory".to_owned())
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
    copy_file(
        &workspace.join(&options.icon),
        &resources.join(format!("{BUNDLE_ICON_NAME}.icns")),
    )?;
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
  <key>CFBundleIconFile</key>
  <string>{BUNDLE_ICON_NAME}</string>
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
            "renderer-pixels feature boundary ok: no external pixels, wgpu (upstream or the fork), wgpu-core, wgpu-hal, or naga packages"
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
        .map_or(line.len(), |offset| version_start + offset);
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

/// One `[[package]]` entry from a Cargo.lock.
struct LockPackage {
    name: String,
    version: Option<String>,
    source: Option<String>,
    checksum: Option<String>,
}

fn lock_packages(lockfile: &toml::Value, keep: impl Fn(&str) -> bool) -> Vec<LockPackage> {
    let Some(packages) = lockfile.get("package").and_then(toml::Value::as_array) else {
        return Vec::new();
    };

    packages
        .iter()
        .filter_map(toml::Value::as_table)
        .filter_map(|package| {
            let name = package.get("name")?.as_str()?;
            if !keep(name) {
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

/// Whether `name` is released in lockstep with Cranpose: every `cranpose*`
/// crate, and `coroflow`, the runtime-independent coroutine library that
/// `cranpose-coroflow` adapts. [`RELEASE_CRATE_PATTERN`] matches the same
/// names inside a manifest.
fn is_release_crate(name: &str) -> bool {
    (name.starts_with("cranpose") && !FORKED_CRATES.contains(&name)) || name == "coroflow"
}

/// Upstream crates Cranpose ships patched under its own names
/// (forks/README.md). They keep the upstream version and publish ahead of
/// the release crates instead of in lockstep with them.
const FORKED_CRATES: &[&str] = &[
    "cranpose-wgpu",
    "cranpose-wgpu-core",
    "cranpose-wgpu-core-deps-apple",
    "cranpose-wgpu-core-deps-emscripten",
    "cranpose-wgpu-core-deps-wasm",
    "cranpose-wgpu-core-deps-windows-linux-android",
    "cranpose-wgpu-hal",
];

/// The names [`is_release_crate`] accepts, as a regex fragment.
const RELEASE_CRATE_PATTERN: &str = r"(?:cranpose[\w-]*|coroflow)";

fn sorted_release_dependencies(table: &toml::Table) -> Vec<(&String, &toml::Value)> {
    let mut entries = table
        .iter()
        .filter(|(name, _)| is_release_crate(name))
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
    let packages = lock_packages(&lockfile, is_release_crate);
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
    check_member_manifest_versions(root, &mut failures);
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
    for (name, spec) in sorted_release_dependencies(dependencies) {
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
    let versioned = workspace_versioned_packages(root);
    let root_versions = lock_versions(&lock_packages(&root_lock, |name| {
        follows_workspace_version(name, &versioned)
    }));
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

/// A workspace member takes every cranpose crate from the workspace
/// dependency table. A version of its own does not move when a release bumps
/// the workspace: v0.9.0 left `cranpose`'s `cranpose-macros = "0.1.164"`
/// behind, which no 0.9 crate satisfies, so the release commit broke main.
fn check_member_manifest_versions(root: &Path, failures: &mut Vec<String>) {
    for member in test_layout::workspace_members(root).unwrap_or_default() {
        let Ok(manifest) = load_toml(&root.join(&member).join("Cargo.toml")) else {
            continue;
        };
        for table in all_dependency_tables(&manifest) {
            for (name, spec) in sorted_release_dependencies(table) {
                if let Some(version) = dependency_version(spec) {
                    failures.push(format!(
                        "{member}/Cargo.toml pins {name} at {version}; take it from the workspace with `{name}.workspace = true`"
                    ));
                }
            }
        }
    }
}

/// Every dependency table of `manifest`: normal, dev and build, top-level and
/// under each `[target.'cfg(..)']`.
fn all_dependency_tables(manifest: &toml::Value) -> Vec<&toml::Table> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let targets = manifest
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|targets| targets.values());
    std::iter::once(manifest)
        .chain(targets)
        .flat_map(|scope| KINDS.iter().filter_map(|kind| scope.get(*kind)?.as_table()))
        .collect()
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
        for (name, spec) in sorted_release_dependencies(table) {
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
    Regex::new(&format!(
        r#"(?m)(?P<head>^[ \t]*{RELEASE_CRATE_PATTERN}[ \t]*=[ \t]*\{{[^}}\n]*?version[ \t]*=[ \t]*")(?P<version>[^"]+)(?P<tail>")"#,
    ))
    .expect("INLINE_TABLE_RE is a valid pattern")
});

/// `cranpose[-foo] = "x"` (bare string) in the same places.
static BARE_STRING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?m)(?P<head>^[ \t]*{RELEASE_CRATE_PATTERN}[ \t]*=[ \t]*")(?P<version>[^"]+)(?P<tail>")"#
    ))
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
static RELEASE_DEP_TABLE_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^\s*{RELEASE_CRATE_PATTERN}\s*=\s*\{{"))
        .expect("RELEASE_DEP_TABLE_LINE_RE is valid")
});
static VERSION_KV_PRESENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"version\s*=\s*"[^"]+""#).expect("VERSION_KV_PRESENT_RE is valid")
});
static VERSION_KV_REPLACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(version\s*=\s*")[^"]+("\s*)"#).expect("VERSION_KV_REPLACE_RE is valid")
});

fn leading_whitespace(line: &str) -> &str {
    LEADING_WHITESPACE_RE.find(line).map_or("", |m| m.as_str())
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
        "license = \"Apache-2.0 OR MIT\"\n".to_owned(),
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

/// Rewrites every `{release crate} = { ... version = "x" ... }` line in
/// `[workspace.dependencies]` ([`is_release_crate`]) to `version`, leaving the rest of each line
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
        if !RELEASE_DEP_TABLE_LINE_RE.is_match(line) {
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
            "Some release workspace dependencies were not updated:\n{}",
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

/// The workspace members whose manifest says `version.workspace = true`.
/// A member whose manifest cannot be read is left out.
fn workspace_versioned_packages(root: &Path) -> BTreeSet<String> {
    test_layout::workspace_members(root)
        .unwrap_or_default()
        .iter()
        .filter_map(|member| load_toml(&root.join(member).join("Cargo.toml")).ok())
        .filter_map(|manifest| {
            let package = manifest.get("package")?.as_table()?;
            let inherits = package
                .get("version")
                .and_then(toml::Value::as_table)
                .and_then(|version| version.get("workspace"))
                .and_then(toml::Value::as_bool)
                == Some(true);
            inherits
                .then(|| package.get("name").and_then(toml::Value::as_str))
                .flatten()
                .map(str::to_owned)
        })
        .collect()
}

/// Whether the root `Cargo.lock` records `name` at the workspace version: a
/// cranpose crate, or a member that inherits the workspace version.
fn follows_workspace_version(name: &str, versioned: &BTreeSet<String>) -> bool {
    is_release_crate(name) || versioned.contains(name)
}

/// Bumps every `[[package]]` in `path` (a `Cargo.lock`) that
/// [`follows_workspace_version`] to `version`, leaving `source`, `checksum`
/// and `dependencies` lines alone. Always rewrites the file (matching the
/// release script this replaces), even when no line actually changed.
fn bump_cargo_lock_version(
    path: &Path,
    version: &str,
    versioned: &BTreeSet<String>,
) -> Result<(), String> {
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
            update_version = follows_workspace_version(name, versioned);
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

const RELEASE_TAG_SHAPE: &str = "v<major>.<minor>.<patch>, as in v0.1.132";

fn is_release_number(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
}

fn release_version_from_tag(tag: &str) -> Result<&str, String> {
    if let Some(version) = tag.strip_prefix('v') {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() == 3 && parts.iter().copied().all(is_release_number) {
            return Ok(version);
        }
    }
    Err(format!(
        "Malformed release tag '{tag}': expected the shape {RELEASE_TAG_SHAPE}"
    ))
}

fn bump_release_version_at(root: &Path, tag: &str) -> Result<(), String> {
    let version = release_version_from_tag(tag)?;
    bump_cargo_toml_version(&root.join("Cargo.toml"), version)?;
    let versioned = workspace_versioned_packages(root);
    bump_cargo_lock_version(&root.join("Cargo.lock"), version, &versioned)?;
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
    let tag_version = release_version_from_tag(tag)?;

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
        for (name, spec) in sorted_release_dependencies(dependencies) {
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
    let unpublished = package
        .get("publish")
        .and_then(serde_json::Value::as_array)
        .is_some_and(Vec::is_empty);
    if !is_release_crate(name) || unpublished {
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
    let [crate_name, version] = args else {
        eprintln!("usage: cargo xtask crate-published <crate> <version>");
        return ExitCode::from(2);
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

mod android_contracts;
mod complexity_gate;
mod duplication_gate;
mod test_layout;
mod thread_local_budget;
mod workflow_contracts;

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

    pub(crate) fn renamed_paths_from_status(status: &str) -> BTreeMap<String, String> {
        status
            .lines()
            .filter_map(|line| {
                let mut columns = line.split('\t');
                let kind = columns.next()?;
                if !kind.starts_with('R') {
                    return None;
                }
                let old = columns.next()?;
                let new = columns.next()?;
                Some((new.to_owned(), old.to_owned()))
            })
            .collect()
    }

    pub(crate) fn renamed_paths(
        root: &Path,
        base: &str,
        pathspecs: &[String],
    ) -> Result<BTreeMap<String, String>, String> {
        let output = Command::new("git")
            .args(["diff", "--name-status", "--find-renames=40%", base, "--"])
            .args(pathspecs)
            .current_dir(root)
            .output()
            .map_err(|error| format!("gate_diff: failed to find renamed paths: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        Ok(renamed_paths_from_status(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

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

    /// Upstream code Cranpose ships patched (forks/README.md) keeps its own
    /// shape; the diff gates judge only Cranpose's code.
    const FORKS_PATHSPEC: &str = ":(exclude)forks";

    pub(crate) fn changed_ranges(
        root: &Path,
        base: &str,
        pathspec: &str,
    ) -> Result<RangesByFile, String> {
        let base_sha = merge_base(root, base)?;
        let old_paths = renamed_paths(
            root,
            &base_sha,
            &[pathspec.to_owned(), FORKS_PATHSPEC.to_owned()],
        )?;
        let output = Command::new("git")
            .args([
                "diff",
                "--find-renames=40%",
                "--unified=0",
                "--no-prefix",
                &base_sha,
                "--",
                pathspec,
                FORKS_PATHSPEC,
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
            let old_path = old_paths.get(file).map_or(file, String::as_str);
            let blob = Command::new("git")
                .args(["show", &format!("{base_sha}:{old_path}")])
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
        // A pathspec containing only the renamed destination can make Git omit
        // the old name. Compare rename status across the tree, then reuse only
        // mappings that match the files being analyzed.
        let old_paths = renamed_paths(root, base_sha, &[])?;
        let mut written = Vec::new();
        for file in files {
            let old_path = old_paths.get(file).map_or(file.as_str(), String::as_str);
            let blob = Command::new("git")
                .args(["show", &format!("{base_sha}:{old_path}")])
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
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
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
            && fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
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
mod ci_gate_reachability;

/// CI runs the robot suite as two halves: `robot-gpu` under a hardware
/// swapchain with a handful of examples skipped, and `robot-captures` under
/// software present running exactly those. Nothing enforced that the two
/// lists agree, so an example removed from one half and not added to the
/// other would stop running anywhere without a gate going red -- the same
/// shape as a hand-maintained list standing in for a property.
mod robot_suite_partition;

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
#[path = "tests/main_robot_suite_partition_tests.rs"]
mod robot_suite_partition_tests;

#[cfg(test)]
#[path = "tests/main_ci_gate_reachability_tests.rs"]
mod ci_gate_reachability_tests;

#[cfg(test)]
#[path = "tests/main_tests.rs"]
mod tests;

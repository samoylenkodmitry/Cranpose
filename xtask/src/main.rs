#![deny(unsafe_code)]

#[cfg(test)]
use std::io;
use std::{
    collections::BTreeMap,
    env, fs,
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
           --sign-identity <id>   Run codesign with this identity"
    );
}

const WORKSPACE_ALLOWED_DUPLICATE_PACKAGES: &[&str] = &[];

const ALL_FEATURES_ALLOWED_DUPLICATE_PACKAGES: &[&str] = &[];

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

    fn cargo_tree_args(self) -> &'static [&'static str] {
        match self {
            Self::Workspace => &["tree", "--duplicates", "--workspace"],
            Self::AllFeatures => &["tree", "--duplicates", "--workspace", "--all-features"],
        }
    }

    fn allowed_duplicate_packages(self) -> &'static [&'static str] {
        match self {
            Self::Workspace => WORKSPACE_ALLOWED_DUPLICATE_PACKAGES,
            Self::AllFeatures => ALL_FEATURES_ALLOWED_DUPLICATE_PACKAGES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DependencyBudgetSlice {
    WgpuStack,
    DesktopPlatform,
    OptionalFeatures,
}

impl DependencyBudgetSlice {
    const ALL: [Self; 3] = [
        Self::WgpuStack,
        Self::DesktopPlatform,
        Self::OptionalFeatures,
    ];

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "wgpu-stack" => Ok(Self::WgpuStack),
            "desktop-platform" => Ok(Self::DesktopPlatform),
            "optional-features" => Ok(Self::OptionalFeatures),
            other => Err(format!("unknown dependency-budget slice `{other}`")),
        }
    }

    fn target_families(self) -> &'static [&'static str] {
        match self {
            Self::WgpuStack => &["foldhash", "hashbrown"],
            Self::DesktopPlatform => &["tiny-skia", "tiny-skia-path"],
            Self::OptionalFeatures => {
                &["async-channel", "event-listener", "getrandom", "roxmltree"]
            }
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::WgpuStack => "wgpu-stack",
            Self::DesktopPlatform => "desktop-platform",
            Self::OptionalFeatures => "optional-features",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DependencyBudgetOptions {
    scopes: Vec<DependencyBudgetScope>,
    explain: bool,
    strict: bool,
    slice: Option<DependencyBudgetSlice>,
}

impl DependencyBudgetOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut scopes = Vec::new();
        let mut explain = false;
        let mut strict = false;
        let mut slice = None;

        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--explain" => explain = true,
                "--strict" => strict = true,
                "--slice" => {
                    let value = required_value(args, &mut index, "--slice")?;
                    slice = Some(DependencyBudgetSlice::parse(&value)?);
                }
                value if value.starts_with("--slice=") => {
                    let value = value.trim_start_matches("--slice=");
                    slice = Some(DependencyBudgetSlice::parse(value)?);
                }
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
            index += 1;
        }

        if slice.is_some() && !strict {
            return Err("--slice requires --strict".to_owned());
        }
        if scopes.is_empty() {
            scopes.push(DependencyBudgetScope::Workspace);
            scopes.push(DependencyBudgetScope::AllFeatures);
        }

        Ok(Self {
            scopes,
            explain,
            strict,
            slice,
        })
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

fn print_dependency_budget_usage() {
    eprintln!(
        "usage: cargo xtask dependency-budget [--workspace-only | --all-features-only] [--explain] [--strict] [--slice wgpu-stack|desktop-platform|optional-features]"
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
    if let Some(identity) = options.sign_identity.as_deref() {
        sign_bundle(&bundle, identity)?;
    }

    println!("macOS app bundle: {}", bundle.display());
    Ok(())
}

fn report_binary_size(options: BinarySizeOptions) -> Result<(), String> {
    let workspace = workspace_root()?;
    if options.build {
        build_binary(&workspace, &options.binary)?;
    }

    let binary = built_binary_path(&workspace, &options.binary);
    let metadata = fs::metadata(&binary)
        .map_err(|error| format!("failed to inspect `{}`: {error}", binary.display()))?;
    let bytes = metadata.len();
    println!(
        "{} {}:{} {} bytes ({:.2} MiB)",
        options.binary.profile,
        options.binary.package,
        options.binary.bin,
        bytes,
        bytes as f64 / (1024.0 * 1024.0)
    );
    if let Some(max_bytes) = options.max_bytes {
        if bytes > max_bytes {
            return Err(format!(
                "{} {}:{} size {} bytes exceeds max {} bytes",
                options.binary.profile,
                options.binary.package,
                options.binary.bin,
                bytes,
                max_bytes
            ));
        }
    }
    Ok(())
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
    let manifest_path = if manifest_path.is_absolute() {
        manifest_path.to_path_buf()
    } else {
        workspace.join(manifest_path)
    };
    manifest_path
        .parent()
        .map(|parent| parent.join("target"))
        .unwrap_or_else(|| workspace.join("target"))
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
        if let Err(error) =
            check_dependency_budget_scope(scope, options.explain, options.strict, options.slice)
        {
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
        ])
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
    strict: bool,
    slice: Option<DependencyBudgetSlice>,
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
    let duplicate_families = duplicate_version_families.clone();
    let strict_duplicate_families =
        select_strict_duplicate_families(&duplicate_version_families, slice);
    let allowed = scope.allowed_duplicate_packages();

    if let Some(error) = duplicate_budget_violation(
        scope,
        &duplicate_families,
        &strict_duplicate_families,
        allowed,
        strict,
    ) {
        if explain {
            let details_to_print = if strict {
                &strict_duplicate_families
            } else {
                &duplicate_families
            };
            print_duplicate_package_details(scope, &duplicate_details, details_to_print);
            let slice_status_families = if strict && slice.is_some() {
                &strict_duplicate_families
            } else {
                &duplicate_version_families
            };
            print_duplicate_version_slice_status(scope, slice_status_families);
        }
        Err(error)
    } else {
        print_dependency_budget_success(scope, strict, slice, &duplicate_families);
        if explain {
            let details_to_print = if strict {
                &strict_duplicate_families
            } else {
                &duplicate_families
            };
            print_duplicate_package_details(scope, &duplicate_details, details_to_print);
            let slice_status_families = if strict && slice.is_some() {
                &strict_duplicate_families
            } else {
                &duplicate_version_families
            };
            print_duplicate_version_slice_status(scope, slice_status_families);
        }
        Ok(())
    }
}

fn print_dependency_budget_success(
    scope: DependencyBudgetScope,
    strict: bool,
    slice: Option<DependencyBudgetSlice>,
    duplicate_families: &[String],
) {
    if strict {
        match slice {
            Some(slice) => println!(
                "strict duplicate dependency version slice ok ({}, {}): none",
                scope.label(),
                slice.label()
            ),
            None => println!(
                "strict duplicate dependency version check ok ({}): none",
                scope.label()
            ),
        }
    } else {
        println!(
            "duplicate dependency version budget ok ({}): {}",
            scope.label(),
            duplicate_families.join(", ")
        );
    }
}

fn duplicate_budget_violation(
    scope: DependencyBudgetScope,
    duplicate_families: &[String],
    duplicate_version_families: &[String],
    allowed: &[&str],
    strict: bool,
) -> Option<String> {
    if strict {
        if duplicate_version_families.is_empty() {
            return None;
        }

        return Some(format!(
            "strict duplicate dependency version check failed for {}: {}",
            scope.label(),
            duplicate_version_families.join(", ")
        ));
    }

    let unexpected = duplicate_families
        .iter()
        .filter(|package| !allowed.contains(&package.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if unexpected.is_empty() {
        None
    } else {
        Some(format!(
            "unexpected duplicate dependency version families for {}: {}; allowed: {}",
            scope.label(),
            unexpected.join(", "),
            allowed.join(", ")
        ))
    }
}

fn select_strict_duplicate_families(
    duplicate_version_families: &[String],
    slice: Option<DependencyBudgetSlice>,
) -> Vec<String> {
    let Some(slice) = slice else {
        return duplicate_version_families.to_vec();
    };

    let targets = slice.target_families();
    duplicate_version_families
        .iter()
        .filter(|family| targets.contains(&family.as_str()))
        .cloned()
        .collect()
}

fn duplicate_version_slice_families(
    duplicate_version_families: &[String],
    slice: DependencyBudgetSlice,
) -> Vec<String> {
    let targets = slice.target_families();
    duplicate_version_families
        .iter()
        .filter(|family| targets.contains(&family.as_str()))
        .cloned()
        .collect()
}

fn unclassified_duplicate_version_families(duplicate_version_families: &[String]) -> Vec<String> {
    duplicate_version_families
        .iter()
        .filter(|family| {
            !DependencyBudgetSlice::ALL
                .iter()
                .any(|slice| slice.target_families().contains(&family.as_str()))
        })
        .cloned()
        .collect()
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

#[cfg(test)]
fn duplicate_package_families(cargo_tree: &str) -> Vec<String> {
    budget_duplicate_package_families(cargo_tree)
}

#[cfg(test)]
fn budget_duplicate_package_families(cargo_tree: &str) -> Vec<String> {
    let mut packages = cargo_tree
        .lines()
        .filter_map(budget_duplicate_package)
        .collect::<Vec<_>>();
    packages.sort();
    packages.dedup();
    packages
}

#[cfg(test)]
fn budget_duplicate_package(line: &str) -> Option<String> {
    if line.starts_with(char::is_whitespace)
        || line.starts_with('├')
        || line.starts_with('└')
        || line.starts_with('│')
        || line.trim().is_empty()
        || line.contains("(*)")
    {
        return None;
    }

    let version_index = line.find(" v")?;
    Some(line[..version_index].to_owned())
}

fn duplicate_package_details(cargo_tree: &str) -> Vec<DuplicatePackageFamily> {
    let mut roots_by_package = BTreeMap::<String, Vec<DuplicatePackageRoot>>::new();
    let mut current = None::<DuplicatePackageRoot>;

    for line in cargo_tree.lines() {
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
        .filter_map(package_name_from_cargo_tree_line)
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

fn print_duplicate_version_slice_status(
    scope: DependencyBudgetScope,
    duplicate_version_families: &[String],
) {
    println!("duplicate dependency version slices ({}):", scope.label());
    for slice in DependencyBudgetSlice::ALL {
        print_slice_status(
            slice.label(),
            &duplicate_version_slice_families(duplicate_version_families, slice),
        );
    }
    print_slice_status(
        "unclassified",
        &unclassified_duplicate_version_families(duplicate_version_families),
    );
}

fn print_slice_status(label: &str, families: &[String]) {
    if families.is_empty() {
        println!("  {label}: none");
    } else {
        println!("  {label}: {}", families.join(", "));
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
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
        assert!(!options.strict);
        assert_eq!(options.slice, None);
    }

    #[test]
    fn parse_dependency_budget_accepts_single_scope_options() {
        let workspace = DependencyBudgetOptions::parse(&["--workspace-only".into()])
            .expect("workspace-only options parse");
        assert_eq!(workspace.scopes, vec![DependencyBudgetScope::Workspace]);
        assert!(!workspace.explain);
        assert_eq!(workspace.slice, None);

        let all_features = DependencyBudgetOptions::parse(&["--all-features-only".into()])
            .expect("all-features-only options parse");
        assert_eq!(
            all_features.scopes,
            vec![DependencyBudgetScope::AllFeatures]
        );
        assert!(!all_features.explain);
        assert_eq!(all_features.slice, None);
    }

    #[test]
    fn parse_dependency_budget_accepts_explain_with_scope() {
        let options =
            DependencyBudgetOptions::parse(&["--workspace-only".into(), "--explain".into()])
                .expect("dependency-budget explain options parse");

        assert_eq!(options.scopes, vec![DependencyBudgetScope::Workspace]);
        assert!(options.explain);
        assert!(!options.strict);
        assert_eq!(options.slice, None);
    }

    #[test]
    fn parse_dependency_budget_accepts_strict_with_explain() {
        let options = DependencyBudgetOptions::parse(&[
            "--all-features-only".into(),
            "--explain".into(),
            "--strict".into(),
        ])
        .expect("dependency-budget strict options parse");

        assert_eq!(options.scopes, vec![DependencyBudgetScope::AllFeatures]);
        assert!(options.explain);
        assert!(options.strict);
        assert_eq!(options.slice, None);
    }

    #[test]
    fn parse_dependency_budget_accepts_strict_slice() {
        let options = DependencyBudgetOptions::parse(&[
            "--strict".into(),
            "--slice".into(),
            "wgpu-stack".into(),
        ])
        .expect("dependency-budget strict slice options parse");

        assert!(options.strict);
        assert_eq!(options.slice, Some(DependencyBudgetSlice::WgpuStack));
    }

    #[test]
    fn parse_dependency_budget_accepts_equals_strict_slice() {
        let options =
            DependencyBudgetOptions::parse(&["--strict".into(), "--slice=desktop-platform".into()])
                .expect("dependency-budget strict equals slice options parse");

        assert!(options.strict);
        assert_eq!(options.slice, Some(DependencyBudgetSlice::DesktopPlatform));
    }

    #[test]
    fn parse_dependency_budget_rejects_slice_without_strict() {
        let error = DependencyBudgetOptions::parse(&["--slice".into(), "wgpu-stack".into()])
            .expect_err("slice without strict should fail");

        assert!(error.contains("--slice requires --strict"));
    }

    #[test]
    fn parse_dependency_budget_rejects_unknown_slice() {
        let error = DependencyBudgetOptions::parse(&["--strict".into(), "--slice=unknown".into()])
            .expect_err("unknown slice should fail");

        assert!(error.contains("unknown dependency-budget slice"));
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

        assert_eq!(
            duplicate_package_families(tree),
            vec!["hashbrown".to_owned()]
        );
    }

    #[test]
    fn duplicate_budget_violation_rejects_duplicate_families_by_default() {
        let families = vec!["foldhash".to_owned(), "hashbrown".to_owned()];

        let violation = duplicate_budget_violation(
            DependencyBudgetScope::Workspace,
            &families,
            &[],
            WORKSPACE_ALLOWED_DUPLICATE_PACKAGES,
            false,
        )
        .expect("duplicate-version families should fail the default budget");

        assert!(violation.contains(
            "unexpected duplicate dependency version families for workspace: foldhash, hashbrown;"
        ));
    }

    #[test]
    fn duplicate_budget_violation_rejects_unbudgeted_families_by_default() {
        let families = vec!["hashbrown".to_owned(), "new-family".to_owned()];

        let violation = duplicate_budget_violation(
            DependencyBudgetScope::Workspace,
            &families,
            &[],
            WORKSPACE_ALLOWED_DUPLICATE_PACKAGES,
            false,
        )
        .expect("unbudgeted duplicate-version family should fail");

        assert!(violation.contains(
            "unexpected duplicate dependency version families for workspace: hashbrown, new-family;"
        ));
    }

    #[test]
    fn duplicate_budget_violation_rejects_version_families_in_strict_mode() {
        let families = vec!["foldhash".to_owned(), "hashbrown".to_owned()];
        let version_families = vec!["hashbrown".to_owned()];

        let violation = duplicate_budget_violation(
            DependencyBudgetScope::Workspace,
            &families,
            &version_families,
            WORKSPACE_ALLOWED_DUPLICATE_PACKAGES,
            true,
        )
        .expect("strict duplicate check should fail");

        assert!(violation.contains("hashbrown"));
        assert!(!violation.contains("foldhash"));
    }

    #[test]
    fn duplicate_budget_violation_accepts_empty_strict_mode() {
        let violation = duplicate_budget_violation(
            DependencyBudgetScope::Workspace,
            &[],
            &[],
            WORKSPACE_ALLOWED_DUPLICATE_PACKAGES,
            true,
        );

        assert_eq!(violation, None);
    }

    #[test]
    fn dependency_budget_result_aggregates_scope_errors() {
        let error = dependency_budget_result(vec![
            "strict duplicate dependency version check failed for workspace: hashbrown".to_owned(),
            "strict duplicate dependency version check failed for workspace all-features: roxmltree"
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
    fn strict_duplicate_families_can_focus_wgpu_slice() {
        let families = vec![
            "foldhash".to_owned(),
            "hashbrown".to_owned(),
            "tiny-skia".to_owned(),
            "roxmltree".to_owned(),
        ];

        assert_eq!(
            select_strict_duplicate_families(&families, Some(DependencyBudgetSlice::WgpuStack)),
            vec!["foldhash".to_owned(), "hashbrown".to_owned()]
        );
    }

    #[test]
    fn strict_duplicate_families_can_focus_desktop_platform_slice() {
        let families = vec![
            "hashbrown".to_owned(),
            "tiny-skia".to_owned(),
            "tiny-skia-path".to_owned(),
            "roxmltree".to_owned(),
        ];

        assert_eq!(
            select_strict_duplicate_families(
                &families,
                Some(DependencyBudgetSlice::DesktopPlatform)
            ),
            vec!["tiny-skia".to_owned(), "tiny-skia-path".to_owned()]
        );
    }

    #[test]
    fn strict_duplicate_families_can_focus_optional_features_slice() {
        let families = vec![
            "async-channel".to_owned(),
            "event-listener".to_owned(),
            "getrandom".to_owned(),
            "hashbrown".to_owned(),
            "roxmltree".to_owned(),
        ];

        assert_eq!(
            select_strict_duplicate_families(
                &families,
                Some(DependencyBudgetSlice::OptionalFeatures)
            ),
            vec![
                "async-channel".to_owned(),
                "event-listener".to_owned(),
                "getrandom".to_owned(),
                "roxmltree".to_owned()
            ]
        );
    }

    #[test]
    fn duplicate_version_slice_families_report_single_slice() {
        let families = vec![
            "async-channel".to_owned(),
            "hashbrown".to_owned(),
            "tiny-skia".to_owned(),
        ];

        assert_eq!(
            duplicate_version_slice_families(&families, DependencyBudgetSlice::DesktopPlatform),
            vec!["tiny-skia".to_owned()]
        );
    }

    #[test]
    fn unclassified_duplicate_version_families_report_unknown_families() {
        let families = vec![
            "hashbrown".to_owned(),
            "unknown-family".to_owned(),
            "roxmltree".to_owned(),
        ];

        assert_eq!(
            unclassified_duplicate_version_families(&families),
            vec!["unknown-family".to_owned()]
        );
    }

    #[test]
    fn strict_duplicate_families_without_slice_keeps_all_version_families() {
        let families = vec!["hashbrown".to_owned(), "tiny-skia".to_owned()];

        assert_eq!(select_strict_duplicate_families(&families, None), families);
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

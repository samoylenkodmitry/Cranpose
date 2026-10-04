use std::{fs, path::Path, process::Command};

fn successful(command: &mut Command) -> String {
    let output = command.output().expect("run command");
    assert!(
        output.status.success(),
        "{command:?}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 output")
}

fn verify_binary_lookup(configured: bool, custom: bool, cross_target: bool, member_cwd: bool) {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path();
    fs::create_dir_all(root.join("member/src")).expect("member directory");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .expect("workspace manifest");
    fs::write(
        root.join("member/Cargo.toml"),
        "[package]\nname = \"artifact-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("member manifest");
    fs::write(root.join("member/src/main.rs"), "fn main() {}\n").expect("source");
    if configured {
        fs::create_dir(root.join(".cargo")).expect("Cargo configuration directory");
        fs::write(
            root.join(".cargo/config.toml"),
            "[build]\ntarget-dir = \"configured artifacts\"\n",
        )
        .expect("Cargo configuration");
    }
    let host = cross_target.then(|| {
        successful(Command::new("rustc").arg("-vV"))
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .expect("host triple")
            .to_owned()
    });
    let configure = |command: &mut Command| {
        command
            .current_dir(if member_cwd {
                root.join("member")
            } else {
                root.to_path_buf()
            })
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET");
        if custom {
            command.env("CARGO_TARGET_DIR", "environment artifacts");
        }
        command.args([
            "--manifest-path",
            if member_cwd {
                "../member/Cargo.toml"
            } else {
                "member/Cargo.toml"
            },
        ]);
        if let Some(host) = &host {
            command.args(["--target", host]);
        }
    };
    let mut build = Command::new("cargo");
    build.arg("build").arg("--offline");
    configure(&mut build);
    successful(&mut build);
    let mut report = Command::new(env!("CARGO_BIN_EXE_xtask"));
    report.args([
        "binary-size",
        "--package",
        "artifact-fixture",
        "--bin",
        "artifact-fixture",
        "--profile",
        "dev",
        "--no-build",
    ]);
    configure(&mut report);
    let output = successful(&mut report);
    let mut expected = if custom && member_cwd {
        root.join("member")
    } else {
        root.to_path_buf()
    }
    .join(if custom {
        "environment artifacts"
    } else if configured {
        "configured artifacts"
    } else {
        "target"
    });
    if let Some(host) = host {
        expected.push(host);
    }
    expected.push(Path::new("debug"));
    expected.push(format!("artifact-fixture{}", std::env::consts::EXE_SUFFIX));
    let bytes = fs::metadata(expected).expect("Cargo artifact").len();
    assert!(output.contains(&format!("{bytes} bytes")), "{output}");
}

#[test]
fn workspace_member_uses_the_workspace_target_directory() {
    verify_binary_lookup(false, false, false, false);
}

#[test]
fn cargo_configuration_selects_the_target_directory() {
    verify_binary_lookup(true, false, false, false);
}

#[test]
fn environment_target_directory_overrides_cargo_configuration() {
    verify_binary_lookup(true, true, false, false);
}

#[test]
fn explicit_target_uses_cargos_target_specific_output_directory() {
    verify_binary_lookup(true, true, true, false);
}

#[test]
fn member_invocation_resolves_relative_environment_directory() {
    verify_binary_lookup(true, true, true, true);
}

#[test]
fn patched_binary_size_build_uses_workspace_coroflow_and_navigation_sources() {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path();
    write_workspace_cranpose_stubs(root);

    let app = root.join("apps/isolated-demo");
    fs::create_dir_all(app.join("src")).expect("fixture app source directory");
    fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"isolated-demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ncoroflow = \"0.0.0\"\ncranpose-coroflow = \"0.0.0\"\ncranpose-navigation = \"0.0.0\"\n\n[workspace]\n",
    )
    .expect("fixture app manifest");
    fs::write(
        app.join("src/main.rs"),
        "fn main() { println!(\"{} {} {}\", coroflow::SOURCE_SENTINEL, cranpose_coroflow::source_sentinel(), cranpose_navigation::source_sentinel()); }\n",
    )
    .expect("fixture app source");

    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .args([
            "binary-size",
            "--manifest-path",
            "apps/isolated-demo/Cargo.toml",
            "--package",
            "isolated-demo",
            "--bin",
            "isolated-demo",
            "--profile",
            "dev",
            "--patch-workspace-cranpose",
        ]);
    let report = successful(&mut command);
    assert!(
        report.contains("dev isolated-demo:isolated-demo "),
        "{report}"
    );
    assert!(report.contains(" bytes ("), "{report}");

    let binary = root.join(format!(
        "target/patched-packages/isolated-demo/target/debug/isolated-demo{}",
        std::env::consts::EXE_SUFFIX
    ));
    let output = Command::new(&binary)
        .output()
        .expect("run the binary built by binary-size");
    assert!(
        output.status.success(),
        "{binary:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 output")
            .trim(),
        "workspace-coroflow-sentinel workspace-coroflow-sentinel workspace-coroflow-sentinel"
    );
}

fn write_workspace_cranpose_stubs(root: &Path) {
    let framework = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("framework workspace root");
    let source =
        fs::read_to_string(framework.join("Cargo.toml")).expect("framework workspace manifest");
    let manifest: toml::Value = toml::from_str(&source).expect("parse framework manifest");
    let dependencies = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
        .expect("framework workspace dependencies");
    let mut workspace_manifest =
        String::from("[workspace]\nmembers = []\nresolver = \"2\"\n\n[workspace.dependencies]\n");

    for (name, spec) in dependencies.iter().filter(|(_, spec)| {
        spec.as_table()
            .is_some_and(|dependency| dependency.contains_key("path"))
    }) {
        let Some(path) = spec
            .as_table()
            .and_then(|dependency| dependency.get("path"))
            .and_then(toml::Value::as_str)
        else {
            continue;
        };
        let package = root.join(path);
        fs::create_dir_all(package.join("src")).expect("stub crate source directory");
        let package_name = spec
            .as_table()
            .and_then(|dependency| dependency.get("package"))
            .and_then(toml::Value::as_str)
            .unwrap_or(name);
        workspace_manifest.push_str(&format!(
            "{name} = {{ package = \"{package_name}\", path = \"{path}\", version = \"0.0.0\" }}\n"
        ));
        let mut crate_manifest = format!(
            "[package]\nname = \"{package_name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"
        );
        let source = match package_name {
            "coroflow" => "pub const SOURCE_SENTINEL: &str = \"workspace-coroflow-sentinel\";\n",
            "cranpose-coroflow" => {
                crate_manifest.push_str("\n[dependencies]\ncoroflow = \"0.0.0\"\n");
                "pub fn source_sentinel() -> &'static str { coroflow::SOURCE_SENTINEL }\n"
            }
            "cranpose-navigation" => {
                crate_manifest.push_str("\n[dependencies]\ncranpose-coroflow = \"0.0.0\"\n");
                "pub fn source_sentinel() -> &'static str { cranpose_coroflow::source_sentinel() }\n"
            }
            _ => "",
        };
        fs::write(package.join("Cargo.toml"), crate_manifest).expect("stub crate manifest");
        fs::write(package.join("src/lib.rs"), source).expect("stub crate source");
    }
    fs::write(root.join("Cargo.toml"), workspace_manifest).expect("fixture workspace manifest");
}

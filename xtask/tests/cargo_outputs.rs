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

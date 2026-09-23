#![cfg(not(target_arch = "wasm32"))]

use cranpose_services::windowless_command;

#[test]
fn a_windowless_command_runs_the_named_program_with_its_arguments() {
    let mut command = windowless_command("reg");
    command.args(["query", "HKCU"]);

    assert_eq!(command.get_program(), "reg");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["query", "HKCU"],
        "the helper only changes how the program is started, never what it runs"
    );
}

#[cfg(unix)]
#[test]
fn a_windowless_command_captures_the_helper_output() {
    let output = windowless_command("printf")
        .arg("theme=dark")
        .output()
        .expect("printf runs");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"theme=dark");
}

#[cfg(windows)]
#[test]
fn a_windowless_command_captures_the_helper_output() {
    let output = windowless_command("cmd")
        .args(["/C", "echo theme=dark"])
        .output()
        .expect("cmd runs");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "theme=dark");
}

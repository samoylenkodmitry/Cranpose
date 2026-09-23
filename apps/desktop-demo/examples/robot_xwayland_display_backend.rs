use std::{process::Command, time::Duration};

use cranpose::AppLauncher;

const CHILD_VAR: &str = "CRANPOSE_ROBOT_XWAYLAND_CHILD";
const ABSENT_WAYLAND_SOCKET: &str = "cranpose-robot-absent-wayland-socket";
const CHILD_PASS: &str = "PASS: launched on the X display";
const GRACE: Duration = Duration::from_secs(3);

fn main() {
    if std::env::var_os(CHILD_VAR).is_some() {
        launch_and_exit();
        return;
    }

    let binary = std::env::current_exe().expect("locate this robot binary");
    let output = Command::new(binary)
        .env(CHILD_VAR, "1")
        .env("WAYLAND_DISPLAY", ABSENT_WAYLAND_SOCKET)
        .output()
        .expect("run this robot binary as a Wayland-session child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stdout}");
    eprint!("{stderr}");

    if !output.status.success() || !stdout.contains(CHILD_PASS) {
        println!(
            "FAIL: with WAYLAND_DISPLAY and DISPLAY both set the app did not launch on the \
             X display (status {}); the native-window shell needs XWayland's global window \
             positions",
            output.status,
        );
        std::process::exit(1);
    }

    println!("PASS: a Wayland session with XWayland launches the desktop shell on X11");
}

fn launch_and_exit() {
    AppLauncher::new()
        .with_title("xwayland_display_backend")
        .with_size(320, 240)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_test_driver(move |robot| {
            let _ = robot.wait_for_idle();
            cranpose::request_exit();
            std::thread::sleep(GRACE);
            println!(
                "FAIL: still running {}s after request_exit",
                GRACE.as_secs()
            );
            std::process::exit(1);
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch the demo");

    println!("{CHILD_PASS}");
}

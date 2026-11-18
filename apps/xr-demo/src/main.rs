#![allow(clippy::print_stderr, reason = "a command-line tool prints its output")]

// In a headset where the machine has an OpenXR runtime with one, and in a flat window elsewhere.
// `RUST_LOG=info` shows which, and `RUST_LOG=cranpose_xr=debug` the frames drawn each second.
fn main() {
    env_logger::init();
    let launcher = cranpose::AppLauncher::new()
        .with_title("Cranpose XR")
        .with_size(960, 540);
    if let Err(error) = cranpose_xr::run(launcher, xr_demo::XrDemo) {
        eprintln!("the XR demo failed to start: {error}");
        std::process::exit(1);
    }
}

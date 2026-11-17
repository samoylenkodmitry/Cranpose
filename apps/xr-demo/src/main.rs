#![allow(clippy::print_stderr, reason = "a command-line tool prints its output")]

fn main() {
    if let Err(error) = cranpose::AppLauncher::new()
        .with_title("Cranpose XR")
        .try_run(xr_demo::XrDemo)
    {
        eprintln!("the XR demo failed to start: {error}");
        std::process::exit(1);
    }
}

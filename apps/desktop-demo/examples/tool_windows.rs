use cranpose::AppLauncher;
use desktop_app::{app::tool_windows_app, fonts::DEMO_FONTS};

fn main() {
    AppLauncher::new()
        .with_title("Cranpose Tool Windows")
        .with_size(1, 1)
        .with_fonts(DEMO_FONTS)
        .run_windows(tool_windows_app);
}

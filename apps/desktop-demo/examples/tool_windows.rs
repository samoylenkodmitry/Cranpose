use cranpose::AppLauncher;
use desktop_app::{app::tool_windows_app, fonts::DEMO_FONTS, init_logging};

fn main() {
    init_logging();
    AppLauncher::new()
        .with_title("Cranpose Tool Windows")
        .with_fonts(DEMO_FONTS)
        .run(tool_windows_app);
}

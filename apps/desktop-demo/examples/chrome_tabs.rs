use cranpose::AppLauncher;
use desktop_app::{app::chrome_tabs_app, fonts::DEMO_FONTS, init_logging};

fn main() {
    init_logging();
    AppLauncher::new()
        .with_title("Cranpose Tabs")
        .with_size(1, 1)
        .with_fonts(DEMO_FONTS)
        .run_windows(chrome_tabs_app);
}

use cranpose::AppLauncher;
use desktop_app::{app::chrome_tabs_app, fonts::DEMO_FONTS, init_logging};

fn main() {
    init_logging();
    AppLauncher::new()
        .with_title("Cranpose Tabs")
        .with_fonts(DEMO_FONTS)
        .run(chrome_tabs_app);
}

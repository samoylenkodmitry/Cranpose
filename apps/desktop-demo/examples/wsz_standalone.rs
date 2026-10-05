use cranpose::AppLauncher;
use desktop_app::{app::WszStandaloneApp, fonts::DEMO_FONTS};

fn main() {
    AppLauncher::new()
        .with_title("WSZ Standalone")
        .with_fonts(DEMO_FONTS)
        .run(WszStandaloneApp);
}

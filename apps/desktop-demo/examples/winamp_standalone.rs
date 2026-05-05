use cranpose::AppLauncher;
use desktop_app::{app::WinampStandaloneApp, fonts::DEMO_FONTS};

fn main() {
    AppLauncher::new()
        .with_title("Winamp Standalone")
        .with_size(1, 1)
        .with_fonts(DEMO_FONTS)
        .run_windows(WinampStandaloneApp);
}

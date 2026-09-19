use cranpose::AppLauncher;
use desktop_app::{app::flame_window_app, fonts::DEMO_FONTS, init_logging};

fn main() {
    init_logging();
    AppLauncher::new()
        .with_title("Cranpose Flame Window")
        .with_fonts(DEMO_FONTS)
        .run(flame_window_app);
}

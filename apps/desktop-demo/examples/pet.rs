use cranpose::AppLauncher;
use desktop_app::{app::pet_app, fonts::DEMO_FONTS, init_logging};

fn main() {
    init_logging();
    AppLauncher::new()
        .with_title("Cranpose Pet")
        .with_fonts(DEMO_FONTS)
        .run(pet_app);
}

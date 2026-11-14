// Everbloom, the shop app of the whiskerrs/benchmarks frameworks comparison
// (`spec/PROMPT.md`), written with Cranpose.

mod api;
mod app;
mod aspect_ratio;
mod cart;
mod cart_store;
mod collection;
mod home;
mod home_ready;
mod icons;
mod load;
mod model;
mod nav_header;
mod near_end;
mod price;
mod product;
mod product_card;
mod remote_image;
mod search;
mod skeleton;
mod sort;
mod states;
mod tab_bar;
mod theme;

use cranpose::{
    AppLauncher,
    text::{FontFamily, FontWeight},
};

fn main() {
    let launcher = AppLauncher::new()
        .with_title("Everbloom")
        .with_size(440, 956)
        .with_system_fonts(
            &FontFamily::SansSerif,
            &[
                FontWeight::NORMAL,
                FontWeight::MEDIUM,
                FontWeight::SEMI_BOLD,
                FontWeight::BOLD,
            ],
        );
    if let Err(error) = launcher.try_run(app::App) {
        eprintln!("Everbloom could not start: {error}");
        std::process::exit(1);
    }
}

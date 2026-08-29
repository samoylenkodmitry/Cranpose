use std::time::Duration;

use cranpose::AppLauncher;
use desktop_app::test_screens::text_wrap_repro::TextWrapReproScreen;

fn main() {
    let headless = !std::env::args().any(|arg| arg == "headless=false");
    println!("Launching the text-wrap repro (headless={headless})...");

    AppLauncher::new()
        .with_title("Wrapped paragraph paints the height it measured")
        .with_size(760, 720)
        .with_headless(headless)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(600));
            match robot.validate_content("Verdict") {
                Ok(()) => println!("repro screen is up"),
                Err(err) => {
                    eprintln!("repro screen did not compose: {err}");
                    std::process::exit(1);
                }
            }
            println!(
                "Look at the window (or the screenshot): every line of both paragraphs must \
                 sit on the blue fill and stay clear of the red rule."
            );
            std::thread::sleep(Duration::from_millis(400));
        })
        .run(TextWrapReproScreen);
}

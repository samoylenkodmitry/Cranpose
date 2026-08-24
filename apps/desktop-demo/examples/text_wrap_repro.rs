//! Runnable demo for "a wrapped paragraph paints the height it measured".
//!
//! ```text
//! cargo run -p desktop-app --features robot-app --example text_wrap_repro
//! ```
//!
//! Opens the repro screen and prints, for every paragraph on it, the height
//! LAYOUT measured next to the height the renderer PAINTED. Fixed, the two
//! agree and the run reports OK. Broken, the painted height is one line taller
//! and the run reports the overrun and exits non-zero — so this is also usable
//! as a check, not only as something to look at.
//!
//! Add `--` `headless=false` to watch it in a window instead of reading the
//! numbers.

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
            // The paragraphs carry no semantics of their own; the screen's own
            // copy is what confirms it composed at all.
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

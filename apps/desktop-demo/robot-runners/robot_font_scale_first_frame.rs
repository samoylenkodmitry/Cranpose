mod robot_exit;
mod robot_launch;

use std::time::Duration;

use cranpose::{Robot, SemanticElement};
use desktop_app::test_screens::font_scale_repro::{
    font_scale_readout, FontScaleReproScreen, PROBE_LABEL, SHOW_COPY,
};

const EXPECTED_FONT_SCALE: f32 = 1.5;
const ATTEMPTS: usize = 40;
const WIDTH_TOLERANCE: f32 = 0.5;
const TEST_TIMEOUT_SECS: u64 = 60;

fn collect_texts<'a>(
    elements: &'a [SemanticElement],
    text: &str,
    found: &mut Vec<&'a SemanticElement>,
) {
    for element in elements {
        if element.text.as_deref() == Some(text) {
            found.push(element);
        }
        collect_texts(&element.children, text, found);
    }
}

fn visible_texts(elements: &[SemanticElement], texts: &mut Vec<String>) {
    for element in elements {
        if let Some(text) = &element.text {
            texts.push(text.clone());
        }
        visible_texts(&element.children, texts);
    }
}

fn label_widths_once_scaled(robot: &Robot) -> Option<(f32, f32)> {
    let semantics = robot.get_semantics().ok()?;
    let mut readout = Vec::new();
    collect_texts(
        &semantics,
        &font_scale_readout(EXPECTED_FONT_SCALE),
        &mut readout,
    );
    if readout.is_empty() {
        return None;
    }
    let mut labels = Vec::new();
    collect_texts(&semantics, PROBE_LABEL, &mut labels);
    match labels.as_slice() {
        [first, copy] => Some((first.bounds.width, copy.bounds.width)),
        _ => None,
    }
}

fn main() {
    env_logger::init();
    println!("=== Robot Font Scale First Frame Test ===");

    robot_launch::launch("Robot Font Scale First Frame", 800, 400)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(TEST_TIMEOUT_SECS);
            let mut widths = None;
            for _ in 0..ATTEMPTS {
                let _ = robot.wait_for_idle();
                if let Err(error) = robot.click_by_text(SHOW_COPY) {
                    robot_exit::fail(&robot, &format!("could not press '{SHOW_COPY}': {error}"));
                }
                let _ = robot.wait_for_idle();
                widths = label_widths_once_scaled(&robot);
                if widths.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let Some((first, copy)) = widths else {
                let mut texts = Vec::new();
                if let Ok(semantics) = robot.get_semantics() {
                    visible_texts(&semantics, &mut texts);
                }
                robot_exit::fail(
                    &robot,
                    &format!(
                        "the app never reported font scale {EXPECTED_FONT_SCALE} with two \
                         labels (run it with CRANPOSE_FONT_SCALE={EXPECTED_FONT_SCALE}); \
                         texts on screen: {texts:?}"
                    ),
                );
            };
            println!("label from the first frame: {first:.1}, fresh copy: {copy:.1}");
            if (first - copy).abs() > WIDTH_TOLERANCE {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "the label measured before the font scale arrived is {first:.1} wide, \
                         a fresh copy is {copy:.1}: its text draws past the box it kept"
                    ),
                );
            }
            println!("✓ PASS: text measured before the font scale arrived is measured again");
            let _ = robot.exit();
        })
        .run(|| {
            FontScaleReproScreen();
        });
}

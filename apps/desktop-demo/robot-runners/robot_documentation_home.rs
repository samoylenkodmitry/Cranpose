use cranpose::{AppLauncher, Robot, SemanticElement, SemanticRect};
use cranpose_testing::{find_button_exact_in_semantics, find_text_in_semantics, find_text_in_semantics_exact};
use desktop_app::app;

use crate::{output_paths, robot_shot};

fn click_button(robot: &Robot, label: &str) {
    let Some((x, y, width, height)) = find_button_exact_in_semantics(robot, label) else {
        robot_shot::save_checked(
            &output_paths::diagnostic_path("documentation-missing-control.png"),
            &robot.screenshot().expect("missing control screenshot"),
        ).expect("save missing control screenshot");
        panic!("documentation button {label:?}");
    };
    robot
        .click(x + width * 0.5, y + height * 0.5)
        .expect("click documentation control");
    robot.wait_for_idle().expect("documentation settles");
}

fn capture(robot: &Robot, width: u32, stage: &str) {
    let filename = format!("documentation-{width}-{stage}.png");
    robot_shot::save_checked(
        &output_paths::diagnostic_path(&filename),
        &robot.screenshot().expect("documentation screenshot"),
    )
    .expect("save documentation screenshot");
}

fn first_visible_chapter(robot: &Robot, height: u32) -> (usize, SemanticRect) {
    let titles: Vec<_> = include_str!("../../../docs/guide.md")
        .split("\n## ")
        .skip(1)
        .filter_map(|chapter| chapter.split_once('\n').map(|(title, _)| title))
        .collect();
    fn find(elements: &[SemanticElement], titles: &[&str], height: u32) -> Option<(usize, SemanticRect)> {
        elements.iter().find_map(|element| {
            let bounds = element.bounds;
            if element.clickable && bounds.y >= 80.0 && bounds.y + bounds.height <= height as f32 - 60.0 {
                if let Some(index) = titles.iter().position(|title| element.text.as_deref() == Some(*title)) {
                    return Some((index, bounds));
                }
            }
            find(&element.children, titles, height)
        })
    }
    find(&robot.get_semantics().expect("wheel semantics"), &titles, height).expect("visible wheel entry")
}

fn return_to_top(robot: &Robot, width: u32, height: u32) {
    robot.move_to(width as f32 * 0.9, height as f32 * 0.6).expect("hover reader");
    let mut visible = false;
    for _ in 0..160 {
        if let Some((_, y, _, button_height)) = find_button_exact_in_semantics(robot, "Back to top") {
            if y >= 80.0 && y + button_height <= height as f32 - 60.0 {
                visible = true;
                break;
            }
        }
        robot.mouse_scroll_and_wait_for_frame(0.0, -240.0).expect("reveal reader footer");
    }
    assert!(visible, "reader footer appears during scroll");
    click_button(robot, "Back to top");
    robot.validate_content("Build apps with Compose in Rust.").expect("top of guide is visible");
}

fn check_wheel_fling(robot: &Robot, width: u32, height: u32) {
    robot.drag_and_wait_for_frames(120.0, 550.0, 120.0, 454.0, 8).expect("flick the wheel");
    let (released_index, released) = first_visible_chapter(robot, height);
    robot.pump_frames(30).expect("wheel coast frames");
    let (coasted_index, coasted) = first_visible_chapter(robot, height);
    assert!(coasted_index > released_index || (coasted_index == released_index && coasted.y < released.y - 10.0), "wheel must coast after release");
    robot.touch_down(120.0, 550.0).expect("catch the spinning wheel");
    capture(robot, width, "fling");
    robot.touch_up(120.0, 550.0).expect("release the stopped wheel");
 }

fn check_counter_preview(robot: &Robot, width: u32, height: u32) {
    for _ in 0..30 {
        if let Some((_, y, _, _)) = find_text_in_semantics(robot, "use cranpose::prelude::*;") {
            if y >= 0.0 && y < height as f32 * 0.65 {
        break;
            }
        }
        robot
            .mouse_scroll_and_wait_for_frame(0.0, -90.0)
            .expect("scroll to code");
    }
    capture(robot, width, "code");
    let mut preview_visible = false;
    for _ in 0..80 {
        if let Some((_, y, _, button_height)) = find_button_exact_in_semantics(robot, "Increment") {
            if y >= 80.0 && y + button_height <= height as f32 - 80.0 {
        preview_visible = true;
        break;
            }
        }
        robot.mouse_scroll_and_wait_for_frame(0.0, -180.0).expect("scroll to interactive example");
    }
    assert!(preview_visible, "counter preview must fit in the reader");
    click_button(robot, "Increment");
    assert!(find_text_in_semantics_exact(robot, "Count: 1").is_some());
    capture(robot, width, "interactive-example");
 }

fn check_mobile_tables_and_controls(robot: &Robot, width: u32, height: u32) {
    for (chapter, target, stage) in [
        ("Testing", "Test scope", "table"),
        ("Liquid components", "Save", "glass-button"),
    ] {
        robot.move_to(80.0, height as f32 * 0.65).expect("hover wheel labels");
        robot.mouse_scroll_and_wait_for_frame(0.0, 10000.0).expect("return to the first chapter");
        robot.wait_for_idle().expect("wheel resets");
        let mut found = false;
        for _ in 0..30 {
            if let Some((_, y, _, button_height)) = find_button_exact_in_semantics(robot, chapter) {
                if y >= 96.0 && y + button_height < height as f32 - 60.0 {
                    found = true;
                    break;
                }
            }
            robot.mouse_scroll_and_wait_for_frame(0.0, -120.0).expect("scroll wheel");
        }
        assert!(found, "wheel reaches {chapter}");
        click_button(robot, chapter);
        let mut found = false;
        for _ in 0..60 {
            if let Some((_, y, _, label_height)) = find_text_in_semantics_exact(robot, target) {
                if y >= 80.0 && y + label_height < height as f32 - 100.0 {
                    found = true;
                    break;
                }
            }
            robot.mouse_scroll_and_wait_for_frame(0.0, -120.0).expect("scroll to guide example");
        }
        assert!(found, "guide displays {target}");
        if stage == "glass-button" {
            click_button(robot, "Save");
            robot.validate_content("Saved").expect("glass button state");
        }
        capture(robot, width, stage);
        click_button(robot, "Back to wheel");
    }
}

pub(crate) fn main() {
    let (width, height) = match std::env::var("CRANPOSE_DOCS_VIEWPORT").as_deref() {
        Ok("compact") => (390, 780),
        Ok("short") => (800, 600),
        Ok("wide") => (1440, 1000),
        Ok("desktop") | Err(std::env::VarError::NotPresent) => (1100, 820),
        other => panic!("invalid documentation viewport: {other:?}"),
    };
    let compact = width < 780;
    AppLauncher::new()
        .with_title("Cranpose guide")
        .with_size(width, height)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot.wait_for_idle().expect("documentation startup");
            if compact {
                robot.validate_content("Welcome").expect("compact wheel opens first");
                robot.validate_content("Build apps with Compose in Rust.").expect("live reader preview");
                robot.click(310.0, 210.0).expect("open the miniature reader");
                robot.wait_for_idle().expect("reader opens");
                robot.validate_content("Back to wheel").expect("expanded reader");
                click_button(&robot, "Back to wheel");
            } else {
                robot.validate_content("Build apps with Compose in Rust.").expect("offline reader");
            }
            capture(&robot, width, "home");
            let github_before = find_text_in_semantics_exact(&robot, "View on GitHub").expect("fixed repository link");
            assert!(github_before.0 < 100.0 && github_before.1 > height as f32 - 55.0);
            robot.move_to(160.0, height as f32 * 0.65).expect("hover wheel");
            robot.mouse_scroll_sequence_and_wait_for_frames(0.0, -150.0, 5).expect("rotate wheel independently");
            robot.wait_for_idle().expect("wheel settles");
            capture(&robot, width, "wheel-scrolling");
            if !compact {
                assert!(find_text_in_semantics_exact(&robot, "Build apps with Compose in Rust.").is_none(), "wheel scroll navigates the document");
            }
            robot.mouse_scroll_and_wait_for_frame(0.0, 750.0).expect("return wheel to first chapter");
            robot.wait_for_idle().expect("wheel returns to first chapter");
            click_button(&robot, "Get started");
            capture(&robot, width, "get-started");
            robot.validate_content("Start with the project template").expect("selected chapter is readable");
            if !compact {
                if let Some((_, y, _, height)) = find_text_in_semantics_exact(&robot, "Counter App") {
                    assert!(y + height <= 0.0, "the tabs must scroll completely out of view: y={y}, height={height}");
                }
            }
            let (_, title_y, _, _) = find_text_in_semantics(&robot, "Start with the project template").expect("article heading");
            robot.move_to(width as f32 * 0.90, height as f32 * 0.70).expect("hover document");
            robot.mouse_scroll_sequence_and_wait_for_frames(0.0, -90.0, 3).expect("scroll reader");
            robot.wait_for_idle().expect("reader settles");
            capture(&robot, width, "scrolling");
            if let Some((_, scrolled_title_y, _, _)) = find_text_in_semantics(&robot, "Start with the project template") {
                assert!(title_y - scrolled_title_y > 50.0, "the document must scroll");
            }
            let github_after = find_text_in_semantics_exact(&robot, "View on GitHub").expect("fixed repository link after scrolling");
            assert!((github_after.1 - github_before.1).abs() < 1.0);
            check_counter_preview(&robot, width, height);
            if compact {
                click_button(&robot, "Back to wheel");
                capture(&robot, width, "back-to-wheel");
                robot.validate_content("Count: 1").expect("preview preserves the counter and scroll position");
                click_button(&robot, "Get started");
                robot.validate_content("Start with the project template").expect("reopen the reader");
            }
            return_to_top(&robot, width, height);
            capture(&robot, width, "back-to-top");
            if !compact {
                click_button(&robot, "Counter App");
                robot.validate_content("Increment").expect("existing demo remains available");
                click_button(&robot, "Cranpose Guide");
                robot.validate_content("Build apps with Compose in Rust.").expect("return to documentation");
            } else {
                click_button(&robot, "Back to wheel");
            }
            check_wheel_fling(&robot, width, height);
            if compact {
                check_mobile_tables_and_controls(&robot, width, height);
            }
            robot.exit().expect("exit documentation robot");
        })
        .run(app::DesktopApp);
}

mod robot_exit;

use std::time::Duration;

use cranpose::{AppLauncher, RobotScreenshot, SemanticElement};
use cranpose_testing::{find_button, find_button_in_semantics, find_in_semantics, find_text};
use desktop_app::app;

type SelectedEditable = ((f32, f32, f32, f32), (usize, usize));

fn matches_accent(r: u8, g: u8, b: u8, target: (i16, i16, i16)) -> bool {
    const TOLERANCE: i16 = 20;
    (r as i16 - target.0).abs() <= TOLERANCE
        && (g as i16 - target.1).abs() <= TOLERANCE
        && (b as i16 - target.2).abs() <= TOLERANCE
}

const HANDLE_BLUE: (i16, i16, i16) = (0, 122, 255);
const HANDLE_PINK: (i16, i16, i16) = (246, 53, 142);

fn main() {
    env_logger::init();
    println!("=== Text Handle Survives Tab Switch ===\n");

    AppLauncher::new()
        .with_title("Text Handle Survives Tab Switch")
        .with_size(900, 700)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);

            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            drag_selection_survives_tab_switch(&robot);
            touch_double_tap_survives_tab_switch(&robot);
            handle_survives_field_to_field_focus_handoff(&robot);

            println!("\nPASS: no stuck handle survived a tab round trip");
            let _ = robot.exit();
        })
        .run(app::combined_app);
}

fn drag_selection_survives_tab_switch(robot: &cranpose::Robot) {
    click_tab(robot, "Text Input");
    wait_for_text(robot, "Text Input Demo");

    let (field_x, field_y, field_w, field_h) = wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |elem| find_text(elem, "Type here..."))
    })
    .expect("text field must be present on the Text Input tab");

    for _ in 0..5 {
        if let Some((x, y, w, h)) = find_in_semantics(robot, |elem| find_button(elem, "Add !")) {
            let _ = robot.click(x + w * 0.5, y + h * 0.5);
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    std::thread::sleep(Duration::from_millis(150));

    let start_x = field_x + field_w - 10.0;
    let end_x = field_x + 10.0;
    let center_y = field_y + field_h * 0.5;
    let _ = robot.mouse_move(start_x, center_y);
    std::thread::sleep(Duration::from_millis(30));
    let _ = robot.mouse_down();
    std::thread::sleep(Duration::from_millis(30));
    for step in 1..=3 {
        let t = step as f32 / 3.0;
        let _ = robot.mouse_move(start_x + (end_x - start_x) * t, center_y);
        std::thread::sleep(Duration::from_millis(30));
    }
    let _ = robot.mouse_up();
    std::thread::sleep(Duration::from_millis(100));

    assert!(
        robot.has_focused_text_field().unwrap_or(false),
        "dragging inside the field must focus it"
    );

    let semantics = robot.get_semantics().expect("semantics after drag");
    let (selected_bounds, selection) = selected_editable(&semantics)
        .expect("the drag must publish a non-collapsed text selection");
    println!("[mouse] selection after drag: {selection:?}, bounds {selected_bounds:?}");

    let before_switch = robot.screenshot().expect("frame with handles visible");
    let (start_px, end_px) = count_handle_bands_in(&before_switch, selected_bounds, HANDLE_BLUE);
    println!("[mouse] handle pixels before tab switch: start={start_px} end={end_px}");
    assert!(
        start_px >= 8 && end_px >= 8,
        "sanity check failed: no selection handles rendered before the tab \
         switch (start={start_px}, end={end_px}); the detector or the drag \
         setup is broken, not the regression under test"
    );

    switch_away_and_back(robot);

    let (new_field_x, new_field_y, new_field_w, new_field_h) =
        wait_for_in_semantics(robot, |robot| {
            find_in_semantics(robot, |elem| find_text(elem, "Type here..."))
        })
        .expect("the field must remount fresh (unedited placeholder) after the round trip");
    settle_after_switch(robot);

    assert!(
        !robot.has_focused_text_field().unwrap_or(true),
        "a freshly remounted field must not read back as focused"
    );

    let after_switch = robot.screenshot().expect("frame after tab round trip");
    let scan_rect = (
        new_field_x - 20.0,
        new_field_y - 60.0,
        new_field_w + 40.0,
        new_field_h + 120.0,
    );
    let ghost_pixels = count_pixels_in(&after_switch, scan_rect, HANDLE_BLUE);
    println!("[mouse] blue handle-colored pixels left after round trip: {ghost_pixels}");
    assert_eq!(
        ghost_pixels, 0,
        "a selection handle from the pre-switch field is still painted over \
         the freshly mounted, unfocused field"
    );
}

fn touch_double_tap_survives_tab_switch(robot: &cranpose::Robot) {
    click_tab(robot, "Text Input");
    wait_for_text(robot, "Text Input Demo");

    let (tx, ty, _tw, _th) = wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |elem| find_text(elem, "Silence. Melody"))
    })
    .expect("touch-selection field text must be present");

    let word_x = tx + 40.0;
    let word_y = ty + 10.0;
    let _ = robot.drag(word_x, word_y, word_x, word_y);
    std::thread::sleep(Duration::from_millis(120));
    let _ = robot.drag(word_x, word_y, word_x, word_y);
    std::thread::sleep(Duration::from_millis(300));
    let _ = robot.wait_for_idle();

    assert!(
        robot.has_focused_text_field().unwrap_or(false),
        "double-tapping the field must focus it"
    );

    let before = robot.screenshot().expect("frame with pink handles visible");
    let scan_rect = (tx - 20.0, ty - 40.0, 760.0, 140.0);
    let handle_px = count_pixels_in(&before, scan_rect, HANDLE_PINK);
    println!("[touch] pink handle pixels before tab switch: {handle_px}");
    assert!(
        handle_px > 0,
        "sanity check failed: double-tap must raise pink selection handles"
    );

    switch_away_and_back(robot);

    let (tx2, ty2, _, _) = wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |elem| find_text(elem, "Silence. Melody"))
    })
    .expect("touch field must remount fresh after the round trip");
    settle_after_switch(robot);

    assert!(
        !robot.has_focused_text_field().unwrap_or(true),
        "a freshly remounted field must not read back as focused"
    );

    let after = robot.screenshot().expect("frame after tab round trip");
    let scan_rect2 = (tx2 - 20.0, ty2 - 40.0, 760.0, 140.0);
    let ghost = count_pixels_in(&after, scan_rect2, HANDLE_PINK);
    println!("[touch] pink ghost pixels after round trip: {ghost}");
    assert_eq!(
        ghost, 0,
        "a touch-created selection handle survived the tab round trip"
    );
}

fn handle_survives_field_to_field_focus_handoff(robot: &cranpose::Robot) {
    click_tab(robot, "Text Input");
    wait_for_text(robot, "Text Input Demo");

    let (field1_x, field1_y, field1_w, field1_h) = wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |elem| find_text(elem, "Type here..."))
    })
    .expect("field1 must be present");

    let center_y1 = field1_y + field1_h * 0.5;
    let _ = robot.click(field1_x + field1_w - 20.0, center_y1);
    std::thread::sleep(Duration::from_millis(150));
    let _ = robot.click(field1_x + 20.0, center_y1);
    std::thread::sleep(Duration::from_millis(150));

    assert!(
        robot.has_focused_text_field().unwrap_or(false),
        "tapping field1 must focus it"
    );

    let before = robot.screenshot().expect("frame with handle visible");
    let handle_before = count_pixels_in(
        &before,
        (
            field1_x - 20.0,
            field1_y - 10.0,
            field1_w + 40.0,
            field1_h + 50.0,
        ),
        HANDLE_BLUE,
    );
    println!("[handoff] handle pixels before moving focus: {handle_before}");
    assert!(
        handle_before > 0,
        "sanity: handle must show after tapping field1"
    );

    let semantics = robot.get_semantics().expect("semantics for field2 lookup");
    let (f2x, f2y, f2w, f2h) = second_editable_bounds(&semantics, (field1_x, field1_y))
        .expect("field2 bounds must be found via semantics");
    let _ = robot.click(f2x + f2w * 0.5, f2y + f2h * 0.5);
    std::thread::sleep(Duration::from_millis(200));
    let _ = robot.pump_frames(10);
    let _ = robot.wait_for_idle();
    for _ in 0..5 {
        let _ = robot.wait_for_present_frame();
    }
    std::thread::sleep(Duration::from_millis(200));

    assert!(
        robot.has_focused_text_field().unwrap_or(false),
        "tapping field2 must focus it"
    );

    let after = robot.screenshot().expect("frame after focus hand-off");
    let ghost = count_pixels_in(
        &after,
        (
            field1_x - 20.0,
            field1_y - 60.0,
            field1_w + 40.0,
            field1_h + 120.0,
        ),
        HANDLE_BLUE,
    );
    println!("[handoff] ghost pixels near field1 after focus moved to field2: {ghost}");
    assert_eq!(
        ghost, 0,
        "field1's handle must disappear once focus moved to field2"
    );
}

fn second_editable_bounds(
    elements: &[SemanticElement],
    exclude_origin: (f32, f32),
) -> Option<(f32, f32, f32, f32)> {
    for element in elements {
        if element.editable_text {
            let bounds = element.bounds;
            let is_field1 = (bounds.x - exclude_origin.0).abs() < 1.0
                && (bounds.y - exclude_origin.1).abs() < 1.0;
            if !is_field1 {
                return Some((bounds.x, bounds.y, bounds.width, bounds.height));
            }
        }
        if let Some(found) = second_editable_bounds(&element.children, exclude_origin) {
            return Some(found);
        }
    }
    None
}

fn switch_away_and_back(robot: &cranpose::Robot) {
    click_tab(robot, "Images");
    std::thread::sleep(Duration::from_millis(200));
    let _ = robot.wait_for_idle();
    click_tab(robot, "Text Input");
    wait_for_text(robot, "Text Input Demo");
}

fn settle_after_switch(robot: &cranpose::Robot) {
    let _ = robot.pump_frames(30);
    let _ = robot.wait_for_idle();
    for _ in 0..10 {
        let _ = robot.wait_for_present_frame();
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = robot.wait_for_idle();
}

fn click_tab(robot: &cranpose::Robot, label: &str) {
    for _ in 0..40 {
        if let Some((x, y, w, h)) = find_button_in_semantics(robot, label) {
            let _ = robot.click(x + w * 0.5, y + h * 0.5);
            let _ = robot.wait_for_idle();
            std::thread::sleep(Duration::from_millis(80));
            return;
        }
        let _ = robot.wait_for_idle();
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("could not find tab button '{label}'");
}

fn wait_for_text(robot: &cranpose::Robot, text: &str) {
    wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |elem| find_text(elem, text))
    })
    .unwrap_or_else(|| panic!("text '{text}' must appear"));
}

fn wait_for_in_semantics(
    robot: &cranpose::Robot,
    mut finder: impl FnMut(&cranpose::Robot) -> Option<(f32, f32, f32, f32)>,
) -> Option<(f32, f32, f32, f32)> {
    for _ in 0..40 {
        if let Some(bounds) = finder(robot) {
            return Some(bounds);
        }
        let _ = robot.wait_for_idle();
        std::thread::sleep(Duration::from_millis(80));
    }
    None
}

fn selected_editable(elements: &[SemanticElement]) -> Option<SelectedEditable> {
    for element in elements {
        if element.editable_text {
            if let Some(selection) = element.text_selection {
                if selection.0 != selection.1 {
                    let bounds = element.bounds;
                    return Some(((bounds.x, bounds.y, bounds.width, bounds.height), selection));
                }
            }
        }
        if let Some(found) = selected_editable(&element.children) {
            return Some(found);
        }
    }
    None
}

fn count_handle_bands_in(
    shot: &RobotScreenshot,
    rect: (f32, f32, f32, f32),
    target: (i16, i16, i16),
) -> (usize, usize) {
    let (sx, sy) = (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    );
    let (x, y, width, height) = rect;
    let left = (x.max(0.0) * sx) as usize;
    let top = (y.max(0.0) * sy) as usize;
    let right = (((x + width) * sx) as usize).min(shot.width as usize);
    let bottom = (((y + height) * sy) as usize).min(shot.height as usize);
    let upper_end = ((y + height * 0.45) * sy) as usize;
    let lower_start = ((y + height * 0.55) * sy) as usize;
    let mut upper = 0;
    let mut lower = 0;
    for py in top..bottom {
        for px in left..right {
            let i = (py * shot.width as usize + px) * 4;
            let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
            if matches_accent(r, g, b, target) {
                if py < upper_end {
                    upper += 1;
                } else if py >= lower_start {
                    lower += 1;
                }
            }
        }
    }
    (upper, lower)
}

fn count_pixels_in(
    shot: &RobotScreenshot,
    rect: (f32, f32, f32, f32),
    target: (i16, i16, i16),
) -> usize {
    let (sx, sy) = (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    );
    let (x, y, width, height) = rect;
    let left = (x.max(0.0) * sx) as usize;
    let top = (y.max(0.0) * sy) as usize;
    let right = (((x + width) * sx) as usize).min(shot.width as usize);
    let bottom = (((y + height) * sy) as usize).min(shot.height as usize);
    let mut count = 0;
    for py in top..bottom {
        for px in left..right {
            let i = (py * shot.width as usize + px) * 4;
            let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
            if matches_accent(r, g, b, target) {
                count += 1;
            }
        }
    }
    count
}

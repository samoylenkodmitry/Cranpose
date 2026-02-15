//! Shared helpers for robot tests.

#![allow(dead_code)]

/// Count pixels that differ between two screenshots by more than `channel_threshold`
/// on any RGBA channel.
pub fn changed_pixel_count(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    channel_threshold: u8,
) -> usize {
    if before.width != after.width || before.height != after.height {
        return usize::MAX;
    }

    before
        .pixels
        .chunks_exact(4)
        .zip(after.pixels.chunks_exact(4))
        .filter(|(a, b)| {
            a[0].abs_diff(b[0]) > channel_threshold
                || a[1].abs_diff(b[1]) > channel_threshold
                || a[2].abs_diff(b[2]) > channel_threshold
                || a[3].abs_diff(b[3]) > channel_threshold
        })
        .count()
}

/// Count pixels that differ within a sub-region (x, y, w, h) of two screenshots.
pub fn changed_pixel_count_in_region(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
    channel_threshold: u8,
) -> usize {
    if before.width != after.width || before.height != after.height {
        return usize::MAX;
    }

    let left = region.0.max(0.0).floor() as u32;
    let top = region.1.max(0.0).floor() as u32;
    let right = (region.0 + region.2).min(before.width as f32).ceil() as u32;
    let bottom = (region.1 + region.3).min(before.height as f32).ceil() as u32;

    if right <= left || bottom <= top {
        return 0;
    }

    let width = before.width as usize;
    let mut changed = 0usize;
    for y in top..bottom {
        for x in left..right {
            let idx = ((y as usize) * width + x as usize) * 4;
            if before.pixels[idx].abs_diff(after.pixels[idx]) > channel_threshold
                || before.pixels[idx + 1].abs_diff(after.pixels[idx + 1]) > channel_threshold
                || before.pixels[idx + 2].abs_diff(after.pixels[idx + 2]) > channel_threshold
                || before.pixels[idx + 3].abs_diff(after.pixels[idx + 3]) > channel_threshold
            {
                changed += 1;
            }
        }
    }

    changed
}

/// Parse "label: value" text from a slider label, returning the numeric value.
pub fn parse_slider_value(text: &str) -> Option<f32> {
    text.split_once(':')
        .and_then(|(_, value)| value.trim().parse::<f32>().ok())
}

/// Scroll down by dragging from `from_y` to `to_y` at `center_x`.
pub fn scroll_down(robot: &cranpose::Robot, center_x: f32, from_y: f32, to_y: f32) {
    let _ = robot.drag(center_x, from_y, center_x, to_y);
    std::thread::sleep(std::time::Duration::from_millis(180));
    let _ = robot.wait_for_idle();
}

/// Scroll up by dragging from `from_y` to `to_y` at `center_x`.
pub fn scroll_up(robot: &cranpose::Robot, center_x: f32, from_y: f32, to_y: f32) {
    let _ = robot.drag(center_x, from_y, center_x, to_y);
    std::thread::sleep(std::time::Duration::from_millis(180));
    let _ = robot.wait_for_idle();
}

/// Check whether a given Y coordinate falls within the visible viewport
/// (with 28px margin top and bottom).
pub fn y_is_visible(robot: &cranpose::Robot, y: f32) -> bool {
    let Some((_, root_y, _, root_h)) = cranpose_testing::root_bounds(robot) else {
        return true;
    };
    let top = root_y + 28.0;
    let bottom = root_y + root_h - 28.0;
    y >= top && y <= bottom
}

/// Scroll until a semantics node with the given `prefix` text is visible.
/// Returns bounds + full text `(x, y, w, h, text)`.
pub fn scroll_prefix_into_view(
    robot: &cranpose::Robot,
    prefix: &str,
    max_attempts: usize,
    scroll_center_x: f32,
    scroll_down_from_y: f32,
    scroll_down_to_y: f32,
    scroll_up_from_y: f32,
    scroll_up_to_y: f32,
) -> Option<(f32, f32, f32, f32, String)> {
    for attempt in 0..max_attempts {
        if let Some(bounds) = cranpose_testing::find_text_by_prefix_in_semantics(robot, prefix) {
            let center_y = bounds.1 + bounds.3 * 0.5;
            if y_is_visible(robot, center_y) {
                return Some(bounds);
            }
            let Some((_, root_y, _, root_h)) = cranpose_testing::root_bounds(robot) else {
                return Some(bounds);
            };
            let viewport_mid = root_y + root_h * 0.5;
            if center_y > viewport_mid {
                scroll_down(robot, scroll_center_x, scroll_down_from_y, scroll_down_to_y);
            } else {
                scroll_up(robot, scroll_center_x, scroll_up_from_y, scroll_up_to_y);
            }
        } else {
            // Not found yet — alternate directions to find it
            if attempt % 2 == 0 {
                scroll_down(robot, scroll_center_x, scroll_down_from_y, scroll_down_to_y);
            } else {
                scroll_up(robot, scroll_center_x, scroll_up_from_y, scroll_up_to_y);
            }
        }
    }
    None
}

/// Set a slider to a given fraction [0, 1] and return the parsed value.
pub fn set_slider_fraction(
    robot: &cranpose::Robot,
    prefix: &str,
    fraction: f32,
    slider_width: f32,
    slider_touch_offset_y: f32,
    scroll_center_x: f32,
    scroll_down_from_y: f32,
    scroll_down_to_y: f32,
    scroll_up_from_y: f32,
    scroll_up_to_y: f32,
) -> Option<f32> {
    let (x, y, _w, h, _) = scroll_prefix_into_view(
        robot,
        prefix,
        18,
        scroll_center_x,
        scroll_down_from_y,
        scroll_down_to_y,
        scroll_up_from_y,
        scroll_up_to_y,
    )?;
    let slider_y = y + h + slider_touch_offset_y;
    let left_x = x + 2.0;
    let target_x = x + slider_width * fraction.clamp(0.0, 1.0);
    let _ = robot.drag(left_x, slider_y, target_x, slider_y);
    std::thread::sleep(std::time::Duration::from_millis(120));
    let _ = robot.wait_for_idle();
    cranpose_testing::find_text_by_prefix_in_semantics(robot, prefix)
        .and_then(|(_, _, _, _, t)| parse_slider_value(&t))
}

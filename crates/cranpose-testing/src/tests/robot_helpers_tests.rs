use cranpose::{RobotScreenshot, SemanticRect};

use super::*;

fn semantic_element(
    role: &str,
    text: Option<&str>,
    clickable: bool,
    bounds: RectBounds,
    children: Vec<SemanticElement>,
) -> SemanticElement {
    SemanticElement {
        role: role.to_string(),
        text: text.map(ToString::to_string),
        state_description: None,
        clickable,
        editable_text: false,
        text_selection: None,
        bounds: SemanticRect {
            x: bounds.0,
            y: bounds.1,
            width: bounds.2,
            height: bounds.3,
        },
        children,
    }
}

#[test]
fn overflow_axis_direction_detects_horizontal_overflow() {
    let root = (0.0, 0.0, 300.0, 200.0);
    let target = (320.0, 20.0, 80.0, 30.0);
    assert_eq!(
        overflow_axis_direction(target, root),
        Some((TabAxis::Horizontal, -1.0))
    );
}

#[test]
fn overflow_axis_direction_detects_vertical_overflow() {
    let root = (0.0, 0.0, 300.0, 200.0);
    let target = (20.0, -60.0, 80.0, 30.0);
    assert_eq!(
        overflow_axis_direction(target, root),
        Some((TabAxis::Vertical, 1.0))
    );
}

#[test]
fn scroll_delta_for_overflow_moves_target_toward_visible_area() {
    let root = (0.0, 0.0, 300.0, 200.0);

    assert_eq!(
        scroll_delta_for_overflow((320.0, 20.0, 80.0, 30.0), root, TabAxis::Horizontal),
        Some((-104.0, 0.0))
    );
    assert_eq!(
        scroll_delta_for_overflow((-42.0, 20.0, 80.0, 30.0), root, TabAxis::Horizontal),
        Some((46.0, 0.0))
    );
    assert_eq!(
        scroll_delta_for_overflow((20.0, 190.0, 80.0, 30.0), root, TabAxis::Vertical),
        Some((0.0, -24.0))
    );
}

#[test]
fn visible_bounds_accepts_subpixel_edge_fit() {
    let root = (0.0, 0.0, 1024.0, 768.0);
    let button = (861.17, 28.0, 158.83, 47.6);

    assert_eq!(visible_bounds(Some(button), root), Some(button));
    assert_eq!(
        scroll_delta_for_overflow(button, root, TabAxis::Horizontal),
        None
    );
}

#[test]
fn visible_bounds_accepts_root_origin_button() {
    let root = (0.0, 0.0, 400.0, 300.0);
    let button = (0.0, 0.0, 92.33, 19.6);

    assert_eq!(visible_bounds(Some(button), root), Some(button));
    assert_eq!(overflow_axis_direction(button, root), None);
}

#[test]
fn exact_button_matching_does_not_match_text_input_for_text_tab() {
    let text_input_tab = semantic_element(
        "Layout",
        None,
        true,
        (10.0, 20.0, 120.0, 40.0),
        vec![semantic_element(
            "Text",
            Some("Text Input"),
            false,
            (18.0, 28.0, 96.0, 24.0),
            Vec::new(),
        )],
    );

    assert!(
        find_button_by(&text_input_tab, "Text", text_contains).is_some(),
        "substring matching should keep the public contains behavior"
    );
    assert!(
        find_button_by(&text_input_tab, "Text", text_equals).is_none(),
        "exact tab matching must not treat Text Input as Text"
    );
}

#[test]
fn scroll_anchor_prefers_edge_safe_button_over_clipped_edge_button() {
    let root = (0.0, 0.0, 1080.0, 820.0);
    let target = (1084.0, 28.0, 82.0, 47.6);
    let elements = vec![semantic_element(
        "Layout",
        None,
        false,
        root,
        vec![
            semantic_element(
                "Layout",
                Some("central"),
                true,
                (920.0, 28.0, 76.0, 47.6),
                Vec::new(),
            ),
            semantic_element(
                "Layout",
                Some("edge"),
                true,
                (1019.0, 28.0, 56.0, 47.6),
                Vec::new(),
            ),
        ],
    )];

    assert_eq!(
        find_scroll_anchor(&elements, target, root, TabAxis::Horizontal),
        Some((920.0, 28.0, 76.0, 47.6))
    );
}

#[test]
fn missing_target_scroll_searches_down_with_periodic_reverse() {
    let directions: Vec<_> = (0..8).map(missing_target_scroll_direction).collect();

    assert_eq!(
        directions,
        vec![
            MissingTargetScrollDirection::Down,
            MissingTargetScrollDirection::Down,
            MissingTargetScrollDirection::Down,
            MissingTargetScrollDirection::Up,
            MissingTargetScrollDirection::Down,
            MissingTargetScrollDirection::Down,
            MissingTargetScrollDirection::Down,
            MissingTargetScrollDirection::Up,
        ]
    );
}

#[test]
fn find_scroll_anchor_prefers_cross_axis_overlap() {
    let root = (0.0, 0.0, 300.0, 200.0);
    let target = (320.0, 22.0, 80.0, 28.0);

    let same_row = semantic_element(
        "Layout",
        Some("same-row"),
        true,
        (120.0, 20.0, 80.0, 30.0),
        vec![],
    );
    let other_row = semantic_element(
        "Layout",
        Some("other-row"),
        true,
        (120.0, 130.0, 80.0, 30.0),
        vec![],
    );
    let root_elem = semantic_element("Layout", None, false, root, vec![same_row, other_row]);

    let anchor = find_scroll_anchor(&[root_elem], target, root, TabAxis::Horizontal)
        .expect("expected anchor");

    assert_eq!(anchor, (120.0, 20.0, 80.0, 30.0));
}

#[test]
fn find_clickables_in_range_handles_nan_x_without_panicking() {
    let malformed = semantic_element(
        "Layout",
        None,
        true,
        (f32::NAN, 20.0, 80.0, 30.0),
        vec![semantic_element(
            "Text",
            Some("Malformed"),
            false,
            (f32::NAN, 24.0, 60.0, 18.0),
            vec![],
        )],
    );
    let finite = semantic_element(
        "Layout",
        None,
        true,
        (12.0, 20.0, 80.0, 30.0),
        vec![semantic_element(
            "Text",
            Some("Finite"),
            false,
            (16.0, 24.0, 60.0, 18.0),
            vec![],
        )],
    );

    let clickables = find_clickables_in_range(&[malformed, finite], 0.0, 40.0);

    assert_eq!(clickables.len(), 2);
    assert_eq!(clickables[0].0, "Finite");
    assert_eq!(clickables[1].0, "Malformed");
    assert!(clickables[1].1.is_nan());
}

#[test]
fn find_button_exact_requires_full_text_match() {
    let exact_button = semantic_element(
        "Button",
        None,
        true,
        (10.0, 20.0, 90.0, 28.0),
        vec![
            semantic_element(
                "Text",
                Some("Text"),
                false,
                (14.0, 24.0, 30.0, 20.0),
                vec![],
            ),
            semantic_element(
                "Text",
                Some("Text Input"),
                false,
                (46.0, 24.0, 48.0, 20.0),
                vec![],
            ),
        ],
    );
    let partial_only_button = semantic_element(
        "Button",
        None,
        true,
        (120.0, 20.0, 90.0, 28.0),
        vec![semantic_element(
            "Text",
            Some("Text Input"),
            false,
            (124.0, 24.0, 48.0, 20.0),
            vec![],
        )],
    );

    assert_eq!(
        find_button_by(&exact_button, "Text", text_equals),
        Some((10.0, 20.0, 90.0, 28.0))
    );
    assert_eq!(
        find_button_by(&partial_only_button, "Text", text_equals),
        None
    );
}

#[test]
fn choose_button_bounds_prefers_visible_match_over_offscreen_first_match() {
    let root = (0.0, 0.0, 320.0, 240.0);
    let candidates = vec![(420.0, 12.0, 80.0, 40.0), (24.0, 12.0, 80.0, 40.0)];

    assert_eq!(
        choose_button_bounds(&candidates, root),
        Some((24.0, 12.0, 80.0, 40.0))
    );
}

#[test]
fn visible_bounds_rejects_offscreen_button_candidates() {
    let root = (0.0, 0.0, 320.0, 240.0);

    assert_eq!(
        visible_bounds(Some((12.0, 12.0, 80.0, 40.0)), root),
        Some((12.0, 12.0, 80.0, 40.0))
    );
    assert_eq!(visible_bounds(Some((340.0, 12.0, 80.0, 40.0)), root), None);
    assert_eq!(visible_bounds(Some((-90.0, 12.0, 80.0, 40.0)), root), None);
}

#[test]
fn screenshot_pixel_reads_expected_value() {
    let screenshot = RobotScreenshot {
        width: 2,
        height: 1,
        logical_width: 2.0,
        logical_height: 1.0,
        pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
    };
    assert_eq!(screenshot_pixel(&screenshot, 1, 0), Some([5, 6, 7, 8]));
}

#[test]
fn screenshot_pixel_rejects_truncated_storage() {
    let screenshot = RobotScreenshot {
        width: 2,
        height: 1,
        logical_width: 2.0,
        logical_height: 1.0,
        pixels: vec![1, 2, 3, 4, 5, 6, 7],
    };

    assert_eq!(screenshot_pixel(&screenshot, 1, 0), None);
    assert_eq!(sample_screenshot_pixel_logical(&screenshot, 1.0, 0.0), None);
}

#[test]
fn crop_screenshot_extracts_region() {
    let screenshot = RobotScreenshot {
        width: 3,
        height: 2,
        logical_width: 3.0,
        logical_height: 2.0,
        pixels: vec![
            1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18,
            255,
        ],
    };
    let cropped = crop_screenshot(&screenshot, 1, 0, 2, 2).expect("crop");
    assert_eq!(cropped.width, 2);
    assert_eq!(cropped.height, 2);
    assert_eq!(cropped.logical_width, 2.0);
    assert_eq!(cropped.logical_height, 2.0);
    assert_eq!(
        cropped.pixels,
        vec![4, 5, 6, 255, 7, 8, 9, 255, 13, 14, 15, 255, 16, 17, 18, 255]
    );
}

#[test]
fn normalize_screenshot_region_preserves_pixel_grid_at_native_size() {
    let screenshot = RobotScreenshot {
        width: 2,
        height: 2,
        logical_width: 2.0,
        logical_height: 2.0,
        pixels: vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255],
    };

    let normalized =
        normalize_screenshot_region(&screenshot, (0.0, 0.0, 2.0, 2.0), 2, 2).expect("norm");

    assert_eq!(normalized.width, screenshot.width);
    assert_eq!(normalized.height, screenshot.height);
    assert_eq!(normalized.logical_width, screenshot.logical_width);
    assert_eq!(normalized.logical_height, screenshot.logical_height);
    assert_eq!(normalized.pixels, screenshot.pixels);
}

#[test]
fn changed_pixel_count_in_region_uses_logical_coordinates() {
    let before = RobotScreenshot {
        width: 4,
        height: 4,
        logical_width: 2.0,
        logical_height: 2.0,
        pixels: vec![
            0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0,
            0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0,
            0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255,
        ],
    };
    let mut after = before.clone();
    for y in 2..4 {
        for x in 2..4 {
            let idx = ((y * after.width + x) * 4) as usize;
            after.pixels[idx] = 255;
        }
    }

    assert_eq!(
        changed_pixel_count_in_region(&before, &after, (1.0, 1.0, 1.0, 1.0), 1),
        4
    );
}

#[test]
fn sample_screenshot_pixel_logical_maps_scaled_capture() {
    let screenshot = RobotScreenshot {
        width: 4,
        height: 4,
        logical_width: 2.0,
        logical_height: 2.0,
        pixels: vec![
            1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255, 7,
            0, 0, 255, 8, 0, 0, 255, 9, 0, 0, 255, 10, 0, 0, 255, 11, 0, 0, 255, 12, 0, 0, 255, 13,
            0, 0, 255, 14, 0, 0, 255, 15, 0, 0, 255, 16, 0, 0, 255,
        ],
    };

    assert_eq!(
        sample_screenshot_pixel_logical(&screenshot, 1.25, 1.25),
        Some([11, 0, 0, 255])
    );
}

#[test]
fn screenshot_difference_stats_reports_first_difference() {
    let before = RobotScreenshot {
        width: 2,
        height: 1,
        logical_width: 2.0,
        logical_height: 1.0,
        pixels: vec![10, 20, 30, 255, 1, 2, 3, 255],
    };
    let after = RobotScreenshot {
        width: 2,
        height: 1,
        logical_width: 2.0,
        logical_height: 1.0,
        pixels: vec![10, 20, 30, 255, 4, 8, 3, 200],
    };

    let stats = screenshot_difference_stats(&before, &after, 3).expect("stats");

    assert_eq!(stats.differing_pixels, 1);
    assert_eq!(stats.max_difference, 55);
    assert_eq!(
        stats.first_difference,
        Some(ScreenshotPixelDifference {
            x: 1,
            y: 0,
            before: [1, 2, 3, 255],
            after: [4, 8, 3, 200],
            difference: 55,
        })
    );
}

#[test]
fn screenshot_difference_stats_rejects_truncated_storage() {
    let before = RobotScreenshot {
        width: 2,
        height: 1,
        logical_width: 2.0,
        logical_height: 1.0,
        pixels: vec![10, 20, 30, 255, 1, 2, 3],
    };
    let after = RobotScreenshot {
        width: 2,
        height: 1,
        logical_width: 2.0,
        logical_height: 1.0,
        pixels: vec![10, 20, 30, 255, 4, 8, 3, 200],
    };

    assert_eq!(screenshot_difference_stats(&before, &after, 3), None);
    assert_eq!(changed_pixel_count(&before, &after, 3), usize::MAX);
    assert_eq!(
        changed_pixel_count_in_region(&before, &after, (0.0, 0.0, 2.0, 1.0), 3),
        usize::MAX
    );
}

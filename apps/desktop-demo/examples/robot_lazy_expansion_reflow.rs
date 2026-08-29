#![allow(non_snake_case)]
//! A lazy row that grows in place must push its siblings' pixels, not only
//! their layout bounds.
//!
//! Reproduces the cranscan "..." regression: after one expansion has settled,
//! expanding a second row updates layout (semantics move) while the scoped
//! scene update applies in place without refreshing the shifted siblings, so
//! the screen keeps the old geometry until a scroll forces a full pass. The
//! first expansion after launch tends to ride a full rebuild and look correct,
//! which is why this test expands twice: cold start, expand once, expand
//! again — the second expansion is the pin.
//!
//! The assertion is pixels-versus-semantics: semantics report where layout put
//! a row, the screenshot reports what the scene drew there. On a stale scene
//! they disagree.

use cranpose::AppLauncher;
use cranpose_core::rememberMutableStateOf;
use cranpose_foundation::lazy::{rememberLazyListState, LazyItems, LazyListScope};
use cranpose_ui::{
    composable,
    widgets::{LazyColumn, LazyColumnSpec},
    Box, BoxSpec, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row,
    RowSpec, Size, Text, TextStyle,
};

const ROW_COLORS: [Color; 4] = [
    Color(0.9, 0.1, 0.1, 1.0),
    Color(0.1, 0.8, 0.1, 1.0),
    Color(0.1, 0.2, 0.9, 1.0),
    Color(0.9, 0.8, 0.1, 1.0),
];
const STRIP_COLOR: Color = Color(0.5, 0.0, 0.5, 1.0);
const SWATCH_WIDTH: f32 = 160.0;
const SWATCH_HEIGHT: f32 = 48.0;
const STRIP_HEIGHT: f32 = 60.0;

#[composable]
fn ExpandableRow(index: usize) {
    let expanded = rememberMutableStateOf(|| false);
    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
        move || {
            Row(
                Modifier::empty(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        move || expanded.update(|e| *e = !*e),
                        move || {
                            Text(
                                format!("more{index}"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                    Box(
                        Modifier::empty()
                            .size(Size::new(SWATCH_WIDTH, SWATCH_HEIGHT))
                            .background(ROW_COLORS[index]),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            if expanded.get() {
                Box(
                    Modifier::empty()
                        .size(Size::new(300.0, STRIP_HEIGHT))
                        .background(STRIP_COLOR),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

#[composable]
fn ExpansionReflowScreen() {
    let state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        state,
        LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        |scope| {
            scope.items(LazyItems::new(ROW_COLORS.len()), ExpandableRow);
        },
    );
}

fn color_at(shot: &cranpose::RobotScreenshot, x: f32, y: f32) -> (u8, u8, u8) {
    let sx = (x / shot.logical_width * shot.width as f32) as usize;
    let sy = (y / shot.logical_height * shot.height as f32) as usize;
    let idx = (sy * shot.width as usize + sx) * 4;
    (shot.pixels[idx], shot.pixels[idx + 1], shot.pixels[idx + 2])
}

fn assert_swatch_drawn_at_semantic_bounds(
    robot: &cranpose::Robot,
    index: usize,
    stage: &str,
) -> Result<(), String> {
    let (bx, by, bw, bh) = robot
        .find_button_bounds_exact(&format!("more{index}"))?
        .ok_or_else(|| format!("{stage}: button more{index} not found in semantics"))?;
    let shot = robot.screenshot()?;
    // The swatch sits in the same row, 8dp after the button; its top aligns
    // with the row top and it is at least as tall as the button, so a point a
    // few pixels into it is inside the swatch no matter which is taller.
    let x = bx + bw + 8.0 + 20.0;
    let y = by + (bh.min(SWATCH_HEIGHT)) / 2.0;
    let (r, g, b) = color_at(&shot, x, y);
    let want = ROW_COLORS[index];
    let (wr, wg, wb) = (
        (want.0 * 255.0) as i32,
        (want.1 * 255.0) as i32,
        (want.2 * 255.0) as i32,
    );
    let close = |a: u8, w: i32| (a as i32 - w).abs() <= 40;
    if close(r, wr) && close(g, wg) && close(b, wb) {
        Ok(())
    } else {
        Err(format!(
            "{stage}: row {index}'s swatch is not drawn where semantics place it: \
             semantics put the row at y={by:.0} but the pixel at ({x:.0},{y:.0}) is \
             rgb({r},{g},{b}), expected ~rgb({wr},{wg},{wb}) — the scene kept stale \
             geometry after a sibling grew"
        ))
    }
}

fn main() {
    AppLauncher::new()
        .with_title("lazy expansion reflow")
        .with_size(520, 700)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_test_driver(move |robot| {
            robot.pump_frames(6).expect("compose the list");
            robot.wait_for_idle().expect("settle after launch");
            for index in 0..ROW_COLORS.len() {
                assert_swatch_drawn_at_semantic_bounds(&robot, index, "at rest")
                    .expect("resting frame agrees with semantics");
            }

            // First expansion: often correct even on broken builds, because
            // launch-time dirt forces the scoped scene update to give up and
            // fully rebuild. It is part of the recipe, not the pin.
            robot.click_by_text("more0").expect("expand row 0");
            robot.pump_frames(6).expect("compose the first strip");
            robot.wait_for_idle().expect("settle the first expansion");
            for index in 0..ROW_COLORS.len() {
                assert_swatch_drawn_at_semantic_bounds(&robot, index, "after first expansion")
                    .expect("first expansion reflows");
            }

            // Second expansion: the pin. Layout shifts rows 2 and 3 down by the
            // strip height; the scene must follow.
            robot.click_by_text("more1").expect("expand row 1");
            robot.pump_frames(6).expect("compose the second strip");
            robot.wait_for_idle().expect("settle the second expansion");
            let mut failures = Vec::new();
            for index in 0..ROW_COLORS.len() {
                if let Err(e) =
                    assert_swatch_drawn_at_semantic_bounds(&robot, index, "after second expansion")
                {
                    failures.push(e);
                }
            }
            robot.exit().ok();
            assert!(
                failures.is_empty(),
                "second expansion left the scene stale:\n{}",
                failures.join("\n")
            );
        })
        .run(ExpansionReflowScreen);
}

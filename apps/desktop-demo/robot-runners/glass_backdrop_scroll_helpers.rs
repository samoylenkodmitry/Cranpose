#![allow(dead_code)]

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use cranpose::{Robot, RobotScreenshot, SemanticElement};
use cranpose_testing::{find_in_semantics, find_text_exact};

use crate::{
    robot_exit,
    scroll_stability_external_helpers::{
        save_robot_screenshot, scroll_once_and_expect_target_delta, ExactScrollStepConfig,
        ScrollStepDriver,
    },
};

pub(crate) const WINDOW_WIDTH: u32 = 800;
pub(crate) const WINDOW_HEIGHT: u32 = 632;
pub(crate) const STEP_COUNT: usize = 10;
const SETTLE_SCROLL: f32 = -430.0;
const SCROLL_DELTA_Y: f32 = -1.0;
const STEP_EPSILON: f32 = 0.05;
const SETTLE_AFTER_SCROLL_EXTRA_MS: u64 = 1_500;
const GLASS_BAR: (f32, f32, f32, f32) = (28.0, 104.0, 744.0, 56.0);
const CHANNEL_TOLERANCE: i32 = 2;
const RECEIPT_ANCHOR_SKIP_FROM_TOP: usize = 2;
const COLD_ALIGN_ATTEMPTS: usize = 40;
const AWAY_TAB: &str = "Markdown";
const RECEIPTS_TAB: &str = "Receipts";
const RECEIPTS_HEADING: &str = "Library";

pub(crate) struct GlassBackdropScrollRun<'a> {
    pub robot: &'a Robot,
    pub output_dir: PathBuf,
    pub capture: &'a mut dyn FnMut(&Robot) -> RobotScreenshot,
}

struct IncrementalStep {
    anchor_y: f32,
    screenshot: RobotScreenshot,
    path: PathBuf,
}

impl GlassBackdropScrollRun<'_> {
    pub(crate) fn run(mut self) {
        std::fs::create_dir_all(&self.output_dir).expect("create output dir");
        println!("Output dir: {}", self.output_dir.display());
        let robot = self.robot;
        std::thread::sleep(Duration::from_millis(1_000));
        let _ = robot.wait_for_idle();
        open_receipts_tab(robot);
        let anchor_text = self.settle_and_pick_anchor();
        let steps = self.walk_incrementally(anchor_text);
        let mismatches = self.compare_against_cold_renders(anchor_text, &steps);
        if !mismatches.is_empty() {
            robot_exit::fail(
                robot,
                &format!(
                    "the glass bar rendered after one-pixel scroll steps differs from a cold render of the same scroll position:\n{}\ncaptures retained in {}",
                    mismatches.join("\n"),
                    self.output_dir.display()
                ),
            );
        }
        for step in &steps {
            let _ = std::fs::remove_file(&step.path);
            let _ = std::fs::remove_file(cold_path(&self.output_dir, &step.path));
        }
        println!(
            "PASS: the glass bar matched a cold render at every one of {STEP_COUNT} one-pixel scroll steps"
        );
        robot.exit().expect("exit");
    }

    fn settle_and_pick_anchor(&mut self) -> &'static str {
        let robot = self.robot;
        settle_list(robot, SETTLE_SCROLL);
        let semantics = robot.get_semantics().expect("query semantics");
        let mut receipts = Vec::new();
        for root in &semantics {
            collect_receipt_subtitles(root, &mut receipts);
        }
        receipts.sort_by(|a, b| a.1.partial_cmp(&b.1).expect("finite y"));
        let anchor_text = receipts
            .get(RECEIPT_ANCHOR_SKIP_FROM_TOP)
            .map(|(text, _)| text.clone())
            .unwrap_or_else(|| {
                robot_exit::fail(
                    robot,
                    &format!(
                        "expected at least {} visible receipt subtitles after settle, found {}",
                        RECEIPT_ANCHOR_SKIP_FROM_TOP + 1,
                        receipts.len()
                    ),
                )
            });
        println!(
            "tracking content anchor: {anchor_text:?} (of {} visible)",
            receipts.len()
        );
        Box::leak(anchor_text.into_boxed_str())
    }

    fn walk_incrementally(&mut self, anchor_text: &'static str) -> Vec<IncrementalStep> {
        let robot = self.robot;
        let mut previous_bounds = anchor_bounds(robot, anchor_text, "after settle");
        let step_config = ExactScrollStepConfig {
            target_text: anchor_text,
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
            scroll_steps: STEP_COUNT,
            scroll_delta_y: SCROLL_DELTA_Y,
            step_epsilon: STEP_EPSILON,
            fallback_trim_top_px: GLASS_BAR.1.round() as u32,
            fallback_trim_bottom_px: (WINDOW_HEIGHT as f32 - (GLASS_BAR.1 + GLASS_BAR.3)).round()
                as u32,
        };
        let mut steps = Vec::with_capacity(STEP_COUNT);
        for step in 0..STEP_COUNT {
            previous_bounds = scroll_once_and_expect_target_delta(
                robot,
                step_config,
                previous_bounds,
                step,
                "glass-step",
                ScrollStepDriver::PointerWheel,
            );
            let screenshot = (self.capture)(robot);
            let path = self
                .output_dir
                .join(format!("glass_step{:02}.png", step + 1));
            save_robot_screenshot(&path, &screenshot);
            if let Some(previous) = steps.last() {
                let previous: &IncrementalStep = previous;
                println!(
                    "step {:02}: bar pixels changed since the previous step: {}",
                    step + 1,
                    mismatching_pixels(&previous.screenshot, &screenshot, GLASS_BAR)
                );
            }
            steps.push(IncrementalStep {
                anchor_y: previous_bounds.1,
                screenshot,
                path,
            });
            std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SCROLL_EXTRA_MS));
        }
        steps
    }

    fn compare_against_cold_renders(
        &mut self,
        anchor_text: &'static str,
        steps: &[IncrementalStep],
    ) -> Vec<String> {
        let robot = self.robot;
        let mut mismatches = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            click_tab(robot, AWAY_TAB);
            open_receipts_tab(robot);
            settle_list(robot, SETTLE_SCROLL + (index + 1) as f32 * SCROLL_DELTA_Y);
            align_anchor(robot, anchor_text, step.anchor_y);
            let cold = (self.capture)(robot);
            save_robot_screenshot(&cold_path(&self.output_dir, &step.path), &cold);
            let mismatching = mismatching_pixels(&step.screenshot, &cold, GLASS_BAR);
            let stale_hint = index
                .checked_sub(1)
                .filter(|previous| {
                    mismatching_pixels(&steps[*previous].screenshot, &step.screenshot, GLASS_BAR)
                        == 0
                })
                .map(|_| " (identical to the previous incremental step)")
                .unwrap_or_default();
            println!(
                "step {:02}: bar pixels differing from the cold render: {mismatching}{stale_hint}",
                index + 1
            );
            if mismatching > 0 {
                mismatches.push(format!(
                    "step {:02}: {mismatching} bar pixels differ from the cold render{stale_hint}",
                    index + 1
                ));
            }
        }
        mismatches
    }
}

fn cold_path(output_dir: &Path, step_path: &Path) -> PathBuf {
    let name = step_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("step capture file name");
    output_dir.join(name.replace("glass_step", "glass_cold"))
}

fn settle_list(robot: &Robot, delta_y: f32) {
    robot
        .mouse_move(WINDOW_WIDTH as f32 * 0.5, WINDOW_HEIGHT as f32 * 0.7)
        .expect("move cursor over list");
    std::thread::sleep(Duration::from_millis(30));
    robot
        .mouse_scroll_and_wait_for_frame(0.0, delta_y)
        .expect("settle scroll");
    std::thread::sleep(Duration::from_millis(500));
    let _ = robot.wait_for_idle();
}

fn align_anchor(robot: &Robot, anchor_text: &'static str, target_y: f32) {
    for _ in 0..COLD_ALIGN_ATTEMPTS {
        let bounds = anchor_bounds(robot, anchor_text, "after remount");
        let error = bounds.1 - target_y;
        if error.abs() <= STEP_EPSILON {
            return;
        }
        println!("cold align: anchor_y={:.2} target={target_y:.2}", bounds.1);
        robot
            .mouse_scroll_and_wait_for_frame(0.0, -error.round().clamp(-1.0, 1.0))
            .expect("cold align step");
        std::thread::sleep(Duration::from_millis(SETTLE_AFTER_SCROLL_EXTRA_MS));
        let _ = robot.wait_for_idle();
    }
    robot_exit::fail(
        robot,
        &format!("could not align the cold render's anchor to y={target_y:.2}"),
    );
}

fn anchor_bounds(robot: &Robot, anchor_text: &'static str, context: &str) -> (f32, f32, f32, f32) {
    find_in_semantics(robot, |elem| find_text_exact(elem, anchor_text)).unwrap_or_else(|| {
        robot_exit::fail(
            robot,
            &format!("content anchor {anchor_text:?} should be visible {context}"),
        )
    })
}

fn region_pixels(
    shot: &RobotScreenshot,
    region: (f32, f32, f32, f32),
) -> (usize, usize, usize, usize) {
    let scale = shot.width as f32 / shot.logical_width;
    (
        (region.0 * scale).round() as usize,
        (region.1 * scale).round() as usize,
        (region.2 * scale).round() as usize,
        (region.3 * scale).round() as usize,
    )
}

fn mismatching_pixels(
    a: &RobotScreenshot,
    b: &RobotScreenshot,
    region: (f32, f32, f32, f32),
) -> usize {
    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "captures must share a size"
    );
    let (x, y, width, height) = region_pixels(a, region);
    let mut mismatching = 0;
    for row in y..y + height {
        for column in x..x + width {
            let index = (row * a.width as usize + column) * 4;
            let differs = (0..3).any(|channel| {
                (a.pixels[index + channel] as i32 - b.pixels[index + channel] as i32).abs()
                    > CHANNEL_TOLERANCE
            });
            if differs {
                mismatching += 1;
            }
        }
    }
    mismatching
}

fn collect_receipt_subtitles(elem: &SemanticElement, out: &mut Vec<(String, f32)>) {
    if let Some(text) = elem.text.as_deref() {
        if text.starts_with("Receipt #") {
            out.push((text.to_string(), elem.bounds.y));
        }
    }
    for child in &elem.children {
        collect_receipt_subtitles(child, out);
    }
}

fn click_tab(robot: &Robot, label: &str) {
    let (x, y, w, h) = cranpose_testing::find_button_in_semantics(robot, label)
        .unwrap_or_else(|| robot_exit::fail(robot, &format!("tab {label:?} not found")));
    robot.click(x + w * 0.5, y + h * 0.5).expect("click tab");
    std::thread::sleep(Duration::from_millis(400));
    let _ = robot.wait_for_idle();
}

pub(crate) fn open_receipts_tab(robot: &Robot) {
    for _ in 0..30 {
        if let Some((x, y, w, h)) = cranpose_testing::find_button_in_semantics(robot, RECEIPTS_TAB)
        {
            robot
                .click(x + w * 0.5, y + h * 0.5)
                .expect("click Receipts tab");
            std::thread::sleep(Duration::from_millis(250));
            let _ = robot.wait_for_idle();
            if cranpose_testing::find_text_in_semantics(robot, RECEIPTS_HEADING).is_some() {
                std::thread::sleep(Duration::from_millis(400));
                let _ = robot.wait_for_idle();
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    robot_exit::fail(
        robot,
        "Receipts tab / Library glass bar not found after 30 attempts",
    );
}

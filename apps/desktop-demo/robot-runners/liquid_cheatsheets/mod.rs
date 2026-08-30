#![allow(non_snake_case)]

mod capture;

#[path = "../text_showcase_external_helpers.rs"]
mod x11_helpers;

use std::{
    fmt::Display,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use capture::{
    capture_x11_keyframes, capture_x11_static_keyframe, compose_comparison,
    save_exact_robot_keyframe_crops, ActualTiming, CaptureRequest, ComparisonGrid, Crop, Keyframe,
};
use cranpose::{
    widgets::{BasicTextFieldOptions, BasicTextFieldWithOptions, Box, BoxSpec, Text},
    AppLauncher, Color, Modifier, RobotTimelineAction, RobotTimelineStep, Size,
};
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::text::{AnnotatedString, TextStyle, TextUnit};
use desktop_app::app;

const LIQUID_WINDOW_SIZE: (f32, f32) = (900.0, 800.0);
const TEXT_WINDOW_SIZE: (f32, f32) = (460.0, 340.0);
const TEXT_FIELD_X: f32 = 20.0;
const TEXT_FIELD_Y: f32 = 170.0;
const TEXT_FIELD_WIDTH: f32 = 420.0;
const TEXT_CONTENT: &str =
    "Silence. Melody. Then beats. Subtle electronic beats goaantra trance pp ulsy catching melody";
const IOS_FORM_REFERENCE_SCALE: f32 = 3.0;

type Bounds = (f32, f32, f32, f32);

struct OnWhiteBar {
    translate: Bounds,
    camera: Bounds,
    conversation: Bounds,
    crop: Crop,
}

trait RobotResultContext<T> {
    fn context(self, message: impl Display) -> Result<T>;
}

impl<T> RobotResultContext<T> for std::result::Result<T, String> {
    fn context(self, message: impl Display) -> Result<T> {
        self.map_err(|error| anyhow::anyhow!("{message}: {error}"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum Case {
    TogglePress,
    MenuOpen,
    TabSwipe,
    Segmented,
    MenuExpand,
    BottomBarForm,
    OnWhiteClick,
    OnWhiteClickHold,
    OnWhiteTouchedUp,
    TextSelection,
}

impl Case {
    fn slug(self) -> &'static str {
        match self {
            Self::TogglePress => "toggle_press",
            Self::MenuOpen => "menu_open",
            Self::TabSwipe => "tab_swipe",
            Self::Segmented => "segmented",
            Self::MenuExpand => "menu_expand",
            Self::BottomBarForm => "bottom_bar_form",
            Self::OnWhiteClick => "on_white_click",
            Self::OnWhiteClickHold => "on_white_click_hold",
            Self::OnWhiteTouchedUp => "on_white_touched_up",
            Self::TextSelection => "text_selection",
        }
    }

    fn window_title(self) -> String {
        format!("Liquid fixture - {}", self.slug())
    }

    fn fixture_dir(self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("robot-runners/liquid_cheatsheets/cases")
            .join(self.slug())
    }

    fn target_sheet(self) -> PathBuf {
        self.fixture_dir().join("target-sheet.png")
    }

    fn expected_frame_count(self) -> Result<usize> {
        let manifest = self.fixture_dir().join("case.toml");
        let contents = std::fs::read_to_string(&manifest)
            .with_context(|| format!("read fixture manifest {}", manifest.display()))?;
        let count = contents
            .lines()
            .find_map(|line| line.trim().strip_prefix("frame_count = "))
            .context("fixture manifest has no frame_count")?
            .parse::<usize>()
            .context("parse fixture frame_count")?;
        if count == 0 {
            bail!("fixture frame_count must be positive");
        }
        Ok(count)
    }

    fn comparison_grid(self) -> ComparisonGrid {
        match self {
            Self::OnWhiteClick | Self::OnWhiteClickHold => ComparisonGrid {
                columns: 5,
                tile_width: 320,
                gap: 8,
            },
            Self::OnWhiteTouchedUp => ComparisonGrid {
                columns: 5,
                tile_width: 220,
                gap: 8,
            },
            _ => ComparisonGrid {
                columns: 4,
                tile_width: 260,
                gap: 10,
            },
        }
    }

    fn actual_timing(self) -> ActualTiming {
        match self {
            Self::BottomBarForm => ActualTiming::SettledState,
            Self::TextSelection => ActualTiming::MixedPhases,
            _ => ActualTiming::PresentedInteraction,
        }
    }
}

pub fn run(case: Case) -> Result<()> {
    let output = std::env::var_os("ROBOT_SHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/liquid-cheatsheets")
                .join(case.slug())
        });
    ensure_generated_output(&output, &case.fixture_dir())?;

    if case == Case::TextSelection {
        run_text_selection(case, output)
    } else {
        run_liquid_case(case, output)
    }
}

fn run_liquid_case(case: Case, output: PathBuf) -> Result<()> {
    let title = case.window_title();
    let driver_title = title.clone();
    let target = case.target_sheet();
    let expected_frames = case.expected_frame_count()?;
    let fixture_case = liquid_fixture_case(case)?;
    AppLauncher::new()
        .with_title(title)
        .with_size(LIQUID_WINDOW_SIZE.0 as u32, LIQUID_WINDOW_SIZE.1 as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_robot_app_hook(move |name, argument| liquid_fixture_app_hook(case, name, argument))
        .with_test_driver(move |robot| {
            settle(&robot, 900);
            let window_id = x11_helpers::find_window_id(&driver_title);
            let frames = capture_liquid_case(case, &robot, &window_id, &output)
                .unwrap_or_else(|error| panic!("{} capture failed: {error:#}", case.slug()));
            compose_comparison(
                &target,
                &frames,
                &output.join("comparison-sheet.png"),
                case.comparison_grid(),
                expected_frames,
                case.actual_timing(),
            )
            .unwrap_or_else(|error| panic!("{} comparison failed: {error:#}", case.slug()));
            println!(
                "PASS: {} comparison written to {}",
                case.slug(),
                output.join("comparison-sheet.png").display()
            );
            robot.exit().expect("exit fixture");
        })
        .try_run(move || app::LiquidReferenceFixture(fixture_case))
        .context("launch Liquid fixture")?;
    Ok(())
}

fn liquid_fixture_app_hook(
    case: Case,
    name: String,
    argument: String,
) -> std::result::Result<Option<String>, String> {
    if case != Case::TabSwipe || name != "tab-swipe-page" {
        return Err(format!(
            "unsupported {} fixture app hook {name}({argument})",
            case.slug()
        ));
    }
    let page = argument
        .parse::<usize>()
        .map_err(|error| format!("parse tab-swipe reference page: {error}"))?;
    app::set_tab_swipe_reference_page(page)?;
    Ok(None)
}

fn liquid_fixture_case(case: Case) -> Result<app::LiquidReferenceFixtureCase> {
    Ok(match case {
        Case::TogglePress => app::LiquidReferenceFixtureCase::TogglePress,
        Case::MenuOpen => app::LiquidReferenceFixtureCase::MenuOpen,
        Case::TabSwipe => app::LiquidReferenceFixtureCase::TabSwipe,
        Case::Segmented => app::LiquidReferenceFixtureCase::Segmented,
        Case::MenuExpand => app::LiquidReferenceFixtureCase::MenuExpand,
        Case::BottomBarForm => app::LiquidReferenceFixtureCase::BottomBarForm,
        Case::OnWhiteClick => app::LiquidReferenceFixtureCase::OnWhiteClick,
        Case::OnWhiteClickHold => app::LiquidReferenceFixtureCase::OnWhiteClickHold,
        Case::OnWhiteTouchedUp => app::LiquidReferenceFixtureCase::OnWhiteTouchedUp,
        Case::TextSelection => bail!("text selection has a dedicated text-field fixture"),
    })
}

fn run_text_selection(case: Case, output: PathBuf) -> Result<()> {
    let title = case.window_title();
    let driver_title = title.clone();
    let target = case.target_sheet();
    let expected_frames = case.expected_frame_count()?;
    AppLauncher::new()
        .with_title(title)
        .with_size(TEXT_WINDOW_SIZE.0 as u32, TEXT_WINDOW_SIZE.1 as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_test_driver(move |robot| {
            settle(&robot, 700);
            let window_id = x11_helpers::find_window_id(&driver_title);
            let frames = capture_text_selection(&robot, &window_id, &output)
                .unwrap_or_else(|error| panic!("text selection capture failed: {error:#}"));
            compose_comparison(
                &target,
                &frames,
                &output.join("comparison-sheet.png"),
                case.comparison_grid(),
                expected_frames,
                case.actual_timing(),
            )
            .unwrap_or_else(|error| panic!("text selection comparison failed: {error:#}"));
            println!(
                "PASS: text_selection comparison written to {}",
                output.join("comparison-sheet.png").display()
            );
            robot.exit().expect("exit fixture");
        })
        .try_run(TextSelectionFixture)
        .context("launch text-selection fixture")?;
    Ok(())
}

fn capture_liquid_case(
    case: Case,
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    match case {
        Case::TogglePress => capture_toggle_press(robot, window_id, output),
        Case::MenuOpen => capture_menu_open(robot, window_id, output),
        Case::TabSwipe => capture_tab_swipe(robot, window_id, output),
        Case::Segmented => capture_segmented(robot, window_id, output),
        Case::MenuExpand => capture_menu_expand(robot, window_id, output),
        Case::BottomBarForm => capture_bottom_bar_form(robot, window_id, output),
        Case::OnWhiteClick => capture_on_white_click(robot, window_id, output),
        Case::OnWhiteClickHold => capture_on_white_click_hold(robot, window_id, output),
        Case::OnWhiteTouchedUp => capture_on_white_touched_up(robot, window_id, output),
        Case::TextSelection => bail!("text selection uses its dedicated fixture"),
    }
}

fn capture_toggle_press(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let toggle = find_button(robot, "Wi-Fi switch")?;
    let (x, y) = center(toggle);
    robot.click(x, y).context("prime toggle off")?;
    settle(robot, 1_200);
    robot.mouse_move(x, y).context("hover toggle")?;
    robot.mouse_down().context("hold toggle")?;
    settle(robot, 220);
    let keyframes = source_keyframes(500, &[0, 150, 300, 450, 600, 750, 883]);
    capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            output,
            LIQUID_WINDOW_SIZE,
            crop_within(
                Crop {
                    x: x - 62.0,
                    y: y - 32.0,
                    width: 113.0,
                    height: 63.0,
                },
                LIQUID_WINDOW_SIZE,
            ),
            Duration::from_millis(1_000),
            &keyframes,
        ),
        |robot| {
            robot.mouse_up().context("release toggle")?;
            Ok(())
        },
        |_, _| Ok(()),
    )
}

fn capture_menu_open(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let card = find_button(robot, "Featured videos")?;
    let anchor = (card.0 + card.2 - 34.0, card.1 + 34.0);
    let keyframes = relative_keyframes(&[0, 100, 200, 300, 500, 800, 1_100, 1_383]);
    capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            output,
            LIQUID_WINDOW_SIZE,
            crop_within(
                Crop {
                    x: card.0 - 20.0,
                    y: card.1 - 40.0,
                    width: 360.0,
                    height: 274.0,
                },
                LIQUID_WINDOW_SIZE,
            ),
            Duration::from_millis(1_500),
            &keyframes,
        ),
        move |robot| {
            robot.click(anchor.0, anchor.1).context("open menu")?;
            Ok(())
        },
        |_, _| Ok(()),
    )
}

fn capture_tab_swipe(
    robot: &cranpose::Robot,
    _window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let discover = find_button(robot, "Discover")?;
    let account = find_button(robot, "Account")?;
    let (start_x, y) = center(discover);
    let (end_x, _) = center(account);
    let tab_pitch = (end_x - start_x) / 3.0;
    let mid_swipe_x = start_x + 1.65 * tab_pitch;
    let wwdc_x = start_x + 2.0 * tab_pitch;
    let initial_x = start_x + 0.55 * tab_pitch;
    robot
        .invoke_app_hook("tab-swipe-page", "0")
        .context("pin initial tab-swipe backdrop page")?;
    robot
        .click(start_x, y)
        .context("arm tab backdrop timeline")?;
    let shots = capture_exact_tab_swipe(robot, y, start_x, initial_x, mid_swipe_x, end_x, wwdc_x)?;
    let labels = TAB_SWIPE_CAPTURE_OFFSETS_MS
        .iter()
        .map(|offset| format!("{offset:06}ms-{}ms", 14_400 + offset))
        .collect::<Vec<_>>();
    let frames = save_exact_robot_keyframe_crops(
        &shots,
        output,
        Crop {
            x: (LIQUID_WINDOW_SIZE.0 - app::TAB_SWIPE_REFERENCE_STAGE_WIDTH) * 0.5,
            y: (LIQUID_WINDOW_SIZE.1 - app::TAB_SWIPE_REFERENCE_STAGE_HEIGHT) * 0.5,
            width: app::TAB_SWIPE_REFERENCE_STAGE_WIDTH,
            height: app::TAB_SWIPE_REFERENCE_STAGE_HEIGHT,
        },
        &labels,
    )?;

    Ok(frames)
}

const TAB_SWIPE_CAPTURE_OFFSETS_MS: [u64; 8] = [0, 267, 533, 800, 1_067, 1_333, 1_600, 1_867];
const TAB_SWIPE_REVERSAL_START_MS: u64 = 1_767;
const TAB_SWIPE_REVERSAL_END_MS: u64 = 1_867;
fn tab_swipe_segment_position(at_ms: u64, from_ms: u64, to_ms: u64, from_x: f32, to_x: f32) -> f32 {
    let progress =
        (at_ms.saturating_sub(from_ms) as f32 / (to_ms - from_ms).max(1) as f32).clamp(0.0, 1.0);
    let eased = progress * progress * (3.0 - 2.0 * progress);
    from_x + (to_x - from_x) * eased
}

fn exact_pointer_segment_times(duration_ms: u64) -> Vec<u64> {
    let mut samples = Vec::new();
    let mut at = 16;
    while at < duration_ms {
        samples.push(at);
        at += 16;
    }
    if duration_ms > 0 {
        samples.push(duration_ms);
    }
    samples
}

fn tab_swipe_linear_position(at_ms: u64, from_ms: u64, to_ms: u64, from_x: f32, to_x: f32) -> f32 {
    let progress =
        (at_ms.saturating_sub(from_ms) as f32 / (to_ms - from_ms).max(1) as f32).clamp(0.0, 1.0);
    from_x + (to_x - from_x) * progress
}

fn tab_swipe_pointer_position(
    at_ms: u64,
    initial_x: f32,
    mid_x: f32,
    account_x: f32,
    wwdc_x: f32,
) -> Option<f32> {
    match at_ms {
        1..=267 => Some(tab_swipe_segment_position(at_ms, 0, 267, initial_x, mid_x)),
        268..=650 => Some(tab_swipe_segment_position(
            at_ms, 267, 650, mid_x, account_x,
        )),
        TAB_SWIPE_REVERSAL_START_MS..=TAB_SWIPE_REVERSAL_END_MS => Some(tab_swipe_linear_position(
            at_ms,
            TAB_SWIPE_REVERSAL_START_MS,
            TAB_SWIPE_REVERSAL_END_MS,
            account_x,
            wwdc_x,
        )),
        _ => None,
    }
}

fn tab_swipe_exact_timeline() -> Vec<u64> {
    let mut times = std::collections::BTreeSet::from(TAB_SWIPE_CAPTURE_OFFSETS_MS);
    for (start, end) in [
        (0, 267),
        (267, 650),
        (TAB_SWIPE_REVERSAL_START_MS, TAB_SWIPE_REVERSAL_END_MS),
    ] {
        let mut at = start + 16;
        while at < end {
            times.insert(at);
            at += 16;
        }
        times.insert(end);
    }
    times.extend([850, 1_660, TAB_SWIPE_REVERSAL_START_MS]);
    times.into_iter().collect()
}

fn capture_exact_tab_swipe(
    robot: &cranpose::Robot,
    y: f32,
    start_x: f32,
    initial_x: f32,
    mid_x: f32,
    account_x: f32,
    wwdc_x: f32,
) -> Result<Vec<cranpose::RobotScreenshot>> {
    let mut steps = vec![RobotTimelineStep {
        advance_ms: 0.0,
        actions: vec![
            RobotTimelineAction::MoveTo { x: start_x, y },
            RobotTimelineAction::MouseDown,
        ],
        capture: false,
    }];
    let mut previous_ms = 0;
    for at_ms in exact_pointer_segment_times(160) {
        steps.push(RobotTimelineStep {
            advance_ms: (at_ms - previous_ms) as f32,
            actions: vec![RobotTimelineAction::MoveTo {
                x: tab_swipe_linear_position(at_ms, 0, 160, start_x, initial_x),
                y,
            }],
            capture: false,
        });
        previous_ms = at_ms;
    }
    previous_ms = 0;
    for at_ms in tab_swipe_exact_timeline() {
        let advance_ms = at_ms - previous_ms;
        previous_ms = at_ms;
        let mut actions = Vec::new();
        if at_ms == 850 {
            actions.push(RobotTimelineAction::MouseUp);
            actions.push(RobotTimelineAction::InvokeAppHook {
                name: "tab-swipe-page".to_string(),
                argument: "2".to_string(),
            });
        } else if at_ms == 1_660 {
            actions.push(RobotTimelineAction::InvokeAppHook {
                name: "tab-swipe-page".to_string(),
                argument: "3".to_string(),
            });
            actions.push(RobotTimelineAction::MoveTo { x: account_x, y });
            actions.push(RobotTimelineAction::MouseDown);
        }
        if let Some(x) = tab_swipe_pointer_position(at_ms, initial_x, mid_x, account_x, wwdc_x) {
            actions.push(RobotTimelineAction::MoveTo { x, y });
        }
        steps.push(RobotTimelineStep {
            advance_ms: advance_ms as f32,
            actions,
            capture: TAB_SWIPE_CAPTURE_OFFSETS_MS.contains(&at_ms),
        });
    }
    steps.push(RobotTimelineStep {
        advance_ms: (1_930 - previous_ms) as f32,
        actions: vec![RobotTimelineAction::MouseUp],
        capture: false,
    });
    let shots = robot
        .capture_interaction_keyframes(IOS_FORM_REFERENCE_SCALE, &steps)
        .context("capture atomic exact-clock tab-swipe interaction")?;
    if shots.len() != TAB_SWIPE_CAPTURE_OFFSETS_MS.len() {
        bail!(
            "exact tab swipe captured {} frames, expected {}",
            shots.len(),
            TAB_SWIPE_CAPTURE_OFFSETS_MS.len()
        );
    }
    Ok(shots)
}

fn capture_segmented(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let receiving = find_button(robot, "Receiving")?;
    let sending = find_button(robot, "Sending")?;
    let errored = find_button(robot, "Errored")?;
    let (receiving_x, y) = center(receiving);
    let (sending_x, _) = center(sending);
    let (errored_x, _) = center(errored);
    robot.click(errored_x, y).context("prime Errored segment")?;
    settle(robot, 900);
    let crop = crop_within(
        Crop {
            x: receiving.0 - 4.0,
            y: y - 22.0,
            width: errored.0 + errored.2 - receiving.0 + 8.0,
            height: 44.0,
        },
        LIQUID_WINDOW_SIZE,
    );
    let mut tap_keyframes = source_keyframes(6_950, &[0, 183, 367, 550, 733]);
    for keyframe in &mut tap_keyframes {
        keyframe.label = format!("tap · {}", keyframe.label);
    }
    let mut frames = capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            &output.join("segments/tap"),
            LIQUID_WINDOW_SIZE,
            crop,
            Duration::from_millis(820),
            &tap_keyframes,
        ),
        move |robot| {
            robot.mouse_move(sending_x, y).context("hover Sending")?;
            robot.mouse_down().context("press Sending")?;
            Ok(())
        },
        move |robot, epoch| {
            epoch.sleep_until(383);
            robot.mouse_up().context("release Sending")?;
            Ok(())
        },
    )?;
    let drag_keyframes = [0, 533, 1_083, 1_633, 2_183]
        .into_iter()
        .zip([7_700, 8_233, 8_783, 9_333, 9_883])
        .map(|(offset, source)| Keyframe {
            capture_ms: offset,
            label: format!("drag · {source}ms"),
        })
        .collect::<Vec<_>>();
    frames.extend(capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            &output.join("segments/drag"),
            LIQUID_WINDOW_SIZE,
            crop,
            Duration::from_millis(2_300),
            &drag_keyframes,
        ),
        move |robot| {
            robot.mouse_move(sending_x, y).context("re-hover Sending")?;
            robot.mouse_down().context("grab segmented lens")?;
            Ok(())
        },
        move |robot, epoch| {
            for step in 1..=136 {
                let at = step * 16;
                epoch.sleep_until(at);
                let ride = step as f32 / 136.0;
                let triangular = if ride <= 0.58 {
                    ride / 0.58
                } else {
                    (1.0 - ride) / 0.42
                }
                .clamp(0.0, 1.0);
                robot
                    .mouse_move(sending_x + (receiving_x - sending_x) * triangular, y)
                    .context("ride segmented lens")?;
            }
            epoch.sleep_until(2_200);
            robot.mouse_up().context("release segmented lens")?;
            Ok(())
        },
    )?);
    Ok(frames)
}

fn capture_menu_expand(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let pill = find_button(robot, "Sort filter pill")?;
    let (pill_x, pill_y) = center(pill);
    let stage_left = pill_x + pill.2 * 0.5 + 16.0 - 440.0;
    let stage_top = pill.1 - 17.0;
    let crop = crop_within(
        Crop {
            x: stage_left + 120.0,
            y: stage_top - 33.0,
            width: 320.0,
            height: 366.0,
        },
        LIQUID_WINDOW_SIZE,
    );
    let mut frames = capture_liquid_triggered_phase(
        robot,
        window_id,
        output,
        "open",
        crop,
        &[
            phase_frame(0, "open", 4_850),
            phase_frame(367, "open", 5_217),
            phase_frame(733, "open", 5_583),
        ],
        move |robot| {
            robot.click(pill_x, pill_y).context("open filter menu")?;
            Ok(())
        },
    )?;
    let sort = find_button(robot, "Sort by")?;
    let (sort_x, sort_y) = center(sort);
    frames.extend(capture_liquid_triggered_phase(
        robot,
        window_id,
        output,
        "expand",
        crop,
        &[
            phase_frame(0, "expand", 5_550),
            phase_frame(367, "expand", 5_917),
            phase_frame(733, "expand", 6_283),
        ],
        move |robot| {
            robot.click(sort_x, sort_y).context("expand sort rows")?;
            Ok(())
        },
    )?);
    frames.extend(capture_liquid_triggered_phase(
        robot,
        window_id,
        output,
        "collapse",
        crop,
        &[
            phase_frame(0, "collapse", 6_300),
            phase_frame(383, "collapse", 6_683),
            phase_frame(783, "collapse", 7_083),
        ],
        move |robot| {
            robot.click(sort_x, sort_y).context("collapse sort rows")?;
            Ok(())
        },
    )?);
    frames.extend(capture_liquid_triggered_phase(
        robot,
        window_id,
        output,
        "close",
        crop,
        &[
            phase_frame(0, "close", 19_800),
            phase_frame(383, "close", 20_183),
            phase_frame(783, "close", 20_583),
        ],
        move |robot| {
            robot
                .click(stage_left + 50.0, stage_top + 270.0)
                .context("dismiss sort menu")?;
            Ok(())
        },
    )?);
    Ok(frames)
}

fn capture_bottom_bar_form(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let today = find_button(robot, "Today")?;
    let (today_x, y) = center(today);
    let crop = crop_within(
        Crop {
            x: (LIQUID_WINDOW_SIZE.0 - 440.0) * 0.5,
            y: y - 57.0,
            width: 440.0,
            height: 113.0,
        },
        LIQUID_WINDOW_SIZE,
    );
    let mut frames = Vec::new();
    frames.push(capture_static_state(
        window_id,
        output,
        "01-orange-purple",
        crop,
        "state 1 · 4000ms",
    )?);
    robot.click(today_x, y).context("select Today")?;
    settle(robot, 1_100);
    frames.push(capture_static_state(
        window_id,
        output,
        "02-tiles-refracting",
        crop,
        "state 2 · 14500ms",
    )?);
    let stage = find_button(robot, "Headers fold stage")?;
    settle(robot, 700);
    let stage_crop = crop_within(
        Crop {
            x: stage.0,
            y: stage.1,
            width: stage.2,
            height: stage.3,
        },
        LIQUID_WINDOW_SIZE,
    );
    frames.push(capture_static_state(
        window_id,
        output,
        "03-headers-folded",
        stage_crop,
        "state 3 · 21000ms",
    )?);
    if let Ok(games) = find_button(robot, "Games") {
        let (x, y) = center(games);
        robot.click(x, y).context("select Games over headers")?;
        settle(robot, 900);
    }
    frames.push(capture_static_state(
        window_id,
        output,
        "04-over-headers",
        stage_crop,
        "state 4 · 30000ms",
    )?);
    Ok(frames)
}

fn capture_on_white_click(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let bar = prepare_on_white_bar(robot)?;
    let translate = bar.translate;
    let conversation = bar.conversation;
    let crop = bar.crop;
    let (translate_x, y) = center(translate);
    let (conversation_x, _) = center(conversation);
    robot.click(translate_x, y).context("prime Translate")?;
    settle(robot, 1_100);
    let keyframes = native_frame_keyframes(0, 120, 5, 0);
    capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            output,
            LIQUID_WINDOW_SIZE,
            crop,
            Duration::from_millis(2_100),
            &keyframes,
        ),
        move |robot| {
            robot
                .click(conversation_x, y)
                .context("select Conversation")?;
            Ok(())
        },
        |_, _| Ok(()),
    )
}

fn capture_on_white_click_hold(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let bar = prepare_on_white_bar(robot)?;
    let translate = bar.translate;
    let camera = bar.camera;
    let conversation = bar.conversation;
    let crop = bar.crop;
    let (translate_x, y) = center(translate);
    let (camera_x, _) = center(camera);
    let (conversation_x, _) = center(conversation);
    robot.click(translate_x, y).context("prime Translate")?;
    settle(robot, 1_100);
    let keyframes = native_frame_keyframes(190, 360, 5, 0);
    capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            output,
            LIQUID_WINDOW_SIZE,
            crop,
            Duration::from_millis(2_950),
            &keyframes,
        ),
        move |robot| {
            robot
                .mouse_move(translate_x, y)
                .context("hover Translate")?;
            robot.mouse_down().context("hold Translate")?;
            Ok(())
        },
        move |robot, epoch| {
            epoch.sleep_until(650);
            robot.mouse_move(camera_x, y).context("drag to Camera")?;
            epoch.sleep_until(800);
            robot
                .mouse_move(conversation_x, y)
                .context("drag to Conversation")?;
            epoch.sleep_until(2_200);
            robot.mouse_up().context("release Conversation")?;
            Ok(())
        },
    )
}

fn capture_on_white_touched_up(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let more = find_button(robot, "More grouped action")?;
    let confirm = find_button(robot, "Confirm grouped action")?;
    let (more_x, y) = center(more);
    let (confirm_x, _) = center(confirm);
    let keyframes = native_frame_keyframes(250, 400, 5, 0);
    capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            output,
            LIQUID_WINDOW_SIZE,
            crop_within(
                Crop {
                    x: more_x - 122.0,
                    y: y - 101.0,
                    width: 220.0,
                    height: 236.0,
                },
                LIQUID_WINDOW_SIZE,
            ),
            Duration::from_millis(2_650),
            &keyframes,
        ),
        move |robot| {
            robot.mouse_move(confirm_x, y).context("hover confirm")?;
            robot.mouse_down().context("press confirm")?;
            Ok(())
        },
        move |robot, epoch| {
            for step in 1..=6 {
                let at = 250 + step * 80;
                epoch.sleep_until(at);
                let progress = step as f32 / 6.0;
                robot
                    .mouse_move(confirm_x + (more_x - confirm_x) * 0.55 * progress, y)
                    .context("drag confirm toward More")?;
            }
            epoch.sleep_until(780);
            robot.mouse_up().context("release confirm")?;
            Ok(())
        },
    )
}

fn capture_text_selection(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let style = text_style();
    let width_of = |text: &str| -> Result<f32> {
        Ok(robot
            .measure_text(&AnnotatedString::from(text), &style)
            .context("measure fixture text")?
            .width)
    };
    let line_height = robot
        .measure_text(&AnnotatedString::from("Ag"), &style)
        .context("measure fixture line height")?
        .height
        .max(1.0);
    let line_mid = TEXT_FIELD_Y + line_height * 0.5;
    let melody_center =
        TEXT_FIELD_X + 0.5 * (width_of("Silence. ")? + width_of("Silence. Melody")?);
    let end_x = TEXT_FIELD_X + width_of("Silence. Melody")?;
    robot
        .drag(melody_center, line_mid, melody_center, line_mid)
        .context("first selection tap")?;
    std::thread::sleep(Duration::from_millis(120));
    robot
        .drag(melody_center, line_mid, melody_center, line_mid)
        .context("second selection tap")?;
    settle(robot, 700);
    let crop = Crop {
        x: 0.0,
        y: 30.0,
        width: TEXT_WINDOW_SIZE.0,
        height: 230.0,
    };
    let grow_keyframes = vec![
        phase_frame(0, "grow", 1_442),
        phase_frame(150, "grow", 1_592),
        phase_frame(350, "grow", 1_792),
    ];
    let mut frames = capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            &output.join("segments/grow"),
            TEXT_WINDOW_SIZE,
            crop,
            Duration::from_millis(430),
            &grow_keyframes,
        ),
        move |robot| {
            robot
                .touch_down(end_x, line_mid)
                .context("grab end handle")?;
            Ok(())
        },
        |_, _| Ok(()),
    )?;
    frames.push(capture_x11_static_keyframe(capture_request(
        window_id,
        &output.join("segments/steady"),
        TEXT_WINDOW_SIZE,
        crop,
        Duration::from_millis(120),
        &[phase_frame(0, "steady", 10_040)],
    ))?);
    robot
        .touch_move(end_x + 55.0, line_mid)
        .context("drag end handle")?;
    robot
        .wait_for_present_frame()
        .context("present dragged end handle")?;
    let release_keyframes = vec![
        phase_frame(0, "dissolve", 4_392),
        phase_frame(50, "dissolve", 4_442),
        phase_frame(100, "dissolve", 4_492),
        phase_frame(250, "menu", 6_958),
        phase_frame(367, "menu", 7_075),
        phase_frame(484, "menu", 7_192),
    ];
    frames.extend(capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            &output.join("segments/release"),
            TEXT_WINDOW_SIZE,
            crop,
            Duration::from_millis(600),
            &release_keyframes,
        ),
        move |robot| {
            robot
                .touch_up(end_x + 55.0, line_mid)
                .context("release end handle")?;
            Ok(())
        },
        |_, _| Ok(()),
    )?);
    Ok(frames)
}

fn prepare_on_white_bar(robot: &cranpose::Robot) -> Result<OnWhiteBar> {
    let conversation = find_button(robot, "Conversation")?;
    let camera = find_button(robot, "Camera")?;
    let translate = find_button(robot, "Translate")?;
    let (_, y) = center(camera);
    let crop = crop_within(
        Crop {
            x: translate.0 - 5.0,
            y: y - 49.0,
            width: 330.0,
            height: 98.0,
        },
        LIQUID_WINDOW_SIZE,
    );
    Ok(OnWhiteBar {
        translate,
        camera,
        conversation,
        crop,
    })
}

fn source_keyframes(source_start_ms: u64, offsets: &[u64]) -> Vec<Keyframe> {
    offsets
        .iter()
        .map(|offset| Keyframe {
            capture_ms: *offset,
            label: format!("{}ms", source_start_ms + offset),
        })
        .collect()
}

fn relative_keyframes(offsets: &[u64]) -> Vec<Keyframe> {
    offsets
        .iter()
        .map(|offset| Keyframe {
            capture_ms: *offset,
            label: format!("{offset}ms"),
        })
        .collect()
}

fn phase_frame(capture_ms: u64, phase: &str, source_ms: u64) -> Keyframe {
    Keyframe {
        capture_ms,
        label: format!("{phase} · {source_ms}ms"),
    }
}

fn capture_request<'a>(
    window_id: &'a str,
    case_dir: &'a Path,
    logical_window_size: (f32, f32),
    crop: Crop,
    total_duration: Duration,
    keyframes: &'a [Keyframe],
) -> CaptureRequest<'a> {
    CaptureRequest {
        window_id,
        case_dir,
        logical_window_size,
        crop,
        total_duration,
        keyframes,
    }
}

fn capture_liquid_triggered_phase<Trigger>(
    robot: &cranpose::Robot,
    window_id: &str,
    output: &Path,
    segment: &str,
    crop: Crop,
    keyframes: &[Keyframe],
    trigger: Trigger,
) -> Result<Vec<PathBuf>>
where
    Trigger: FnOnce(&cranpose::Robot) -> Result<()>,
{
    let duration_ms = keyframes
        .last()
        .context("captured phase has no keyframes")?
        .capture_ms
        .checked_add(120)
        .context("captured phase duration overflow")?;
    capture_x11_keyframes(
        robot,
        capture_request(
            window_id,
            &output.join("segments").join(segment),
            LIQUID_WINDOW_SIZE,
            crop,
            Duration::from_millis(duration_ms),
            keyframes,
        ),
        trigger,
        |_, _| Ok(()),
    )
}

fn native_frame_keyframes(
    first_frame: u64,
    last_frame: u64,
    step: u64,
    capture_start_ms: u64,
) -> Vec<Keyframe> {
    (first_frame..=last_frame)
        .step_by(step as usize)
        .map(|frame| Keyframe {
            capture_ms: capture_start_ms + (frame - first_frame) * 1_000 / 60,
            label: format!(
                "frame {frame:04} · {}ms",
                (frame - first_frame) * 1_000 / 60
            ),
        })
        .collect()
}

fn capture_static_state(
    window_id: &str,
    output: &Path,
    segment: &str,
    crop: Crop,
    label: &str,
) -> Result<PathBuf> {
    capture_x11_static_keyframe(capture_request(
        window_id,
        &output.join("segments").join(segment),
        LIQUID_WINDOW_SIZE,
        crop,
        Duration::from_millis(100),
        &[Keyframe {
            capture_ms: 0,
            label: label.to_owned(),
        }],
    ))
}

fn ensure_generated_output(output: &Path, fixture_dir: &Path) -> Result<()> {
    let resolved_output = prospective_real_path(output)?;
    let resolved_fixture = std::fs::canonicalize(fixture_dir)
        .with_context(|| format!("resolve fixture directory {}", fixture_dir.display()))?;
    if resolved_output.starts_with(&resolved_fixture)
        || resolved_fixture.starts_with(&resolved_output)
    {
        bail!(
            "generated output must not overlap static fixture assets: {}",
            output.display()
        );
    }
    if let Ok(metadata) = std::fs::symlink_metadata(output) {
        if metadata.file_type().is_symlink() {
            bail!(
                "generated output root must not be a symlink: {}",
                output.display()
            );
        }
        if !metadata.is_dir() {
            bail!(
                "generated output path is not a directory: {}",
                output.display()
            );
        }
    }
    std::fs::create_dir_all(output)
        .with_context(|| format!("create generated output {}", output.display()))?;
    for name in [
        "actual",
        "segments",
        "actual-sheet.png",
        "target-labeled-sheet.png",
        "comparison-sheet.png",
    ] {
        let generated = output.join(name);
        if generated.exists() {
            remove_generated_path(&generated)?;
        }
    }
    Ok(())
}

fn prospective_real_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .context("resolve current directory")?
            .join(path)
    };
    let mut existing = absolute.as_path();
    let mut missing_components = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing_components.push(
                    existing
                        .file_name()
                        .context("generated output has no existing ancestor")?
                        .to_owned(),
                );
                existing = existing
                    .parent()
                    .context("generated output has no existing ancestor")?;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("inspect generated output ancestor {}", existing.display())
                });
            }
        }
    }
    let mut resolved = std::fs::canonicalize(existing)
        .with_context(|| format!("resolve generated output ancestor {}", existing.display()))?;
    for component in missing_components.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn remove_generated_path(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("inspect generated path {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        bail!("generated output contains a symlink: {}", path.display());
    }
    if metadata.is_file() {
        std::fs::remove_file(path)
            .with_context(|| format!("remove generated file {}", path.display()))?;
        return Ok(());
    }
    if !metadata.is_dir() {
        bail!("unsupported generated path type: {}", path.display());
    }
    for entry in std::fs::read_dir(path)
        .with_context(|| format!("read generated directory {}", path.display()))?
    {
        remove_generated_path(&entry.context("read generated output entry")?.path())?;
    }
    std::fs::remove_dir(path)
        .with_context(|| format!("remove generated directory {}", path.display()))?;
    Ok(())
}

fn find_button(robot: &cranpose::Robot, label: &str) -> Result<(f32, f32, f32, f32)> {
    robot
        .find_button_bounds_exact(label)
        .context("query button semantics")?
        .with_context(|| format!("fixture button '{label}' is not visible"))
}

fn crop_within(mut crop: Crop, window: (f32, f32)) -> Crop {
    crop.x = crop.x.clamp(0.0, (window.0 - crop.width).max(0.0));
    crop.y = crop.y.clamp(0.0, (window.1 - crop.height).max(0.0));
    crop.width = crop.width.min(window.0 - crop.x);
    crop.height = crop.height.min(window.1 - crop.y);
    crop
}

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn settle(robot: &cranpose::Robot, milliseconds: u64) {
    std::thread::sleep(Duration::from_millis(milliseconds));
    let _ = robot.wait_for_idle();
}

fn text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(Color(0.94, 0.92, 0.90, 1.0));
    style.span_style.font_size = TextUnit::Sp(16.0);
    style.paragraph_style.line_height = TextUnit::Sp(24.0);
    style
}

#[cranpose::composable]
fn TextSelectionFixture() {
    Box(
        Modifier::empty()
            .size(Size::new(TEXT_WINDOW_SIZE.0, TEXT_WINDOW_SIZE.1))
            .background(Color(0.149, 0.129, 0.125, 1.0)),
        BoxSpec::default(),
        || {
            let mut ghost_style = TextStyle::default();
            ghost_style.span_style.color = Some(Color(0.62, 0.58, 0.56, 1.0));
            ghost_style.span_style.font_size = TextUnit::Sp(15.0);
            Text(
                "Styles  •  cinematic  •  anime  •  catchy  •  beats  •  trance  •  lo-fi  •  vocal",
                Modifier::empty().absolute_offset(34.0, 122.0),
                ghost_style,
            );
            let state = cranpose_core::remember(|| TextFieldState::new(TEXT_CONTENT))
                .with(TextFieldState::clone);
            BasicTextFieldWithOptions(
                state,
                Modifier::empty()
                    .absolute_offset(TEXT_FIELD_X, TEXT_FIELD_Y)
                    .size(Size::new(TEXT_FIELD_WIDTH, 120.0)),
                BasicTextFieldOptions {
                    text_style: text_style(),
                    cursor_color: Color(0.965, 0.208, 0.557, 1.0),
                    ..BasicTextFieldOptions::default()
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_tab_swipe_timeline_contains_every_reference_frame_once() {
        let timeline = tab_swipe_exact_timeline();
        assert!(timeline.windows(2).all(|pair| pair[0] < pair[1]));
        for capture in TAB_SWIPE_CAPTURE_OFFSETS_MS {
            assert_eq!(timeline.iter().filter(|time| **time == capture).count(), 1);
        }
        assert!(timeline.contains(&850));
        assert!(timeline.contains(&1_660));
        assert!(timeline.contains(&TAB_SWIPE_REVERSAL_END_MS));
    }

    #[test]
    fn exact_tab_swipe_trajectory_preserves_speed_and_reversal_phases() {
        let initial = 0.55;
        let mid = 1.65;
        let account = 3.0;
        let wwdc = 2.0;
        let launch =
            tab_swipe_pointer_position(16, initial, mid, account, wwdc).expect("launch position");
        let cruise =
            tab_swipe_pointer_position(267, initial, mid, account, wwdc).expect("cruise position");
        let arrival =
            tab_swipe_pointer_position(650, initial, mid, account, wwdc).expect("arrival position");
        let reversal = tab_swipe_pointer_position(1_817, initial, mid, account, wwdc)
            .expect("reversal position");
        assert!(initial < launch && launch < cruise && cruise < arrival);
        assert_eq!(arrival, account);
        assert!(wwdc < reversal && reversal < account);
        assert_eq!(
            tab_swipe_pointer_position(TAB_SWIPE_REVERSAL_END_MS, initial, mid, account, wwdc,),
            Some(wwdc)
        );
    }

    #[test]
    fn exact_tab_swipe_preroll_has_real_pointer_cadence_before_frame_zero() {
        let samples = exact_pointer_segment_times(160);
        assert_eq!(samples.first(), Some(&16));
        assert_eq!(samples.last(), Some(&160));
        assert!(samples.windows(2).all(|pair| pair[1] - pair[0] <= 16));
        assert!(samples.len() >= 10);
        let previous = tab_swipe_linear_position(144, 0, 160, 0.0, 100.0);
        let terminal = tab_swipe_linear_position(160, 0, 160, 0.0, 100.0);
        assert_eq!(terminal - previous, 10.0);
    }
}

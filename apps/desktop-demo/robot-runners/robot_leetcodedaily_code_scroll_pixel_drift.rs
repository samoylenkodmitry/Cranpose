//! Robot test: capture screenshots of a leetcodedaily-style Code layout while scrolling by
//! exactly one logical pixel per step and require stable overlap inside the code workspace
//! viewport.

mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use cranpose_foundation::{
    text::{TextFieldLineLimits, TextFieldState},
    SemanticsConfiguration,
};
use cranpose_ui::{
    composable,
    text::{FontFamily, FontWeight, SpanStyle, TextUnit},
    widgets::BasicTextFieldWithOptions,
    Alignment, BasicTextFieldOptions, Box as ComposeBox, BoxSpec, Button, ButtonSpec, Color,
    Column, ColumnSpec, ContentScale, CornerRadii, Image, ImageBitmap, LinearArrangement, Modifier,
    Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};
use scroll_stability_external_helpers::{
    run_internal_scroll_stability_capture, ScrollStabilityConfig,
};
use std::time::Duration;
use text_showcase_external_helpers::scroll_text_into_view_between;

const WINDOW_WIDTH: u32 = 980;
const WINDOW_HEIGHT: u32 = 720;
const WINDOW_TITLE: &str = "Robot LeetcodeDaily Code Scroll Pixel Drift";
const VIEWPORT_TAG: &str = "LeetcodeDailyCodeViewport";
const TARGET_TEXT: &str = "Rust Code";
const SCROLL_STEPS: usize = 10;
const SCROLL_DELTA_Y: f32 = -1.0;
const STEP_EPSILON: f32 = 0.05;
const COMPARE_SEARCH_OFFSET_PX: u32 = 32;
const COMPARE_MAX_ADJACENT_SCORE: u32 = 8_192;
const COMPARE_STABILIZED_GUARD_PX: u32 = 28;
const COMPARE_VIEWPORT_INSET_PX: u32 = 36;
const TARGET_MIN_CENTER_Y: f32 = 260.0;
const TARGET_MAX_CENTER_Y: f32 = 430.0;
const CAPTURE_SCALE: f32 = 1.0;
const RENDER_STATS_ENV: &str = "CRANPOSE_LEETCODEDAILY_CODE_SCROLL_RENDER_STATS";

const KOTLIN_CODE: &str = r#"class Solution {
    fun countGood(nums: IntArray, k: Int): Long {
        var left = 0
        var pairs = 0L
        var answer = 0L
        val freq = HashMap<Int, Int>()

        for (right in nums.indices) {
            val value = nums[right]
            val seen = freq.getOrDefault(value, 0)
            pairs += seen.toLong()
            freq[value] = seen + 1

            while (pairs >= k) {
                answer += (nums.size - right).toLong()
                val remove = nums[left++]
                val count = freq.getValue(remove)
                pairs -= (count - 1).toLong()
                if (count == 1) freq.remove(remove) else freq[remove] = count - 1
            }
        }

        return answer
    }
}"#;

const RUST_CODE: &str = r#"impl Solution {
    pub fn count_good(nums: Vec<i32>, k: i32) -> i64 {
        let mut left = 0usize;
        let mut pairs = 0i64;
        let mut answer = 0i64;
        let mut freq = std::collections::HashMap::<i32, i64>::new();

        for right in 0..nums.len() {
            let value = nums[right];
            let seen = *freq.get(&value).unwrap_or(&0);
            pairs += seen;
            freq.insert(value, seen + 1);

            while pairs >= k as i64 {
                answer += (nums.len() - right) as i64;
                let remove = nums[left];
                left += 1;
                if let Some(count) = freq.get_mut(&remove) {
                    pairs -= *count - 1;
                    *count -= 1;
                }
            }
        }

        answer
    }
}"#;

fn primary_text() -> Color {
    Color::from_rgb_u8(236, 242, 255)
}

fn muted_text() -> Color {
    Color::from_rgb_u8(148, 163, 184)
}

fn card_surface() -> Color {
    Color(0.075, 0.090, 0.135, 0.96)
}

fn input_surface() -> Color {
    Color(0.025, 0.035, 0.065, 1.0)
}

fn accent_color() -> Color {
    Color::from_rgb_u8(87, 214, 255)
}

fn title_style(size: f32) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(primary_text()),
        font_size: TextUnit::Sp(size),
        font_weight: Some(FontWeight::BOLD),
        ..SpanStyle::default()
    })
}

fn label_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(accent_color()),
        font_size: TextUnit::Sp(11.0),
        font_weight: Some(FontWeight::SEMI_BOLD),
        ..SpanStyle::default()
    })
}

fn body_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(muted_text()),
        font_size: TextUnit::Sp(13.0),
        ..SpanStyle::default()
    })
}

fn code_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(primary_text()),
        font_family: Some(FontFamily::Monospace),
        font_size: TextUnit::Sp(14.0),
        ..SpanStyle::default()
    })
}

fn glass_panel_modifier(modifier: Modifier, radius: f32) -> Modifier {
    modifier
        .background(card_surface())
        .rounded_corners(radius)
        .draw_behind(move |scope| {
            scope.draw_round_rect(
                cranpose_ui::Brush::solid(Color(1.0, 1.0, 1.0, 0.055)),
                CornerRadii::uniform(radius),
            );
        })
}

fn code_icon_bitmap(kind: CodeLanguage) -> ImageBitmap {
    const WIDTH: u32 = 24;
    const HEIGHT: u32 = 24;
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let (base, accent) = match kind {
        CodeLanguage::Kotlin => ([107, 92, 255, 255], [255, 136, 87, 255]),
        CodeLanguage::Rust => ([248, 113, 113, 255], [87, 214, 255, 255]),
    };

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let border = x < 2 || y < 2 || x >= WIDTH - 2 || y >= HEIGHT - 2;
            let diagonal = x + y >= 16 && x + y <= 20;
            let bar = (14..=16).contains(&y) && (6..=18).contains(&x);
            let rgba = if border {
                [236, 242, 255, 255]
            } else if diagonal || bar {
                accent
            } else if ((x / 4) + (y / 4)) % 2 == 0 {
                base
            } else {
                [20, 28, 48, 255]
            };
            let index = ((y * WIDTH + x) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&rgba);
        }
    }

    ImageBitmap::from_rgba8(WIDTH, HEIGHT, pixels).expect("valid code icon bitmap")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CodeLanguage {
    Kotlin,
    Rust,
}

#[allow(non_snake_case)]
#[composable]
fn Pill(text: &'static str) {
    Text(
        text,
        Modifier::empty()
            .background(Color(0.12, 0.16, 0.24, 0.92))
            .rounded_corners(999.0)
            .padding_symmetric(12.0, 7.0),
        body_style(),
    );
}

#[allow(non_snake_case)]
#[composable]
fn ActionButton(label: &'static str) {
    Button(
        Modifier::empty()
            .width(66.0)
            .height(38.0)
            .background(Color(0.15, 0.20, 0.30, 0.96))
            .rounded_corners(12.0)
            .padding(8.0),
        ButtonSpec::default(),
        || {},
        move || {
            Text(label, Modifier::empty(), label_style());
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn CodeField(
    label: &'static str,
    runtime: &'static str,
    language: CodeLanguage,
    state: TextFieldState,
) {
    let icon =
        cranpose_core::remember(move || code_icon_bitmap(language)).with(|bitmap| bitmap.clone());
    ComposeBox(
        glass_panel_modifier(Modifier::empty().fill_max_width(), 12.0)
            .padding_symmetric(12.0, 10.0),
        BoxSpec::default(),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default()
                    .horizontal_arrangement(LinearArrangement::spaced_by(16.0))
                    .vertical_alignment(VerticalAlignment::Top),
                {
                    let state = state.clone();
                    let icon = icon.clone();
                    move || {
                        let icon_for_image = icon.clone();
                        ComposeBox(
                            Modifier::empty()
                                .size(Size::new(44.0, 44.0))
                                .background(Color(0.18, 0.23, 0.33, 0.96))
                                .rounded_corners(12.0)
                                .padding(9.0),
                            BoxSpec::default().content_alignment(Alignment::CENTER),
                            move || {
                                Image(
                                    icon_for_image.clone(),
                                    Some(format!("{label} icon")),
                                    Modifier::empty().fill_max_size(),
                                    Alignment::CENTER,
                                    ContentScale::Fit,
                                    1.0,
                                    None,
                                );
                            },
                        );

                        Column(
                            Modifier::empty().weight(1.0),
                            ColumnSpec::default()
                                .vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                            {
                                let state = state.clone();
                                move || {
                                    Row(
                                        Modifier::empty().fill_max_width(),
                                        RowSpec::default()
                                            .horizontal_arrangement(LinearArrangement::SpaceBetween)
                                            .vertical_alignment(
                                                VerticalAlignment::CenterVertically,
                                            ),
                                        move || {
                                            Text(label, Modifier::empty(), label_style());
                                            Text(runtime, Modifier::empty(), body_style());
                                        },
                                    );
                                    ComposeBox(
                                        Modifier::empty()
                                            .fill_max_width()
                                            .background(input_surface())
                                            .rounded_corners(8.0)
                                            .padding(12.0),
                                        BoxSpec::default(),
                                        {
                                            let state = state.clone();
                                            move || {
                                                BasicTextFieldWithOptions(
                                                    state.clone(),
                                                    Modifier::empty().fill_max_width(),
                                                    BasicTextFieldOptions {
                                                        text_style: code_style(),
                                                        cursor_color: accent_color(),
                                                        line_limits:
                                                            TextFieldLineLimits::MultiLine {
                                                                min_lines: 10,
                                                                max_lines: 18,
                                                            },
                                                    },
                                                );
                                            }
                                        },
                                    );
                                }
                            },
                        );

                        Column(
                            Modifier::empty(),
                            ColumnSpec::default()
                                .vertical_arrangement(LinearArrangement::spaced_by(8.0)),
                            move || {
                                ActionButton("Copy");
                                ActionButton("Clear");
                            },
                        );
                    }
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn SummaryCard() {
    ComposeBox(
        glass_panel_modifier(Modifier::empty().fill_max_width(), 16.0).padding(18.0),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(10.0)),
                move || {
                    Text("Daily Problem", Modifier::empty(), title_style(24.0));
                    Text(
                        "Count the Number of Good Subarrays",
                        Modifier::empty(),
                        title_style(18.0),
                    );
                    Text(
                        "Sliding window with pair accounting, duplicate frequencies, and a shrinking left edge once the window reaches the requested pair threshold.",
                        Modifier::empty(),
                        body_style(),
                    );
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::default()
                            .horizontal_arrangement(LinearArrangement::spaced_by(10.0)),
                        move || {
                            Pill("Difficulty: Medium");
                            Pill("Runtime notes ready");
                            Pill("Cranpose preview");
                        },
                    );
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn WriteupCard() {
    ComposeBox(
        glass_panel_modifier(Modifier::empty().fill_max_width(), 16.0).padding(18.0),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
                move || {
                    Text("Writeup", Modifier::empty(), title_style(22.0));
                    Text(
                        "The queue keeps the author moving through preparation, writeup, code, review, and shipping while preserving the language-specific solution notes.",
                        Modifier::empty(),
                        body_style(),
                    );
                    Text(
                        "Approach: expand right, count new equal pairs, and shrink left while the current window already satisfies the requested pair threshold.",
                        Modifier::empty(),
                        body_style(),
                    );
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn CodeCard() {
    let kotlin_state =
        cranpose_core::remember(|| TextFieldState::new(KOTLIN_CODE)).with(|state| state.clone());
    let rust_state =
        cranpose_core::remember(|| TextFieldState::new(RUST_CODE)).with(|state| state.clone());

    ComposeBox(
        glass_panel_modifier(Modifier::empty().fill_max_width(), 18.0).padding(18.0),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
                {
                    let kotlin_state = kotlin_state.clone();
                    let rust_state = rust_state.clone();
                    move || {
                        Text("Code Blocks", Modifier::empty(), title_style(28.0));
                        CodeField(
                            "Kotlin Code",
                            "Runtime 146 ms",
                            CodeLanguage::Kotlin,
                            kotlin_state.clone(),
                        );
                        CodeField(
                            TARGET_TEXT,
                            "Runtime 0 ms",
                            CodeLanguage::Rust,
                            rust_state.clone(),
                        );
                    }
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn LeetcodeDailyCodeScrollApp() {
    let scroll_state =
        cranpose_core::remember(|| cranpose_ui::ScrollState::new(0.0)).with(|state| state.clone());

    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.030, 0.040, 0.075, 1.0))
            .padding(20.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
        move || {
            Text(
                "LeetcodeDaily Code Layout",
                Modifier::empty(),
                title_style(26.0),
            );
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(10.0)),
                move || {
                    Pill("Prepare");
                    Pill("Write");
                    Pill("Code");
                    Pill("Review");
                    Pill("Ship");
                },
            );
            ComposeBox(
                glass_panel_modifier(
                    Modifier::empty().fill_max_width().weight(1.0).semantics(
                        |config: &mut SemanticsConfiguration| {
                            config.content_description = Some(VIEWPORT_TAG.to_string());
                        },
                    ),
                    20.0,
                )
                .clip_to_bounds(),
                BoxSpec::default(),
                {
                    let scroll_state = scroll_state.clone();
                    move || {
                        Column(
                            Modifier::empty()
                                .fill_max_size()
                                .vertical_scroll(scroll_state.clone(), false)
                                .padding(22.0),
                            ColumnSpec::default()
                                .vertical_arrangement(LinearArrangement::spaced_by(22.0)),
                            move || {
                                SummaryCard();
                                WriteupCard();
                                Spacer(Size::new(0.0, 70.0));
                                CodeCard();
                                Spacer(Size::new(0.0, 96.0));
                            },
                        );
                    }
                },
            );
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== Robot LeetcodeDaily Code Scroll Pixel Drift ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(true)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            scroll_text_into_view_between(
                &robot,
                TARGET_TEXT,
                TARGET_MIN_CENTER_Y,
                TARGET_MAX_CENTER_Y,
                48,
            );
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let compare_ok = run_internal_scroll_stability_capture(
                &robot,
                ScrollStabilityConfig {
                    window_title: WINDOW_TITLE,
                    output_name: "leetcodedaily_code_scroll_pixel_drift",
                    file_prefix: "leetcodedaily_code_scroll",
                    target_text: TARGET_TEXT,
                    viewport_tag: Some(VIEWPORT_TAG),
                    window_width: WINDOW_WIDTH,
                    window_height: WINDOW_HEIGHT,
                    scroll_steps: SCROLL_STEPS,
                    scroll_delta_y: SCROLL_DELTA_Y,
                    step_epsilon: STEP_EPSILON,
                    fallback_trim_top_px: 112,
                    fallback_trim_bottom_px: 88,
                    compare_search_offset_px: COMPARE_SEARCH_OFFSET_PX,
                    compare_max_adjacent_score: COMPARE_MAX_ADJACENT_SCORE,
                    compare_stabilized_guard_px: COMPARE_STABILIZED_GUARD_PX,
                    compare_viewport_inset_px: COMPARE_VIEWPORT_INSET_PX,
                    render_stats_env: Some(RENDER_STATS_ENV),
                },
                CAPTURE_SCALE,
            );
            if !compare_ok {
                std::process::exit(1);
            }

            println!("\n=== Test Summary ===");
            println!("PASS: leetcodedaily Code layout stayed within the pixel-drift budget");
            robot.exit().expect("exit");
        })
        .run(LeetcodeDailyCodeScrollApp);
}

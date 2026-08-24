//! Robot test: full LeetCode Daily Composer layout scroll stability repro for issue #269.

#![expect(
    clippy::too_many_arguments,
    reason = "full-layout robot fixture keeps application call shapes intact"
)]

mod output_paths;
mod perf_robot_stats;
mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use cranpose::AppLauncher;
use cranpose_core::{remember, rememberMutableStateOf, MutableState};
use cranpose_foundation::{
    text::{TextFieldLineLimits, TextFieldState},
    DrawScope, SemanticsConfiguration,
};
use cranpose_testing::{
    crop_screenshot_logical, normalize_screenshot_region, screenshot_difference_stats,
};
use cranpose_ui::{
    composable, widgets::BasicTextFieldWithOptions, Alignment, BasicText, BasicTextFieldOptions,
    BitmapPainter, Box as ComposeBox, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Brush,
    Button, ButtonSpec, Color, ColorFilter, Column, ColumnSpec, CompositingStrategy, ContentScale,
    CornerRadii, Image, ImageBitmap, LayerShape, LinearArrangement, Modifier, ParagraphStyle,
    Point, Rect, RoundedCornerShape, Row, RowSpec, ScrollState, Size, Spacer, SpanStyle, Text,
    TextOverflow, TextStyle, VerticalAlignment,
};
use image::{imageops::FilterType, ImageBuffer, RgbaImage};
use perf_robot_stats::{print_render_summary, RenderStatsAccumulator};
use scroll_stability_external_helpers::{run_scroll_stability_capture, ScrollStabilityConfig};
use text_showcase_external_helpers::{find_window_id, take_x11_screenshot};

const APP_WIDTH: u32 = 1100;
const APP_HEIGHT: u32 = 1100;
const APP_BOTTOM_LIST_GAP: f32 = 50.0;
const WORKSPACE_VIEWPORT_FALLBACK_TOP: f32 = 96.0;
const INTERACTIVE_QUEUE_CHIP_WIDTH: f32 = 214.0;
const INTERACTIVE_QUEUE_CHIP_GAP: f32 = 10.0;
const BUTTON_ACTIVITY_INDICATOR_WIDTH: f32 = 66.0;
const BUTTON_ACTIVITY_INDICATOR_HEIGHT: f32 = 18.0;
const BUTTON_PRESS_SCALE_DELTA: f32 = 0.018;
const BUTTON_PRESS_TRANSLATION_Y: f32 = 1.4;

const WINDOW_TITLE: &str = "Robot LeetcodeDaily Full Layout Scroll Stability";
const VIEWPORT_TAG: &str = "LeetcodeDailyFullWorkspaceViewport";
const TARGET_TEXT: &str = "Rust Code";
const SCROLL_STEPS: usize = 2;
const SCROLL_DELTA_Y: f32 = -4.0;
const STEP_EPSILON: f32 = 0.05;
const COMPARE_SEARCH_OFFSET_PX: u32 = 32;
const COMPARE_MAX_ADJACENT_SCORE: u32 = 275_000;
const COMPARE_STABILIZED_GUARD_PX: u32 = 8;
const WORKSPACE_SCROLL_SHADOW_HEIGHT: f32 = 82.0;
const COMPARE_VIEWPORT_INSET_PX: u32 = WORKSPACE_SCROLL_SHADOW_HEIGHT as u32;
const TARGET_MIN_CENTER_Y: f32 = 720.0;
const TARGET_MAX_CENTER_Y: f32 = 865.0;
const WORKSPACE_SEEK_MAX_SCROLL_DELTA_Y: f32 = 240.0;
const BUTTON_QUALITY_MAX_EDGE_LOSS_RATIO: f32 = 0.10;
const BUTTON_QUALITY_MAX_EXTRA_COLORS: usize = 180;
const BUTTON_QUALITY_SAFE_EDGE: f32 = 2.0;
const BUTTON_QUALITY_TARGET_MIN_STABLE_RATIO: f32 = 0.24;
const BUTTON_QUALITY_TARGET_MAX_STABLE_RATIO: f32 = 0.58;
const BUTTON_REFERENCE_FIXTURE_WIDTH: f32 = 152.0;
const BUTTON_REFERENCE_FIXTURE_HEIGHT: f32 = 60.0;
const BUTTON_REFERENCE_DIFF_TOLERANCE: u32 = 4;
const BUTTON_REFERENCE_MAX_DIFFERING_RATIO: f32 = 0.10;
const BUTTON_REFERENCE_MAX_EDGE_LOSS_RATIO: f32 = 0.06;
const BUTTON_REFERENCE_MAX_RED_EDGE_LOSS_RATIO: f32 = 0.06;
const BUTTON_REFERENCE_MAX_EXTRA_COLORS: usize = 160;
const ROW_MICRO_STABILITY_STEPS: usize = 20;
const ROW_MICRO_SCROLL_DELTA_Y: f32 = -1.0;
const ROW_MICRO_TARGET_MIN_STABLE_RATIO: f32 = 0.50;
const ROW_MICRO_TARGET_MAX_STABLE_RATIO: f32 = 0.68;
const ROW_MICRO_REGION_LEFT_PAD: f32 = 78.0;
const ROW_MICRO_REGION_RIGHT_PAD: f32 = 12.0;
const ROW_MICRO_REGION_VERTICAL_PAD: f32 = 18.0;
const ROW_MICRO_DELTA_EPSILON: f32 = 0.05;
const ROW_MICRO_GEOMETRY_EPSILON: f32 = 0.05;
const ROW_MICRO_DIFF_TOLERANCE: u32 = 10;
const ROW_MICRO_MAX_DIFFERING_PIXELS: usize = 30_000;
const ROW_MICRO_MAX_PIXEL_DIFF: u32 = 255;
const ROW_MICRO_MAX_ICON_CENTER_DELTA_PX: f32 = 0.25;
const ROW_MICRO_MAX_ICON_WIDTH_DELTA_PX: i32 = 1;
const ROW_MICRO_MAX_ICON_COUNT_DELTA_RATIO: f32 = 0.035;
const ROW_MICRO_HORIZONTAL_SEARCH_RADIUS_PX: i32 = 4;
const ROW_MICRO_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO: f32 = 0.05;
const ROW_MICRO_FIELD_ICON_LEFT_PAD: f32 = 96.0;
const ROW_MICRO_FIELD_ICON_REGION_WIDTH: f32 = 88.0;
const WORKSPACE_STRIP_STABILITY_STEPS: usize = 1;
const WORKSPACE_STRIP_SCROLL_DELTA_Y: f32 = -1.0;
const WORKSPACE_STRIP_TARGET_MIN_STABLE_RATIO: f32 = 0.34;
const WORKSPACE_STRIP_TARGET_MAX_STABLE_RATIO: f32 = 0.58;
const WORKSPACE_STRIP_REGION_INSET_X: f32 = 12.0;
const WORKSPACE_STRIP_REGION_INSET_Y: f32 = 8.0;
const WORKSPACE_STRIP_DELTA_EPSILON: f32 = 0.05;
const WORKSPACE_STRIP_HORIZONTAL_SEARCH_RADIUS_PX: i32 = 4;
const WORKSPACE_STRIP_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO: f32 = 0.025;
const WORKSPACE_STRIP_FRACTIONAL_HORIZONTAL_SEARCH_RADIUS_PX: f32 = 1.5;
const WORKSPACE_STRIP_FRACTIONAL_HORIZONTAL_STEP_PX: f32 = 0.25;
const WORKSPACE_STRIP_FRACTIONAL_HORIZONTAL_MIN_ABS_SHIFT_PX: f32 = 0.20;
const WORKSPACE_STRIP_FRACTIONAL_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO: f32 = 0.015;
const WORKSPACE_STRIP_ACTIVE_SETTLE_FRAMES: u32 = 8;
const WORKSPACE_STRIP_ACTIVE_DIFF_TOLERANCE: u32 = 4;
const WORKSPACE_STRIP_ACTIVE_MAX_DIFFERING_PIXELS: usize = 2_500;
const WORKSPACE_STRIP_ACTIVE_MAX_DIFFERING_RATIO: f32 = 0.0025;
const WORKSPACE_STRIP_ACTIVE_MAX_PIXEL_DIFF: u32 = 72;
const WORKSPACE_STRIP_ACTIVE_MAX_EDGE_LOSS_RATIO: f32 = 0.025;
const WORKSPACE_STRIP_ACTIVE_MAX_EXTRA_COLORS: usize = 96;
const WORKSPACE_STRIP_ACTIVE_CAPTURE_ATTEMPTS: usize = 36;
const WORKSPACE_STRIP_ACTIVE_CAPTURE_SLEEP_MS: u64 = 16;
const WORKSPACE_ACTIVE_VIEWPORT_STABILITY_STEPS: usize = 1;
const WORKSPACE_ACTIVE_VIEWPORT_SCROLL_DELTA_Y: f32 = -1.0;
const WORKSPACE_ACTIVE_VIEWPORT_DIFF_TOLERANCE: u32 = 4;
const WORKSPACE_ACTIVE_VIEWPORT_MAX_DIFFERING_RATIO: f32 = 0.0025;
const WORKSPACE_ACTIVE_VIEWPORT_MAX_PIXEL_DIFF: u32 = 72;
const WORKSPACE_ACTIVE_VIEWPORT_MAX_EDGE_LOSS_RATIO: f32 = 0.025;
const WORKSPACE_ACTIVE_VIEWPORT_MAX_EXTRA_COLORS: usize = 160;
const WORKSPACE_ACTIVE_VIEWPORT_ACTIVE_PUMP_FRAMES: u32 = 1;
const WORKSPACE_SURFACE_GEOMETRY_EPSILON: f32 = 0.05;
const WORKSPACE_RIGID_STABILITY_STEPS: usize = 1;
const WORKSPACE_RIGID_SCROLL_DELTA_Y: f32 = -1.0;
const WORKSPACE_RIGID_TARGET_MIN_STABLE_RATIO: f32 = 0.40;
const WORKSPACE_RIGID_TARGET_MAX_STABLE_RATIO: f32 = 0.58;
const WORKSPACE_RIGID_REGION_INSET_X: f32 = 22.0;
const WORKSPACE_RIGID_REGION_INSET_Y: f32 = 18.0;
const WORKSPACE_RIGID_DELTA_EPSILON: f32 = 0.05;
const WORKSPACE_RIGID_DIFF_TOLERANCE: u32 = 8;
const WORKSPACE_RIGID_MAX_DIFFERING_PIXELS: usize = 2_500;
const WORKSPACE_RIGID_MAX_DIFFERING_RATIO: f32 = 0.006;
const WORKSPACE_RIGID_PUMP_FRAMES_AFTER_SCROLL: u32 = 1;
const WORKSPACE_ACTIVE_CRISPNESS_SETTLE_FRAMES: u32 = 8;
const WORKSPACE_ACTIVE_CRISPNESS_MAX_EDGE_LOSS_RATIO: f32 = 0.04;
const WORKSPACE_ACTIVE_CRISPNESS_MAX_EXTRA_COLORS: usize = 48;
const WORKSPACE_ANCHOR_FEATURE_PAD: f32 = 10.0;
const WORKSPACE_ANCHOR_FIELD_LEFT_PAD: f32 = 82.0;
const WORKSPACE_ANCHOR_FIELD_WIDTH: f32 = 72.0;
const WORKSPACE_ANCHOR_FIELD_TOP_PAD: f32 = 18.0;
const WORKSPACE_ANCHOR_FIELD_HEIGHT: f32 = 62.0;
const WORKSPACE_ANCHOR_MAX_CENTER_DELTA_PX: f32 = 0.55;
const WORKSPACE_ANCHOR_MAX_CENTROID_DELTA_PX: f32 = 4.0;
const WORKSPACE_ANCHOR_MAX_TEXT_CENTROID_DELTA_PX: f32 = 10.0;
const WORKSPACE_ANCHOR_MAX_WIDTH_DELTA_PX: i32 = 1;
const WORKSPACE_ANCHOR_MAX_COUNT_DELTA_RATIO: f32 = 0.14;
const WORKSPACE_ANCHOR_MAX_TEXT_COUNT_DELTA_RATIO: f32 = 0.30;
const WORKSPACE_ANCHOR_MAX_PAIR_DISTANCE_DELTA_PX: f32 = 1.10;
const WORKSPACE_TOP_BAND_STABILITY_STEPS: usize = 2;
const WORKSPACE_TOP_BAND_SCROLL_DELTA_Y: f32 = -1.0;
const WORKSPACE_TOP_BAND_REGION_INSET_X: f32 = 12.0;
const WORKSPACE_TOP_BAND_REGION_TOP: f32 = 12.0;
const WORKSPACE_TOP_BAND_REGION_HEIGHT: f32 = 56.0;
const WORKSPACE_TOP_BAND_DELTA_EPSILON: f32 = 0.05;
const WORKSPACE_TOP_BAND_HORIZONTAL_SEARCH_RADIUS_PX: i32 = 4;
const WORKSPACE_TOP_BAND_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO: f32 = 0.025;
const WORKSPACE_TOP_BAND_FRACTIONAL_HORIZONTAL_SEARCH_RADIUS_PX: f32 = 1.5;
const WORKSPACE_TOP_BAND_FRACTIONAL_HORIZONTAL_STEP_PX: f32 = 0.25;
const WORKSPACE_TOP_BAND_FRACTIONAL_HORIZONTAL_MIN_ABS_SHIFT_PX: f32 = 0.20;
const WORKSPACE_TOP_BAND_FRACTIONAL_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO: f32 = 0.015;
const FULL_FRAME_COLORFUL_MIN_SPAN: u8 = 45;
const FULL_FRAME_COLORFUL_MIN_VALUE: u8 = 100;
const FULL_FRAME_MIN_COLORFUL_RATIO: f32 = 0.16;
const FULL_FRAME_MIN_BLUE_CYAN_RATIO: f32 = 0.20;
const FULL_FRAME_MAX_DARK_RATIO: f32 = 0.18;
const FULL_FRAME_MIN_UNIQUE_RGB_COUNT: usize = 40_000;
const BOTTOM_CLEAR_STABILITY_STEPS: usize = 12;
const BOTTOM_CLEAR_SCROLL_DELTA_Y: f32 = -1.0;
const BOTTOM_CLEAR_TARGET_MIN_STABLE_RATIO: f32 = 0.36;
const BOTTOM_CLEAR_TARGET_MAX_STABLE_RATIO: f32 = 0.56;
const BOTTOM_CLEAR_FIXED_REGION_WIDTH: f32 = 190.0;
const BOTTOM_CLEAR_FIXED_REGION_HEIGHT: f32 = 220.0;
const BOTTOM_CLEAR_BUTTON_CROP_PAD: f32 = 10.0;
const BOTTOM_CLEAR_DELTA_EPSILON: f32 = 0.05;
const BOTTOM_CLEAR_CROP_DIFF_TOLERANCE: u32 = 5;
const BOTTOM_CLEAR_MAX_DIFFERING_PIXELS: usize = 30_000;
const BOTTOM_CLEAR_MAX_PIXEL_DIFF: u32 = 255;
const BOTTOM_CLEAR_MAX_RED_BOUNDS_CENTER_DELTA_PX: f32 = 0.0;
const BOTTOM_CLEAR_MAX_RED_BOUNDS_WIDTH_DELTA_PX: i32 = 0;
const BOTTOM_CLEAR_MAX_RED_CENTROID_DELTA_PX: f32 = 0.06;
const BOTTOM_CLEAR_MAX_RED_MASS_DELTA_RATIO: f32 = 0.018;
const BOTTOM_CLEAR_MAX_RED_PROFILE_L1_RATIO: f32 = 0.055;
const BOTTOM_CLEAR_MAX_RED_COLUMN_DELTA_RATIO: f32 = 0.10;
const BOTTOM_CLEAR_HORIZONTAL_SEARCH_RADIUS_PX: i32 = 4;
const BOTTOM_CLEAR_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO: f32 = 0.05;
const BOTTOM_CLEAR_ACTIVE_CRISPNESS_SETTLE_FRAMES: u32 = 8;
const BOTTOM_CLEAR_ACTIVE_CRISPNESS_MAX_EDGE_LOSS_RATIO: f32 = 0.04;
const BOTTOM_CLEAR_ACTIVE_CRISPNESS_MAX_EXTRA_COLORS: usize = 48;
const RENDER_STATS_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_SCROLL_RENDER_STATS";
const PERF_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_PERF";
const PERF_DURATION_SECS_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_PERF_DURATION_SECS";
const PERF_MIN_FPS_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_PERF_MIN_FPS";
const PERF_SCROLL_DELTA_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_PERF_SCROLL_DELTA";
const PERF_SCENARIO_NAME: &str = "leetcodedaily_full_layout_scroll";
const INTERACTIVE_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_INTERACTIVE";
const EXTENDED_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_EXTENDED";
const WINDOW_WIDTH_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_WINDOW_WIDTH";
const WINDOW_HEIGHT_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_WINDOW_HEIGHT";
const STRIP_ONLY_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_STRIP_ONLY";
const STRIP_TARGET_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_STRIP_TARGET";
const STRIP_STEPS_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_STRIP_STEPS";
const STRIP_SCROLL_DELTA_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_STRIP_SCROLL_DELTA";
const ACTIVE_VIEWPORT_ONLY_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_ACTIVE_VIEWPORT_ONLY";
const ACTIVE_VIEWPORT_TARGET_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_ACTIVE_VIEWPORT_TARGET";
const ACTIVE_VIEWPORT_STEPS_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_ACTIVE_VIEWPORT_STEPS";
const ACTIVE_VIEWPORT_SCROLL_DELTA_ENV: &str =
    "CRANPOSE_LEETCODEDAILY_FULL_ACTIVE_VIEWPORT_SCROLL_DELTA";
const RIGID_ONLY_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_RIGID_ONLY";
const RIGID_TARGET_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_RIGID_TARGET";
const RIGID_STEPS_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_RIGID_STEPS";
const TOP_BAND_ONLY_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_TOP_BAND_ONLY";
const TOP_BAND_TARGET_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_TOP_BAND_TARGET";
const TOP_BAND_STEPS_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_TOP_BAND_STEPS";
const BOTTOM_CLEAR_ONLY_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_BOTTOM_CLEAR_ONLY";
const VISUAL_ANCHORS_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_VISUAL_ANCHORS";
const TOP_BAND_CAPTURE_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_TOP_BAND_CAPTURE";
const BUTTON_QUALITY_CAPTURE_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_BUTTON_QUALITY_CAPTURE";
const BUTTON_REFERENCE_ONLY_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_BUTTON_REFERENCE_ONLY";
const BUTTON_REFERENCE_CAPTURE_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_BUTTON_REFERENCE_CAPTURE";
const BUTTON_REFERENCE_OUTSIDE_TAG: &str = "LeetcodeDailyFullOutsideReferenceClear";
const BUTTON_REFERENCE_INSIDE_TAG: &str = "LeetcodeDailyFullInsideReferenceClear";
const BOTTOM_CLEAR_CAPTURE_ENV: &str = "CRANPOSE_LEETCODEDAILY_FULL_BOTTOM_CLEAR_CAPTURE";

thread_local! {
    static X11_CAPTURE_STATE: RefCell<X11CaptureState> = RefCell::new(X11CaptureState::default());
    static LEETCODEDAILY_IMAGE_CACHE: RefCell<LeetcodeDailyImageCache> =
        RefCell::new(LeetcodeDailyImageCache::default());
}

struct X11CaptureState {
    window_id: Option<String>,
    next_capture_id: u64,
}

impl Default for X11CaptureState {
    fn default() -> Self {
        Self {
            window_id: None,
            next_capture_id: 1,
        }
    }
}

impl X11CaptureState {
    fn next_capture(&mut self) -> (String, u64) {
        let window_id = self
            .window_id
            .get_or_insert_with(|| find_window_id(WINDOW_TITLE))
            .clone();
        let capture_id = self.next_capture_id;
        self.next_capture_id = self.next_capture_id.saturating_add(1);
        (window_id, capture_id)
    }
}

#[derive(Default)]
struct LeetcodeDailyImageCache {
    app_background: Option<Option<ImageBitmap>>,
    hero: Option<[Option<ImageBitmap>; 5]>,
    app_logo: Option<Option<ImageBitmap>>,
    ui_icons: Option<Option<ImageBitmap>>,
    ui_icons_24: Option<Option<ImageBitmap>>,
    ui_icons_44: Option<Option<ImageBitmap>>,
    ui_icons_58: Option<Option<ImageBitmap>>,
    preview: Option<ImageBitmap>,
    sharp: HashMap<SharpBitmapKey, ImageBitmap>,
}

impl LeetcodeDailyImageCache {
    fn app_background_bitmap(&mut self) -> Option<ImageBitmap> {
        self.app_background
            .get_or_insert_with(|| bitmap_from_png(APP_BACKGROUND_PNG))
            .clone()
    }

    fn hero_bitmap(&mut self, stage: WorkStage) -> Option<ImageBitmap> {
        self.hero.get_or_insert_with(|| {
            [
                bitmap_from_png(HERO_PREPARE_PNG),
                bitmap_from_png(HERO_WRITE_PNG),
                bitmap_from_png(HERO_CODE_PNG),
                bitmap_from_png(HERO_REVIEW_PNG),
                bitmap_from_png(HERO_SHIP_PNG),
            ]
        })[stage.sort_index() as usize]
            .clone()
    }

    fn app_logo_bitmap(&mut self) -> Option<ImageBitmap> {
        self.app_logo
            .get_or_insert_with(|| bitmap_from_png(APP_LOGO_PNG))
            .clone()
    }

    fn ui_icons_bitmap(&mut self) -> Option<ImageBitmap> {
        self.ui_icons
            .get_or_insert_with(|| bitmap_from_png(UI_ICONS_PNG))
            .clone()
    }

    fn ui_icons_bitmap_24(&mut self) -> Option<ImageBitmap> {
        self.ui_icons_24
            .get_or_insert_with(|| bitmap_from_png(UI_ICONS_24_PNG))
            .clone()
    }

    fn ui_icons_bitmap_44(&mut self) -> Option<ImageBitmap> {
        self.ui_icons_44
            .get_or_insert_with(|| bitmap_from_png(UI_ICONS_44_PNG))
            .clone()
    }

    fn ui_icons_bitmap_58(&mut self) -> Option<ImageBitmap> {
        self.ui_icons_58
            .get_or_insert_with(|| bitmap_from_png(UI_ICONS_58_PNG))
            .clone()
    }

    fn preview_bitmap(&mut self) -> ImageBitmap {
        self.preview
            .get_or_insert_with(build_preview_bitmap)
            .clone()
    }

    fn sharp_bitmap_for_draw(
        &mut self,
        bitmap: &ImageBitmap,
        key: SharpBitmapKey,
    ) -> Option<ImageBitmap> {
        if let Some(cached) = self.sharp.get(&key).cloned() {
            return Some(cached);
        }

        let source =
            RgbaImage::from_raw(bitmap.width(), bitmap.height(), bitmap.pixels().to_vec())?;
        let cropped =
            image::imageops::crop_imm(&source, key.src_x, key.src_y, key.src_width, key.src_height)
                .to_image();
        let resized = if key.src_width == key.target_width && key.src_height == key.target_height {
            cropped
        } else {
            image::imageops::resize(
                &cropped,
                key.target_width,
                key.target_height,
                FilterType::Lanczos3,
            )
        };
        let sharp_bitmap =
            ImageBitmap::from_rgba8(resized.width(), resized.height(), resized.into_raw()).ok()?;
        self.sharp.insert(key, sharp_bitmap.clone());
        Some(sharp_bitmap)
    }
}

const APP_BACKGROUND_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/app-background.png");
const APP_LOGO_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/app-logo.png");
const UI_ICONS_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/ui-icons.png");
const UI_ICONS_24_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/ui-icons-24.png");
const UI_ICONS_44_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/ui-icons-44.png");
const UI_ICONS_58_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/ui-icons-58.png");
const HERO_PREPARE_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/hero-prepare.png");
const HERO_WRITE_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/hero-write.png");
const HERO_CODE_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/hero-code.png");
const HERO_REVIEW_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/hero-review.png");
const HERO_SHIP_PNG: &[u8] = include_bytes!("../assets/leetcodedaily/hero-ship.png");
const DEJAVU_SANS_TTF: &[u8] = include_bytes!("../assets/leetcodedaily/fonts/DejaVuSans.ttf");
const DEJAVU_SANS_MONO_TTF: &[u8] =
    include_bytes!("../assets/leetcodedaily/fonts/DejaVuSansMono.ttf");
const MONASPACE_KRYPTON_TTF: &[u8] =
    include_bytes!("../assets/leetcodedaily/fonts/MonaspaceKryptonVarVF.ttf");
static LEETCODEDAILY_APP_FONTS: &[&[u8]] =
    &[DEJAVU_SANS_TTF, MONASPACE_KRYPTON_TTF, DEJAVU_SANS_MONO_TTF];

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
                    if *count == 0 {
                        freq.remove(&remove);
                    }
                }
            }
        }

        answer
    }
}"#;

const ACTION_BUTTONS: [ActionButtonId; 14] = [
    ActionButtonId::CopyLeetcode,
    ActionButtonId::CopyYoutube,
    ActionButtonId::CopyBlog,
    ActionButtonId::CopyTelegram,
    ActionButtonId::CopyTitle,
    ActionButtonId::CopySubtitle,
    ActionButtonId::CopyRichText,
    ActionButtonId::RefreshRasterPreview,
    ActionButtonId::RefreshCranposePreview,
    ActionButtonId::SaveRasterWebp,
    ActionButtonId::SaveCranposeWebp,
    ActionButtonId::PublishBlog,
    ActionButtonId::PostTelegram,
    ActionButtonId::PostTelegramComment,
];

const WRITEUP_FIELDS: [EditorFieldId; 5] = [
    EditorFieldId::ProblemTldr,
    EditorFieldId::Intuition,
    EditorFieldId::Approach,
    EditorFieldId::TimeComplexity,
    EditorFieldId::SpaceComplexity,
];

const CODE_FIELDS: [EditorFieldId; 4] = [
    EditorFieldId::KotlinRuntimeMs,
    EditorFieldId::KotlinCode,
    EditorFieldId::RustRuntimeMs,
    EditorFieldId::RustCode,
];

const WORKFLOW_FIELDS: [EditorFieldId; 18] = [
    EditorFieldId::ProblemTitle,
    EditorFieldId::ProblemUrl,
    EditorFieldId::Difficulty,
    EditorFieldId::ProblemTldr,
    EditorFieldId::Intuition,
    EditorFieldId::Approach,
    EditorFieldId::TimeComplexity,
    EditorFieldId::SpaceComplexity,
    EditorFieldId::KotlinRuntimeMs,
    EditorFieldId::KotlinCode,
    EditorFieldId::RustRuntimeMs,
    EditorFieldId::RustCode,
    EditorFieldId::BlogPostUrl,
    EditorFieldId::SubstackUrl,
    EditorFieldId::YoutubeUrl,
    EditorFieldId::ReferenceUrl,
    EditorFieldId::TelegramText,
    EditorFieldId::Date,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ActionButtonId {
    RefreshRasterPreview,
    RefreshCranposePreview,
    CopyLeetcode,
    CopyYoutube,
    CopyBlog,
    CopyTelegram,
    CopyTitle,
    CopySubtitle,
    CopyRichText,
    SaveRasterWebp,
    SaveCranposeWebp,
    PublishBlog,
    PostTelegram,
    PostTelegramComment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum LongAction {
    RefreshRasterPreview,
    RefreshCranposePreview,
    CopyRichText,
    SaveRasterWebp,
    SaveCranposeWebp,
    PublishBlog,
    PostTelegram,
    PostTelegramComment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EditorFieldId {
    Date,
    ProblemTitle,
    ProblemUrl,
    Difficulty,
    BlogPostUrl,
    SubstackUrl,
    YoutubeUrl,
    ReferenceUrl,
    TelegramText,
    ProblemTldr,
    Intuition,
    Approach,
    TimeComplexity,
    SpaceComplexity,
    KotlinRuntimeMs,
    KotlinCode,
    RustRuntimeMs,
    RustCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum WorkStage {
    Prepare,
    Write,
    Code,
    Review,
    Ship,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum NextWorkItem {
    Field(EditorFieldId),
    Action(ActionButtonId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum UiIcon {
    AppLogo,
    Save,
    Code,
    Telegram,
    Title,
    Subtitle,
    RichText,
    Youtube,
    Comment,
    Blog,
    Refresh,
    RefreshAlt,
    CranposeSave,
    Document,
    Leetcode,
    Substack,
    Date,
    Difficulty,
    Web,
    StagePrepare,
    StageWrite,
    StageCode,
    StageReview,
    StageShip,
    Theme,
    Paste,
    Clear,
    Generic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FieldQueueCommand {
    Edit,
    Paste,
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ThemeMode {
    Dark,
    Light,
}

#[derive(Clone, PartialEq)]
struct EditorFields {
    date: TextFieldState,
    problem_title: TextFieldState,
    problem_url: TextFieldState,
    difficulty: TextFieldState,
    blog_post_url: TextFieldState,
    substack_url: TextFieldState,
    youtube_url: TextFieldState,
    reference_url: TextFieldState,
    telegram_text: TextFieldState,
    problem_tldr: TextFieldState,
    intuition: TextFieldState,
    approach: TextFieldState,
    time_complexity: TextFieldState,
    space_complexity: TextFieldState,
    kotlin_runtime_ms: TextFieldState,
    kotlin_code: TextFieldState,
    rust_runtime_ms: TextFieldState,
    rust_code: TextFieldState,
}

#[derive(Clone, Debug, PartialEq, Hash)]
struct PostDraft {
    date: String,
    problem_title: String,
    problem_url: String,
    difficulty: String,
    blog_post_url: String,
    substack_url: String,
    youtube_url: String,
    reference_url: String,
    telegram_text: String,
    problem_tldr: String,
    intuition: String,
    approach: String,
    time_complexity: String,
    space_complexity: String,
    kotlin_runtime_ms: String,
    kotlin_code: String,
    rust_runtime_ms: String,
    rust_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UiPreferences {
    theme: ThemeMode,
    button_counts: BTreeMap<String, u64>,
    component_order: BTreeMap<String, u64>,
    interactive_queue: Vec<String>,
}

#[derive(Clone, PartialEq)]
struct PreviewState {
    bitmap: ImageBitmap,
    last_saved_webp_path: Option<String>,
}

#[derive(Clone, Copy)]
struct NineSliceInsets {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SharpBitmapKey {
    image_id: u64,
    src_x: u32,
    src_y: u32,
    src_width: u32,
    src_height: u32,
    target_width: u32,
    target_height: u32,
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

impl Default for PostDraft {
    fn default() -> Self {
        Self {
            date: "2026-05-17".to_string(),
            problem_title: "Count the Number of Good Subarrays".to_string(),
            problem_url: "https://leetcode.com/problems/count-the-number-of-good-subarrays/description/".to_string(),
            difficulty: "medium".to_string(),
            blog_post_url: String::new(),
            substack_url: String::new(),
            youtube_url: "https://www.youtube.com/watch?v=cranpose-daily".to_string(),
            reference_url: "https://dmitrysamoylenko.com/2023/07/14/leetcode_daily.html".to_string(),
            telegram_text: "Daily LeetCode notes, Kotlin and Rust snippets, and Cranpose UI progress.".to_string(),
            problem_tldr: "Count subarrays whose equal-value pairs reach the requested threshold.".to_string(),
            intuition: "A subarray becomes good when duplicate frequencies inside the current window contribute at least k equal pairs. Once it is good, every extension to the right also remains good.".to_string(),
            approach: "Move the right edge across the array, add the number of matching previous values to the pair count, and shrink the left edge while the window already satisfies the target. Each shrink accounts for all suffix extensions ending at or after the current right edge.".to_string(),
            time_complexity: "n".to_string(),
            space_complexity: "n".to_string(),
            kotlin_runtime_ms: "146".to_string(),
            kotlin_code: KOTLIN_CODE.to_string(),
            rust_runtime_ms: "0".to_string(),
            rust_code: RUST_CODE.to_string(),
        }
    }
}

impl PostDraft {
    fn from_fields(fields: &EditorFields) -> Self {
        Self {
            date: fields.date.text(),
            problem_title: fields.problem_title.text(),
            problem_url: fields.problem_url.text(),
            difficulty: fields.difficulty.text(),
            blog_post_url: fields.blog_post_url.text(),
            substack_url: fields.substack_url.text(),
            youtube_url: fields.youtube_url.text(),
            reference_url: fields.reference_url.text(),
            telegram_text: fields.telegram_text.text(),
            problem_tldr: fields.problem_tldr.text(),
            intuition: fields.intuition.text(),
            approach: fields.approach.text(),
            time_complexity: fields.time_complexity.text(),
            space_complexity: fields.space_complexity.text(),
            kotlin_runtime_ms: fields.kotlin_runtime_ms.text(),
            kotlin_code: fields.kotlin_code.text(),
            rust_runtime_ms: fields.rust_runtime_ms.text(),
            rust_code: fields.rust_code.text(),
        }
    }

    fn blog_template(&self) -> String {
        format!(
            "# {}\n\n[{}]({}) {}\n\n{}\n\n#### Problem TLDR\n{}\n\n#### Intuition\n{}\n\n#### Approach\n{}\n\n#### Complexity\n- Time complexity: O({})\n- Space complexity: O({})\n\n#### Code\n```kotlin\n{}\n```\n\n```rust\n{}\n```",
            self.date,
            self.problem_title,
            self.problem_url,
            self.difficulty,
            self.reference_url,
            self.problem_tldr,
            self.intuition,
            self.approach,
            self.time_complexity,
            self.space_complexity,
            self.kotlin_code,
            self.rust_code
        )
    }
}

impl EditorFields {
    fn from_draft(draft: &PostDraft) -> Self {
        Self {
            date: TextFieldState::new(draft.date.clone()),
            problem_title: TextFieldState::new(draft.problem_title.clone()),
            problem_url: TextFieldState::new(draft.problem_url.clone()),
            difficulty: TextFieldState::new(draft.difficulty.clone()),
            blog_post_url: TextFieldState::new(draft.blog_post_url.clone()),
            substack_url: TextFieldState::new(draft.substack_url.clone()),
            youtube_url: TextFieldState::new(draft.youtube_url.clone()),
            reference_url: TextFieldState::new(draft.reference_url.clone()),
            telegram_text: TextFieldState::new(draft.telegram_text.clone()),
            problem_tldr: TextFieldState::new(draft.problem_tldr.clone()),
            intuition: TextFieldState::new(draft.intuition.clone()),
            approach: TextFieldState::new(draft.approach.clone()),
            time_complexity: TextFieldState::new(draft.time_complexity.clone()),
            space_complexity: TextFieldState::new(draft.space_complexity.clone()),
            kotlin_runtime_ms: TextFieldState::new(draft.kotlin_runtime_ms.clone()),
            kotlin_code: TextFieldState::new(draft.kotlin_code.clone()),
            rust_runtime_ms: TextFieldState::new(draft.rust_runtime_ms.clone()),
            rust_code: TextFieldState::new(draft.rust_code.clone()),
        }
    }
}

impl Default for UiPreferences {
    fn default() -> Self {
        let interactive_queue = [
            "field.problem_title",
            "field.youtube_url",
            "field.problem_url",
            "field.telegram_text",
            "field.reference_url",
            "field.substack_url",
            "field.date",
            "field.difficulty",
            "field.blog_post_url",
            "field.problem_tldr",
            "field.intuition",
            "field.approach",
            "field.time_complexity",
            "field.space_complexity",
            "field.kotlin_runtime_ms",
            "field.kotlin_code",
            "field.rust_runtime_ms",
            "copy.leetcode",
            "copy.youtube",
            "copy.blog",
            "preview.raster",
            "preview.cranpose",
            "save.cranpose_webp",
        ]
        .iter()
        .map(|key| key.to_string())
        .collect();
        Self {
            theme: ThemeMode::Light,
            button_counts: BTreeMap::new(),
            component_order: BTreeMap::new(),
            interactive_queue,
        }
    }
}

impl UiPreferences {
    fn button_count(&self, key: &str) -> u64 {
        self.button_counts.get(key).copied().unwrap_or(0)
    }

    fn increment_button_count(&mut self, key: &str) -> u64 {
        let count = self.button_counts.entry(key.to_string()).or_default();
        *count = count.saturating_add(1);
        *count
    }

    fn component_order(&self, key: &str) -> u64 {
        self.component_order.get(key).copied().unwrap_or(0)
    }

    fn mark_component_used(&mut self, key: &str) -> u64 {
        let next_order = self
            .component_order
            .values()
            .copied()
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.component_order.insert(key.to_string(), next_order);
        next_order
    }

    fn interactive_queue(&self) -> &[String] {
        &self.interactive_queue
    }

    fn record_interactive_queue_item(&mut self, key: &str) {
        let key = key.trim();
        if key.is_empty() || self.interactive_queue.iter().any(|item| item == key) {
            return;
        }
        self.interactive_queue.push(key.to_string());
    }
}

impl PreviewState {
    fn placeholder() -> Self {
        Self {
            bitmap: preview_bitmap(),
            last_saved_webp_path: None,
        }
    }
}

impl ThemeMode {
    fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

impl ActionButtonId {
    fn from_count_key(key: &str) -> Option<Self> {
        ACTION_BUTTONS
            .iter()
            .copied()
            .find(|action| action.count_key() == key)
    }

    fn icon(self) -> UiIcon {
        match self {
            Self::RefreshRasterPreview => UiIcon::Refresh,
            Self::RefreshCranposePreview => UiIcon::RefreshAlt,
            Self::CopyLeetcode => UiIcon::Code,
            Self::CopyYoutube => UiIcon::Youtube,
            Self::CopyBlog => UiIcon::Document,
            Self::CopyTelegram => UiIcon::Telegram,
            Self::CopyTitle => UiIcon::Title,
            Self::CopySubtitle => UiIcon::Subtitle,
            Self::CopyRichText => UiIcon::RichText,
            Self::SaveRasterWebp => UiIcon::Save,
            Self::SaveCranposeWebp => UiIcon::CranposeSave,
            Self::PublishBlog => UiIcon::Blog,
            Self::PostTelegram => UiIcon::Telegram,
            Self::PostTelegramComment => UiIcon::Comment,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::RefreshRasterPreview => "Refresh Raster",
            Self::RefreshCranposePreview => "Refresh Cranpose",
            Self::CopyLeetcode => "Copy LeetCode",
            Self::CopyYoutube => "Copy YouTube",
            Self::CopyBlog => "Copy Blog",
            Self::CopyTelegram => "Copy Telegram",
            Self::CopyTitle => "Copy Title",
            Self::CopySubtitle => "Copy Subtitle",
            Self::CopyRichText => "Copy Rich Text",
            Self::SaveRasterWebp => "Save Raster WebP",
            Self::SaveCranposeWebp => "Save Cranpose WebP",
            Self::PublishBlog => "Publish Blog",
            Self::PostTelegram => "Post Telegram",
            Self::PostTelegramComment => "Post TG Comment",
        }
    }

    fn count_key(self) -> &'static str {
        match self {
            Self::RefreshRasterPreview => "preview.raster",
            Self::RefreshCranposePreview => "preview.cranpose",
            Self::CopyLeetcode => "copy.leetcode",
            Self::CopyYoutube => "copy.youtube",
            Self::CopyBlog => "copy.blog",
            Self::CopyTelegram => "copy.telegram",
            Self::CopyTitle => "copy.title",
            Self::CopySubtitle => "copy.subtitle",
            Self::CopyRichText => "copy.rich_text",
            Self::SaveRasterWebp => "save.raster_webp",
            Self::SaveCranposeWebp => "save.cranpose_webp",
            Self::PublishBlog => "publish.blog",
            Self::PostTelegram => "post.telegram",
            Self::PostTelegramComment => "post.telegram_comment",
        }
    }

    fn long_action(self) -> Option<LongAction> {
        match self {
            Self::RefreshRasterPreview => Some(LongAction::RefreshRasterPreview),
            Self::RefreshCranposePreview => Some(LongAction::RefreshCranposePreview),
            Self::CopyRichText => Some(LongAction::CopyRichText),
            Self::SaveRasterWebp => Some(LongAction::SaveRasterWebp),
            Self::SaveCranposeWebp => Some(LongAction::SaveCranposeWebp),
            Self::PublishBlog => Some(LongAction::PublishBlog),
            Self::PostTelegram => Some(LongAction::PostTelegram),
            Self::PostTelegramComment => Some(LongAction::PostTelegramComment),
            _ => None,
        }
    }
}

impl LongAction {
    fn label(self) -> &'static str {
        match self {
            Self::RefreshRasterPreview => "Refresh Raster",
            Self::RefreshCranposePreview => "Refresh Cranpose",
            Self::CopyRichText => "Copy Rich Text",
            Self::SaveRasterWebp => "Save Raster WebP",
            Self::SaveCranposeWebp => "Save Cranpose WebP",
            Self::PublishBlog => "Publish Blog",
            Self::PostTelegram => "Post Telegram",
            Self::PostTelegramComment => "Post TG Comment",
        }
    }
}

impl WorkStage {
    fn icon(self) -> UiIcon {
        match self {
            Self::Prepare => UiIcon::StagePrepare,
            Self::Write => UiIcon::StageWrite,
            Self::Code => UiIcon::StageCode,
            Self::Review => UiIcon::StageReview,
            Self::Ship => UiIcon::StageShip,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Prepare => "Prepare",
            Self::Write => "Write",
            Self::Code => "Code",
            Self::Review => "Review",
            Self::Ship => "Ship",
        }
    }

    fn sort_index(self) -> u8 {
        match self {
            Self::Prepare => 0,
            Self::Write => 1,
            Self::Code => 2,
            Self::Review => 3,
            Self::Ship => 4,
        }
    }
}

impl NextWorkItem {
    fn stage(self) -> WorkStage {
        match self {
            Self::Field(field) => field_stage(field),
            Self::Action(action) => action_stage(action),
        }
    }

    fn title(self) -> String {
        match self {
            Self::Field(field) => format!("Fill {}", field.label()),
            Self::Action(action) => action.label().to_string(),
        }
    }
}

impl EditorFieldId {
    fn from_field_id(field_id: &str) -> Option<Self> {
        WORKFLOW_FIELDS
            .iter()
            .copied()
            .find(|field| field.field_id() == field_id)
    }

    fn icon(self) -> UiIcon {
        UiIcon::for_field_id(self.field_id())
    }

    fn label(self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::ProblemTitle => "Problem Title",
            Self::ProblemUrl => "Problem URL",
            Self::Difficulty => "Difficulty",
            Self::BlogPostUrl => "Blog Post URL",
            Self::SubstackUrl => "Substack URL",
            Self::YoutubeUrl => "YouTube URL",
            Self::ReferenceUrl => "Reference URL",
            Self::TelegramText => "Telegram CTA Text",
            Self::ProblemTldr => "Problem TLDR",
            Self::Intuition => "Intuition",
            Self::Approach => "Approach",
            Self::TimeComplexity => "Time Complexity Inner Value",
            Self::SpaceComplexity => "Space Complexity Inner Value",
            Self::KotlinRuntimeMs => "Kotlin Runtime (ms)",
            Self::KotlinCode => "Kotlin Code",
            Self::RustRuntimeMs => "Rust Runtime (ms)",
            Self::RustCode => "Rust Code",
        }
    }

    fn field_id(self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::ProblemTitle => "problem_title",
            Self::ProblemUrl => "problem_url",
            Self::Difficulty => "difficulty",
            Self::BlogPostUrl => "blog_post_url",
            Self::SubstackUrl => "substack_url",
            Self::YoutubeUrl => "youtube_url",
            Self::ReferenceUrl => "reference_url",
            Self::TelegramText => "telegram_text",
            Self::ProblemTldr => "problem_tldr",
            Self::Intuition => "intuition",
            Self::Approach => "approach",
            Self::TimeComplexity => "time_complexity",
            Self::SpaceComplexity => "space_complexity",
            Self::KotlinRuntimeMs => "kotlin_runtime_ms",
            Self::KotlinCode => "kotlin_code",
            Self::RustRuntimeMs => "rust_runtime_ms",
            Self::RustCode => "rust_code",
        }
    }

    fn component_key(self) -> String {
        format!("field.{}", self.field_id())
    }
}

impl UiIcon {
    fn for_field_id(field_id: &str) -> Self {
        match field_id {
            "date" => Self::Date,
            "problem_title" => Self::Document,
            "problem_url" => Self::Leetcode,
            "difficulty" => Self::Difficulty,
            "blog_post_url" => Self::Web,
            "substack_url" => Self::Substack,
            "youtube_url" => Self::Youtube,
            "reference_url" => Self::Web,
            "telegram_text" => Self::Telegram,
            "problem_tldr" => Self::Document,
            "intuition" => Self::RichText,
            "approach" => Self::Document,
            "time_complexity" | "space_complexity" => Self::Difficulty,
            "kotlin_runtime_ms" | "kotlin_code" | "rust_runtime_ms" | "rust_code" => Self::Code,
            _ => Self::Generic,
        }
    }

    fn src_rect_for_cell(self, cell: f32) -> Rect {
        let index = self.sheet_index();
        Rect {
            x: (index % ICON_SHEET_COLUMNS) as f32 * cell,
            y: (index / ICON_SHEET_COLUMNS) as f32 * cell,
            width: cell,
            height: cell,
        }
    }

    fn sheet_index(self) -> u32 {
        match self {
            Self::AppLogo => 0,
            Self::CranposeSave => 1,
            Self::Save => 2,
            Self::Code => 3,
            Self::Telegram => 4,
            Self::Title => 5,
            Self::Subtitle => 6,
            Self::RichText => 7,
            Self::Youtube => 8,
            Self::Comment => 9,
            Self::Blog => 10,
            Self::Refresh => 11,
            Self::RefreshAlt => 12,
            Self::Document => 14,
            Self::Leetcode => 15,
            Self::Substack => 16,
            Self::Date => 17,
            Self::Difficulty => 18,
            Self::Web => 19,
            Self::StagePrepare => 20,
            Self::StageWrite => 21,
            Self::StageCode => 22,
            Self::StageReview => 23,
            Self::StageShip => 24,
            Self::Theme => 25,
            Self::Paste => 26,
            Self::Clear => 27,
            Self::Generic => 28,
        }
    }

    fn is_round_icon(self) -> bool {
        matches!(
            self,
            Self::Telegram
                | Self::Blog
                | Self::Web
                | Self::Refresh
                | Self::RefreshAlt
                | Self::StagePrepare
                | Self::StageWrite
                | Self::StageCode
                | Self::StageReview
                | Self::StageShip
                | Self::Theme
                | Self::Clear
        )
    }

    fn fallback_color(self) -> Color {
        match self {
            Self::Save | Self::Difficulty => Color::from_rgb_u8(87, 200, 56),
            Self::StagePrepare => Color::from_rgb_u8(42, 139, 224),
            Self::StageWrite => Color::from_rgb_u8(69, 139, 214),
            Self::StageCode => Color::from_rgb_u8(42, 166, 204),
            Self::Youtube | Self::Clear => Color::from_rgb_u8(232, 49, 45),
            Self::Telegram => Color::from_rgb_u8(45, 169, 230),
            Self::Substack => Color::from_rgb_u8(255, 116, 43),
            Self::StageReview | Self::Title => Color::from_rgb_u8(247, 177, 20),
            Self::RefreshAlt => Color::from_rgb_u8(155, 73, 218),
            Self::StageShip => Color::from_rgb_u8(139, 169, 188),
            Self::Comment => Color::from_rgb_u8(109, 205, 58),
            Self::RichText => Color::from_rgb_u8(48, 143, 177),
            Self::CranposeSave | Self::Document | Self::Subtitle => {
                Color::from_rgb_u8(38, 151, 226)
            }
            Self::Leetcode => Color::from_rgb_u8(248, 177, 48),
            Self::Blog | Self::Web => Color::from_rgb_u8(42, 162, 231),
            Self::Refresh => Color::from_rgb_u8(92, 196, 57),
            _ => Color::from_rgb_u8(38, 151, 226),
        }
    }
}

#[allow(non_snake_case)]
#[composable]
fn LeetcodeDailyFullLayoutApp() {
    let scroll_state = remember(|| ScrollState::new(0.0)).with(|state| *state);
    let saved_draft = remember(PostDraft::default).with(|draft| draft.clone());
    let fields = remember({
        let saved_draft = saved_draft.clone();
        move || EditorFields::from_draft(&saved_draft)
    })
    .with(|fields| fields.clone());
    let ui_preferences = rememberMutableStateOf(UiPreferences::default);
    let startup_interactive_queue = remember({
        let initial_queue = ui_preferences.value().interactive_queue().to_vec();
        move || initial_queue
    })
    .with(|queue| queue.clone());
    let layout_preferences = remember({
        let initial_preferences = ui_preferences.value();
        move || initial_preferences
    })
    .with(|preferences| preferences.clone());
    let autosave_destination =
        remember(|| "Autosave: robot fixture".to_string()).with(Clone::clone);
    let preview_state = rememberMutableStateOf(PreviewState::placeholder);
    let preview_loading = rememberMutableStateOf(|| false);
    let compose_preview_state = rememberMutableStateOf(PreviewState::placeholder);
    let compose_loading = rememberMutableStateOf(|| false);
    let compose_error = rememberMutableStateOf(String::new);
    let telegram_post_link = rememberMutableStateOf(String::new);
    let status = rememberMutableStateOf(|| "Ready.".to_string());
    let busy_action = rememberMutableStateOf(|| None::<LongAction>);
    let active_queue_target = rememberMutableStateOf(|| None::<String>);
    let current_draft = PostDraft::from_fields(&fields);
    let markdown_preview = current_draft.blog_template();
    let theme = ui_preferences.value().theme;

    ComposeBox(
        Modifier::empty().fill_max_size().draw_behind(move |scope| {
            draw_app_background(scope, theme);
        }),
        BoxSpec::default(),
        {
            let fields = fields.clone();
            let markdown_preview = markdown_preview.clone();
            let autosave_destination = autosave_destination.clone();
            let saved_draft = saved_draft.clone();
            let layout_preferences = layout_preferences.clone();
            let startup_interactive_queue = startup_interactive_queue.clone();
            move || {
                Column(Modifier::empty().fill_max_size(), ColumnSpec::default(), {
                    let fields = fields.clone();
                    let markdown_preview = markdown_preview.clone();
                    let autosave_destination = autosave_destination.clone();
                    let saved_draft = saved_draft.clone();
                    let layout_preferences = layout_preferences.clone();
                    let startup_interactive_queue = startup_interactive_queue.clone();
                    move || {
                        BoxWithConstraints(Modifier::empty().fill_max_width().weight(1.0), {
                            let fields = fields.clone();
                            let markdown_preview = markdown_preview.clone();
                            let autosave_destination = autosave_destination.clone();
                            let saved_draft = saved_draft.clone();
                            let layout_preferences = layout_preferences.clone();
                            let startup_interactive_queue = startup_interactive_queue.clone();
                            move |scope| {
                                let compact = scope.max_width().0 < 1120.0;
                                let root_horizontal_padding = if scope.max_width().0 < 700.0 {
                                    18.0
                                } else if compact {
                                    24.0
                                } else {
                                    34.0
                                };
                                Column(
                                    Modifier::empty().fill_max_size().padding_each(
                                        root_horizontal_padding,
                                        30.0,
                                        root_horizontal_padding,
                                        APP_BOTTOM_LIST_GAP,
                                    ),
                                    ColumnSpec::default()
                                        .vertical_arrangement(LinearArrangement::spaced_by(14.0)),
                                    {
                                        let fields = fields.clone();
                                        let markdown_preview = markdown_preview.clone();
                                        let autosave_destination = autosave_destination.clone();
                                        let saved_draft = saved_draft.clone();
                                        let layout_preferences = layout_preferences.clone();
                                        let startup_interactive_queue =
                                            startup_interactive_queue.clone();
                                        let workspace_scroll_state = scroll_state;
                                        move || {
                                            ActionsCard(
                                                fields.clone(),
                                                status,
                                                preview_state,
                                                autosave_destination.clone(),
                                                telegram_post_link,
                                                ui_preferences,
                                                layout_preferences.clone(),
                                                startup_interactive_queue.clone(),
                                                busy_action,
                                                active_queue_target,
                                                theme,
                                                compact,
                                            );
                                            if button_reference_enabled() {
                                                ButtonQualityReferenceFixture(
                                                    BUTTON_REFERENCE_OUTSIDE_TAG,
                                                    "quality.reference.outside.clear",
                                                    status,
                                                    ui_preferences,
                                                    theme,
                                                );
                                            }
                                            let viewport_scroll_state = workspace_scroll_state;
                                            BoxWithConstraints(
                                                Modifier::empty()
                                                    .fill_max_width()
                                                    .weight(1.0)
                                                    .padding_each(0.0, 12.0, 0.0, 0.0),
                                                {
                                                    let fields = fields.clone();
                                                    let compose_preview_state =
                                                        compose_preview_state;
                                                    let markdown_preview = markdown_preview.clone();
                                                    let saved_draft = saved_draft.clone();
                                                    let layout_preferences =
                                                        layout_preferences.clone();
                                                    move |viewport_scope| {
                                                        let viewport_width =
                                                            viewport_scope.max_width().0;
                                                        let viewport_height =
                                                            viewport_scope.max_height().0;
                                                        ComposeBox(
                                                            workspace_viewport_modifier(
                                                                Modifier::empty()
                                                                    .fill_max_size()
                                                                    .semantics(
                                                                        |config: &mut SemanticsConfiguration| {
                                                                            config.content_description =
                                                                                Some(VIEWPORT_TAG.to_string());
                                                                        },
                                                                    ),
                                                                theme,
                                                                viewport_scroll_state,
                                                                viewport_width,
                                                                viewport_height,
                                                            ),
                                                            BoxSpec::default(),
                                                            {
                                                                let fields = fields.clone();
                                                                let preview_state =
                                                                    preview_state;
                                                                let preview_loading =
                                                                    preview_loading;
                                                                let compose_preview_state =
                                                                    compose_preview_state;
                                                                let compose_loading =
                                                                    compose_loading;
                                                                let compose_error =
                                                                    compose_error;
                                                                let markdown_preview =
                                                                    markdown_preview.clone();
                                                                let saved_draft =
                                                                    saved_draft.clone();
                                                                let ui_preferences =
                                                                    ui_preferences;
                                                                let layout_preferences =
                                                                    layout_preferences.clone();
                                                                let active_queue_target =
                                                                    active_queue_target;
                                                                let viewport_scroll_state =
                                                                    viewport_scroll_state;
                                                                move || {
                                                                    Column(
                                                                        Modifier::empty()
                                                                            .fill_max_size()
                                                                            .vertical_scroll(
                                                                                viewport_scroll_state,
                                                                                false,
                                                                            )
                                                                            .padding_each(
                                                                                0.0, 22.0, 0.0, 0.0,
                                                                            ),
                                                                        ColumnSpec::default()
                                                                            .vertical_arrangement(
                                                                                LinearArrangement::spaced_by(22.0),
                                                                            ),
                                                                        {
                                                                            let fields = fields.clone();
                                                                            let markdown_preview = markdown_preview.clone();
                                                                            let saved_draft = saved_draft.clone();
                                                                            let layout_preferences = layout_preferences.clone();
                                                                            move || {
                                                                                GuidedWorkspace(
                                                                                    fields.clone(),
                                                                                    preview_state,
                                                                                    preview_loading,
                                                                                    compose_preview_state,
                                                                                    compose_loading,
                                                                                    compose_error,
                                                                                    markdown_preview.clone(),
                                                                                    status,
                                                                                    saved_draft.clone(),
                                                                                    ui_preferences,
                                                                                    layout_preferences.clone(),
                                                                                    active_queue_target,
                                                                                    theme,
                                                                                    compact,
                                                                                );
                                                                                Spacer(Size::new(0.0, 86.0));
                                                                            }
                                                                        },
                                                                    );
                                                                }
                                                            },
                                                        );
                                                    }
                                                },
                                            );
                                        }
                                    },
                                );
                            }
                        });
                        BottomListGapMask(theme);
                    }
                });
            }
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn ButtonQualityReferenceFixture(
    tag: &'static str,
    count_key: &'static str,
    status: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    theme: ThemeMode,
) {
    ComposeBox(
        Modifier::empty()
            .size(Size::new(
                BUTTON_REFERENCE_FIXTURE_WIDTH,
                BUTTON_REFERENCE_FIXTURE_HEIGHT,
            ))
            .background(reference_fixture_background(theme))
            .semantics(move |config: &mut SemanticsConfiguration| {
                config.content_description = Some(tag.to_string());
            }),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            subtle_button(
                "Clear".to_string(),
                count_key.to_string(),
                ui_preferences,
                theme,
                move || {
                    status.set("Reference clear pressed.".to_string());
                },
            );
        },
    );
}

fn button_reference_enabled() -> bool {
    std::env::var_os(BUTTON_REFERENCE_ONLY_ENV).is_some()
}

fn reference_fixture_background(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(20, 33, 55),
        ThemeMode::Light => Color::from_rgb_u8(218, 247, 249),
    }
}

#[allow(non_snake_case)]
#[composable]
fn GuidedWorkspace(
    fields: EditorFields,
    preview_state: MutableState<PreviewState>,
    preview_loading: MutableState<bool>,
    compose_preview_state: MutableState<PreviewState>,
    compose_loading: MutableState<bool>,
    compose_error: MutableState<String>,
    markdown_preview: String,
    status: MutableState<String>,
    saved_draft: PostDraft,
    ui_preferences: MutableState<UiPreferences>,
    layout_preferences: UiPreferences,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
    compact: bool,
) {
    if button_reference_enabled() {
        ButtonQualityReferenceFixture(
            BUTTON_REFERENCE_INSIDE_TAG,
            "quality.reference.inside.clear",
            status,
            ui_preferences,
            theme,
        );
    }
    ProblemMetaCard(
        fields.clone(),
        status,
        saved_draft.clone(),
        ui_preferences,
        active_queue_target,
        theme,
        compact,
    );
    WriteupCard(
        fields.clone(),
        status,
        saved_draft.clone(),
        ui_preferences,
        layout_preferences.clone(),
        active_queue_target,
        theme,
    );
    Spacer(Size::new(0.0, 82.0));
    CodeCard(
        fields,
        status,
        saved_draft,
        ui_preferences,
        layout_preferences,
        active_queue_target,
        theme,
    );
    PreviewCard(preview_state, preview_loading, theme);
    ComposePreviewCard(compose_preview_state, compose_loading, compose_error, theme);
    MarkdownCard(markdown_preview, theme);
}

#[allow(non_snake_case)]
#[composable]
fn ActionsCard(
    fields: EditorFields,
    status: MutableState<String>,
    preview_state: MutableState<PreviewState>,
    autosave_destination: String,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    layout_preferences: UiPreferences,
    startup_interactive_queue: Vec<String>,
    busy_action: MutableState<Option<LongAction>>,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
    compact: bool,
) {
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
        {
            let fields = fields.clone();
            let autosave_destination = autosave_destination.clone();
            let layout_preferences = layout_preferences.clone();
            let startup_interactive_queue = startup_interactive_queue.clone();
            move || {
                HeaderBar(
                    autosave_destination.clone(),
                    ui_preferences,
                    status,
                    theme,
                    compact,
                );

                let draft = PostDraft::from_fields(&fields);
                let preview = preview_state.value();
                let latest_telegram_link = telegram_post_link.value();
                let current_queue = ui_preferences.value().interactive_queue().to_vec();
                let next_queue_key =
                    interactive_queue_next_key(&startup_interactive_queue, &current_queue);
                let next_item = next_queue_key
                    .as_deref()
                    .and_then(next_work_item_from_queue_key)
                    .unwrap_or_else(|| {
                        recommended_next_work(
                            &draft,
                            &preview,
                            &latest_telegram_link,
                            &layout_preferences,
                        )
                    });
                let next_title = next_queue_key
                    .as_deref()
                    .map(|key| interactive_queue_label(key, false, false));
                if compact {
                    Column(
                        Modifier::empty().fill_max_width(),
                        ColumnSpec::default()
                            .vertical_arrangement(LinearArrangement::spaced_by(14.0)),
                        {
                            let fields = fields.clone();
                            let layout_preferences = layout_preferences.clone();
                            move || {
                                NextWorkPanel(
                                    next_item,
                                    next_title.clone(),
                                    fields.clone(),
                                    status,
                                    telegram_post_link,
                                    ui_preferences,
                                    busy_action,
                                    active_queue_target,
                                    theme,
                                    true,
                                );
                                QuickActionsPanel(
                                    fields.clone(),
                                    status,
                                    telegram_post_link,
                                    ui_preferences,
                                    layout_preferences.clone(),
                                    busy_action,
                                    theme,
                                    true,
                                );
                            }
                        },
                    );
                } else {
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::default()
                            .horizontal_arrangement(LinearArrangement::spaced_by(18.0)),
                        {
                            let fields = fields.clone();
                            let layout_preferences = layout_preferences.clone();
                            move || {
                                NextWorkPanel(
                                    next_item,
                                    next_title.clone(),
                                    fields.clone(),
                                    status,
                                    telegram_post_link,
                                    ui_preferences,
                                    busy_action,
                                    active_queue_target,
                                    theme,
                                    false,
                                );
                                QuickActionsPanel(
                                    fields.clone(),
                                    status,
                                    telegram_post_link,
                                    ui_preferences,
                                    layout_preferences.clone(),
                                    busy_action,
                                    theme,
                                    false,
                                );
                            }
                        },
                    );
                }

                InteractiveQueuePanel(
                    startup_interactive_queue.clone(),
                    fields.clone(),
                    status,
                    telegram_post_link,
                    ui_preferences,
                    busy_action,
                    active_queue_target,
                    theme,
                );

                StatusStrip(status.value(), theme);

                if let Some(saved_webp) = preview_state.value().last_saved_webp_path {
                    Text(
                        format!("Latest WebP: {saved_webp}"),
                        Modifier::empty(),
                        body_style(theme),
                    );
                }
                if !latest_telegram_link.is_empty() {
                    Text(
                        format!("Latest Telegram post: {latest_telegram_link}"),
                        Modifier::empty(),
                        body_style(theme),
                    );
                }
            }
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn HeaderBar(
    autosave_destination: String,
    ui_preferences: MutableState<UiPreferences>,
    status: MutableState<String>,
    theme: ThemeMode,
    compact: bool,
) {
    if compact {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(10.0)),
            move || {
                HeaderTitle(autosave_destination.clone(), theme, true);
                let next_theme = theme.toggled();
                theme_button(format!("Theme: {}", theme.label()), theme, move || {
                    set_theme_preference(ui_preferences, next_theme, status);
                });
            },
        );
    } else {
        Row(
            Modifier::empty().fill_max_width(),
            RowSpec::default()
                .horizontal_arrangement(LinearArrangement::SpaceBetween)
                .vertical_alignment(VerticalAlignment::CenterVertically),
            move || {
                HeaderTitle(autosave_destination.clone(), theme, false);
                let next_theme = theme.toggled();
                theme_button(format!("Theme: {}", theme.label()), theme, move || {
                    set_theme_preference(ui_preferences, next_theme, status);
                });
            },
        );
    }
}

#[allow(non_snake_case)]
#[composable]
fn HeaderTitle(autosave_destination: String, theme: ThemeMode, compact: bool) {
    Row(
        Modifier::empty(),
        RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(18.0)),
        move || {
            AppLogo();
            let autosave_destination_for_column = autosave_destination.clone();
            Column(
                Modifier::empty(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(3.0)),
                move || {
                    BasicText(
                        "LeetCode Daily Composer",
                        Modifier::empty(),
                        app_title_style(theme, compact),
                        TextOverflow::Ellipsis,
                        false,
                        1,
                        1,
                    );
                    BasicText(
                        autosave_destination_for_column.clone(),
                        Modifier::empty(),
                        muted_style(theme),
                        TextOverflow::Ellipsis,
                        false,
                        1,
                        1,
                    );
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn QuickActionsPanel(
    fields: EditorFields,
    status: MutableState<String>,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    layout_preferences: UiPreferences,
    busy_action: MutableState<Option<LongAction>>,
    theme: ThemeMode,
    compact: bool,
) {
    let modifier = if compact {
        Modifier::empty().fill_max_width()
    } else {
        Modifier::empty().weight(2.04)
    };
    glass_panel(modifier, theme, 18.0, 18.0, {
        let fields = fields.clone();
        let layout_preferences = layout_preferences.clone();
        move || {
            let layout_preferences_for_column = layout_preferences.clone();
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
                {
                    let fields = fields.clone();
                    move || {
                        Text("Quick Actions", Modifier::empty(), panel_title_style(theme));
                        ActionButtons(
                            fields.clone(),
                            status,
                            telegram_post_link,
                            ui_preferences,
                            layout_preferences_for_column.clone(),
                            busy_action,
                            theme,
                        );
                    }
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn StatusStrip(message: String, theme: ThemeMode) {
    glass_panel(
        Modifier::empty().fill_max_width(),
        theme,
        14.0,
        12.0,
        move || {
            let message_for_row = message.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(10.0)),
                move || {
                    StatusDot(true, theme);
                    BasicText(
                        message_for_row.clone(),
                        Modifier::empty().weight(1.0),
                        accent_style(theme),
                        TextOverflow::Ellipsis,
                        false,
                        1,
                        1,
                    );
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn NextWorkPanel(
    next_item: NextWorkItem,
    title_override: Option<String>,
    fields: EditorFields,
    status: MutableState<String>,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    busy_action: MutableState<Option<LongAction>>,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
    compact: bool,
) {
    let modifier = if compact {
        Modifier::empty().fill_max_width()
    } else {
        Modifier::empty().weight(1.0)
    };
    let title = title_override.unwrap_or_else(|| next_item.title());
    glass_panel(modifier, theme, 18.0, 18.0, {
        let fields = fields.clone();
        let title = title.clone();
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(18.0)),
                {
                    let fields = fields.clone();
                    let row_title = title.clone();
                    move || {
                        HeroTile(next_item.stage(), theme);
                        Column(
                            Modifier::empty().weight(1.0),
                            ColumnSpec::default()
                                .vertical_arrangement(LinearArrangement::spaced_by(12.0)),
                            {
                                let fields = fields.clone();
                                let title = row_title.clone();
                                move || {
                                    Row(
                                        Modifier::empty().fill_max_width(),
                                        RowSpec::default().horizontal_arrangement(
                                            LinearArrangement::SpaceBetween,
                                        ),
                                        move || {
                                            Text("Now", Modifier::empty(), eyebrow_style(theme));
                                            Text(
                                                next_item.stage().label(),
                                                Modifier::empty(),
                                                stage_label_style(theme),
                                            );
                                        },
                                    );
                                    Text(
                                        title.clone(),
                                        Modifier::empty(),
                                        heading_style(21.0, theme),
                                    );
                                    match next_item {
                                        NextWorkItem::Field(field) => {
                                            FieldSuggestion(
                                                field,
                                                active_queue_target,
                                                status,
                                                theme,
                                            );
                                        }
                                        NextWorkItem::Action(action) => {
                                            focus_action_button(
                                                action,
                                                fields.clone(),
                                                status,
                                                telegram_post_link,
                                                ui_preferences,
                                                busy_action,
                                                theme,
                                            );
                                        }
                                    }
                                }
                            },
                        );
                    }
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn InteractiveQueuePanel(
    queue: Vec<String>,
    fields: EditorFields,
    status: MutableState<String>,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    busy_action: MutableState<Option<LongAction>>,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
) {
    if queue.is_empty() {
        return;
    }

    let scroll_state = remember(|| ScrollState::new(0.0)).with(|state| *state);
    glass_panel(Modifier::empty().fill_max_width(), theme, 14.0, 10.0, {
        let fields = fields.clone();
        let queue = queue.clone();
        move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(9.0)),
                {
                    let fields = fields.clone();
                    let queue = queue.clone();
                    let current_queue = ui_preferences.value().interactive_queue().to_vec();
                    move || {
                        Text("Interactive Queue", Modifier::empty(), eyebrow_style(theme));
                        BoxWithConstraints(Modifier::empty().fill_max_width(), {
                            let fields = fields.clone();
                            let row_queue = queue.clone();
                            let row_current_queue = current_queue.clone();
                            move |scope| {
                                let _viewport_width = scope.max_width().0;
                                Row(
                                    Modifier::empty()
                                        .fill_max_width()
                                        .height(58.0)
                                        .clip_to_bounds()
                                        .horizontal_scroll(scroll_state, false),
                                    RowSpec::default().horizontal_arrangement(
                                        LinearArrangement::spaced_by(INTERACTIVE_QUEUE_CHIP_GAP),
                                    ),
                                    {
                                        let fields = fields.clone();
                                        let row_queue = row_queue.clone();
                                        let row_current_queue = row_current_queue.clone();
                                        move || {
                                            for item_key in &row_queue {
                                                InteractiveQueueChip(
                                                    item_key.clone(),
                                                    row_current_queue.contains(item_key),
                                                    fields.clone(),
                                                    status,
                                                    telegram_post_link,
                                                    ui_preferences,
                                                    busy_action,
                                                    active_queue_target,
                                                    theme,
                                                );
                                            }
                                        }
                                    },
                                );
                            }
                        });
                        QueueCurrentRow(
                            active_queue_target.value(),
                            fields.clone(),
                            status,
                            ui_preferences,
                            theme,
                        );
                    }
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn QueueCurrentRow(
    active_key: Option<String>,
    fields: EditorFields,
    status: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    theme: ThemeMode,
) {
    let Some(active_key) = active_key else {
        return;
    };
    let Some((field, FieldQueueCommand::Edit)) = parse_field_queue_key(&active_key) else {
        return;
    };

    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(7.0)),
        move || {
            Text(
                "Current",
                Modifier::empty(),
                queue_current_label_style(theme),
            );
            QueueCurrentEditorField(field, fields.clone(), status, ui_preferences, theme);
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn QueueCurrentEditorField(
    field: EditorFieldId,
    fields: EditorFields,
    status: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    theme: ThemeMode,
) {
    let state = field_state(&fields, field);
    let saved_text = state.text();
    match field {
        EditorFieldId::KotlinCode | EditorFieldId::RustCode => labeled_code_field(
            field.label(),
            field.field_id(),
            state,
            saved_text,
            6,
            14,
            status,
            ui_preferences,
            false,
            theme,
        ),
        EditorFieldId::ProblemTldr | EditorFieldId::Intuition | EditorFieldId::Approach => {
            labeled_field(
                field.label(),
                field.field_id(),
                state,
                saved_text,
                3,
                8,
                status,
                ui_preferences,
                false,
                theme,
                true,
            );
        }
        EditorFieldId::TimeComplexity | EditorFieldId::SpaceComplexity => labeled_field(
            field.label(),
            field.field_id(),
            state,
            saved_text,
            2,
            4,
            status,
            ui_preferences,
            false,
            theme,
            true,
        ),
        _ => labeled_field(
            field.label(),
            field.field_id(),
            state,
            saved_text,
            1,
            1,
            status,
            ui_preferences,
            false,
            theme,
            true,
        ),
    }
}

#[allow(non_snake_case)]
#[composable]
fn InteractiveQueueChip(
    item_key: String,
    done: bool,
    fields: EditorFields,
    status: MutableState<String>,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    busy_action: MutableState<Option<LongAction>>,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
) {
    let action = ActionButtonId::from_count_key(&item_key);
    let long_action = action.and_then(ActionButtonId::long_action);
    let action_busy = busy_action.value();
    let is_busy = long_action.is_some() && action_busy == long_action;
    let disabled = long_action.is_some() && action_busy.is_some();
    let invokes_button = queue_item_invokes_button(&item_key);
    let background = interactive_queue_surface(theme, done, disabled, is_busy, invokes_button);
    Button(
        glass_button_modifier_with_press(
            Modifier::empty()
                .width(INTERACTIVE_QUEUE_CHIP_WIDTH)
                .height(48.0),
            theme,
            !disabled,
            done || is_busy,
            background,
            10.0,
            0.0,
        )
        .padding_symmetric(10.0, 8.0),
        ButtonSpec::default(),
        {
            let item_key = item_key.clone();
            move || {
                if disabled {
                    return;
                }
                handle_interactive_queue_press(
                    &item_key,
                    fields.clone(),
                    status,
                    telegram_post_link,
                    ui_preferences,
                    busy_action,
                    active_queue_target,
                    theme,
                );
            }
        },
        move || {
            interactive_queue_content(
                interactive_queue_icon(&item_key),
                interactive_queue_label(&item_key, done, is_busy),
                interactive_queue_text_style(theme, done, disabled, is_busy),
                theme,
                is_busy,
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn interactive_queue_content(
    icon: UiIcon,
    label: String,
    style: TextStyle,
    theme: ThemeMode,
    busy: bool,
) {
    let icon_size = 24.0;
    Row(
        icon_overlay_modifier(
            Modifier::empty().fill_max_width(),
            icon,
            icon_size,
            0.0,
            theme,
            busy,
        ),
        RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(5.0)),
        move || {
            Spacer(Size::new(icon_size, 0.0));
            BasicText(
                label.clone(),
                Modifier::empty().weight(1.0),
                style.clone(),
                TextOverflow::Ellipsis,
                false,
                1,
                1,
            );
            ButtonActivityIndicator(theme, busy);
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn ActionButtons(
    fields: EditorFields,
    status: MutableState<String>,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    layout_preferences: UiPreferences,
    busy_action: MutableState<Option<LongAction>>,
    theme: ThemeMode,
) {
    let ordered_actions = ordered_action_buttons(&layout_preferences);
    BoxWithConstraints(Modifier::empty().fill_max_width(), {
        let fields = fields.clone();
        move |scope| {
            let width = scope.max_width().0;
            let columns = if width >= 820.0 {
                5
            } else if width >= 640.0 {
                4
            } else if width >= 480.0 {
                3
            } else {
                2
            };
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
                {
                    let fields = fields.clone();
                    let ordered_actions = ordered_actions.clone();
                    move || {
                        for row in ordered_actions.chunks(columns) {
                            let row_actions = row.to_vec();
                            let fields = fields.clone();
                            Row(
                                Modifier::empty().fill_max_width(),
                                RowSpec::default()
                                    .horizontal_arrangement(LinearArrangement::spaced_by(12.0)),
                                move || {
                                    for action in &row_actions {
                                        ActionButton(
                                            *action,
                                            fields.clone(),
                                            status,
                                            telegram_post_link,
                                            ui_preferences,
                                            busy_action,
                                            theme,
                                        );
                                    }
                                },
                            );
                        }
                    }
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn ActionButton(
    action: ActionButtonId,
    fields: EditorFields,
    status: MutableState<String>,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    busy_action: MutableState<Option<LongAction>>,
    theme: ThemeMode,
) {
    let action_busy = busy_action.value();
    let long_action = action.long_action();
    let is_busy = long_action.is_some() && action_busy == long_action;
    let disabled = long_action.is_some() && action_busy.is_some();
    primary_button(
        action.icon(),
        action.label(),
        action.count_key(),
        ui_preferences,
        theme,
        disabled,
        is_busy,
        move || {
            handle_action_button(
                action,
                fields.clone(),
                status,
                telegram_post_link,
                busy_action,
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn focus_action_button(
    action: ActionButtonId,
    fields: EditorFields,
    status: MutableState<String>,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    busy_action: MutableState<Option<LongAction>>,
    theme: ThemeMode,
) {
    let action_busy = busy_action.value();
    let long_action = action.long_action();
    let is_busy = long_action.is_some() && action_busy == long_action;
    let disabled = long_action.is_some() && action_busy.is_some();
    let count_key = action.count_key().to_string();
    let count = ui_preferences.value().button_count(&count_key);
    let background = if is_busy {
        button_surface(theme).with_alpha(0.86)
    } else if disabled {
        disabled_button_surface(theme)
    } else {
        button_surface(theme)
    };
    let style = if disabled {
        disabled_button_text_style(theme)
    } else {
        focus_button_text_style(theme)
    };
    Button(
        glass_button_modifier_with_press(
            Modifier::empty().fill_max_width(),
            theme,
            !disabled,
            is_busy,
            background,
            14.0,
            0.0,
        )
        .height(64.0)
        .padding_symmetric(14.0, 16.0),
        ButtonSpec::default(),
        move || {
            if disabled {
                return;
            }
            record_button_press(ui_preferences, &count_key);
            handle_action_button(
                action,
                fields.clone(),
                status,
                telegram_post_link,
                busy_action,
            );
        },
        move || {
            button_content(
                action.icon(),
                action.label().to_string(),
                count,
                style.clone(),
                theme,
                is_busy,
                true,
            );
        },
    );
}

fn handle_interactive_queue_press(
    item_key: &str,
    fields: EditorFields,
    status: MutableState<String>,
    telegram_post_link: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    busy_action: MutableState<Option<LongAction>>,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
) {
    if let Some(action) = ActionButtonId::from_count_key(item_key) {
        record_button_press(ui_preferences, item_key);
        handle_action_button(action, fields, status, telegram_post_link, busy_action);
        return;
    }

    if item_key == "theme.toggle" {
        record_button_press(ui_preferences, item_key);
        set_theme_preference(ui_preferences, theme.toggled(), status);
        return;
    }

    if let Some((field, command)) = parse_field_queue_key(item_key) {
        match command {
            FieldQueueCommand::Edit => {
                active_queue_target.set(Some(field.component_key()));
                status.set(format!("Current queue row: {}.", field.label()));
            }
            FieldQueueCommand::Paste => {
                record_button_press(ui_preferences, item_key);
                field_state(&fields, field).set_text(format!("Pasted {}", field.label()));
                status.set(format!("{} replaced from clipboard.", field.label()));
            }
            FieldQueueCommand::Clear => {
                record_button_press(ui_preferences, item_key);
                field_state(&fields, field).set_text(String::new());
                status.set(format!("{} cleared.", field.label()));
            }
        }
        return;
    }

    active_queue_target.set(Some(item_key.to_string()));
    status.set(format!("Current queued target: {item_key}."));
}

fn interactive_queue_label(item_key: &str, done: bool, busy: bool) -> String {
    let label = if let Some(action) = ActionButtonId::from_count_key(item_key) {
        action.label().to_string()
    } else if item_key == "theme.toggle" {
        "Toggle Theme".to_string()
    } else if let Some((field, command)) = parse_field_queue_key(item_key) {
        match command {
            FieldQueueCommand::Edit => format!("Edit {}", field.label()),
            FieldQueueCommand::Paste => format!("Paste {}", field.label()),
            FieldQueueCommand::Clear => format!("Clear {}", field.label()),
        }
    } else {
        item_key.to_string()
    };

    if done && !busy {
        format!("Done: {label}")
    } else {
        label
    }
}

fn queue_item_invokes_button(item_key: &str) -> bool {
    ActionButtonId::from_count_key(item_key).is_some()
        || item_key == "theme.toggle"
        || parse_field_queue_key(item_key)
            .is_some_and(|(_, command)| command != FieldQueueCommand::Edit)
}

fn interactive_queue_icon(item_key: &str) -> UiIcon {
    if let Some(action) = ActionButtonId::from_count_key(item_key) {
        return action.icon();
    }
    if item_key == "theme.toggle" {
        return UiIcon::Theme;
    }
    if let Some((field, command)) = parse_field_queue_key(item_key) {
        return match command {
            FieldQueueCommand::Edit => field.icon(),
            FieldQueueCommand::Paste => UiIcon::Paste,
            FieldQueueCommand::Clear => UiIcon::Clear,
        };
    }
    UiIcon::Generic
}

fn parse_field_queue_key(item_key: &str) -> Option<(EditorFieldId, FieldQueueCommand)> {
    let field_key = item_key.strip_prefix("field.")?;
    if let Some(field_id) = field_key.strip_suffix(".paste") {
        return EditorFieldId::from_field_id(field_id)
            .map(|field| (field, FieldQueueCommand::Paste));
    }
    if let Some(field_id) = field_key.strip_suffix(".clear") {
        return EditorFieldId::from_field_id(field_id)
            .map(|field| (field, FieldQueueCommand::Clear));
    }
    EditorFieldId::from_field_id(field_key).map(|field| (field, FieldQueueCommand::Edit))
}

fn field_state(fields: &EditorFields, field: EditorFieldId) -> TextFieldState {
    match field {
        EditorFieldId::Date => fields.date,
        EditorFieldId::ProblemTitle => fields.problem_title,
        EditorFieldId::ProblemUrl => fields.problem_url,
        EditorFieldId::Difficulty => fields.difficulty,
        EditorFieldId::BlogPostUrl => fields.blog_post_url,
        EditorFieldId::SubstackUrl => fields.substack_url,
        EditorFieldId::YoutubeUrl => fields.youtube_url,
        EditorFieldId::ReferenceUrl => fields.reference_url,
        EditorFieldId::TelegramText => fields.telegram_text,
        EditorFieldId::ProblemTldr => fields.problem_tldr,
        EditorFieldId::Intuition => fields.intuition,
        EditorFieldId::Approach => fields.approach,
        EditorFieldId::TimeComplexity => fields.time_complexity,
        EditorFieldId::SpaceComplexity => fields.space_complexity,
        EditorFieldId::KotlinRuntimeMs => fields.kotlin_runtime_ms,
        EditorFieldId::KotlinCode => fields.kotlin_code,
        EditorFieldId::RustRuntimeMs => fields.rust_runtime_ms,
        EditorFieldId::RustCode => fields.rust_code,
    }
}

fn handle_action_button(
    action: ActionButtonId,
    fields: EditorFields,
    status: MutableState<String>,
    _telegram_post_link: MutableState<String>,
    busy_action: MutableState<Option<LongAction>>,
) {
    let draft = PostDraft::from_fields(&fields);
    if let Some(long_action) = action.long_action() {
        busy_action.set(Some(long_action));
        status.set(format!("{} started...", long_action.label()));
        busy_action.set(None);
        return;
    }

    status.set(match action {
        ActionButtonId::CopyLeetcode => {
            format!("LeetCode template copied for {}.", draft.problem_title)
        }
        ActionButtonId::CopyYoutube => "YouTube template copied.".to_string(),
        ActionButtonId::CopyBlog => "Blog template copied.".to_string(),
        ActionButtonId::CopyTelegram => "Telegram template copied.".to_string(),
        ActionButtonId::CopyTitle => "Title copied.".to_string(),
        ActionButtonId::CopySubtitle => "Subtitle copied.".to_string(),
        _ => "Action completed.".to_string(),
    });
}

fn ordered_action_buttons(preferences: &UiPreferences) -> Vec<ActionButtonId> {
    let mut actions = ACTION_BUTTONS.to_vec();
    actions.sort_by_key(|action| {
        component_sort_key(
            preferences,
            action.count_key(),
            ACTION_BUTTONS
                .iter()
                .position(|candidate| candidate == action)
                .unwrap_or(usize::MAX),
        )
    });
    actions
}

fn component_sort_key(
    preferences: &UiPreferences,
    component_key: &str,
    default_index: usize,
) -> (u8, u64, usize) {
    let usage_order = preferences.component_order(component_key);
    if usage_order == 0 {
        (1, 0, default_index)
    } else {
        (0, usage_order, default_index)
    }
}

fn recommended_next_work(
    draft: &PostDraft,
    preview: &PreviewState,
    telegram_link: &str,
    preferences: &UiPreferences,
) -> NextWorkItem {
    work_queue(draft, preview, telegram_link, preferences)
        .into_iter()
        .next()
        .unwrap_or(NextWorkItem::Action(ActionButtonId::CopyBlog))
}

fn work_queue(
    draft: &PostDraft,
    preview: &PreviewState,
    telegram_link: &str,
    preferences: &UiPreferences,
) -> Vec<NextWorkItem> {
    let mut queue = Vec::new();
    for field in ordered_workflow_fields(preferences) {
        if field_needs_attention(field, draft) {
            queue.push(NextWorkItem::Field(field));
        }
    }

    if preview.last_saved_webp_path.is_none() {
        queue.push(NextWorkItem::Action(ActionButtonId::SaveRasterWebp));
    }
    if draft.blog_post_url.trim().is_empty() {
        queue.push(NextWorkItem::Action(ActionButtonId::CopyBlog));
    } else {
        queue.push(NextWorkItem::Action(ActionButtonId::PublishBlog));
    }
    if telegram_link.trim().is_empty() {
        queue.push(NextWorkItem::Action(ActionButtonId::PostTelegram));
    } else {
        queue.push(NextWorkItem::Action(ActionButtonId::PostTelegramComment));
    }

    for action in ordered_action_buttons(preferences) {
        let item = NextWorkItem::Action(action);
        if !queue.contains(&item) {
            queue.push(item);
        }
    }

    queue
}

fn interactive_queue_next_key(queue: &[String], done_queue: &[String]) -> Option<String> {
    queue
        .iter()
        .filter(|key| !done_queue.contains(key))
        .find(|key| next_work_item_from_queue_key(key).is_some())
        .cloned()
        .or_else(|| {
            queue
                .iter()
                .find(|key| next_work_item_from_queue_key(key).is_some())
                .cloned()
        })
}

fn next_work_item_from_queue_key(item_key: &str) -> Option<NextWorkItem> {
    if let Some(action) = ActionButtonId::from_count_key(item_key) {
        return Some(NextWorkItem::Action(action));
    }
    parse_field_queue_key(item_key).map(|(field, _)| NextWorkItem::Field(field))
}

fn ordered_workflow_fields(preferences: &UiPreferences) -> Vec<EditorFieldId> {
    let mut fields = WORKFLOW_FIELDS.to_vec();
    fields.sort_by_key(|field| {
        (
            field_stage(*field).sort_index(),
            component_sort_key(
                preferences,
                &field.component_key(),
                WORKFLOW_FIELDS
                    .iter()
                    .position(|candidate| candidate == field)
                    .unwrap_or(usize::MAX),
            ),
        )
    });
    fields
}

fn field_needs_attention(field: EditorFieldId, draft: &PostDraft) -> bool {
    match field {
        EditorFieldId::ProblemTitle => draft.problem_title.trim().is_empty(),
        EditorFieldId::ProblemUrl => draft.problem_url.trim().is_empty(),
        EditorFieldId::Difficulty => draft.difficulty.trim().is_empty(),
        EditorFieldId::ProblemTldr => draft.problem_tldr.trim().is_empty(),
        EditorFieldId::Intuition => draft.intuition.trim().is_empty(),
        EditorFieldId::Approach => draft.approach.trim().is_empty(),
        EditorFieldId::TimeComplexity => draft.time_complexity.trim().is_empty(),
        EditorFieldId::SpaceComplexity => draft.space_complexity.trim().is_empty(),
        EditorFieldId::KotlinRuntimeMs => draft.kotlin_runtime_ms.trim().is_empty(),
        EditorFieldId::KotlinCode => draft.kotlin_code.trim().is_empty(),
        EditorFieldId::RustRuntimeMs => draft.rust_runtime_ms.trim().is_empty(),
        EditorFieldId::RustCode => draft.rust_code.trim().is_empty(),
        EditorFieldId::Date
        | EditorFieldId::BlogPostUrl
        | EditorFieldId::SubstackUrl
        | EditorFieldId::YoutubeUrl
        | EditorFieldId::ReferenceUrl
        | EditorFieldId::TelegramText => false,
    }
}

fn field_stage(field: EditorFieldId) -> WorkStage {
    match field {
        EditorFieldId::Date
        | EditorFieldId::ProblemTitle
        | EditorFieldId::ProblemUrl
        | EditorFieldId::Difficulty => WorkStage::Prepare,
        EditorFieldId::ProblemTldr
        | EditorFieldId::Intuition
        | EditorFieldId::Approach
        | EditorFieldId::TimeComplexity
        | EditorFieldId::SpaceComplexity => WorkStage::Write,
        EditorFieldId::KotlinRuntimeMs
        | EditorFieldId::KotlinCode
        | EditorFieldId::RustRuntimeMs
        | EditorFieldId::RustCode => WorkStage::Code,
        EditorFieldId::BlogPostUrl
        | EditorFieldId::SubstackUrl
        | EditorFieldId::YoutubeUrl
        | EditorFieldId::ReferenceUrl
        | EditorFieldId::TelegramText => WorkStage::Ship,
    }
}

fn action_stage(action: ActionButtonId) -> WorkStage {
    match action {
        ActionButtonId::RefreshRasterPreview
        | ActionButtonId::RefreshCranposePreview
        | ActionButtonId::SaveRasterWebp
        | ActionButtonId::SaveCranposeWebp => WorkStage::Review,
        ActionButtonId::PublishBlog
        | ActionButtonId::PostTelegram
        | ActionButtonId::PostTelegramComment
        | ActionButtonId::CopyLeetcode
        | ActionButtonId::CopyYoutube
        | ActionButtonId::CopyBlog
        | ActionButtonId::CopyTelegram
        | ActionButtonId::CopyTitle
        | ActionButtonId::CopySubtitle
        | ActionButtonId::CopyRichText => WorkStage::Ship,
    }
}

fn ordered_fields(defaults: &[EditorFieldId], preferences: &UiPreferences) -> Vec<EditorFieldId> {
    let mut fields = defaults.to_vec();
    fields.sort_by_key(|field| {
        component_sort_key(
            preferences,
            &field.component_key(),
            defaults
                .iter()
                .position(|candidate| candidate == field)
                .unwrap_or(usize::MAX),
        )
    });
    fields
}

#[allow(non_snake_case)]
#[composable]
fn PreviewCard(
    preview_state: MutableState<PreviewState>,
    preview_loading: MutableState<bool>,
    theme: ThemeMode,
) {
    section_card(theme, {
        move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
                move || {
                    let preview = preview_state.value();
                    Text(
                        "Card Preview",
                        Modifier::empty(),
                        heading_style(28.0, theme),
                    );
                    if preview_loading.value() {
                        Text(
                            "Rendering preview in the background...",
                            Modifier::empty(),
                            accent_style(theme),
                        );
                    }
                    ComposeBox(
                        Modifier::empty()
                            .size(Size::new(1200.0, 675.0))
                            .background(panel_surface(theme))
                            .rounded_corners(8.0)
                            .padding(18.0),
                        BoxSpec::default().content_alignment(Alignment::CENTER),
                        move || {
                            Image(
                                BitmapPainter(preview.bitmap.clone()),
                                Some("Generated preview".to_string()),
                                Modifier::empty().fill_max_size().rounded_corners(8.0),
                                Alignment::CENTER,
                                ContentScale::Fit,
                                1.0,
                                None,
                            );
                        },
                    );
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn ComposePreviewCard(
    compose_preview_state: MutableState<PreviewState>,
    compose_loading: MutableState<bool>,
    compose_error: MutableState<String>,
    theme: ThemeMode,
) {
    section_card(theme, {
        move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
                move || {
                    let preview = compose_preview_state.value();
                    let error = compose_error.value();
                    Text(
                        "Cranpose Preview",
                        Modifier::empty(),
                        heading_style(28.0, theme),
                    );
                    if compose_loading.value() {
                        Text(
                            "Preparing Cranpose preview in the background...",
                            Modifier::empty(),
                            accent_style(theme),
                        );
                    } else if !error.is_empty() {
                        Text(error.clone(), Modifier::empty(), body_style(theme));
                    }
                    ComposeBox(
                        Modifier::empty()
                            .size(Size::new(1200.0, 675.0))
                            .background(panel_surface(theme))
                            .rounded_corners(8.0)
                            .padding(18.0),
                        BoxSpec::default().content_alignment(Alignment::CENTER),
                        move || {
                            if !compose_loading.value() && !error.is_empty() {
                                Text(
                                    error.clone(),
                                    Modifier::empty().fill_max_width(),
                                    body_style(theme),
                                );
                            } else {
                                Image(
                                    BitmapPainter(preview.bitmap.clone()),
                                    Some("Cranpose preview".to_string()),
                                    Modifier::empty().fill_max_size().rounded_corners(8.0),
                                    Alignment::CENTER,
                                    ContentScale::Fit,
                                    1.0,
                                    None,
                                );
                            }
                        },
                    );
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn MarkdownCard(markdown_preview: String, theme: ThemeMode) {
    section_card(theme, {
        let markdown_preview = markdown_preview.clone();
        move || {
            let markdown_preview_for_column = markdown_preview.clone();
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
                move || {
                    Text(
                        "Blog Template Preview",
                        Modifier::empty(),
                        heading_style(28.0, theme),
                    );
                    ComposeBox(
                        Modifier::empty()
                            .fill_max_width()
                            .background(panel_surface(theme))
                            .rounded_corners(8.0)
                            .padding(18.0),
                        BoxSpec::default(),
                        {
                            let markdown_preview_for_box = markdown_preview_for_column.clone();
                            move || {
                                Text(
                                    markdown_preview_for_box.clone(),
                                    Modifier::empty().fill_max_width(),
                                    code_text_style(18.0, theme),
                                );
                            }
                        },
                    );
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn ProblemMetaCard(
    fields: EditorFields,
    status: MutableState<String>,
    saved_draft: PostDraft,
    ui_preferences: MutableState<UiPreferences>,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
    compact: bool,
) {
    section_card(theme, {
        let fields = fields.clone();
        let saved_draft = saved_draft.clone();
        move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
                {
                    let fields = fields.clone();
                    let saved_draft = saved_draft.clone();
                    move || {
                        SectionHeader("Problem Meta", UiIcon::Document, theme);
                        if compact {
                            MetaFieldColumn(
                                vec![
                                    EditorFieldId::ProblemTitle,
                                    EditorFieldId::YoutubeUrl,
                                    EditorFieldId::ProblemUrl,
                                    EditorFieldId::TelegramText,
                                    EditorFieldId::ReferenceUrl,
                                    EditorFieldId::SubstackUrl,
                                    EditorFieldId::Date,
                                    EditorFieldId::Difficulty,
                                    EditorFieldId::BlogPostUrl,
                                ],
                                fields.clone(),
                                saved_draft.clone(),
                                status,
                                ui_preferences,
                                active_queue_target,
                                theme,
                                false,
                            );
                        } else {
                            Row(
                                Modifier::empty().fill_max_width(),
                                RowSpec::default()
                                    .horizontal_arrangement(LinearArrangement::spaced_by(18.0)),
                                {
                                    let fields = fields.clone();
                                    let saved_draft = saved_draft.clone();
                                    move || {
                                        MetaFieldColumn(
                                            vec![
                                                EditorFieldId::ProblemTitle,
                                                EditorFieldId::YoutubeUrl,
                                                EditorFieldId::ProblemUrl,
                                                EditorFieldId::TelegramText,
                                                EditorFieldId::ReferenceUrl,
                                            ],
                                            fields.clone(),
                                            saved_draft.clone(),
                                            status,
                                            ui_preferences,
                                            active_queue_target,
                                            theme,
                                            true,
                                        );
                                        MetaFieldColumn(
                                            vec![
                                                EditorFieldId::SubstackUrl,
                                                EditorFieldId::Date,
                                                EditorFieldId::Difficulty,
                                                EditorFieldId::BlogPostUrl,
                                            ],
                                            fields.clone(),
                                            saved_draft.clone(),
                                            status,
                                            ui_preferences,
                                            active_queue_target,
                                            theme,
                                            true,
                                        );
                                    }
                                },
                            );
                        }
                    }
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn MetaFieldColumn(
    field_ids: Vec<EditorFieldId>,
    fields: EditorFields,
    saved_draft: PostDraft,
    status: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
    weighted: bool,
) {
    let modifier = if weighted {
        Modifier::empty().weight(1.0)
    } else {
        Modifier::empty().fill_max_width()
    };
    Column(
        modifier,
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            for field in &field_ids {
                EditorField(
                    *field,
                    fields.clone(),
                    saved_draft.clone(),
                    status,
                    ui_preferences,
                    active_queue_target,
                    theme,
                );
            }
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn WriteupCard(
    fields: EditorFields,
    status: MutableState<String>,
    saved_draft: PostDraft,
    ui_preferences: MutableState<UiPreferences>,
    layout_preferences: UiPreferences,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
) {
    section_card(theme, {
        let fields = fields.clone();
        let saved_draft = saved_draft.clone();
        let layout_preferences = layout_preferences.clone();
        move || {
            let ordered_fields = ordered_fields(&WRITEUP_FIELDS, &layout_preferences);
            let fields_for_column = fields.clone();
            let saved_draft_for_column = saved_draft.clone();
            let status_for_column = status;
            let ui_preferences_for_column = ui_preferences;
            let active_queue_target_for_column = active_queue_target;
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
                move || {
                    Text("Writeup", Modifier::empty(), heading_style(28.0, theme));
                    for field in &ordered_fields {
                        EditorField(
                            *field,
                            fields_for_column.clone(),
                            saved_draft_for_column.clone(),
                            status_for_column,
                            ui_preferences_for_column,
                            active_queue_target_for_column,
                            theme,
                        );
                    }
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn CodeCard(
    fields: EditorFields,
    status: MutableState<String>,
    saved_draft: PostDraft,
    ui_preferences: MutableState<UiPreferences>,
    layout_preferences: UiPreferences,
    active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
) {
    section_card(theme, {
        let fields = fields.clone();
        let saved_draft = saved_draft.clone();
        let layout_preferences = layout_preferences.clone();
        move || {
            let ordered_fields = ordered_fields(&CODE_FIELDS, &layout_preferences);
            let fields_for_column = fields.clone();
            let saved_draft_for_column = saved_draft.clone();
            let status_for_column = status;
            let ui_preferences_for_column = ui_preferences;
            let active_queue_target_for_column = active_queue_target;
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(14.0)),
                move || {
                    Text("Code Blocks", Modifier::empty(), heading_style(28.0, theme));
                    for field in &ordered_fields {
                        EditorField(
                            *field,
                            fields_for_column.clone(),
                            saved_draft_for_column.clone(),
                            status_for_column,
                            ui_preferences_for_column,
                            active_queue_target_for_column,
                            theme,
                        );
                    }
                },
            );
        }
    });
}

#[allow(non_snake_case)]
#[composable]
fn EditorField(
    field: EditorFieldId,
    fields: EditorFields,
    saved_draft: PostDraft,
    status: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    _active_queue_target: MutableState<Option<String>>,
    theme: ThemeMode,
) {
    let highlighted = false;
    match field {
        EditorFieldId::Date => labeled_field(
            field.label(),
            field.field_id(),
            fields.date,
            saved_draft.date.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::ProblemTitle => labeled_field(
            field.label(),
            field.field_id(),
            fields.problem_title,
            saved_draft.problem_title.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::ProblemUrl => labeled_field(
            field.label(),
            field.field_id(),
            fields.problem_url,
            saved_draft.problem_url.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::Difficulty => labeled_field(
            field.label(),
            field.field_id(),
            fields.difficulty,
            saved_draft.difficulty.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::BlogPostUrl => labeled_field(
            field.label(),
            field.field_id(),
            fields.blog_post_url,
            saved_draft.blog_post_url.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::SubstackUrl => labeled_field(
            field.label(),
            field.field_id(),
            fields.substack_url,
            saved_draft.substack_url.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::YoutubeUrl => labeled_field(
            field.label(),
            field.field_id(),
            fields.youtube_url,
            saved_draft.youtube_url.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::ReferenceUrl => labeled_field(
            field.label(),
            field.field_id(),
            fields.reference_url,
            saved_draft.reference_url.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::TelegramText => labeled_field(
            field.label(),
            field.field_id(),
            fields.telegram_text,
            saved_draft.telegram_text.clone(),
            1,
            2,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::ProblemTldr => labeled_field(
            field.label(),
            field.field_id(),
            fields.problem_tldr,
            saved_draft.problem_tldr.clone(),
            3,
            6,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::Intuition => labeled_field(
            field.label(),
            field.field_id(),
            fields.intuition,
            saved_draft.intuition.clone(),
            6,
            14,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::Approach => labeled_field(
            field.label(),
            field.field_id(),
            fields.approach,
            saved_draft.approach.clone(),
            6,
            14,
            status,
            ui_preferences,
            highlighted,
            theme,
            true,
        ),
        EditorFieldId::TimeComplexity => labeled_field(
            field.label(),
            field.field_id(),
            fields.time_complexity,
            saved_draft.time_complexity.clone(),
            1,
            2,
            status,
            ui_preferences,
            highlighted,
            theme,
            false,
        ),
        EditorFieldId::SpaceComplexity => labeled_field(
            field.label(),
            field.field_id(),
            fields.space_complexity,
            saved_draft.space_complexity.clone(),
            1,
            2,
            status,
            ui_preferences,
            highlighted,
            theme,
            false,
        ),
        EditorFieldId::KotlinRuntimeMs => labeled_field(
            field.label(),
            field.field_id(),
            fields.kotlin_runtime_ms,
            saved_draft.kotlin_runtime_ms.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            false,
        ),
        EditorFieldId::KotlinCode => labeled_code_field(
            field.label(),
            field.field_id(),
            fields.kotlin_code,
            saved_draft.kotlin_code.clone(),
            10,
            18,
            status,
            ui_preferences,
            highlighted,
            theme,
        ),
        EditorFieldId::RustRuntimeMs => labeled_field(
            field.label(),
            field.field_id(),
            fields.rust_runtime_ms,
            saved_draft.rust_runtime_ms.clone(),
            1,
            1,
            status,
            ui_preferences,
            highlighted,
            theme,
            false,
        ),
        EditorFieldId::RustCode => labeled_code_field(
            field.label(),
            field.field_id(),
            fields.rust_code,
            saved_draft.rust_code.clone(),
            10,
            18,
            status,
            ui_preferences,
            highlighted,
            theme,
        ),
    }
}

#[allow(non_snake_case)]
#[composable]
fn ReferenceIcon(icon: UiIcon, size: Size, theme: ThemeMode, active: bool) {
    ComposeBox(
        Modifier::empty().size(size).draw_behind(move |scope| {
            let size = scope.size();
            draw_ui_icon(
                scope,
                icon,
                rect(0.0, 0.0, size.width, size.height),
                theme,
                if active { 1.0 } else { 0.96 },
            );
        }),
        BoxSpec::default(),
        || {},
    );
}

#[allow(non_snake_case)]
#[composable]
fn AppLogo() {
    ComposeBox(
        Modifier::empty()
            .size(Size::new(76.0, 76.0))
            .drop_shadow(
                LayerShape::Rounded(RoundedCornerShape::uniform(38.0)),
                |shadow| {
                    shadow.radius = 18.0;
                    shadow.spread = 1.0;
                    shadow.offset = Point::new(0.0, 8.0);
                    shadow.color = Color::from_rgba_u8(34, 127, 194, 112);
                },
            )
            .draw_behind(|scope| {
                let dst = snap_rect(rect(0.0, 0.0, 76.0, 76.0));
                if let Some(bitmap) = app_logo_bitmap() {
                    draw_sharp_image_src(
                        scope,
                        bitmap.clone(),
                        rect(0.0, 0.0, bitmap.width() as f32, bitmap.height() as f32),
                        dst,
                        1.0,
                        None,
                    );
                } else {
                    draw_ui_icon(scope, UiIcon::AppLogo, dst, ThemeMode::Light, 1.0);
                }
            }),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        || {},
    );
}

#[allow(non_snake_case)]
#[composable]
fn SectionHeader(title: &'static str, icon: UiIcon, theme: ThemeMode) {
    Row(
        icon_overlay_modifier(Modifier::empty(), icon, 24.0, 0.0, theme, false),
        RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(10.0)),
        move || {
            Spacer(Size::new(24.0, 0.0));
            Text(title, Modifier::empty(), heading_style(24.0, theme));
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn HeroTile(stage: WorkStage, theme: ThemeMode) {
    ComposeBox(
        glass_panel_modifier(Modifier::empty().size(Size::new(184.0, 164.0)), theme, 18.0)
            .padding(0.0),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            if let Some(bitmap) = hero_bitmap(stage) {
                ComposeBox(
                    Modifier::empty()
                        .size(Size::new(176.0, 160.0))
                        .draw_behind(move |scope| {
                            draw_sharp_image_src(
                                scope,
                                bitmap.clone(),
                                rect(0.0, 0.0, bitmap.width() as f32, bitmap.height() as f32),
                                snap_rect(rect(0.0, 0.0, 176.0, 160.0)),
                                1.0,
                                None,
                            );
                        }),
                    BoxSpec::default(),
                    || {},
                );
            } else {
                ReferenceIcon(stage.icon(), Size::new(112.0, 112.0), theme, true);
            }
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn FieldSuggestion(
    field: EditorFieldId,
    active_queue_target: MutableState<Option<String>>,
    status: MutableState<String>,
    theme: ThemeMode,
) {
    let icon = field.icon();
    Button(
        glass_button_modifier_with_press(
            Modifier::empty().fill_max_width(),
            theme,
            true,
            false,
            soft_button_surface(theme),
            11.0,
            0.0,
        )
        .padding_symmetric(13.0, 11.0),
        ButtonSpec::default(),
        move || {
            active_queue_target.set(Some(field.component_key()));
            status.set(format!("Current queue row: {}.", field.label()));
        },
        move || {
            Row(
                icon_overlay_modifier(Modifier::empty(), icon, 24.0, 0.0, theme, false),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(9.0)),
                move || {
                    Spacer(Size::new(24.0, 0.0));
                    Text(field.label(), Modifier::empty(), queue_text_style(theme));
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn StatusDot(ok: bool, theme: ThemeMode) {
    ComposeBox(
        glass_button_modifier(
            Modifier::empty().size(Size::new(22.0, 22.0)),
            theme,
            true,
            ok,
            if ok {
                Color::from_rgb_u8(91, 204, 68)
            } else {
                Color::from_rgb_u8(247, 168, 21)
            },
            11.0,
        ),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                if ok { "OK" } else { "!" },
                Modifier::empty(),
                dot_style(theme),
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn section_card(theme: ThemeMode, content: impl FnMut() + 'static) {
    let radius = 18.0;
    let shape = LayerShape::Rounded(RoundedCornerShape::uniform(radius));
    glass_panel(
        Modifier::empty()
            .fill_max_width()
            .drop_shadow(shape, move |shadow| {
                shadow.radius = 22.0;
                shadow.spread = 1.0;
                shadow.offset = Point::new(0.0, 14.0);
                shadow.color = Color::from_rgba_u8(16, 79, 122, 82);
                shadow.alpha = 0.36;
            }),
        theme,
        radius,
        20.0,
        content,
    );
}

fn workspace_viewport_modifier(
    modifier: Modifier,
    theme: ThemeMode,
    scroll_state: ScrollState,
    viewport_width: f32,
    viewport_height: f32,
) -> Modifier {
    modifier
        .draw_with_content(move |scope| {
            scope.draw_content();
            let size = scope.size();
            let current_scroll = scroll_state.value_non_reactive();
            let max_scroll = scroll_state.max_value();
            if current_scroll > 0.5 {
                draw_workspace_scroll_shadow(scope, theme, size, true);
            }
            if max_scroll - current_scroll > 0.5 {
                draw_workspace_scroll_shadow(scope, theme, size, false);
            }
        })
        .graphics_layer_block(|layer| {
            layer.clip = true;
            layer.compositing_strategy = CompositingStrategy::Offscreen;
        })
        .rounded_alpha_mask(viewport_width, viewport_height, 0.0, 0.0)
        .clip_to_bounds()
}

fn draw_workspace_scroll_shadow(
    scope: &mut dyn DrawScope,
    theme: ThemeMode,
    size: Size,
    top: bool,
) {
    let shadow_height = WORKSPACE_SCROLL_SHADOW_HEIGHT.min(size.height.max(0.0));
    if shadow_height <= 0.0 || size.width <= 0.0 {
        return;
    }
    let y = if top {
        0.0
    } else {
        (size.height - shadow_height).max(0.0)
    };
    let edge_color = match theme {
        ThemeMode::Dark => Color::from_rgba_u8(3, 33, 72, 178),
        ThemeMode::Light => Color::from_rgba_u8(4, 57, 105, 152),
    };
    let mid_color = match theme {
        ThemeMode::Dark => Color::from_rgba_u8(6, 76, 132, 92),
        ThemeMode::Light => Color::from_rgba_u8(8, 86, 140, 78),
    };
    let soft_color = match theme {
        ThemeMode::Dark => Color::from_rgba_u8(8, 86, 140, 38),
        ThemeMode::Light => Color::from_rgba_u8(8, 86, 140, 34),
    };
    let colors = if top {
        vec![edge_color, mid_color, soft_color, Color::TRANSPARENT]
    } else {
        vec![Color::TRANSPARENT, soft_color, mid_color, edge_color]
    };
    scope.draw_rect_at(
        rect(0.0, y, size.width, shadow_height),
        Brush::linear_gradient_range(
            colors,
            Point::new(0.0, y),
            Point::new(0.0, y + shadow_height),
        ),
    );
}

#[allow(non_snake_case)]
#[composable]
fn glass_panel(
    modifier: Modifier,
    theme: ThemeMode,
    radius: f32,
    padding: f32,
    content: impl FnMut() + 'static,
) {
    ComposeBox(
        glass_panel_modifier(modifier, theme, radius).padding(padding),
        BoxSpec::default(),
        content,
    );
}

fn glass_panel_modifier(modifier: Modifier, theme: ThemeMode, radius: f32) -> Modifier {
    let shape = LayerShape::Rounded(RoundedCornerShape::uniform(radius));
    modifier
        .drop_shadow(shape, move |shadow| {
            shadow.radius = 18.0;
            shadow.spread = 0.0;
            shadow.offset = Point::new(0.0, 8.0);
            shadow.color = shadow_color(theme);
            shadow.alpha = 0.78;
        })
        .draw_behind(move |scope| {
            let size = scope.size();
            let radii = CornerRadii::uniform(radius);
            scope.draw_round_rect(panel_brush(theme, size), radii);
            let gloss_stops = match theme {
                ThemeMode::Dark => vec![
                    Color::from_rgba_u8(120, 168, 220, 92),
                    Color::from_rgba_u8(60, 96, 148, 30),
                    Color::from_rgba_u8(8, 18, 36, 0),
                ],
                ThemeMode::Light => vec![
                    Color::from_rgba_u8(255, 255, 255, 205),
                    Color::from_rgba_u8(255, 255, 255, 62),
                    Color::from_rgba_u8(17, 144, 212, 42),
                ],
            };
            scope.draw_round_rect(
                Brush::linear_gradient_range(
                    gloss_stops,
                    Point::new(0.0, 0.0),
                    Point::new(size.width, size.height),
                ),
                radii,
            );
            let highlight = match theme {
                ThemeMode::Dark => Color::from_rgba_u8(140, 188, 240, 110),
                ThemeMode::Light => Color::from_rgba_u8(255, 255, 255, 180),
            };
            scope.draw_rect_at(
                rect(2.0, 2.0, (size.width - 4.0).max(0.0), 2.0),
                Brush::horizontal_gradient(
                    vec![Color::TRANSPARENT, highlight, Color::TRANSPARENT],
                    0.0,
                    size.width,
                ),
            );
        })
        .inner_shadow(shape, move |shadow| {
            shadow.radius = 8.0;
            shadow.spread = -1.0;
            shadow.offset = Point::new(0.0, 2.0);
            shadow.color = match theme {
                ThemeMode::Dark => Color::from_rgba_u8(150, 195, 240, 110),
                ThemeMode::Light => Color::from_rgba_u8(255, 255, 255, 150),
            };
            shadow.alpha = match theme {
                ThemeMode::Dark => 0.45,
                ThemeMode::Light => 0.72,
            };
        })
        .rounded_corners(radius)
}

fn glass_button_modifier(
    modifier: Modifier,
    theme: ThemeMode,
    enabled: bool,
    active: bool,
    base: Color,
    radius: f32,
) -> Modifier {
    glass_button_modifier_with_press(modifier, theme, enabled, active, base, radius, 0.0)
}

fn glass_button_modifier_with_press(
    modifier: Modifier,
    theme: ThemeMode,
    enabled: bool,
    active: bool,
    base: Color,
    radius: f32,
    press: f32,
) -> Modifier {
    let press = if enabled { press.clamp(0.0, 1.0) } else { 0.0 };
    let shape = LayerShape::Rounded(RoundedCornerShape::uniform(radius));
    let shadow_alpha = if enabled { 0.64 } else { 0.18 };
    let scale = 1.0 - press * BUTTON_PRESS_SCALE_DELTA;
    let translation_y = press * BUTTON_PRESS_TRANSLATION_Y;
    modifier
        .graphics_layer_block(move |layer| {
            layer.scale_x = scale;
            layer.scale_y = scale;
            layer.translation_y = translation_y;
        })
        .drop_shadow(shape, move |shadow| {
            let base_radius = if active { 13.0 } else { 9.0 };
            let base_spread = if active { 1.0 } else { 0.0 };
            let base_offset = if active { 6.0 } else { 4.0 };
            shadow.radius = (base_radius - press * 3.0).max(2.0);
            shadow.spread = (base_spread - press * 0.45).max(0.0);
            shadow.offset = Point::new(0.0, (base_offset - press * 1.8).max(1.0));
            shadow.color = shadow_color(theme);
            shadow.alpha = shadow_alpha * (1.0 - press * 0.24);
        })
        .draw_behind(move |scope| {
            let size = scope.size();
            let radii = CornerRadii::uniform(radius);
            let top = if enabled {
                lighten_color(base, (if active { 0.56 } else { 0.38 }) - press * 0.08)
            } else {
                base.with_alpha(0.5)
            };
            let bottom = if enabled {
                darken_color(base, (if active { 0.16 } else { 0.06 }) + press * 0.08)
            } else {
                base.with_alpha(0.38)
            };
            scope.draw_round_rect(
                Brush::linear_gradient_range(
                    vec![top, base.with_alpha(base.a().max(0.82)), bottom],
                    Point::new(0.0, 0.0),
                    Point::new(0.0, size.height),
                ),
                radii,
            );
            let gloss_height = (size.height * 0.44).max(1.0);
            scope.draw_round_rect(
                Brush::linear_gradient_range(
                    vec![
                        Color::from_rgba_u8(255, 255, 255, if active { 145 } else { 118 }),
                        Color::from_rgba_u8(255, 255, 255, if active { 58 } else { 42 }),
                        Color::TRANSPARENT,
                    ],
                    Point::new(0.0, 0.0),
                    Point::new(0.0, gloss_height),
                ),
                radii,
            );
            scope.draw_rect_at(
                rect(
                    radius * 0.45,
                    0.0,
                    (size.width - radius * 0.9).max(0.0),
                    3.0,
                ),
                Brush::horizontal_gradient(
                    vec![
                        Color::TRANSPARENT,
                        Color::from_rgba_u8(255, 255, 255, if active { 230 } else { 190 }),
                        Color::TRANSPARENT,
                    ],
                    0.0,
                    size.width,
                ),
            );
        })
        .inner_shadow(shape, move |shadow| {
            shadow.radius = 5.0 + press * 2.0;
            shadow.spread = -1.0;
            shadow.offset = Point::new(0.0, 1.0 + press);
            shadow.color = Color::from_rgba_u8(255, 255, 255, if active { 180 } else { 115 });
            shadow.alpha = if enabled { 0.72 - press * 0.18 } else { 0.3 };
        })
        .rounded_corners(radius)
}

#[allow(non_snake_case)]
#[composable]
fn primary_button(
    icon: UiIcon,
    label: &'static str,
    count_key: &'static str,
    ui_preferences: MutableState<UiPreferences>,
    theme: ThemeMode,
    disabled: bool,
    busy: bool,
    on_click: impl FnMut() + 'static,
) {
    let count = ui_preferences.value().button_count(count_key);
    let count_key = count_key.to_string();
    let background = if busy {
        button_surface(theme).with_alpha(0.86)
    } else if disabled {
        disabled_button_surface(theme)
    } else {
        button_surface(theme)
    };
    let text_style = if busy {
        busy_button_text_style(theme)
    } else if disabled {
        disabled_button_text_style(theme)
    } else {
        button_text_style(theme)
    };
    Button(
        glass_button_modifier_with_press(
            Modifier::empty().weight(1.0),
            theme,
            !disabled,
            busy,
            background,
            10.0,
            0.0,
        )
        .height(46.0)
        .padding_symmetric(8.0, 9.0),
        ButtonSpec::default(),
        move || {
            if disabled {
                return;
            }
            record_button_press(ui_preferences, &count_key);
            on_click();
        },
        move || {
            button_content(
                icon,
                label.to_string(),
                count,
                text_style.clone(),
                theme,
                busy,
                true,
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn subtle_button(
    label: String,
    count_key: String,
    ui_preferences: MutableState<UiPreferences>,
    theme: ThemeMode,
    on_click: impl FnMut() + 'static,
) {
    let count = ui_preferences.value().button_count(&count_key);
    Button(
        glass_button_modifier_with_press(
            Modifier::empty(),
            theme,
            true,
            false,
            soft_button_surface(theme),
            9.0,
            0.0,
        )
        .padding_symmetric(9.0, 7.0),
        ButtonSpec::default(),
        move || {
            record_button_press(ui_preferences, &count_key);
            on_click();
        },
        move || {
            button_content(
                button_icon_for_label(&label),
                label.clone(),
                count,
                subtle_button_text_style(theme),
                theme,
                false,
                false,
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn theme_button(label: String, theme: ThemeMode, on_click: impl FnMut() + 'static) {
    Button(
        glass_button_modifier_with_press(
            Modifier::empty(),
            theme,
            true,
            false,
            soft_button_surface(theme),
            9.0,
            0.0,
        )
        .padding_symmetric(10.0, 7.0),
        ButtonSpec::default(),
        on_click,
        move || {
            let label_for_row = label.clone();
            Row(
                icon_overlay_modifier(Modifier::empty(), UiIcon::Theme, 24.0, 0.0, theme, false),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                move || {
                    Spacer(Size::new(24.0, 0.0));
                    Text(
                        label_for_row.clone(),
                        Modifier::empty(),
                        subtle_button_text_style(theme),
                    );
                    Text("v", Modifier::empty(), subtle_button_text_style(theme));
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn button_content(
    icon: UiIcon,
    label: String,
    count: u64,
    style: TextStyle,
    theme: ThemeMode,
    busy: bool,
    expand_label: bool,
) {
    let icon_size = 24.0;
    let row_modifier = if expand_label {
        Modifier::empty().fill_max_width()
    } else {
        Modifier::empty()
    };
    Row(
        icon_overlay_modifier(row_modifier, icon, icon_size, 0.0, theme, busy),
        RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            Spacer(Size::new(icon_size, 0.0));
            if expand_label {
                BasicText(
                    label.clone(),
                    Modifier::empty().weight(1.0),
                    style.clone(),
                    TextOverflow::Ellipsis,
                    false,
                    1,
                    1,
                );
            } else {
                BasicText(
                    label.clone(),
                    Modifier::empty(),
                    style.clone(),
                    TextOverflow::Ellipsis,
                    false,
                    1,
                    1,
                );
            }
            ButtonActivityIndicator(theme, busy);
            button_badge(count, theme);
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn ButtonActivityIndicator(theme: ThemeMode, active: bool) {
    let indicator_width = if active {
        BUTTON_ACTIVITY_INDICATOR_WIDTH
    } else {
        0.0
    };
    ComposeBox(
        Modifier::empty()
            .size(Size::new(indicator_width, BUTTON_ACTIVITY_INDICATOR_HEIGHT))
            .clip_to_bounds()
            .draw_behind(move |scope| {
                if !active {
                    return;
                }
                let size = scope.size();
                let track_color = match theme {
                    ThemeMode::Dark => Color::from_rgba_u8(150, 195, 240, 86),
                    ThemeMode::Light => Color::from_rgba_u8(4, 73, 124, 78),
                };
                scope.draw_rect_at(rect(0.0, 7.0, size.width, 5.0), Brush::solid(track_color));
                scope.draw_rect_at(
                    rect(0.0, 3.0, 24.0_f32.min(size.width), 12.0),
                    Brush::horizontal_gradient(
                        vec![
                            accent_color(theme).with_alpha(0.96),
                            Color::from_rgba_u8(255, 255, 255, 250),
                        ],
                        0.0,
                        24.0,
                    ),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[allow(non_snake_case)]
#[composable]
fn button_badge(count: u64, theme: ThemeMode) {
    ComposeBox(
        Modifier::empty()
            .background(badge_surface(theme))
            .rounded_corners(999.0)
            .padding_symmetric(5.0, 1.0),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                count.to_string(),
                Modifier::empty(),
                badge_text_style(theme),
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn labeled_field(
    label: &'static str,
    field_id: &'static str,
    state: TextFieldState,
    saved_text: String,
    min_lines: usize,
    max_lines: usize,
    status: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    _highlighted: bool,
    theme: ThemeMode,
    allow_paste: bool,
) {
    let current_text = state.text();
    let is_changed = current_text != saved_text;
    let icon = UiIcon::for_field_id(field_id);
    ComposeBox(
        glass_panel_modifier(Modifier::empty().fill_max_width(), theme, 12.0)
            .padding_symmetric(12.0, 10.0),
        BoxSpec::default(),
        move || {
            Row(
                icon_overlay_modifier(
                    Modifier::empty().fill_max_width(),
                    icon,
                    44.0,
                    0.0,
                    theme,
                    false,
                ),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(16.0)),
                {
                    move || {
                        Spacer(Size::new(44.0, 0.0));
                        let field_state = state;
                        Column(
                            Modifier::empty().weight(1.0),
                            ColumnSpec::default()
                                .vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                            {
                                move || {
                                    Text(label, Modifier::empty(), label_style(theme, is_changed));
                                    ComposeBox(
                                        Modifier::empty()
                                            .fill_max_width()
                                            .background(input_surface(theme))
                                            .rounded_corners(8.0)
                                            .padding_symmetric(11.0, 7.0),
                                        BoxSpec::default(),
                                        move || {
                                            BasicTextFieldWithOptions(
                                                field_state,
                                                Modifier::empty().fill_max_width(),
                                                BasicTextFieldOptions {
                                                    text_style: field_text_style(theme),
                                                    cursor_color: accent_color(theme),
                                                    line_limits: if min_lines == 1 && max_lines == 1
                                                    {
                                                        TextFieldLineLimits::SingleLine
                                                    } else {
                                                        TextFieldLineLimits::MultiLine {
                                                            min_lines,
                                                            max_lines,
                                                        }
                                                    },
                                                },
                                            );
                                        },
                                    );
                                }
                            },
                        );
                        field_action_buttons(
                            label,
                            field_id,
                            state,
                            status,
                            allow_paste,
                            ui_preferences,
                            theme,
                        );
                    }
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn labeled_code_field(
    label: &'static str,
    field_id: &'static str,
    state: TextFieldState,
    saved_text: String,
    min_lines: usize,
    max_lines: usize,
    status: MutableState<String>,
    ui_preferences: MutableState<UiPreferences>,
    _highlighted: bool,
    theme: ThemeMode,
) {
    let current_text = state.text();
    let is_changed = current_text != saved_text;
    let icon = UiIcon::for_field_id(field_id);
    ComposeBox(
        glass_panel_modifier(Modifier::empty().fill_max_width(), theme, 12.0)
            .padding_symmetric(12.0, 10.0),
        BoxSpec::default(),
        move || {
            Row(
                icon_overlay_modifier(
                    Modifier::empty().fill_max_width(),
                    icon,
                    44.0,
                    0.0,
                    theme,
                    false,
                ),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(16.0)),
                {
                    move || {
                        Spacer(Size::new(44.0, 0.0));
                        let field_state = state;
                        Column(
                            Modifier::empty().weight(1.0),
                            ColumnSpec::default()
                                .vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                            {
                                move || {
                                    Text(label, Modifier::empty(), label_style(theme, is_changed));
                                    ComposeBox(
                                        Modifier::empty()
                                            .fill_max_width()
                                            .background(input_surface(theme))
                                            .rounded_corners(8.0)
                                            .padding(12.0),
                                        BoxSpec::default(),
                                        move || {
                                            BasicTextFieldWithOptions(
                                                field_state,
                                                Modifier::empty().fill_max_width(),
                                                BasicTextFieldOptions {
                                                    text_style: code_field_style(theme),
                                                    cursor_color: accent_color(theme),
                                                    line_limits: if min_lines == 1 && max_lines == 1
                                                    {
                                                        TextFieldLineLimits::SingleLine
                                                    } else {
                                                        TextFieldLineLimits::MultiLine {
                                                            min_lines,
                                                            max_lines,
                                                        }
                                                    },
                                                },
                                            );
                                        },
                                    );
                                }
                            },
                        );
                        field_action_buttons(
                            label,
                            field_id,
                            state,
                            status,
                            true,
                            ui_preferences,
                            theme,
                        );
                    }
                },
            );
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn field_action_buttons(
    label: &'static str,
    field_id: &'static str,
    state: TextFieldState,
    status: MutableState<String>,
    allow_paste: bool,
    ui_preferences: MutableState<UiPreferences>,
    theme: ThemeMode,
) {
    Row(
        Modifier::empty(),
        RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(10.0)),
        move || {
            if allow_paste {
                let paste_state = state;
                let paste_status = status;
                subtle_button(
                    "Paste".to_string(),
                    format!("field.{field_id}.paste"),
                    ui_preferences,
                    theme,
                    move || {
                        paste_state.set_text(format!("Pasted {label}"));
                        paste_status.set(format!("{label} replaced from clipboard."));
                    },
                );
            }

            let clear_state = state;
            let clear_status = status;
            subtle_button(
                "Clear".to_string(),
                format!("field.{field_id}.clear"),
                ui_preferences,
                theme,
                move || {
                    clear_state.set_text(String::new());
                    clear_status.set(format!("{label} cleared."));
                },
            );
        },
    );
}

fn record_button_press(ui_preferences: MutableState<UiPreferences>, count_key: &str) {
    ui_preferences.update(|preferences| {
        preferences.increment_button_count(count_key);
        preferences.mark_component_used(count_key);
        preferences.record_interactive_queue_item(count_key);
        preferences.clone()
    });
}

fn set_theme_preference(
    ui_preferences: MutableState<UiPreferences>,
    theme: ThemeMode,
    status: MutableState<String>,
) {
    ui_preferences.update(|preferences| {
        preferences.theme = theme;
        preferences.clone()
    });
    status.set(format!("Theme set to {}.", theme.label()));
}

const ICON_SHEET_COLUMNS: u32 = 8;
const ICON_SHEET_CELL: u32 = 256;
const SHARP_IMAGE_MAX_PIXELS: u32 = 4_000_000;

fn app_background_bitmap() -> Option<ImageBitmap> {
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().app_background_bitmap())
}

fn hero_bitmap(stage: WorkStage) -> Option<ImageBitmap> {
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().hero_bitmap(stage))
}

fn app_logo_bitmap() -> Option<ImageBitmap> {
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().app_logo_bitmap())
}

fn ui_icons_bitmap() -> Option<ImageBitmap> {
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().ui_icons_bitmap())
}

fn ui_icons_bitmap_24() -> Option<ImageBitmap> {
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().ui_icons_bitmap_24())
}

fn ui_icons_bitmap_44() -> Option<ImageBitmap> {
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().ui_icons_bitmap_44())
}

fn ui_icons_bitmap_58() -> Option<ImageBitmap> {
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().ui_icons_bitmap_58())
}

fn bitmap_from_png(bytes: &[u8]) -> Option<ImageBitmap> {
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    ImageBitmap::from_rgba8(image.width(), image.height(), image.into_raw()).ok()
}

fn preview_bitmap() -> ImageBitmap {
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().preview_bitmap())
}

fn build_preview_bitmap() -> ImageBitmap {
    let width = 1200u32;
    let height = 675u32;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let stripe = ((x / 40) + (y / 36)) % 2 == 0;
            let panel = x > 78 && x < width - 78 && y > 70 && y < height - 70;
            let rgba = if panel && stripe {
                [226, 248, 255, 255]
            } else if panel {
                [196, 236, 244, 255]
            } else {
                [28, 66, 104, 255]
            };
            pixels[index..index + 4].copy_from_slice(&rgba);
        }
    }
    ImageBitmap::from_rgba8(width, height, pixels).expect("preview bitmap")
}

fn draw_sharp_image_src<S: DrawScope + ?Sized>(
    scope: &mut S,
    bitmap: ImageBitmap,
    src: Rect,
    dst: Rect,
    alpha: f32,
    color_filter: Option<ColorFilter>,
) {
    let dst = snap_rect(dst);
    if let Some(sharp_bitmap) = sharp_bitmap_for_draw(&bitmap, src, dst) {
        let sharp_src = rect(
            0.0,
            0.0,
            sharp_bitmap.width() as f32,
            sharp_bitmap.height() as f32,
        );
        scope.draw_image_src(sharp_bitmap, sharp_src, dst, alpha, color_filter);
    } else {
        scope.draw_image_src(bitmap, src, dst, alpha, color_filter);
    }
}

fn sharp_bitmap_for_draw(bitmap: &ImageBitmap, src: Rect, dst: Rect) -> Option<ImageBitmap> {
    let (src_x, src_y, src_width, src_height) = source_rect_pixels(bitmap, src)?;
    let (target_width, target_height) = target_image_pixels(dst)?;
    if target_width.saturating_mul(target_height) > SHARP_IMAGE_MAX_PIXELS {
        return None;
    }
    let source_is_full =
        src_x == 0 && src_y == 0 && src_width == bitmap.width() && src_height == bitmap.height();
    if source_is_full && src_width == target_width && src_height == target_height {
        return Some(bitmap.clone());
    }

    let key = SharpBitmapKey {
        image_id: bitmap.id(),
        src_x,
        src_y,
        src_width,
        src_height,
        target_width,
        target_height,
    };
    LEETCODEDAILY_IMAGE_CACHE.with(|cache| cache.borrow_mut().sharp_bitmap_for_draw(bitmap, key))
}

fn source_rect_pixels(bitmap: &ImageBitmap, src: Rect) -> Option<(u32, u32, u32, u32)> {
    let max_width = bitmap.width() as f32;
    let max_height = bitmap.height() as f32;
    let x = src.x.round().clamp(0.0, max_width) as u32;
    let y = src.y.round().clamp(0.0, max_height) as u32;
    let right = (src.x + src.width).round().clamp(0.0, max_width) as u32;
    let bottom = (src.y + src.height).round().clamp(0.0, max_height) as u32;
    (right > x && bottom > y).then_some((x, y, right - x, bottom - y))
}

fn target_image_pixels(dst: Rect) -> Option<(u32, u32)> {
    if dst.width <= 0.0 || dst.height <= 0.0 {
        return None;
    }
    let density = cranpose_ui::current_density();
    let density = if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    };
    let width = (dst.width * density).round().max(1.0) as u32;
    let height = (dst.height * density).round().max(1.0) as u32;
    Some((width, height))
}

fn draw_app_background<S: DrawScope + ?Sized>(scope: &mut S, theme: ThemeMode) {
    let size = scope.size();
    let gradient_stops = match theme {
        ThemeMode::Dark => vec![
            Color::from_rgb_u8(8, 14, 28),
            Color::from_rgb_u8(14, 24, 46),
            Color::from_rgb_u8(20, 32, 58),
            Color::from_rgb_u8(10, 18, 32),
        ],
        ThemeMode::Light => vec![
            Color::from_rgb_u8(218, 244, 255),
            Color::from_rgb_u8(236, 251, 255),
            Color::from_rgb_u8(188, 242, 249),
            Color::from_rgb_u8(120, 226, 209),
        ],
    };
    scope.draw_rect(Brush::linear_gradient_range(
        gradient_stops,
        Point::new(0.0, 0.0),
        Point::new(size.width, size.height),
    ));

    if matches!(theme, ThemeMode::Light) {
        if let Some(bitmap) = app_background_bitmap() {
            draw_stretchable_app_background(scope, bitmap, size, app_background_slices());
        }
    } else {
        scope.draw_rect(Brush::radial_gradient(
            vec![Color::from_rgba_u8(46, 86, 148, 70), Color::TRANSPARENT],
            Point::new(size.width * 0.5, size.height * 0.32),
            size.width.max(size.height) * 0.7,
        ));
    }
}

#[allow(non_snake_case)]
#[composable]
fn BottomListGapMask(theme: ThemeMode) {
    ComposeBox(
        Modifier::empty()
            .fill_max_width()
            .height(APP_BOTTOM_LIST_GAP)
            .draw_behind(move |scope| draw_bottom_list_gap_mask(scope, theme)),
        BoxSpec::default(),
        || {},
    );
}

fn draw_bottom_list_gap_mask<S: DrawScope + ?Sized>(scope: &mut S, theme: ThemeMode) {
    let size = scope.size();
    let horizontal_padding = if size.width < 700.0 {
        18.0
    } else if size.width < 1120.0 {
        24.0
    } else {
        34.0
    };
    let y = (size.height - APP_BOTTOM_LIST_GAP).max(0.0);
    let content_width = (size.width - horizontal_padding * 2.0).max(0.0);
    let (gradient_stops, highlight) = match theme {
        ThemeMode::Dark => (
            vec![
                Color::from_rgba_u8(28, 44, 70, 255),
                Color::from_rgba_u8(20, 34, 58, 255),
                Color::from_rgba_u8(14, 24, 44, 255),
            ],
            Color::from_rgba_u8(110, 170, 230, 110),
        ),
        ThemeMode::Light => (
            vec![
                Color::from_rgba_u8(209, 247, 252, 255),
                Color::from_rgba_u8(153, 236, 231, 255),
                Color::from_rgba_u8(71, 218, 218, 255),
            ],
            Color::from_rgba_u8(255, 255, 255, 172),
        ),
    };
    scope.draw_rect_at(
        rect(horizontal_padding, y, content_width, APP_BOTTOM_LIST_GAP),
        Brush::linear_gradient_range(
            gradient_stops,
            Point::new(0.0, y),
            Point::new(size.width, size.height),
        ),
    );
    scope.draw_rect_at(
        rect(horizontal_padding, y, content_width, 2.0),
        Brush::horizontal_gradient(
            vec![Color::TRANSPARENT, highlight, Color::TRANSPARENT],
            0.0,
            size.width,
        ),
    );
}

fn app_background_slices() -> NineSliceInsets {
    NineSliceInsets {
        left: 220.0,
        top: 180.0,
        right: 220.0,
        bottom: 340.0,
    }
}

fn draw_stretchable_app_background<S: DrawScope + ?Sized>(
    scope: &mut S,
    bitmap: ImageBitmap,
    size: Size,
    insets: NineSliceInsets,
) {
    let source_width = bitmap.width() as f32;
    let source_height = bitmap.height() as f32;
    if source_width <= 0.0 || source_height <= 0.0 || size.width <= 0.0 || size.height <= 0.0 {
        return;
    }
    let full_src = rect(0.0, 0.0, source_width, source_height);
    let full_dst = rect(0.0, 0.0, size.width, size.height);

    if size.width <= source_width && size.height <= source_height {
        draw_sharp_image_src(scope, bitmap, full_src, full_dst, 1.0, None);
    } else {
        draw_nine_slice_bitmap(scope, bitmap, size, insets);
    }
}

fn draw_nine_slice_bitmap<S: DrawScope + ?Sized>(
    scope: &mut S,
    bitmap: ImageBitmap,
    size: Size,
    insets: NineSliceInsets,
) {
    let source_width = bitmap.width() as f32;
    let source_height = bitmap.height() as f32;
    if source_width <= 0.0 || source_height <= 0.0 || size.width <= 0.0 || size.height <= 0.0 {
        return;
    }

    let source_left = insets.left.clamp(0.0, source_width);
    let source_right = insets
        .right
        .clamp(0.0, (source_width - source_left).max(0.0));
    let source_top = insets.top.clamp(0.0, source_height);
    let source_bottom = insets
        .bottom
        .clamp(0.0, (source_height - source_top).max(0.0));

    let horizontal_scale = (size.width / source_width).min(1.0);
    let vertical_scale = (size.height / source_height).min(1.0);
    let dest_left = (source_left * horizontal_scale).round();
    let dest_right = (source_right * horizontal_scale).round();
    let dest_top = (source_top * vertical_scale).round();
    let dest_bottom = (source_bottom * vertical_scale).round();

    let src_x = [0.0, source_left, source_width - source_right, source_width];
    let src_y = [
        0.0,
        source_top,
        source_height - source_bottom,
        source_height,
    ];
    let dst_x = [0.0, dest_left, size.width - dest_right, size.width];
    let dst_y = [0.0, dest_top, size.height - dest_bottom, size.height];

    for row in 0..3 {
        for column in 0..3 {
            let src = rect(
                src_x[column],
                src_y[row],
                src_x[column + 1] - src_x[column],
                src_y[row + 1] - src_y[row],
            );
            let dst = rect(
                dst_x[column],
                dst_y[row],
                dst_x[column + 1] - dst_x[column],
                dst_y[row + 1] - dst_y[row],
            );
            if src.width > 0.0 && src.height > 0.0 && dst.width > 0.0 && dst.height > 0.0 {
                draw_sharp_image_src(scope, bitmap.clone(), src, dst, 1.0, None);
            }
        }
    }
}

fn draw_ui_icon<S: DrawScope + ?Sized>(
    scope: &mut S,
    icon: UiIcon,
    dst_rect: Rect,
    theme: ThemeMode,
    alpha: f32,
) {
    let requested_size = dst_rect.width.min(dst_rect.height).round().max(1.0);
    let (bitmap, source_cell) = icon_bitmap_for_size(requested_size);
    if let Some(bitmap) = bitmap {
        let dst_rect = rect(dst_rect.x, dst_rect.y, requested_size, requested_size);
        draw_sharp_image_src(
            scope,
            bitmap,
            icon.src_rect_for_cell(source_cell as f32),
            snap_rect(dst_rect),
            alpha,
            None,
        );
    } else {
        draw_reference_icon(scope, icon, dst_rect, theme, alpha);
    }
}

fn icon_bitmap_for_size(size: f32) -> (Option<ImageBitmap>, u32) {
    if size <= 30.0 {
        (ui_icons_bitmap_24(), 48)
    } else if size <= 50.0 {
        (ui_icons_bitmap_44(), 88)
    } else if size <= 80.0 {
        (ui_icons_bitmap_58(), 116)
    } else {
        (ui_icons_bitmap(), ICON_SHEET_CELL)
    }
}

fn snap_rect(input: Rect) -> Rect {
    rect(
        input.x.round(),
        input.y.round(),
        input.width.round(),
        input.height.round(),
    )
}

fn draw_reference_icon<S: DrawScope + ?Sized>(
    scope: &mut S,
    icon: UiIcon,
    dst_rect: Rect,
    _theme: ThemeMode,
    alpha: f32,
) {
    let color = icon.fallback_color().with_alpha(0.9 * alpha);
    let radius = if icon.is_round_icon() {
        dst_rect.width.min(dst_rect.height) * 0.5
    } else {
        dst_rect.width.min(dst_rect.height) * 0.24
    };
    scope.draw_round_rect(
        Brush::linear_gradient_range(
            vec![lighten_color(color, 0.38), color, darken_color(color, 0.18)],
            Point::new(dst_rect.x, dst_rect.y),
            Point::new(dst_rect.x + dst_rect.width, dst_rect.y + dst_rect.height),
        ),
        CornerRadii::uniform(radius),
    );
    scope.draw_rect_at(
        rect(
            dst_rect.x + dst_rect.width * 0.20,
            dst_rect.y + dst_rect.height * 0.17,
            dst_rect.width * 0.34,
            dst_rect.height * 0.10,
        ),
        Brush::solid(Color::from_rgba_u8(255, 255, 255, 82)),
    );
}

fn icon_overlay_modifier(
    modifier: Modifier,
    icon: UiIcon,
    icon_size: f32,
    x_offset: f32,
    theme: ThemeMode,
    active: bool,
) -> Modifier {
    modifier.draw_with_content(move |scope| {
        scope.draw_content();
        let size = scope.size();
        draw_ui_icon(
            scope,
            icon,
            rect(
                x_offset,
                ((size.height - icon_size) * 0.5).max(0.0),
                icon_size,
                icon_size,
            ),
            theme,
            if active { 1.0 } else { 0.96 },
        );
    })
}

fn button_icon_for_label(label: &str) -> UiIcon {
    match label {
        "Paste" => UiIcon::Paste,
        "Clear" => UiIcon::Clear,
        _ if label.starts_with("Theme:") => UiIcon::Theme,
        _ => UiIcon::Generic,
    }
}

fn panel_brush(theme: ThemeMode, size: Size) -> Brush {
    match theme {
        ThemeMode::Dark => Brush::linear_gradient_range(
            vec![
                Color::from_rgba_u8(36, 52, 80, 215),
                Color::from_rgba_u8(24, 38, 62, 195),
                Color::from_rgba_u8(18, 28, 50, 175),
            ],
            Point::new(0.0, 0.0),
            Point::new(size.width, size.height),
        ),
        ThemeMode::Light => Brush::linear_gradient_range(
            vec![
                Color::from_rgba_u8(255, 255, 255, 242),
                Color::from_rgba_u8(232, 251, 255, 214),
                Color::from_rgba_u8(214, 247, 239, 188),
            ],
            Point::new(0.0, 0.0),
            Point::new(size.width, size.height),
        ),
    }
}

fn shadow_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba_u8(0, 0, 0, 175),
        ThemeMode::Light => Color::from_rgba_u8(44, 147, 184, 86),
    }
}

fn lighten_color(color: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color::rgba(
        color.r() + (1.0 - color.r()) * amount,
        color.g() + (1.0 - color.g()) * amount,
        color.b() + (1.0 - color.b()) * amount,
        color.a(),
    )
}

fn darken_color(color: Color, amount: f32) -> Color {
    let amount = 1.0 - amount.clamp(0.0, 1.0);
    Color::rgba(
        color.r() * amount,
        color.g() * amount,
        color.b() * amount,
        color.a(),
    )
}

fn text_style(
    color: Color,
    size: f32,
    weight: Option<cranpose_ui::text::FontWeight>,
    family: Option<cranpose_ui::text::FontFamily>,
) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: cranpose_ui::text::TextUnit::Sp(size),
            font_weight: weight,
            font_family: family,
            ..SpanStyle::default()
        },
        paragraph_style: ParagraphStyle::default(),
    }
}

fn app_title_style(theme: ThemeMode, compact: bool) -> TextStyle {
    text_style(
        primary_text_color(theme),
        if compact { 28.0 } else { 35.0 },
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn panel_title_style(theme: ThemeMode) -> TextStyle {
    text_style(
        primary_text_color(theme),
        18.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn dot_style(theme: ThemeMode) -> TextStyle {
    text_style(
        button_text_color(theme),
        8.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn heading_style(size: f32, theme: ThemeMode) -> TextStyle {
    text_style(
        primary_text_color(theme),
        size,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn muted_style(theme: ThemeMode) -> TextStyle {
    text_style(muted_text_color(theme), 14.0, None, None)
}

fn body_style(theme: ThemeMode) -> TextStyle {
    text_style(body_text_color(theme), 16.0, None, None)
}

fn eyebrow_style(theme: ThemeMode) -> TextStyle {
    text_style(
        accent_color(theme),
        15.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn stage_label_style(theme: ThemeMode) -> TextStyle {
    text_style(
        muted_text_color(theme),
        16.0,
        Some(cranpose_ui::text::FontWeight::SEMI_BOLD),
        None,
    )
}

fn accent_style(theme: ThemeMode) -> TextStyle {
    text_style(accent_color(theme), 17.0, None, None)
}

fn code_text_style(size: f32, theme: ThemeMode) -> TextStyle {
    text_style(
        primary_text_color(theme),
        size,
        None,
        Some(cranpose_ui::text::FontFamily::Monospace),
    )
}

fn field_text_style(theme: ThemeMode) -> TextStyle {
    text_style(primary_text_color(theme), 14.0, None, None)
}

fn code_field_style(theme: ThemeMode) -> TextStyle {
    code_text_style(14.0, theme)
}

fn label_style(theme: ThemeMode, is_changed: bool) -> TextStyle {
    text_style(
        if is_changed {
            changed_label_color(theme)
        } else {
            label_color(theme)
        },
        11.0,
        Some(cranpose_ui::text::FontWeight::SEMI_BOLD),
        None,
    )
}

fn button_text_style(theme: ThemeMode) -> TextStyle {
    text_style(
        button_text_color(theme),
        10.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn focus_button_text_style(theme: ThemeMode) -> TextStyle {
    text_style(
        button_text_color(theme).with_alpha(0.82),
        15.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn busy_button_text_style(theme: ThemeMode) -> TextStyle {
    text_style(
        button_text_color(theme).with_alpha(0.72),
        10.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn disabled_button_text_style(theme: ThemeMode) -> TextStyle {
    text_style(
        muted_text_color(theme),
        10.0,
        Some(cranpose_ui::text::FontWeight::SEMI_BOLD),
        None,
    )
}

fn subtle_button_text_style(theme: ThemeMode) -> TextStyle {
    text_style(
        label_color(theme),
        12.0,
        Some(cranpose_ui::text::FontWeight::SEMI_BOLD),
        None,
    )
}

fn queue_text_style(theme: ThemeMode) -> TextStyle {
    text_style(
        primary_text_color(theme),
        12.0,
        Some(cranpose_ui::text::FontWeight::SEMI_BOLD),
        None,
    )
}

fn queue_current_label_style(theme: ThemeMode) -> TextStyle {
    text_style(
        accent_color(theme).with_alpha(0.86),
        10.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn interactive_queue_text_style(
    theme: ThemeMode,
    done: bool,
    disabled: bool,
    busy: bool,
) -> TextStyle {
    text_style(
        if disabled {
            muted_text_color(theme)
        } else if done || busy {
            button_text_color(theme).with_alpha(0.82)
        } else {
            primary_text_color(theme)
        },
        12.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn badge_text_style(theme: ThemeMode) -> TextStyle {
    text_style(
        badge_text_color(theme),
        11.0,
        Some(cranpose_ui::text::FontWeight::BOLD),
        None,
    )
}

fn panel_surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba_u8(223, 245, 255, 190),
        ThemeMode::Light => Color::from_rgba_u8(238, 252, 255, 205),
    }
}

fn input_surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba_u8(28, 42, 66, 220),
        ThemeMode::Light => Color::from_rgba_u8(255, 255, 255, 226),
    }
}

fn button_surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(98, 220, 130),
        ThemeMode::Light => Color::from_rgb_u8(39, 145, 224),
    }
}

fn disabled_button_surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba_u8(58, 76, 100, 200),
        ThemeMode::Light => Color::from_rgba_u8(213, 230, 236, 170),
    }
}

fn interactive_queue_surface(
    theme: ThemeMode,
    done: bool,
    disabled: bool,
    busy: bool,
    invokes_button: bool,
) -> Color {
    if busy {
        return button_surface(theme).with_alpha(0.66);
    }
    if disabled {
        return disabled_button_surface(theme);
    }
    if invokes_button && !done {
        return button_surface(theme);
    }
    if done {
        return match theme {
            ThemeMode::Dark => Color::from_rgb_u8(70, 178, 100),
            ThemeMode::Light => Color::from_rgb_u8(77, 184, 91),
        };
    }
    match theme {
        ThemeMode::Dark => Color::from_rgba_u8(38, 56, 82, 215),
        ThemeMode::Light => Color::from_rgba_u8(255, 255, 255, 218),
    }
}

fn badge_surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba_u8(48, 70, 102, 225),
        ThemeMode::Light => Color::from_rgba_u8(226, 245, 252, 230),
    }
}

fn soft_button_surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba_u8(38, 58, 88, 215),
        ThemeMode::Light => Color::from_rgba_u8(237, 250, 255, 185),
    }
}

fn primary_text_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(228, 240, 252),
        ThemeMode::Light => Color::from_rgb_u8(14, 58, 96),
    }
}

fn body_text_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(202, 218, 238),
        ThemeMode::Light => Color::from_rgb_u8(52, 84, 107),
    }
}

fn muted_text_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(150, 175, 205),
        ThemeMode::Light => Color::from_rgb_u8(60, 96, 128),
    }
}

fn label_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(170, 205, 240),
        ThemeMode::Light => Color::from_rgb_u8(18, 87, 145),
    }
}

fn changed_label_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(98, 224, 224),
        ThemeMode::Light => Color::from_rgb_u8(9, 131, 154),
    }
}

fn accent_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(94, 198, 255),
        ThemeMode::Light => Color::from_rgb_u8(13, 117, 181),
    }
}

fn button_text_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(8, 36, 22),
        ThemeMode::Light => Color::from_rgb_u8(9, 58, 103),
    }
}

fn badge_text_color(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb_u8(220, 235, 250),
        ThemeMode::Light => Color::from_rgb_u8(18, 81, 116),
    }
}

fn scroll_workspace_text_into_view_between(
    robot: &cranpose::Robot,
    text: &str,
    min_center_y: f32,
    max_center_y: f32,
    max_attempts: usize,
) {
    assert!(
        min_center_y < max_center_y,
        "invalid target center range: {min_center_y}..{max_center_y}"
    );
    for attempt in 0..max_attempts {
        if let Some(bounds) = workspace_text_bounds_exact(robot, text) {
            let center_y = bounds.center_y();
            println!("seek {attempt}: target_center_y={center_y:.2}");
            if center_y >= min_center_y - 0.5 && center_y <= max_center_y + 0.5 {
                return;
            }
        }
        let (anchor_x, anchor_y) = workspace_scroll_anchor(robot);
        robot
            .mouse_move(anchor_x, anchor_y)
            .expect("move cursor to workspace");
        std::thread::sleep(Duration::from_millis(30));
        let scroll_delta_y = workspace_text_bounds_exact(robot, text)
            .map(|bounds| {
                let center_y = bounds.center_y();
                if center_y < min_center_y {
                    (min_center_y - center_y).clamp(4.0, WORKSPACE_SEEK_MAX_SCROLL_DELTA_Y)
                } else if center_y > max_center_y {
                    -(center_y - max_center_y).clamp(4.0, WORKSPACE_SEEK_MAX_SCROLL_DELTA_Y)
                } else {
                    0.0
                }
            })
            .unwrap_or(-WORKSPACE_SEEK_MAX_SCROLL_DELTA_Y);
        robot
            .mouse_scroll(0.0, scroll_delta_y)
            .expect("scroll workspace to find text");
        std::thread::sleep(Duration::from_millis(200));
        let _ = robot.wait_for_idle();
    }
    panic!("text '{text}' not found after {max_attempts} workspace scroll attempts");
}

fn scroll_workspace_text_into_stable_band(
    robot: &cranpose::Robot,
    text: &str,
    min_stable_ratio: f32,
    max_stable_ratio: f32,
    max_attempts: usize,
) {
    assert!(
        min_stable_ratio < max_stable_ratio,
        "invalid stable target ratio range: {min_stable_ratio}..{max_stable_ratio}"
    );
    let stable_band = stable_workspace_content_bounds(robot);
    scroll_workspace_text_into_view_between(
        robot,
        text,
        stable_band.y + stable_band.height * min_stable_ratio,
        stable_band.y + stable_band.height * max_stable_ratio,
        max_attempts,
    );
}

fn scroll_workspace_related_button_into_stable_band(
    robot: &cranpose::Robot,
    anchor_text: &str,
    button_label: &str,
    min_stable_ratio: f32,
    max_stable_ratio: f32,
    max_attempts: usize,
) {
    assert!(
        min_stable_ratio < max_stable_ratio,
        "invalid stable target ratio range: {min_stable_ratio}..{max_stable_ratio}"
    );
    for attempt in 0..max_attempts {
        let stable_band = stable_workspace_content_bounds(robot);
        let min_center_y = stable_band.y + stable_band.height * min_stable_ratio;
        let max_center_y = stable_band.y + stable_band.height * max_stable_ratio;
        if let Some(bounds) =
            workspace_related_button_bounds_after_anchor(robot, anchor_text, button_label)
        {
            let center_y = bounds.center_y();
            println!(
                "button_seek {attempt}: anchor={anchor_text:?} button={button_label:?} center_y={center_y:.2}"
            );
            if center_y >= min_center_y - 0.5
                && center_y <= max_center_y + 0.5
                && stable_band.inset_contains(bounds, BUTTON_QUALITY_SAFE_EDGE)
            {
                return;
            }

            let stable_bottom = stable_band.y + stable_band.height;
            let scroll_delta_y = if center_y < min_center_y {
                (min_center_y - center_y).clamp(4.0, 80.0)
            } else if center_y > max_center_y {
                -(center_y - max_center_y).clamp(4.0, 80.0)
            } else if bounds.y < stable_band.y + BUTTON_QUALITY_SAFE_EDGE {
                (stable_band.y + BUTTON_QUALITY_SAFE_EDGE - bounds.y).clamp(4.0, 80.0)
            } else if bounds.y + bounds.height > stable_bottom - BUTTON_QUALITY_SAFE_EDGE {
                -((bounds.y + bounds.height) - (stable_bottom - BUTTON_QUALITY_SAFE_EDGE))
                    .clamp(4.0, 80.0)
            } else {
                0.0
            };
            let (anchor_x, anchor_y) = workspace_scroll_anchor(robot);
            robot
                .mouse_move(anchor_x, anchor_y)
                .expect("move cursor to workspace");
            std::thread::sleep(Duration::from_millis(30));
            robot
                .mouse_scroll(0.0, scroll_delta_y)
                .expect("scroll workspace to place related button");
            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();
            continue;
        }

        scroll_workspace_text_into_stable_band(robot, anchor_text, 0.40, 0.62, 1);
    }

    panic!(
        "button '{button_label}' related to '{anchor_text}' not found in the stable workspace band after {max_attempts} scroll attempts"
    );
}

fn workspace_scroll_anchor(robot: &cranpose::Robot) -> (f32, f32) {
    let viewport = stable_workspace_content_bounds(robot);
    (
        viewport.x + viewport.width * 0.5,
        viewport.y + viewport.height * 0.5,
    )
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Bounds {
    fn center_y(self) -> f32 {
        self.y + self.height * 0.5
    }

    fn inset_contains(self, other: Self, inset: f32) -> bool {
        other.x >= self.x + inset
            && other.y >= self.y + inset
            && other.x + other.width <= self.x + self.width - inset
            && other.y + other.height <= self.y + self.height - inset
    }

    fn overlaps_x(self, other: Self) -> bool {
        other.x + other.width > self.x && other.x < self.x + self.width
    }

    fn padded(self, padding: f32) -> Self {
        Self {
            x: self.x - padding,
            y: self.y - padding,
            width: self.width + padding * 2.0,
            height: self.height + padding * 2.0,
        }
    }

    fn clipped_to(self, limit: Self) -> Option<Self> {
        let left = self.x.max(limit.x);
        let top = self.y.max(limit.y);
        let right = (self.x + self.width).min(limit.x + limit.width);
        let bottom = (self.y + self.height).min(limit.y + limit.height);
        if right <= left || bottom <= top {
            return None;
        }
        Some(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }

    fn tuple(self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.width, self.height)
    }
}

#[derive(Debug)]
struct ButtonQualitySample {
    label: &'static str,
    phase: String,
    bounds: Bounds,
    edge_energy: f32,
    unique_rgb_count: usize,
    crop_path: Option<PathBuf>,
}

#[derive(Debug)]
struct ButtonReferenceSample {
    tag: &'static str,
    bounds: Bounds,
    edge_energy: f32,
    red_edge_energy: f32,
    unique_rgb_count: usize,
    red_profile: RedIconHorizontalProfile,
    crop: cranpose::RobotScreenshot,
    crop_path: Option<PathBuf>,
}

#[derive(Debug)]
struct RowMicroStabilitySample {
    label_bounds: Bounds,
    clear_bounds: Bounds,
    region: Bounds,
    clear_icon_bounds: PixelFeatureBounds,
    field_icon_bounds: PixelFeatureBounds,
    screenshot: cranpose::RobotScreenshot,
}

#[derive(Debug)]
struct WorkspaceStripStabilitySample {
    target_bounds: Bounds,
    region: Bounds,
    screenshot: cranpose::RobotScreenshot,
}

struct WorkspaceRigidStabilitySample {
    target_bounds: Bounds,
    region: Bounds,
    visual_anchors: Vec<WorkspaceVisualAnchor>,
    screenshot: cranpose::RobotScreenshot,
}

#[derive(Clone, Debug)]
struct WorkspaceVisualAnchor {
    label: &'static str,
    region: Bounds,
    bounds: PixelFeatureBounds,
    edge_energy: f32,
    unique_rgb_count: usize,
}

struct WorkspaceTopBandStabilitySample {
    target_bounds: Bounds,
    region: Bounds,
    screenshot: cranpose::RobotScreenshot,
}

#[derive(Clone, Copy, Debug)]
struct ScreenshotColorHealth {
    total_pixels: usize,
    colorful_pixels: usize,
    blue_cyan_pixels: usize,
    dark_pixels: usize,
    unique_rgb_count: usize,
}

impl ScreenshotColorHealth {
    fn colorful_ratio(self) -> f32 {
        self.colorful_pixels as f32 / self.total_pixels.max(1) as f32
    }

    fn blue_cyan_ratio(self) -> f32 {
        self.blue_cyan_pixels as f32 / self.total_pixels.max(1) as f32
    }

    fn dark_ratio(self) -> f32 {
        self.dark_pixels as f32 / self.total_pixels.max(1) as f32
    }
}

#[derive(Debug)]
struct BottomClearStabilitySample {
    label_bounds: Bounds,
    clear_bounds: Bounds,
    fixed_region: Bounds,
    screenshot: cranpose::RobotScreenshot,
    fixed_crop: cranpose::RobotScreenshot,
    clear_crop: cranpose::RobotScreenshot,
    red_profile: RedIconHorizontalProfile,
    clear_edge_energy: f32,
    clear_unique_rgb_count: usize,
}

#[derive(Clone, Debug)]
struct RedIconHorizontalProfile {
    bounds: PixelFeatureBounds,
    column_mass: Vec<u32>,
    total_mass: u64,
    centroid_x: f32,
    max_column_mass: u32,
}

#[derive(Clone, Copy, Debug)]
struct PixelFeatureBounds {
    min_x: u32,
    max_x: u32,
    min_y: u32,
    max_y: u32,
    count: usize,
    sum_x: u64,
    sum_y: u64,
}

impl PixelFeatureBounds {
    fn width(self) -> u32 {
        self.max_x - self.min_x + 1
    }

    fn height(self) -> u32 {
        self.max_y - self.min_y + 1
    }

    fn center_x(self) -> f32 {
        (self.min_x + self.max_x) as f32 * 0.5
    }

    fn center_y(self) -> f32 {
        (self.min_y + self.max_y) as f32 * 0.5
    }

    fn centroid_x(self) -> f32 {
        self.sum_x as f32 / self.count.max(1) as f32
    }

    fn centroid_y(self) -> f32 {
        self.sum_y as f32 / self.count.max(1) as f32
    }
}

fn assert_deep_button_quality_matches_top(robot: &cranpose::Robot) {
    scroll_workspace_related_button_into_stable_band(
        robot,
        "Problem TLDR",
        "Paste",
        BUTTON_QUALITY_TARGET_MIN_STABLE_RATIO,
        BUTTON_QUALITY_TARGET_MAX_STABLE_RATIO,
        80,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let top_paste = capture_workspace_button_quality(robot, "Problem TLDR", "Paste", "top_paste");

    scroll_workspace_related_button_into_stable_band(
        robot,
        "Rust Code",
        "Paste",
        BUTTON_QUALITY_TARGET_MIN_STABLE_RATIO,
        BUTTON_QUALITY_TARGET_MAX_STABLE_RATIO,
        80,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let deep_paste =
        capture_workspace_button_quality(robot, "Rust Code", "Paste", "rust_code_paste");
    assert_button_quality_matches_baseline(&top_paste, &deep_paste);
}

fn assert_rust_runtime_row_stable_under_micro_scroll(robot: &cranpose::Robot) {
    scroll_workspace_text_into_stable_band(
        robot,
        "Rust Runtime (ms)",
        ROW_MICRO_TARGET_MIN_STABLE_RATIO,
        ROW_MICRO_TARGET_MAX_STABLE_RATIO,
        80,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let mut previous = capture_rust_runtime_row_micro_sample(robot);
    for step in 1..=ROW_MICRO_STABILITY_STEPS {
        let (anchor_x, anchor_y) = workspace_scroll_anchor(robot);
        robot
            .mouse_move(anchor_x, anchor_y)
            .expect("move cursor to workspace for row micro stability");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll(0.0, ROW_MICRO_SCROLL_DELTA_Y)
            .expect("row micro stability scroll should succeed");
        std::thread::sleep(Duration::from_millis(120));
        let _ = robot.wait_for_idle();

        let current = capture_rust_runtime_row_micro_sample(robot);
        log_row_micro_render_stats(robot, step);
        let actual_delta_y = current.label_bounds.y - previous.label_bounds.y;
        println!(
            "row_micro_geometry step={step} label_delta_y={actual_delta_y:.3} region=({:.1},{:.1},{:.1},{:.1}) clear=({:.1},{:.1},{:.1},{:.1})",
            current.region.x,
            current.region.y,
            current.region.width,
            current.region.height,
            current.clear_bounds.x,
            current.clear_bounds.y,
            current.clear_bounds.width,
            current.clear_bounds.height,
        );
        if (actual_delta_y - ROW_MICRO_SCROLL_DELTA_Y).abs() > ROW_MICRO_DELTA_EPSILON {
            fail_with_robot(
                robot,
                &format!(
                    "Rust Runtime row did not move by exact micro scroll at step {step}: expected {:.3}, got {actual_delta_y:.3}",
                    ROW_MICRO_SCROLL_DELTA_Y
                ),
            );
        }
        assert_row_horizontal_geometry_stable(robot, step, &previous, &current);
        assert_row_icon_features_stable(robot, step, &previous, &current);
        assert_row_micro_picture_stable(robot, step, &previous, &current, actual_delta_y);
        previous = current;
    }
}

fn assert_workspace_strip_stable_under_micro_scroll(robot: &cranpose::Robot, target_text: &str) {
    let strip_steps = env_usize(STRIP_STEPS_ENV).unwrap_or(WORKSPACE_STRIP_STABILITY_STEPS);
    let scroll_delta_y = env_f32(STRIP_SCROLL_DELTA_ENV).unwrap_or(WORKSPACE_STRIP_SCROLL_DELTA_Y);
    scroll_workspace_text_into_stable_band(
        robot,
        target_text,
        WORKSPACE_STRIP_TARGET_MIN_STABLE_RATIO,
        WORKSPACE_STRIP_TARGET_MAX_STABLE_RATIO,
        120,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let mut previous = capture_workspace_strip_stability_sample(robot, target_text);
    for step in 1..=strip_steps {
        let (anchor_x, anchor_y) = workspace_scroll_anchor(robot);
        robot
            .mouse_move(anchor_x, anchor_y)
            .expect("move cursor to workspace for strip stability");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll(0.0, scroll_delta_y)
            .expect("workspace strip stability scroll should succeed");

        let active_screenshot = capture_first_moved_workspace_strip_screenshot(
            robot,
            target_text,
            step,
            &previous,
            scroll_delta_y,
        );
        robot
            .pump_frames(WORKSPACE_STRIP_ACTIVE_SETTLE_FRAMES)
            .expect("workspace strip settle frame pump should succeed");
        let current = capture_workspace_strip_stability_sample(robot, target_text);
        let active = WorkspaceStripStabilitySample {
            target_bounds: current.target_bounds,
            region: previous.region,
            screenshot: active_screenshot,
        };
        log_workspace_strip_render_stats(robot, step);
        let actual_delta_y = current.target_bounds.y - previous.target_bounds.y;
        println!(
            "workspace_strip_geometry step={step} target_delta_y={actual_delta_y:.3} region=({:.1},{:.1},{:.1},{:.1})",
            current.region.x, current.region.y, current.region.width, current.region.height,
        );
        if (actual_delta_y - scroll_delta_y).abs() > WORKSPACE_STRIP_DELTA_EPSILON {
            save_workspace_strip_failure(
                step,
                &previous.screenshot,
                &current.screenshot,
                previous.region,
                actual_delta_y,
            );
            fail_with_robot(
                robot,
            &format!(
                "{target_text} workspace strip did not move by exact micro scroll at step {step}: expected {:.3}, got {actual_delta_y:.3}",
                scroll_delta_y,
            ),
        );
        }
        assert_workspace_strip_horizontal_stable(
            robot,
            target_text,
            step,
            &previous,
            &active,
            actual_delta_y,
        );
        assert_workspace_strip_active_frame_matches_settled(
            robot,
            target_text,
            step,
            &active,
            &current,
        );
        previous = current;
    }
}

fn capture_workspace_active_viewport_sample(
    robot: &cranpose::Robot,
    target_text: &str,
) -> WorkspaceStripStabilitySample {
    let target_bounds = workspace_text_bounds_exact(robot, target_text)
        .unwrap_or_else(|| fail_with_robot(robot, &format!("{target_text} label not found")));
    let stable_band = stable_workspace_content_bounds(robot);
    if !stable_band.overlaps_x(target_bounds)
        || !stable_band.inset_contains(target_bounds, WORKSPACE_STRIP_REGION_INSET_Y)
    {
        fail_with_robot(
            robot,
            &format!(
                "{target_text} label is outside the active viewport stable band: bounds=({:.1},{:.1},{:.1},{:.1}) viewport=({:.1},{:.1},{:.1},{:.1})",
                target_bounds.x,
                target_bounds.y,
                target_bounds.width,
                target_bounds.height,
                stable_band.x,
                stable_band.y,
                stable_band.width,
                stable_band.height
            ),
        );
    }

    let region = Bounds {
        x: stable_band.x + WORKSPACE_STRIP_REGION_INSET_X,
        y: stable_band.y + WORKSPACE_STRIP_REGION_INSET_Y,
        width: (stable_band.width - WORKSPACE_STRIP_REGION_INSET_X * 2.0).max(0.0),
        height: (stable_band.height - WORKSPACE_STRIP_REGION_INSET_Y * 2.0).max(0.0),
    };
    let screenshot = capture_visible_window_screenshot("workspace-active-viewport");

    WorkspaceStripStabilitySample {
        target_bounds,
        region,
        screenshot,
    }
}

fn assert_workspace_active_viewport_first_frame_matches_settled(
    robot: &cranpose::Robot,
    target_text: &str,
) {
    let steps =
        env_usize(ACTIVE_VIEWPORT_STEPS_ENV).unwrap_or(WORKSPACE_ACTIVE_VIEWPORT_STABILITY_STEPS);
    let scroll_delta_y = env_f32(ACTIVE_VIEWPORT_SCROLL_DELTA_ENV)
        .unwrap_or(WORKSPACE_ACTIVE_VIEWPORT_SCROLL_DELTA_Y);
    scroll_workspace_text_into_stable_band(
        robot,
        target_text,
        WORKSPACE_STRIP_TARGET_MIN_STABLE_RATIO,
        WORKSPACE_STRIP_TARGET_MAX_STABLE_RATIO,
        120,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let mut previous = capture_workspace_active_viewport_sample(robot, target_text);
    for step in 1..=steps {
        let (anchor_x, anchor_y) = workspace_scroll_anchor(robot);
        robot
            .mouse_move(anchor_x, anchor_y)
            .expect("move cursor to workspace for active viewport stability");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll(0.0, scroll_delta_y)
            .expect("workspace active viewport micro scroll should succeed");

        robot
            .pump_frames(WORKSPACE_ACTIVE_VIEWPORT_ACTIVE_PUMP_FRAMES)
            .expect("workspace active viewport first frame pump should succeed");
        let active_screenshot =
            capture_visible_window_screenshot("workspace-active-viewport-first");
        robot
            .pump_frames(WORKSPACE_STRIP_ACTIVE_SETTLE_FRAMES)
            .expect("workspace active viewport settle frame pump should succeed");
        let settled = capture_workspace_active_viewport_sample(robot, target_text);
        let actual_delta_y = settled.target_bounds.y - previous.target_bounds.y;
        println!(
            "workspace_active_viewport_geometry step={step} target={target_text} target_delta_y={actual_delta_y:.3} region=({:.1},{:.1},{:.1},{:.1})",
            settled.region.x, settled.region.y, settled.region.width, settled.region.height,
        );
        if (actual_delta_y - scroll_delta_y).abs() > WORKSPACE_STRIP_DELTA_EPSILON {
            save_workspace_strip_failure(
                step,
                &previous.screenshot,
                &settled.screenshot,
                previous.region,
                actual_delta_y,
            );
            fail_with_robot(
                robot,
                &format!(
                    "{target_text} active viewport did not move by exact micro scroll at step {step}: expected {:.3}, got {actual_delta_y:.3}",
                    scroll_delta_y,
                ),
            );
        }
        assert_workspace_active_viewport_first_frame_moved(
            robot,
            target_text,
            step,
            &previous,
            &active_screenshot,
            scroll_delta_y,
        );
        let active = WorkspaceStripStabilitySample {
            target_bounds: settled.target_bounds,
            region: previous.region,
            screenshot: active_screenshot,
        };
        assert_workspace_active_viewport_frame_matches_settled(
            robot,
            target_text,
            step,
            &active,
            &settled,
        );
        previous = settled;
    }
}

fn assert_workspace_active_viewport_first_frame_moved(
    robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    previous: &WorkspaceStripStabilitySample,
    active_screenshot: &cranpose::RobotScreenshot,
    expected_delta_y: f32,
) {
    let previous_crop = normalize_workspace_region(&previous.screenshot, previous.region);
    let followed_region = Bounds {
        y: previous.region.y + expected_delta_y,
        ..previous.region
    };
    let unmoved_crop = normalize_workspace_region(active_screenshot, previous.region);
    let followed_crop = normalize_workspace_region(active_screenshot, followed_region);
    let unmoved_score = screenshot_abs_difference_score(&previous_crop, &unmoved_crop)
        .expect("active viewport unmoved crop should match size");
    let followed_score = screenshot_abs_difference_score(&previous_crop, &followed_crop)
        .expect("active viewport followed crop should match size");
    let improvement_ratio = if unmoved_score == 0 {
        0.0
    } else {
        unmoved_score.saturating_sub(followed_score) as f32 / unmoved_score as f32
    };
    println!(
        "workspace_active_viewport_first_frame step={step} target={target_text} unmoved_score={unmoved_score} followed_score={followed_score} improvement_ratio={improvement_ratio:.4}"
    );
    if followed_score >= unmoved_score {
        save_workspace_strip_failure(
            step,
            &previous.screenshot,
            active_screenshot,
            previous.region,
            expected_delta_y,
        );
        fail_with_robot(
            robot,
            &format!(
                "{target_text} active viewport first frame was stale at step {step}: unmoved_score={unmoved_score} followed_score={followed_score}"
            ),
        );
    }
}

fn assert_workspace_active_viewport_frame_matches_settled(
    robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    active: &WorkspaceStripStabilitySample,
    settled: &WorkspaceStripStabilitySample,
) {
    let target_dx = settled.target_bounds.x - active.target_bounds.x;
    let target_dy = settled.target_bounds.y - active.target_bounds.y;
    if target_dx.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
        || target_dy.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
    {
        save_workspace_strip_failure(
            step,
            &active.screenshot,
            &settled.screenshot,
            active.region,
            target_dy,
        );
        fail_with_robot(
            robot,
            &format!(
                "{target_text} active viewport frame and settled frame do not describe the same scroll position at step {step}: dx={target_dx:.3} dy={target_dy:.3}"
            ),
        );
    }

    let active_crop = normalize_workspace_region(&active.screenshot, active.region);
    let settled_crop = normalize_workspace_region(&settled.screenshot, active.region);
    let stats = screenshot_difference_stats(
        &active_crop,
        &settled_crop,
        WORKSPACE_ACTIVE_VIEWPORT_DIFF_TOLERANCE,
    )
    .expect("workspace active viewport and settled crops should match size");
    let pixel_count = (active_crop.width as usize).saturating_mul(active_crop.height as usize);
    let differing_ratio = stats.differing_pixels as f32 / pixel_count.max(1) as f32;
    let active_edge = screenshot_edge_energy(&active_crop);
    let settled_edge = screenshot_edge_energy(&settled_crop);
    let min_edge = settled_edge * (1.0 - WORKSPACE_ACTIVE_VIEWPORT_MAX_EDGE_LOSS_RATIO);
    let active_unique = screenshot_unique_rgb_count(&active_crop);
    let settled_unique = screenshot_unique_rgb_count(&settled_crop);
    let max_unique = settled_unique + WORKSPACE_ACTIVE_VIEWPORT_MAX_EXTRA_COLORS;
    println!(
        "workspace_active_viewport_settled step={step} target={target_text} differing_pixels={} differing_ratio={differing_ratio:.5} max_diff={} active_edge={active_edge:.3} settled_edge={settled_edge:.3} min_edge={min_edge:.3} active_unique={active_unique} settled_unique={settled_unique} max_unique={max_unique}",
        stats.differing_pixels,
        stats.max_difference,
    );

    let changed_too_much = differing_ratio > WORKSPACE_ACTIVE_VIEWPORT_MAX_DIFFERING_RATIO;
    let hard_pixel_delta = stats.max_difference > WORKSPACE_ACTIVE_VIEWPORT_MAX_PIXEL_DIFF
        && stats.differing_pixels > pixel_count / 1_000;
    let blurrier_than_settled = active_edge < min_edge || active_unique > max_unique;
    if changed_too_much || hard_pixel_delta || blurrier_than_settled {
        save_workspace_strip_failure(
            step,
            &active.screenshot,
            &settled.screenshot,
            active.region,
            0.0,
        );
        let first = stats
            .first_difference
            .as_ref()
            .expect("failing workspace active viewport diff should have first difference");
        fail_with_robot(
            robot,
            &format!(
                "{target_text} first active viewport frame differs from its settled frame at step {step}: differing_pixels={} differing_ratio={differing_ratio:.5} max_diff={} first_diff=({}, {}) before={:?} after={:?} active_edge={active_edge:.3} settled_edge={settled_edge:.3} min_edge={min_edge:.3} active_unique={active_unique} settled_unique={settled_unique} max_unique={max_unique}",
                stats.differing_pixels,
                stats.max_difference,
                first.x,
                first.y,
                first.before,
                first.after
            ),
        );
    }
}

fn assert_workspace_rigid_picture_stable_during_active_scroll(
    robot: &cranpose::Robot,
    target_text: &str,
) {
    let steps = env_usize(RIGID_STEPS_ENV).unwrap_or(WORKSPACE_RIGID_STABILITY_STEPS);
    scroll_workspace_text_into_stable_band(
        robot,
        target_text,
        WORKSPACE_RIGID_TARGET_MIN_STABLE_RATIO,
        WORKSPACE_RIGID_TARGET_MAX_STABLE_RATIO,
        120,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let mut previous = capture_workspace_rigid_stability_sample(robot, target_text);
    for step in 1..=steps {
        let (anchor_x, anchor_y) = workspace_scroll_anchor(robot);
        robot
            .mouse_move(anchor_x, anchor_y)
            .expect("move cursor to workspace for rigid active-scroll stability");
        robot
            .mouse_scroll(0.0, WORKSPACE_RIGID_SCROLL_DELTA_Y)
            .expect("workspace rigid active-scroll should succeed");
        robot
            .pump_frames(WORKSPACE_RIGID_PUMP_FRAMES_AFTER_SCROLL)
            .expect("workspace rigid active-scroll frame pump should succeed");

        let current = capture_workspace_rigid_stability_sample(robot, target_text);
        log_top_isolated_layer_render_stats(robot, "workspace_rigid", step);
        let actual_delta_y = current.target_bounds.y - previous.target_bounds.y;
        println!(
            "workspace_rigid_geometry step={step} target={target_text} target_delta_y={actual_delta_y:.3} region=({:.1},{:.1},{:.1},{:.1})",
            current.region.x, current.region.y, current.region.width, current.region.height,
        );
        if (actual_delta_y - WORKSPACE_RIGID_SCROLL_DELTA_Y).abs() > WORKSPACE_RIGID_DELTA_EPSILON {
            save_workspace_strip_failure(
                step,
                &previous.screenshot,
                &current.screenshot,
                previous.region,
                actual_delta_y,
            );
            fail_with_robot(
                robot,
                &format!(
                    "{target_text} workspace rigid active-scroll did not move by exact micro scroll at step {step}: expected {:.3}, got {actual_delta_y:.3}",
                    WORKSPACE_RIGID_SCROLL_DELTA_Y,
                ),
            );
        }
        assert_workspace_visual_anchors_stable(robot, target_text, step, &previous, &current);
        assert_workspace_rigid_picture_stable(
            robot,
            target_text,
            step,
            &previous,
            &current,
            actual_delta_y,
        );
        robot
            .pump_frames(WORKSPACE_ACTIVE_CRISPNESS_SETTLE_FRAMES)
            .expect("workspace active-scroll settle frame pump should succeed");
        let settled = capture_workspace_rigid_stability_sample(robot, target_text);
        assert_workspace_active_frame_crispness_matches_settled(
            robot,
            target_text,
            step,
            &current,
            &settled,
        );
        previous = settled;
    }
    let _ = robot.wait_for_idle();
}

fn assert_workspace_top_band_stable_under_micro_scroll(robot: &cranpose::Robot, target_text: &str) {
    let top_band_steps =
        env_usize(TOP_BAND_STEPS_ENV).unwrap_or(WORKSPACE_TOP_BAND_STABILITY_STEPS);
    scroll_workspace_text_into_stable_band(
        robot,
        target_text,
        WORKSPACE_STRIP_TARGET_MIN_STABLE_RATIO,
        WORKSPACE_STRIP_TARGET_MAX_STABLE_RATIO,
        120,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let mut previous = capture_workspace_top_band_stability_sample(robot, target_text);
    save_workspace_top_band_sample_if_requested(0, &previous);
    for step in 1..=top_band_steps {
        let (anchor_x, anchor_y) = workspace_scroll_anchor(robot);
        robot
            .mouse_move(anchor_x, anchor_y)
            .expect("move cursor to workspace for top-band stability");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll(0.0, WORKSPACE_TOP_BAND_SCROLL_DELTA_Y)
            .expect("workspace top-band micro scroll should succeed");
        std::thread::sleep(Duration::from_millis(120));
        let _ = robot.wait_for_idle();

        let current = capture_workspace_top_band_stability_sample(robot, target_text);
        save_workspace_top_band_sample_if_requested(step, &current);
        log_top_isolated_layer_render_stats(robot, "workspace_top_band", step);
        let actual_delta_y = current.target_bounds.y - previous.target_bounds.y;
        println!(
            "workspace_top_band_geometry step={step} target_delta_y={actual_delta_y:.3} region=({:.1},{:.1},{:.1},{:.1})",
            current.region.x, current.region.y, current.region.width, current.region.height,
        );
        if (actual_delta_y - WORKSPACE_TOP_BAND_SCROLL_DELTA_Y).abs()
            > WORKSPACE_TOP_BAND_DELTA_EPSILON
        {
            save_workspace_top_band_failure(
                step,
                &previous.screenshot,
                &current.screenshot,
                previous.region,
                actual_delta_y,
            );
            fail_with_robot(
                robot,
                &format!(
                    "{target_text} workspace top band did not move by exact micro scroll at step {step}: expected {:.3}, got {actual_delta_y:.3}",
                    WORKSPACE_TOP_BAND_SCROLL_DELTA_Y,
                ),
            );
        }
        assert_workspace_top_band_horizontal_stable(
            robot,
            target_text,
            step,
            &previous,
            &current,
            actual_delta_y,
        );
        previous = current;
    }
}

fn assert_bottom_clear_button_stable_under_one_px_scroll(robot: &cranpose::Robot) {
    scroll_workspace_text_into_stable_band(
        robot,
        "Rust Runtime (ms)",
        BOTTOM_CLEAR_TARGET_MIN_STABLE_RATIO,
        BOTTOM_CLEAR_TARGET_MAX_STABLE_RATIO,
        120,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let mut previous = capture_bottom_clear_stability_sample(robot);
    save_bottom_clear_sample_if_requested(0, &previous);
    for step in 1..=BOTTOM_CLEAR_STABILITY_STEPS {
        let (anchor_x, anchor_y) = workspace_scroll_anchor(robot);
        robot
            .mouse_move(anchor_x, anchor_y)
            .expect("move cursor to workspace for bottom Clear stability");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll(0.0, BOTTOM_CLEAR_SCROLL_DELTA_Y)
            .expect("bottom Clear micro scroll should succeed");
        robot
            .pump_frames(1)
            .expect("bottom Clear active-scroll frame pump should succeed");

        let current = capture_bottom_clear_stability_sample(robot);
        save_bottom_clear_sample_if_requested(step, &current);
        let actual_delta_y = current.clear_bounds.y - previous.clear_bounds.y;
        println!(
            "bottom_clear_geometry step={step} clear_delta_y={actual_delta_y:.3} label=({:.1},{:.1},{:.1},{:.1}) clear=({:.1},{:.1},{:.1},{:.1}) fixed=({:.1},{:.1},{:.1},{:.1})",
            current.label_bounds.x,
            current.label_bounds.y,
            current.label_bounds.width,
            current.label_bounds.height,
            current.clear_bounds.x,
            current.clear_bounds.y,
            current.clear_bounds.width,
            current.clear_bounds.height,
            current.fixed_region.x,
            current.fixed_region.y,
            current.fixed_region.width,
            current.fixed_region.height,
        );
        if (actual_delta_y - BOTTOM_CLEAR_SCROLL_DELTA_Y).abs() > BOTTOM_CLEAR_DELTA_EPSILON {
            save_bottom_clear_failure(step, &previous, &current);
            fail_with_robot(
                robot,
                &format!(
                    "bottom Clear button did not move by exact 1px scroll at step {step}: expected {:.3}, got {actual_delta_y:.3}",
                    BOTTOM_CLEAR_SCROLL_DELTA_Y
                ),
            );
        }
        assert_bottom_clear_screen_crop_stable(robot, step, &previous, &current, actual_delta_y);
        assert_bottom_clear_icon_profile_stable(robot, step, &previous, &current);
        robot
            .pump_frames(BOTTOM_CLEAR_ACTIVE_CRISPNESS_SETTLE_FRAMES)
            .expect("bottom Clear settle frame pump should succeed");
        let settled = capture_bottom_clear_stability_sample(robot);
        assert_bottom_clear_active_frame_crispness_matches_settled(robot, step, &current, &settled);
        previous = settled;
    }
}

fn log_row_micro_render_stats(robot: &cranpose::Robot, step: usize) {
    log_top_isolated_layer_render_stats(robot, "row_micro", step);
}

fn log_workspace_strip_render_stats(robot: &cranpose::Robot, step: usize) {
    log_top_isolated_layer_render_stats(robot, "workspace_strip", step);
}

fn log_top_isolated_layer_render_stats(robot: &cranpose::Robot, label: &str, step: usize) {
    if std::env::var_os(RENDER_STATS_ENV).is_none() {
        return;
    }
    match robot.get_render_stats() {
        Ok(Some(stats)) => {
            println!(
                "{label}_render_stats step={step} isolated_layer_renders={} top_isolated_layer_count={}",
                stats.isolated_layer_renders, stats.top_isolated_layer_count
            );
            for (index, layer) in stats.top_isolated_layers().enumerate() {
                println!(
                    "{label}_isolated_layer step={step} rank={index} node={:?} rect=({:.3},{:.3},{:.3},{:.3}) target={}x{} reasons={}",
                    layer.node_id,
                    layer.logical_rect.x,
                    layer.logical_rect.y,
                    layer.logical_rect.width,
                    layer.logical_rect.height,
                    layer.width,
                    layer.height,
                    layer.reasons.display()
                );
            }
        }
        Ok(None) => println!("{label}_render_stats step={step} unavailable"),
        Err(err) => println!("{label}_render_stats step={step} error={err}"),
    }
}

fn capture_rust_runtime_row_micro_sample(robot: &cranpose::Robot) -> RowMicroStabilitySample {
    let label_bounds = workspace_text_bounds_exact(robot, "Rust Runtime (ms)")
        .unwrap_or_else(|| fail_with_robot(robot, "Rust Runtime (ms) label not found"));
    let stable_band = stable_workspace_content_bounds(robot);
    if !stable_band.overlaps_x(label_bounds) || !stable_band.inset_contains(label_bounds, 0.0) {
        fail_with_robot(
            robot,
            &format!(
                "Rust Runtime (ms) label is outside stable viewport band: bounds=({:.1},{:.1},{:.1},{:.1}) viewport=({:.1},{:.1},{:.1},{:.1})",
                label_bounds.x,
                label_bounds.y,
                label_bounds.width,
                label_bounds.height,
                stable_band.x,
                stable_band.y,
                stable_band.width,
                stable_band.height
            ),
        );
    }
    let screenshot = capture_visible_window_screenshot("rust-runtime-row-micro");
    let clear_bounds = visible_workspace_button_bounds_nearest_to_y_with_red_icon(
        robot,
        &screenshot,
        "Clear",
        stable_band,
        label_bounds.center_y(),
    )
    .unwrap_or_else(|| {
        fail_with_robot(
            robot,
            &format!(
                "visible Rust Runtime Clear button not found in stable band: band=({:.1},{:.1},{:.1},{:.1}) label=({:.1},{:.1},{:.1},{:.1}) all_clear={:?}",
                stable_band.x,
                stable_band.y,
                stable_band.width,
                stable_band.height,
                label_bounds.x,
                label_bounds.y,
                label_bounds.width,
                label_bounds.height,
                workspace_button_bounds_exact(robot, "Clear"),
            ),
        )
    });

    let left = label_bounds.x - ROW_MICRO_REGION_LEFT_PAD;
    let right = clear_bounds.x + clear_bounds.width + ROW_MICRO_REGION_RIGHT_PAD;
    let top = label_bounds.y.min(clear_bounds.y) - ROW_MICRO_REGION_VERTICAL_PAD;
    let bottom = (label_bounds.y + label_bounds.height).max(clear_bounds.y + clear_bounds.height)
        + ROW_MICRO_REGION_VERTICAL_PAD;
    let region = Bounds {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
    .clipped_to(stable_band)
    .unwrap_or_else(|| fail_with_robot(robot, "Rust Runtime row stability region is clipped out"));
    let clear_icon_bounds = row_micro_feature_bounds(
        &screenshot,
        clear_bounds.padded(BUTTON_QUALITY_SAFE_EDGE),
        red_clear_icon_pixel,
        "Clear red icon",
    );
    let field_icon_bounds = row_micro_feature_bounds(
        &screenshot,
        Bounds {
            x: (label_bounds.x - ROW_MICRO_FIELD_ICON_LEFT_PAD).max(0.0),
            y: region.y,
            width: ROW_MICRO_FIELD_ICON_REGION_WIDTH,
            height: region.height,
        },
        blue_field_icon_pixel,
        "Rust Runtime field icon",
    );

    RowMicroStabilitySample {
        label_bounds,
        clear_bounds,
        region,
        clear_icon_bounds,
        field_icon_bounds,
        screenshot,
    }
}

fn capture_workspace_strip_stability_sample(
    robot: &cranpose::Robot,
    target_text: &str,
) -> WorkspaceStripStabilitySample {
    let target_bounds = workspace_text_bounds_exact(robot, target_text)
        .unwrap_or_else(|| fail_with_robot(robot, &format!("{target_text} label not found")));
    let stable_band = stable_workspace_content_bounds(robot);
    if !stable_band.overlaps_x(target_bounds) {
        fail_with_robot(
            robot,
            &format!(
                "{target_text} label is outside the workspace x-range: bounds=({:.1},{:.1},{:.1},{:.1}) viewport=({:.1},{:.1},{:.1},{:.1})",
                target_bounds.x,
                target_bounds.y,
                target_bounds.width,
                target_bounds.height,
                stable_band.x,
                stable_band.y,
                stable_band.width,
                stable_band.height
            ),
        );
    }

    let region = Bounds {
        x: stable_band.x + WORKSPACE_STRIP_REGION_INSET_X,
        y: stable_band.y + WORKSPACE_STRIP_REGION_INSET_Y,
        width: (stable_band.width - WORKSPACE_STRIP_REGION_INSET_X * 2.0).max(0.0),
        height: (stable_band.height - WORKSPACE_STRIP_REGION_INSET_Y * 2.0).max(0.0),
    };
    let screenshot = capture_visible_window_screenshot("workspace-strip-stability");

    WorkspaceStripStabilitySample {
        target_bounds,
        region,
        screenshot,
    }
}

fn capture_first_moved_workspace_strip_screenshot(
    robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    previous: &WorkspaceStripStabilitySample,
    expected_delta_y: f32,
) -> cranpose::RobotScreenshot {
    let previous_crop = normalize_workspace_strip(&previous.screenshot, previous.region);
    let followed_region = Bounds {
        y: previous.region.y + expected_delta_y,
        ..previous.region
    };
    let mut best_attempt = None::<(cranpose::RobotScreenshot, usize, u64, u64, f32)>;

    for attempt in 0..=WORKSPACE_STRIP_ACTIVE_CAPTURE_ATTEMPTS {
        if attempt > 0 {
            robot
                .pump_frames(1)
                .expect("workspace strip active capture frame pump should succeed");
            std::thread::sleep(Duration::from_millis(
                WORKSPACE_STRIP_ACTIVE_CAPTURE_SLEEP_MS,
            ));
        }

        let screenshot = capture_visible_window_screenshot("workspace-strip-active-first-moved");
        let unmoved_crop = normalize_workspace_strip(&screenshot, previous.region);
        let followed_crop = normalize_workspace_strip(&screenshot, followed_region);
        let unmoved_score = screenshot_abs_difference_score(&previous_crop, &unmoved_crop)
            .expect("workspace strip unmoved active crop should match size");
        let followed_score = screenshot_abs_difference_score(&previous_crop, &followed_crop)
            .expect("workspace strip followed active crop should match size");
        let improvement_ratio = if unmoved_score == 0 {
            0.0
        } else {
            unmoved_score.saturating_sub(followed_score) as f32 / unmoved_score as f32
        };
        println!(
            "workspace_strip_active_capture step={step} target={target_text} attempt={attempt} unmoved_score={unmoved_score} followed_score={followed_score} improvement_ratio={improvement_ratio:.4}"
        );

        if best_attempt
            .as_ref()
            .is_none_or(|(_, _, _, best_score, _)| followed_score < *best_score)
        {
            best_attempt = Some((
                screenshot.clone(),
                attempt,
                unmoved_score,
                followed_score,
                improvement_ratio,
            ));
        }
        if followed_score < unmoved_score {
            return screenshot;
        }
    }

    let (best_screenshot, best_attempt, best_unmoved, best_followed, best_improvement) =
        best_attempt.expect("workspace strip active capture should run at least once");
    save_workspace_strip_failure(
        step,
        &previous.screenshot,
        &best_screenshot,
        previous.region,
        expected_delta_y,
    );
    fail_with_robot(
        robot,
        &format!(
            "{target_text} workspace strip did not present a moved X11 frame after scroll step {step}: best_attempt={best_attempt} unmoved_score={best_unmoved} followed_score={best_followed} improvement_ratio={best_improvement:.4}"
        ),
    );
}

fn capture_workspace_rigid_stability_sample(
    robot: &cranpose::Robot,
    target_text: &str,
) -> WorkspaceRigidStabilitySample {
    let target_bounds = workspace_text_bounds_exact(robot, target_text)
        .unwrap_or_else(|| fail_with_robot(robot, &format!("{target_text} label not found")));
    let stable_band = stable_workspace_content_bounds(robot);
    if !stable_band.overlaps_x(target_bounds)
        || !stable_band.inset_contains(target_bounds, WORKSPACE_RIGID_REGION_INSET_Y)
    {
        fail_with_robot(
            robot,
            &format!(
                "{target_text} label is outside the rigid active-scroll stable band: bounds=({:.1},{:.1},{:.1},{:.1}) viewport=({:.1},{:.1},{:.1},{:.1})",
                target_bounds.x,
                target_bounds.y,
                target_bounds.width,
                target_bounds.height,
                stable_band.x,
                stable_band.y,
                stable_band.width,
                stable_band.height
            ),
        );
    }

    let vertical_inset = WORKSPACE_RIGID_REGION_INSET_Y
        + WORKSPACE_RIGID_SCROLL_DELTA_Y.abs().ceil()
        + BUTTON_QUALITY_SAFE_EDGE;
    let region = Bounds {
        x: stable_band.x + WORKSPACE_RIGID_REGION_INSET_X,
        y: stable_band.y + vertical_inset,
        width: (stable_band.width - WORKSPACE_RIGID_REGION_INSET_X * 2.0).max(0.0),
        height: (stable_band.height - vertical_inset * 2.0).max(0.0),
    };
    let screenshot = capture_visible_window_screenshot("workspace-rigid-active-scroll");
    let visual_anchors = if std::env::var_os(VISUAL_ANCHORS_ENV).is_some() {
        capture_workspace_visual_anchors(
            robot,
            &screenshot,
            target_text,
            target_bounds,
            stable_band,
        )
    } else {
        Vec::new()
    };

    WorkspaceRigidStabilitySample {
        target_bounds,
        region,
        visual_anchors,
        screenshot,
    }
}

fn capture_workspace_visual_anchors(
    robot: &cranpose::Robot,
    screenshot: &cranpose::RobotScreenshot,
    target_text: &str,
    target_bounds: Bounds,
    stable_band: Bounds,
) -> Vec<WorkspaceVisualAnchor> {
    let paste_bounds = visible_workspace_button_bounds_nearest_to_y(
        robot,
        "Paste",
        stable_band,
        target_bounds.center_y(),
    )
    .unwrap_or_else(|| {
        fail_with_robot(
            robot,
            &format!("{target_text} Paste button not found for visual anchor stability"),
        )
    });
    let clear_bounds = visible_workspace_button_bounds_nearest_to_y_with_red_icon(
        robot,
        screenshot,
        "Clear",
        stable_band,
        target_bounds.center_y(),
    )
    .or_else(|| {
        visible_workspace_button_bounds_nearest_to_y(
            robot,
            "Clear",
            stable_band,
            target_bounds.center_y(),
        )
    })
    .unwrap_or_else(|| {
        fail_with_robot(
            robot,
            &format!("{target_text} Clear button not found for visual anchor stability"),
        )
    });

    let field_region = Bounds {
        x: (target_bounds.x - WORKSPACE_ANCHOR_FIELD_LEFT_PAD).max(0.0),
        y: (target_bounds.y - WORKSPACE_ANCHOR_FIELD_TOP_PAD).max(0.0),
        width: WORKSPACE_ANCHOR_FIELD_WIDTH,
        height: WORKSPACE_ANCHOR_FIELD_HEIGHT,
    };
    let label_region = target_bounds.padded(WORKSPACE_ANCHOR_FEATURE_PAD);
    let paste_region = paste_bounds.padded(WORKSPACE_ANCHOR_FEATURE_PAD);
    let clear_region = clear_bounds.padded(WORKSPACE_ANCHOR_FEATURE_PAD);

    vec![
        WorkspaceVisualAnchor {
            label: "field icon",
            region: field_region,
            bounds: feature_bounds_in_logical_region(
                screenshot,
                field_region,
                blue_field_icon_pixel,
                "field icon",
            ),
            edge_energy: screenshot_region_edge_energy(screenshot, field_region, "field icon"),
            unique_rgb_count: screenshot_region_unique_rgb_count(
                screenshot,
                field_region,
                "field icon",
            ),
        },
        WorkspaceVisualAnchor {
            label: "label ink",
            region: label_region,
            bounds: feature_bounds_in_logical_region(
                screenshot,
                label_region,
                blue_label_text_pixel,
                "label ink",
            ),
            edge_energy: screenshot_region_edge_energy(screenshot, label_region, "label ink"),
            unique_rgb_count: screenshot_region_unique_rgb_count(
                screenshot,
                label_region,
                "label ink",
            ),
        },
        WorkspaceVisualAnchor {
            label: "Paste icon",
            region: paste_region,
            bounds: feature_bounds_in_logical_region(
                screenshot,
                paste_region,
                blue_field_icon_pixel,
                "Paste icon",
            ),
            edge_energy: screenshot_region_edge_energy(screenshot, paste_region, "Paste icon"),
            unique_rgb_count: screenshot_region_unique_rgb_count(
                screenshot,
                paste_region,
                "Paste icon",
            ),
        },
        WorkspaceVisualAnchor {
            label: "Clear icon",
            region: clear_region,
            bounds: feature_bounds_in_logical_region(
                screenshot,
                clear_region,
                red_clear_icon_pixel,
                "Clear icon",
            ),
            edge_energy: screenshot_region_edge_energy(screenshot, clear_region, "Clear icon"),
            unique_rgb_count: screenshot_region_unique_rgb_count(
                screenshot,
                clear_region,
                "Clear icon",
            ),
        },
    ]
}

fn assert_workspace_visual_anchors_stable(
    robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    previous: &WorkspaceRigidStabilitySample,
    current: &WorkspaceRigidStabilitySample,
) {
    let target_dx = current.target_bounds.x - previous.target_bounds.x;
    let target_width_delta = current.target_bounds.width - previous.target_bounds.width;
    println!(
        "workspace_visual_anchor_geometry step={step} target={target_text} target_dx={target_dx:.3} target_width_delta={target_width_delta:.3}"
    );
    if target_dx.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
        || target_width_delta.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
    {
        save_workspace_strip_failure(
            step,
            &previous.screenshot,
            &current.screenshot,
            previous.region,
            current.target_bounds.y - previous.target_bounds.y,
        );
        fail_with_robot(
            robot,
            &format!(
                "{target_text} visual anchor semantics shifted horizontally during active scroll step {step}: target_dx={target_dx:.3} target_width_delta={target_width_delta:.3}"
            ),
        );
    }

    if previous.visual_anchors.len() != current.visual_anchors.len() {
        fail_with_robot(
            robot,
            &format!(
                "{target_text} visual anchor count changed during active scroll step {step}: previous={} current={}",
                previous.visual_anchors.len(),
                current.visual_anchors.len()
            ),
        );
    }

    for (previous_anchor, current_anchor) in previous
        .visual_anchors
        .iter()
        .zip(current.visual_anchors.iter())
    {
        if previous_anchor.label != current_anchor.label {
            fail_with_robot(
                robot,
                &format!(
                    "{target_text} visual anchor order changed during active scroll step {step}: previous={} current={}",
                    previous_anchor.label, current_anchor.label
                ),
            );
        }

        let center_delta =
            (current_anchor.bounds.center_x() - previous_anchor.bounds.center_x()).abs();
        let centroid_delta =
            (current_anchor.bounds.centroid_x() - previous_anchor.bounds.centroid_x()).abs();
        let width_delta =
            current_anchor.bounds.width() as i32 - previous_anchor.bounds.width() as i32;
        let count_delta =
            current_anchor.bounds.count as isize - previous_anchor.bounds.count as isize;
        let count_delta_ratio =
            count_delta.unsigned_abs() as f32 / previous_anchor.bounds.count.max(1) as f32;
        let max_centroid_delta = if previous_anchor.label == "label ink" {
            WORKSPACE_ANCHOR_MAX_TEXT_CENTROID_DELTA_PX
        } else {
            WORKSPACE_ANCHOR_MAX_CENTROID_DELTA_PX
        };
        let max_count_delta_ratio = if previous_anchor.label == "label ink" {
            WORKSPACE_ANCHOR_MAX_TEXT_COUNT_DELTA_RATIO
        } else {
            WORKSPACE_ANCHOR_MAX_COUNT_DELTA_RATIO
        };
        println!(
            "workspace_visual_anchor step={step} target={target_text} label={} center_delta={center_delta:.3} centroid_delta={centroid_delta:.3} width_delta={width_delta} count_delta={count_delta} count_delta_ratio={count_delta_ratio:.4} previous={:?} current={:?}",
            previous_anchor.label,
            previous_anchor.bounds,
            current_anchor.bounds
        );
        if center_delta > WORKSPACE_ANCHOR_MAX_CENTER_DELTA_PX
            || centroid_delta > max_centroid_delta
            || width_delta.abs() > WORKSPACE_ANCHOR_MAX_WIDTH_DELTA_PX
            || count_delta_ratio > max_count_delta_ratio
        {
            save_workspace_strip_failure(
                step,
                &previous.screenshot,
                &current.screenshot,
                previous.region,
                current.target_bounds.y - previous.target_bounds.y,
            );
            fail_with_robot(
                robot,
                &format!(
                    "{target_text} {} visual anchor changed horizontally during active scroll step {step}: center_delta={center_delta:.3} centroid_delta={centroid_delta:.3} width_delta={width_delta} count_delta_ratio={count_delta_ratio:.4}",
                    previous_anchor.label
                ),
            );
        }
    }

    for left_index in 0..previous.visual_anchors.len() {
        for right_index in (left_index + 1)..previous.visual_anchors.len() {
            let previous_left = &previous.visual_anchors[left_index];
            let previous_right = &previous.visual_anchors[right_index];
            let current_left = &current.visual_anchors[left_index];
            let current_right = &current.visual_anchors[right_index];
            let previous_distance =
                previous_right.bounds.center_x() - previous_left.bounds.center_x();
            let current_distance = current_right.bounds.center_x() - current_left.bounds.center_x();
            let distance_delta = (current_distance - previous_distance).abs();
            println!(
                "workspace_visual_anchor_pair step={step} target={target_text} left={} right={} distance_delta={distance_delta:.3} previous_distance={previous_distance:.3} current_distance={current_distance:.3}",
                previous_left.label,
                previous_right.label,
            );
            if distance_delta > WORKSPACE_ANCHOR_MAX_PAIR_DISTANCE_DELTA_PX {
                save_workspace_strip_failure(
                    step,
                    &previous.screenshot,
                    &current.screenshot,
                    previous.region,
                    current.target_bounds.y - previous.target_bounds.y,
                );
                fail_with_robot(
                    robot,
                    &format!(
                        "{target_text} visual anchors changed relative horizontal distance during active scroll step {step}: left={} right={} distance_delta={distance_delta:.3}",
                        previous_left.label,
                        previous_right.label
                    ),
                );
            }
        }
    }
}

fn assert_workspace_active_frame_crispness_matches_settled(
    robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    active: &WorkspaceRigidStabilitySample,
    settled: &WorkspaceRigidStabilitySample,
) {
    let target_dx = settled.target_bounds.x - active.target_bounds.x;
    let target_dy = settled.target_bounds.y - active.target_bounds.y;
    if target_dx.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
        || target_dy.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
    {
        fail_with_robot(
            robot,
            &format!(
                "{target_text} active frame and settled frame do not describe the same scroll position at step {step}: dx={target_dx:.3} dy={target_dy:.3}"
            ),
        );
    }

    for (active_anchor, settled_anchor) in active
        .visual_anchors
        .iter()
        .zip(settled.visual_anchors.iter())
    {
        if active_anchor.label != settled_anchor.label {
            fail_with_robot(
                robot,
                &format!(
                    "{target_text} active/settled anchor order changed at step {step}: active={} settled={}",
                    active_anchor.label, settled_anchor.label
                ),
            );
        }

        let min_edge_energy =
            settled_anchor.edge_energy * (1.0 - WORKSPACE_ACTIVE_CRISPNESS_MAX_EDGE_LOSS_RATIO);
        let max_unique_rgb =
            settled_anchor.unique_rgb_count + WORKSPACE_ACTIVE_CRISPNESS_MAX_EXTRA_COLORS;
        println!(
            "workspace_active_crispness step={step} target={target_text} label={} active_edge={:.3} settled_edge={:.3} min_edge={:.3} active_unique={} settled_unique={} max_unique={} region=({:.1},{:.1},{:.1},{:.1})",
            active_anchor.label,
            active_anchor.edge_energy,
            settled_anchor.edge_energy,
            min_edge_energy,
            active_anchor.unique_rgb_count,
            settled_anchor.unique_rgb_count,
            max_unique_rgb,
            active_anchor.region.x,
            active_anchor.region.y,
            active_anchor.region.width,
            active_anchor.region.height,
        );
        if active_anchor.edge_energy < min_edge_energy
            || active_anchor.unique_rgb_count > max_unique_rgb
        {
            save_workspace_strip_failure(
                step,
                &active.screenshot,
                &settled.screenshot,
                active.region,
                settled.target_bounds.y - active.target_bounds.y,
            );
            fail_with_robot(
                robot,
                &format!(
                    "{target_text} {} is blurrier in the first active scroll frame than after settling at step {step}: active_edge={:.3} settled_edge={:.3} min_edge={:.3} active_unique={} settled_unique={} max_unique={}",
                    active_anchor.label,
                    active_anchor.edge_energy,
                    settled_anchor.edge_energy,
                    min_edge_energy,
                    active_anchor.unique_rgb_count,
                    settled_anchor.unique_rgb_count,
                    max_unique_rgb
                ),
            );
        }
    }
}

fn assert_workspace_rigid_picture_stable(
    _robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    previous: &WorkspaceRigidStabilitySample,
    current: &WorkspaceRigidStabilitySample,
    actual_delta_y: f32,
) {
    let before = normalize_workspace_region(&previous.screenshot, previous.region);
    let current_region = Bounds {
        y: previous.region.y + actual_delta_y,
        ..previous.region
    };
    let after = normalize_workspace_region(&current.screenshot, current_region);
    let stats = screenshot_difference_stats(&before, &after, WORKSPACE_RIGID_DIFF_TOLERANCE)
        .expect("workspace rigid normalized screenshots should match size");
    let pixel_count = (before.width as usize).saturating_mul(before.height as usize);
    let differing_ratio = stats.differing_pixels as f32 / pixel_count.max(1) as f32;
    println!(
        "workspace_rigid_picture step={step} target={target_text} differing_pixels={} differing_ratio={differing_ratio:.5} max_diff={}",
        stats.differing_pixels, stats.max_difference
    );
    if stats.differing_pixels > WORKSPACE_RIGID_MAX_DIFFERING_PIXELS
        && differing_ratio > WORKSPACE_RIGID_MAX_DIFFERING_RATIO
    {
        let first = stats
            .first_difference
            .as_ref()
            .expect("failing workspace rigid diff should have first difference");
        println!(
            "{target_text} workspace rigid full-strip diff exceeded the diagnostic budget during active scroll step {step}: differing_pixels={} differing_ratio={differing_ratio:.5} max_diff={} first_diff=({}, {}) before={:?} after={:?}",
                stats.differing_pixels,
                stats.max_difference,
                first.x,
                first.y,
                first.before,
                first.after
        );
    }
}

fn capture_workspace_top_band_stability_sample(
    robot: &cranpose::Robot,
    target_text: &str,
) -> WorkspaceTopBandStabilitySample {
    let target_bounds = workspace_text_bounds_exact(robot, target_text)
        .unwrap_or_else(|| fail_with_robot(robot, &format!("{target_text} label not found")));
    let viewport = workspace_viewport_bounds(robot);
    if !viewport.overlaps_x(target_bounds) {
        fail_with_robot(
            robot,
            &format!(
                "{target_text} label is outside the workspace x-range: bounds=({:.1},{:.1},{:.1},{:.1}) viewport=({:.1},{:.1},{:.1},{:.1})",
                target_bounds.x,
                target_bounds.y,
                target_bounds.width,
                target_bounds.height,
                viewport.x,
                viewport.y,
                viewport.width,
                viewport.height
            ),
        );
    }

    let region = Bounds {
        x: viewport.x + WORKSPACE_TOP_BAND_REGION_INSET_X,
        y: viewport.y + WORKSPACE_TOP_BAND_REGION_TOP,
        width: (viewport.width - WORKSPACE_TOP_BAND_REGION_INSET_X * 2.0).max(0.0),
        height: WORKSPACE_TOP_BAND_REGION_HEIGHT,
    }
    .clipped_to(viewport)
    .unwrap_or_else(|| fail_with_robot(robot, "workspace top-band stability region is empty"));
    let screenshot = capture_visible_window_screenshot("workspace-top-band-stability");
    assert_leetcodedaily_full_frame_color_health(
        robot,
        "workspace top-band stability",
        &screenshot,
    );

    WorkspaceTopBandStabilitySample {
        target_bounds,
        region,
        screenshot,
    }
}

fn capture_bottom_clear_stability_sample(robot: &cranpose::Robot) -> BottomClearStabilitySample {
    let label_bounds = workspace_text_bounds_exact(robot, "Rust Runtime (ms)")
        .unwrap_or_else(|| fail_with_robot(robot, "Rust Runtime (ms) label not found"));
    let stable_band = stable_workspace_content_bounds(robot);
    if !stable_band.overlaps_x(label_bounds) || !stable_band.inset_contains(label_bounds, 0.0) {
        fail_with_robot(
            robot,
            &format!(
                "Rust Runtime (ms) label is outside stable viewport band: bounds=({:.1},{:.1},{:.1},{:.1}) viewport=({:.1},{:.1},{:.1},{:.1})",
                label_bounds.x,
                label_bounds.y,
                label_bounds.width,
                label_bounds.height,
                stable_band.x,
                stable_band.y,
                stable_band.width,
                stable_band.height
            ),
        );
    }

    let screenshot = capture_visible_window_screenshot("bottom-clear-stability");
    let clear_bounds = visible_workspace_button_bounds_nearest_to_y_with_red_icon(
        robot,
        &screenshot,
        "Clear",
        stable_band,
        label_bounds.center_y(),
    )
    .unwrap_or_else(|| {
        let dir = bottom_clear_capture_dir();
        let full_path = dir.join("missing-visual-clear-full-window.png");
        save_robot_screenshot_png(&full_path, &screenshot);
        fail_with_robot(
            robot,
            &format!(
                "bottom Rust Runtime Clear button not found in stable band: band=({:.1},{:.1},{:.1},{:.1}) label=({:.1},{:.1},{:.1},{:.1}) full={} all_clear={:?}",
                stable_band.x,
                stable_band.y,
                stable_band.width,
                stable_band.height,
                label_bounds.x,
                label_bounds.y,
                label_bounds.width,
                label_bounds.height,
                full_path.display(),
                workspace_button_bounds_exact(robot, "Clear"),
            ),
        )
    });
    let fixed_region = bottom_clear_fixed_screen_region(clear_bounds, stable_band)
        .unwrap_or_else(|| fail_with_robot(robot, "bottom Clear fixed screen crop is invalid"));
    let output_size = row_micro_output_size(&screenshot, fixed_region);
    let fixed_crop = normalize_screenshot_region(
        &screenshot,
        fixed_region.tuple(),
        output_size.0,
        output_size.1,
    )
    .expect("normalize bottom Clear fixed screen crop");
    let clear_crop = capture_bottom_clear_button_crop(&screenshot, clear_bounds);
    let clear_edge_energy = screenshot_edge_energy(&clear_crop);
    let clear_unique_rgb_count = screenshot_unique_rgb_count(&clear_crop);
    let red_profile = red_icon_horizontal_profile(&clear_crop).unwrap_or_else(|| {
        let dir = bottom_clear_capture_dir();
        let clear_path = dir.join("missing-red-profile-clear-button.png");
        save_robot_screenshot_png(&clear_path, &clear_crop);
        fail_with_robot(
            robot,
            &format!(
                "bottom Clear red icon pixels not found: clear=({:.1},{:.1},{:.1},{:.1}) crop={} stable_band=({:.1},{:.1},{:.1},{:.1})",
                clear_bounds.x,
                clear_bounds.y,
                clear_bounds.width,
                clear_bounds.height,
                clear_path.display(),
                stable_band.x,
                stable_band.y,
                stable_band.width,
                stable_band.height
            ),
        )
    });

    BottomClearStabilitySample {
        label_bounds,
        clear_bounds,
        fixed_region,
        screenshot,
        fixed_crop,
        clear_crop,
        red_profile,
        clear_edge_energy,
        clear_unique_rgb_count,
    }
}

fn bottom_clear_fixed_screen_region(clear_bounds: Bounds, stable_band: Bounds) -> Option<Bounds> {
    let width = BOTTOM_CLEAR_FIXED_REGION_WIDTH.max(clear_bounds.width + 48.0);
    let center_x = clear_bounds.x + clear_bounds.width * 0.5;
    let min_y = stable_band.y;
    let max_y = stable_band.y + stable_band.height - BOTTOM_CLEAR_FIXED_REGION_HEIGHT;
    let y = if max_y >= min_y {
        (clear_bounds.center_y() - BOTTOM_CLEAR_FIXED_REGION_HEIGHT * 0.5).clamp(min_y, max_y)
    } else {
        stable_band.y
    };
    Bounds {
        x: center_x - width * 0.5,
        y,
        width,
        height: BOTTOM_CLEAR_FIXED_REGION_HEIGHT,
    }
    .clipped_to(stable_band)
}

fn capture_bottom_clear_button_crop(
    screenshot: &cranpose::RobotScreenshot,
    clear_bounds: Bounds,
) -> cranpose::RobotScreenshot {
    let window = Bounds {
        x: 0.0,
        y: 0.0,
        width: screenshot.logical_width,
        height: screenshot.logical_height,
    };
    let crop_bounds = clear_bounds
        .padded(BOTTOM_CLEAR_BUTTON_CROP_PAD)
        .clipped_to(window)
        .expect("bottom Clear button crop is inside screenshot");
    let output_size = row_micro_output_size(screenshot, crop_bounds);
    normalize_screenshot_region(
        screenshot,
        crop_bounds.tuple(),
        output_size.0,
        output_size.1,
    )
    .expect("normalize bottom Clear button crop")
}

fn stable_window_bounds() -> Bounds {
    let (width, height) = app_window_size();
    Bounds {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    }
}

fn stable_workspace_content_bounds(robot: &cranpose::Robot) -> Bounds {
    let viewport = workspace_viewport_bounds(robot);
    let vertical_inset = WORKSPACE_SCROLL_SHADOW_HEIGHT + BUTTON_QUALITY_SAFE_EDGE;
    Bounds {
        x: viewport.x + BUTTON_QUALITY_SAFE_EDGE,
        y: viewport.y + vertical_inset,
        width: (viewport.width - BUTTON_QUALITY_SAFE_EDGE * 2.0).max(0.0),
        height: (viewport.height - vertical_inset * 2.0).max(0.0),
    }
}

fn visible_workspace_button_bounds_nearest_to_y_with_red_icon(
    robot: &cranpose::Robot,
    screenshot: &cranpose::RobotScreenshot,
    label: &str,
    viewport: Bounds,
    target_y: f32,
) -> Option<Bounds> {
    let mut candidates: Vec<_> = workspace_button_bounds_exact(robot, label)
        .into_iter()
        .filter(|candidate| viewport.inset_contains(*candidate, BUTTON_QUALITY_SAFE_EDGE))
        .filter(|candidate| {
            let crop = capture_bottom_clear_button_crop(screenshot, *candidate);
            red_icon_horizontal_profile(&crop).is_some()
        })
        .collect();
    candidates.sort_by(|left, right| {
        (left.center_y() - target_y)
            .abs()
            .total_cmp(&(right.center_y() - target_y).abs())
    });
    candidates.into_iter().next()
}

fn visible_workspace_button_bounds_nearest_to_y(
    robot: &cranpose::Robot,
    label: &str,
    viewport: Bounds,
    target_y: f32,
) -> Option<Bounds> {
    let mut candidates: Vec<_> = workspace_button_bounds_exact(robot, label)
        .into_iter()
        .filter(|candidate| viewport.inset_contains(*candidate, BUTTON_QUALITY_SAFE_EDGE))
        .collect();
    candidates.sort_by(|left, right| {
        (left.center_y() - target_y)
            .abs()
            .total_cmp(&(right.center_y() - target_y).abs())
    });
    candidates.into_iter().next()
}

fn assert_row_horizontal_geometry_stable(
    robot: &cranpose::Robot,
    step: usize,
    previous: &RowMicroStabilitySample,
    current: &RowMicroStabilitySample,
) {
    let clear_delta_x = (current.clear_bounds.x - previous.clear_bounds.x).abs();
    let clear_delta_width = (current.clear_bounds.width - previous.clear_bounds.width).abs();
    let region_delta_x = (current.region.x - previous.region.x).abs();
    let region_delta_width = (current.region.width - previous.region.width).abs();
    if clear_delta_x > ROW_MICRO_GEOMETRY_EPSILON
        || clear_delta_width > ROW_MICRO_GEOMETRY_EPSILON
        || region_delta_x > ROW_MICRO_GEOMETRY_EPSILON
        || region_delta_width > ROW_MICRO_GEOMETRY_EPSILON
    {
        fail_with_robot(
            robot,
            &format!(
                "Rust Runtime row horizontal semantics changed at micro step {step}: clear_dx={clear_delta_x:.3} clear_dw={clear_delta_width:.3} region_dx={region_delta_x:.3} region_dw={region_delta_width:.3}"
            ),
        );
    }
}

fn assert_row_icon_features_stable(
    robot: &cranpose::Robot,
    step: usize,
    previous: &RowMicroStabilitySample,
    current: &RowMicroStabilitySample,
) {
    let clear_region = current.clear_bounds.padded(BUTTON_QUALITY_SAFE_EDGE);
    if let Err(message) = pixel_feature_stability_error(
        step,
        "Clear red icon",
        previous.clear_icon_bounds,
        current.clear_icon_bounds,
    ) {
        save_row_micro_feature_failure(
            step,
            "clear-red-icon",
            &previous.screenshot,
            &current.screenshot,
            previous.clear_bounds.padded(BUTTON_QUALITY_SAFE_EDGE),
            clear_region,
        );
        fail_with_robot(robot, &message);
    }

    let field_region = Bounds {
        x: (current.label_bounds.x - ROW_MICRO_FIELD_ICON_LEFT_PAD).max(0.0),
        y: current.region.y,
        width: ROW_MICRO_FIELD_ICON_REGION_WIDTH,
        height: current.region.height,
    };
    if let Err(message) = pixel_feature_stability_error(
        step,
        "Rust Runtime field icon",
        previous.field_icon_bounds,
        current.field_icon_bounds,
    ) {
        save_row_micro_feature_failure(
            step,
            "rust-runtime-field-icon",
            &previous.screenshot,
            &current.screenshot,
            Bounds {
                x: (previous.label_bounds.x - ROW_MICRO_FIELD_ICON_LEFT_PAD).max(0.0),
                y: previous.region.y,
                width: ROW_MICRO_FIELD_ICON_REGION_WIDTH,
                height: previous.region.height,
            },
            field_region,
        );
        fail_with_robot(robot, &message);
    }
}

fn pixel_feature_stability_error(
    step: usize,
    label: &str,
    previous: PixelFeatureBounds,
    current: PixelFeatureBounds,
) -> Result<(), String> {
    let bounds_center_delta = (current.center_x() - previous.center_x()).abs();
    let centroid_delta = (current.centroid_x() - previous.centroid_x()).abs();
    let vertical_center_delta = (current.center_y() - previous.center_y()).abs();
    let vertical_centroid_delta = (current.centroid_y() - previous.centroid_y()).abs();
    let width_delta = current.width() as i32 - previous.width() as i32;
    let height_delta = current.height() as i32 - previous.height() as i32;
    let count_delta = current.count as isize - previous.count as isize;
    let count_delta_ratio = count_delta.unsigned_abs() as f32 / previous.count.max(1) as f32;
    println!(
        "row_micro_icon step={step} label={label} bounds_center_delta={bounds_center_delta:.3} centroid_delta={centroid_delta:.3} vertical_center_delta={vertical_center_delta:.3} vertical_centroid_delta={vertical_centroid_delta:.3} width_delta={width_delta} height_delta={height_delta} count_delta={count_delta} count_delta_ratio={count_delta_ratio:.4} previous={previous:?} current={current:?}"
    );
    if bounds_center_delta > ROW_MICRO_MAX_ICON_CENTER_DELTA_PX
        || width_delta.abs() > ROW_MICRO_MAX_ICON_WIDTH_DELTA_PX
        || count_delta_ratio > ROW_MICRO_MAX_ICON_COUNT_DELTA_RATIO
    {
        return Err(format!(
            "{label} changed horizontally under Rust Runtime micro scroll step {step}: bounds_center_delta={bounds_center_delta:.3} centroid_delta={centroid_delta:.3} width_delta={width_delta} count_delta_ratio={count_delta_ratio:.4} previous={previous:?} current={current:?}"
        ));
    }
    Ok(())
}

fn assert_row_micro_picture_stable(
    robot: &cranpose::Robot,
    step: usize,
    previous: &RowMicroStabilitySample,
    current: &RowMicroStabilitySample,
    actual_delta_y: f32,
) {
    let output_size = row_micro_output_size(&previous.screenshot, previous.region);
    let before = normalize_screenshot_region(
        &previous.screenshot,
        previous.region.tuple(),
        output_size.0,
        output_size.1,
    )
    .expect("normalize previous Rust Runtime row");
    let after_region = Bounds {
        y: previous.region.y + actual_delta_y,
        ..previous.region
    };
    let after = normalize_screenshot_region(
        &current.screenshot,
        after_region.tuple(),
        output_size.0,
        output_size.1,
    )
    .expect("normalize current Rust Runtime row");
    let horizontal = best_horizontal_shift_for_followed_region(
        &before,
        &current.screenshot,
        previous.region,
        actual_delta_y,
        ROW_MICRO_HORIZONTAL_SEARCH_RADIUS_PX,
    );
    println!(
        "row_micro_horizontal_shift step={step} best_dx={} zero_score={} best_score={} improvement_ratio={:.4}",
        horizontal.best_dx,
        horizontal.zero_score,
        horizontal.best_score,
        horizontal.improvement_ratio(),
    );
    if horizontal.best_dx != 0
        && horizontal.improvement_ratio() > ROW_MICRO_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO
    {
        save_row_micro_failure(step, &before, &after);
        fail_with_robot(
            robot,
            &format!(
                "Rust Runtime row crop moved horizontally under micro scroll step {step}: best_dx={} zero_score={} best_score={} improvement_ratio={:.4}",
                horizontal.best_dx,
                horizontal.zero_score,
                horizontal.best_score,
                horizontal.improvement_ratio(),
            ),
        );
    }
    let stats = screenshot_difference_stats(&before, &after, ROW_MICRO_DIFF_TOLERANCE)
        .expect("row micro normalized screenshots should match size");
    println!(
        "row_micro_picture step={step} differing_pixels={} max_diff={}",
        stats.differing_pixels, stats.max_difference
    );
    if stats.differing_pixels > ROW_MICRO_MAX_DIFFERING_PIXELS
        || stats.max_difference > ROW_MICRO_MAX_PIXEL_DIFF
    {
        save_row_micro_failure(step, &before, &after);
        let first = stats
            .first_difference
            .as_ref()
            .expect("failing row micro diff should have first difference");
        fail_with_robot(
            robot,
            &format!(
                "Rust Runtime row picture changed horizontally under micro scroll step {step}: differing_pixels={} max_diff={} first_diff=({}, {}) before={:?} after={:?}",
                stats.differing_pixels,
                stats.max_difference,
                first.x,
                first.y,
                first.before,
                first.after
            ),
        );
    }
}

fn assert_workspace_strip_horizontal_stable(
    robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    previous: &WorkspaceStripStabilitySample,
    current: &WorkspaceStripStabilitySample,
    actual_delta_y: f32,
) {
    let previous_crop = normalize_workspace_strip(&previous.screenshot, previous.region);
    let horizontal = best_horizontal_shift_for_followed_region(
        &previous_crop,
        &current.screenshot,
        previous.region,
        actual_delta_y,
        WORKSPACE_STRIP_HORIZONTAL_SEARCH_RADIUS_PX,
    );
    println!(
        "workspace_strip_horizontal_shift step={step} best_dx={} zero_score={} best_score={} improvement_ratio={:.4}",
        horizontal.best_dx,
        horizontal.zero_score,
        horizontal.best_score,
        horizontal.improvement_ratio(),
    );
    if horizontal.best_dx != 0
        && horizontal.improvement_ratio() > WORKSPACE_STRIP_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO
    {
        save_workspace_strip_failure(
            step,
            &previous.screenshot,
            &current.screenshot,
            previous.region,
            actual_delta_y,
        );
        fail_with_robot(
            robot,
            &format!(
                "{target_text} workspace strip moved horizontally under micro scroll step {step}: best_dx={} zero_score={} best_score={} improvement_ratio={:.4}",
                horizontal.best_dx,
                horizontal.zero_score,
                horizontal.best_score,
                horizontal.improvement_ratio(),
            ),
        );
    }

    let fractional = best_fractional_horizontal_shift_for_followed_region(
        &previous_crop,
        &current.screenshot,
        previous.region,
        actual_delta_y,
        WORKSPACE_STRIP_FRACTIONAL_HORIZONTAL_SEARCH_RADIUS_PX,
        WORKSPACE_STRIP_FRACTIONAL_HORIZONTAL_STEP_PX,
    );
    println!(
        "workspace_strip_fractional_horizontal_shift step={step} best_dx={:.2} zero_score={} best_score={} improvement_ratio={:.4}",
        fractional.best_dx,
        fractional.zero_score,
        fractional.best_score,
        fractional.improvement_ratio(),
    );
    if fractional.best_dx.abs() > WORKSPACE_STRIP_FRACTIONAL_HORIZONTAL_MIN_ABS_SHIFT_PX
        && fractional.improvement_ratio()
            > WORKSPACE_STRIP_FRACTIONAL_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO
    {
        save_workspace_strip_failure(
            step,
            &previous.screenshot,
            &current.screenshot,
            previous.region,
            actual_delta_y,
        );
        fail_with_robot(
            robot,
            &format!(
                "{target_text} workspace strip moved by a fractional horizontal phase under micro scroll step {step}: best_dx={:.2} zero_score={} best_score={} improvement_ratio={:.4}",
                fractional.best_dx,
                fractional.zero_score,
                fractional.best_score,
                fractional.improvement_ratio(),
            ),
        );
    }
}

fn assert_workspace_strip_active_frame_matches_settled(
    robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    active: &WorkspaceStripStabilitySample,
    settled: &WorkspaceStripStabilitySample,
) {
    let target_dx = settled.target_bounds.x - active.target_bounds.x;
    let target_dy = settled.target_bounds.y - active.target_bounds.y;
    if target_dx.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
        || target_dy.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
    {
        save_workspace_strip_failure(
            step,
            &active.screenshot,
            &settled.screenshot,
            active.region,
            target_dy,
        );
        fail_with_robot(
            robot,
            &format!(
                "{target_text} active strip frame and settled strip frame do not describe the same scroll position at step {step}: dx={target_dx:.3} dy={target_dy:.3}"
            ),
        );
    }

    let active_crop = normalize_workspace_strip(&active.screenshot, active.region);
    let settled_crop = normalize_workspace_strip(&settled.screenshot, active.region);
    let stats = screenshot_difference_stats(
        &active_crop,
        &settled_crop,
        WORKSPACE_STRIP_ACTIVE_DIFF_TOLERANCE,
    )
    .expect("workspace strip active and settled crops should match size");
    let pixel_count = (active_crop.width as usize).saturating_mul(active_crop.height as usize);
    let differing_ratio = stats.differing_pixels as f32 / pixel_count.max(1) as f32;
    let active_edge = screenshot_edge_energy(&active_crop);
    let settled_edge = screenshot_edge_energy(&settled_crop);
    let min_edge = settled_edge * (1.0 - WORKSPACE_STRIP_ACTIVE_MAX_EDGE_LOSS_RATIO);
    let active_unique = screenshot_unique_rgb_count(&active_crop);
    let settled_unique = screenshot_unique_rgb_count(&settled_crop);
    let max_unique = settled_unique + WORKSPACE_STRIP_ACTIVE_MAX_EXTRA_COLORS;
    println!(
        "workspace_strip_active_settled step={step} target={target_text} differing_pixels={} differing_ratio={differing_ratio:.5} max_diff={} active_edge={active_edge:.3} settled_edge={settled_edge:.3} min_edge={min_edge:.3} active_unique={active_unique} settled_unique={settled_unique} max_unique={max_unique}",
        stats.differing_pixels,
        stats.max_difference,
    );

    let changed_too_much = stats.differing_pixels > WORKSPACE_STRIP_ACTIVE_MAX_DIFFERING_PIXELS
        && differing_ratio > WORKSPACE_STRIP_ACTIVE_MAX_DIFFERING_RATIO;
    let hard_pixel_delta = stats.max_difference > WORKSPACE_STRIP_ACTIVE_MAX_PIXEL_DIFF
        && stats.differing_pixels > WORKSPACE_STRIP_ACTIVE_MAX_DIFFERING_PIXELS / 2;
    let blurrier_than_settled = active_edge < min_edge || active_unique > max_unique;
    if changed_too_much || hard_pixel_delta || blurrier_than_settled {
        save_workspace_strip_failure(
            step,
            &active.screenshot,
            &settled.screenshot,
            active.region,
            0.0,
        );
        let first = stats
            .first_difference
            .as_ref()
            .expect("failing workspace strip active/settled diff should have first difference");
        fail_with_robot(
            robot,
            &format!(
                "{target_text} first active scroll strip frame differs from its settled frame at step {step}: differing_pixels={} differing_ratio={differing_ratio:.5} max_diff={} first_diff=({}, {}) before={:?} after={:?} active_edge={active_edge:.3} settled_edge={settled_edge:.3} min_edge={min_edge:.3} active_unique={active_unique} settled_unique={settled_unique} max_unique={max_unique}",
                stats.differing_pixels,
                stats.max_difference,
                first.x,
                first.y,
                first.before,
                first.after
            ),
        );
    }
}

fn assert_workspace_top_band_horizontal_stable(
    robot: &cranpose::Robot,
    target_text: &str,
    step: usize,
    previous: &WorkspaceTopBandStabilitySample,
    current: &WorkspaceTopBandStabilitySample,
    actual_delta_y: f32,
) {
    let previous_crop = normalize_workspace_region(&previous.screenshot, previous.region);
    let horizontal = best_horizontal_shift_for_followed_region(
        &previous_crop,
        &current.screenshot,
        previous.region,
        actual_delta_y,
        WORKSPACE_TOP_BAND_HORIZONTAL_SEARCH_RADIUS_PX,
    );
    println!(
        "workspace_top_band_horizontal_shift step={step} best_dx={} zero_score={} best_score={} improvement_ratio={:.4}",
        horizontal.best_dx,
        horizontal.zero_score,
        horizontal.best_score,
        horizontal.improvement_ratio(),
    );
    if horizontal.best_dx != 0
        && horizontal.improvement_ratio()
            > WORKSPACE_TOP_BAND_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO
    {
        save_workspace_top_band_failure(
            step,
            &previous.screenshot,
            &current.screenshot,
            previous.region,
            actual_delta_y,
        );
        fail_with_robot(
            robot,
            &format!(
                "{target_text} workspace top band moved horizontally under micro scroll step {step}: best_dx={} zero_score={} best_score={} improvement_ratio={:.4}",
                horizontal.best_dx,
                horizontal.zero_score,
                horizontal.best_score,
                horizontal.improvement_ratio(),
            ),
        );
    }

    let fractional = best_fractional_horizontal_shift_for_followed_region(
        &previous_crop,
        &current.screenshot,
        previous.region,
        actual_delta_y,
        WORKSPACE_TOP_BAND_FRACTIONAL_HORIZONTAL_SEARCH_RADIUS_PX,
        WORKSPACE_TOP_BAND_FRACTIONAL_HORIZONTAL_STEP_PX,
    );
    println!(
        "workspace_top_band_fractional_horizontal_shift step={step} best_dx={:.2} zero_score={} best_score={} improvement_ratio={:.4}",
        fractional.best_dx,
        fractional.zero_score,
        fractional.best_score,
        fractional.improvement_ratio(),
    );
    if fractional.best_dx.abs() > WORKSPACE_TOP_BAND_FRACTIONAL_HORIZONTAL_MIN_ABS_SHIFT_PX
        && fractional.improvement_ratio()
            > WORKSPACE_TOP_BAND_FRACTIONAL_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO
    {
        save_workspace_top_band_failure(
            step,
            &previous.screenshot,
            &current.screenshot,
            previous.region,
            actual_delta_y,
        );
        fail_with_robot(
            robot,
            &format!(
                "{target_text} workspace top band moved by a fractional horizontal phase under micro scroll step {step}: best_dx={:.2} zero_score={} best_score={} improvement_ratio={:.4}",
                fractional.best_dx,
                fractional.zero_score,
                fractional.best_score,
                fractional.improvement_ratio(),
            ),
        );
    }
}

fn normalize_workspace_strip(
    screenshot: &cranpose::RobotScreenshot,
    region: Bounds,
) -> cranpose::RobotScreenshot {
    normalize_workspace_region(screenshot, region)
}

fn normalize_workspace_region(
    screenshot: &cranpose::RobotScreenshot,
    region: Bounds,
) -> cranpose::RobotScreenshot {
    let output_size = row_micro_output_size(screenshot, region);
    normalize_screenshot_region(screenshot, region.tuple(), output_size.0, output_size.1)
        .expect("normalize workspace region")
}

fn assert_bottom_clear_screen_crop_stable(
    robot: &cranpose::Robot,
    step: usize,
    previous: &BottomClearStabilitySample,
    current: &BottomClearStabilitySample,
    actual_delta_y: f32,
) {
    let horizontal = bottom_clear_best_horizontal_shift(previous, current, actual_delta_y);
    println!(
        "bottom_clear_horizontal_shift step={step} best_dx={} zero_score={} best_score={} improvement_ratio={:.4}",
        horizontal.best_dx,
        horizontal.zero_score,
        horizontal.best_score,
        horizontal.improvement_ratio(),
    );
    if horizontal.best_dx != 0
        && horizontal.improvement_ratio() > BOTTOM_CLEAR_HORIZONTAL_SHIFT_MIN_IMPROVEMENT_RATIO
    {
        save_bottom_clear_failure(step, previous, current);
        fail_with_robot(
            robot,
            &format!(
                "bottom Clear fixed screen crop moved horizontally under 1px vertical scroll step {step}: best_dx={} zero_score={} best_score={} improvement_ratio={:.4}",
                horizontal.best_dx,
                horizontal.zero_score,
                horizontal.best_score,
                horizontal.improvement_ratio(),
            ),
        );
    }

    let current_region = Bounds {
        y: previous.fixed_region.y + actual_delta_y,
        ..previous.fixed_region
    };
    let output_size = (previous.fixed_crop.width, previous.fixed_crop.height);
    let current_following_crop = normalize_screenshot_region(
        &current.screenshot,
        current_region.tuple(),
        output_size.0,
        output_size.1,
    );

    let current_fixed_crop = if let Some(crop) = current_following_crop {
        crop
    } else {
        save_bottom_clear_failure(step, previous, current);
        fail_with_robot(
            robot,
            &format!(
                "bottom Clear current following fixed-screen crop is outside screenshot at step {step}: region=({:.1},{:.1},{:.1},{:.1})",
                current_region.x, current_region.y, current_region.width, current_region.height
            ),
        );
    };
    let stats = screenshot_difference_stats(
        &previous.fixed_crop,
        &current_fixed_crop,
        BOTTOM_CLEAR_CROP_DIFF_TOLERANCE,
    )
    .expect("bottom Clear screen crops should have equal size");
    println!(
        "bottom_clear_screen_crop step={step} differing_pixels={} max_diff={}",
        stats.differing_pixels, stats.max_difference
    );
    if stats.differing_pixels > BOTTOM_CLEAR_MAX_DIFFERING_PIXELS
        || stats.max_difference > BOTTOM_CLEAR_MAX_PIXEL_DIFF
    {
        save_bottom_clear_failure(step, previous, current);
        let first = stats
            .first_difference
            .as_ref()
            .expect("failing bottom Clear diff should have first difference");
        fail_with_robot(
            robot,
            &format!(
                "bottom Clear fixed screen crop changed under 1px scroll step {step}: differing_pixels={} max_diff={} first_diff=({}, {}) before={:?} after={:?}",
                stats.differing_pixels,
                stats.max_difference,
                first.x,
                first.y,
                first.before,
                first.after
            ),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HorizontalShiftMatch {
    best_dx: i32,
    zero_score: u64,
    best_score: u64,
}

impl HorizontalShiftMatch {
    fn improvement_ratio(self) -> f32 {
        if self.zero_score == 0 {
            0.0
        } else {
            (self.zero_score.saturating_sub(self.best_score)) as f32 / self.zero_score as f32
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FractionalHorizontalShiftMatch {
    best_dx: f32,
    zero_score: u64,
    best_score: u64,
}

impl FractionalHorizontalShiftMatch {
    fn improvement_ratio(self) -> f32 {
        if self.zero_score == 0 {
            0.0
        } else {
            (self.zero_score.saturating_sub(self.best_score)) as f32 / self.zero_score as f32
        }
    }
}

fn bottom_clear_best_horizontal_shift(
    previous: &BottomClearStabilitySample,
    current: &BottomClearStabilitySample,
    actual_delta_y: f32,
) -> HorizontalShiftMatch {
    best_horizontal_shift_for_followed_region(
        &previous.fixed_crop,
        &current.screenshot,
        previous.fixed_region,
        actual_delta_y,
        BOTTOM_CLEAR_HORIZONTAL_SEARCH_RADIUS_PX,
    )
}

fn best_horizontal_shift_for_followed_region(
    previous_crop: &cranpose::RobotScreenshot,
    current_screenshot: &cranpose::RobotScreenshot,
    previous_region: Bounds,
    actual_delta_y: f32,
    search_radius_px: i32,
) -> HorizontalShiftMatch {
    let output_size = (previous_crop.width, previous_crop.height);
    let mut zero_score = None;
    let mut best = None;

    for dx in -search_radius_px..=search_radius_px {
        let current_region = Bounds {
            x: previous_region.x + dx as f32,
            y: previous_region.y + actual_delta_y,
            ..previous_region
        };
        let Some(current_crop) = normalize_screenshot_region(
            current_screenshot,
            current_region.tuple(),
            output_size.0,
            output_size.1,
        ) else {
            continue;
        };
        let score = screenshot_abs_difference_score(previous_crop, &current_crop)
            .expect("horizontal-shift crops should have equal size");
        if dx == 0 {
            zero_score = Some(score);
        }
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((dx, score));
        }
    }

    let zero_score = zero_score.expect("zero horizontal shift crop should be inside screenshot");
    let (best_dx, best_score) = best.expect("horizontal shift search should have candidates");
    HorizontalShiftMatch {
        best_dx,
        zero_score,
        best_score,
    }
}

fn best_fractional_horizontal_shift_for_followed_region(
    previous_crop: &cranpose::RobotScreenshot,
    current_screenshot: &cranpose::RobotScreenshot,
    previous_region: Bounds,
    actual_delta_y: f32,
    search_radius_px: f32,
    step_px: f32,
) -> FractionalHorizontalShiftMatch {
    assert!(
        search_radius_px.is_finite() && search_radius_px >= 0.0,
        "fractional horizontal search radius must be non-negative"
    );
    assert!(
        step_px.is_finite() && step_px > 0.0,
        "fractional horizontal search step must be positive"
    );
    let output_size = (previous_crop.width, previous_crop.height);
    let step_count = (search_radius_px / step_px).round() as i32;
    let mut zero_score = None;
    let mut best = None;

    for step in -step_count..=step_count {
        let dx = step as f32 * step_px;
        let current_region = Bounds {
            x: previous_region.x + dx,
            y: previous_region.y + actual_delta_y,
            ..previous_region
        };
        let Some(current_crop) = normalize_screenshot_region(
            current_screenshot,
            current_region.tuple(),
            output_size.0,
            output_size.1,
        ) else {
            continue;
        };
        let score = screenshot_abs_difference_score(previous_crop, &current_crop)
            .expect("fractional horizontal-shift crops should have equal size");
        if dx.abs() < f32::EPSILON {
            zero_score = Some(score);
        }
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((dx, score));
        }
    }

    let zero_score =
        zero_score.expect("zero fractional horizontal shift crop should be inside screenshot");
    let (best_dx, best_score) =
        best.expect("fractional horizontal shift search should have candidates");
    FractionalHorizontalShiftMatch {
        best_dx,
        zero_score,
        best_score,
    }
}

fn screenshot_abs_difference_score(
    left: &cranpose::RobotScreenshot,
    right: &cranpose::RobotScreenshot,
) -> Option<u64> {
    if left.width != right.width || left.height != right.height {
        return None;
    }
    Some(
        left.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(right.pixels.as_chunks::<4>().0)
            .map(|(left, right)| {
                left.iter()
                    .zip(right)
                    .map(|(left, right)| left.abs_diff(*right) as u64)
                    .sum::<u64>()
            })
            .sum(),
    )
}

fn assert_bottom_clear_icon_profile_stable(
    robot: &cranpose::Robot,
    step: usize,
    previous: &BottomClearStabilitySample,
    current: &BottomClearStabilitySample,
) {
    let previous_profile = &previous.red_profile;
    let current_profile = &current.red_profile;
    let centroid_delta = (current_profile.centroid_x - previous_profile.centroid_x).abs();
    let mass_delta = current_profile
        .total_mass
        .abs_diff(previous_profile.total_mass);
    let mass_delta_ratio = mass_delta as f32 / previous_profile.total_mass.max(1) as f32;
    let (profile_l1_ratio, max_column_delta_ratio) =
        red_profile_delta_ratios(previous_profile, current_profile);
    let bounds_width_delta =
        current_profile.bounds.width() as i32 - previous_profile.bounds.width() as i32;
    let bounds_center_delta =
        (current_profile.bounds.center_x() - previous_profile.bounds.center_x()).abs();
    println!(
        "bottom_clear_icon_profile step={step} centroid_delta={centroid_delta:.4} bounds_center_delta={bounds_center_delta:.4} bounds_width_delta={bounds_width_delta} mass_delta_ratio={mass_delta_ratio:.4} profile_l1_ratio={profile_l1_ratio:.4} max_column_delta_ratio={max_column_delta_ratio:.4} previous={:?} current={:?}",
        previous_profile.bounds,
        current_profile.bounds
    );

    if centroid_delta > BOTTOM_CLEAR_MAX_RED_CENTROID_DELTA_PX
        || bounds_center_delta > BOTTOM_CLEAR_MAX_RED_BOUNDS_CENTER_DELTA_PX
        || bounds_width_delta.abs() > BOTTOM_CLEAR_MAX_RED_BOUNDS_WIDTH_DELTA_PX
        || mass_delta_ratio > BOTTOM_CLEAR_MAX_RED_MASS_DELTA_RATIO
        || profile_l1_ratio > BOTTOM_CLEAR_MAX_RED_PROFILE_L1_RATIO
        || max_column_delta_ratio > BOTTOM_CLEAR_MAX_RED_COLUMN_DELTA_RATIO
    {
        save_bottom_clear_failure(step, previous, current);
        fail_with_robot(
            robot,
            &format!(
                "bottom Clear red icon changed horizontally under 1px scroll step {step}: centroid_delta={centroid_delta:.4} bounds_center_delta={bounds_center_delta:.4} bounds_width_delta={bounds_width_delta} mass_delta_ratio={mass_delta_ratio:.4} profile_l1_ratio={profile_l1_ratio:.4} max_column_delta_ratio={max_column_delta_ratio:.4}"
            ),
        );
    }
}

fn assert_bottom_clear_active_frame_crispness_matches_settled(
    robot: &cranpose::Robot,
    step: usize,
    active: &BottomClearStabilitySample,
    settled: &BottomClearStabilitySample,
) {
    let clear_dx = settled.clear_bounds.x - active.clear_bounds.x;
    let clear_dy = settled.clear_bounds.y - active.clear_bounds.y;
    if clear_dx.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
        || clear_dy.abs() > WORKSPACE_SURFACE_GEOMETRY_EPSILON
    {
        fail_with_robot(
            robot,
            &format!(
                "bottom Clear active frame and settled frame do not describe the same scroll position at step {step}: dx={clear_dx:.3} dy={clear_dy:.3}"
            ),
        );
    }

    let min_edge_energy =
        settled.clear_edge_energy * (1.0 - BOTTOM_CLEAR_ACTIVE_CRISPNESS_MAX_EDGE_LOSS_RATIO);
    let max_unique_rgb =
        settled.clear_unique_rgb_count + BOTTOM_CLEAR_ACTIVE_CRISPNESS_MAX_EXTRA_COLORS;
    println!(
        "bottom_clear_active_crispness step={step} active_edge={:.3} settled_edge={:.3} min_edge={:.3} active_unique={} settled_unique={} max_unique={}",
        active.clear_edge_energy,
        settled.clear_edge_energy,
        min_edge_energy,
        active.clear_unique_rgb_count,
        settled.clear_unique_rgb_count,
        max_unique_rgb,
    );
    if active.clear_edge_energy < min_edge_energy || active.clear_unique_rgb_count > max_unique_rgb
    {
        save_bottom_clear_failure(step, active, settled);
        fail_with_robot(
            robot,
            &format!(
                "bottom Clear button is blurrier in the first active scroll frame than after settling at step {step}: active_edge={:.3} settled_edge={:.3} min_edge={:.3} active_unique={} settled_unique={} max_unique={}",
                active.clear_edge_energy,
                settled.clear_edge_energy,
                min_edge_energy,
                active.clear_unique_rgb_count,
                settled.clear_unique_rgb_count,
                max_unique_rgb
            ),
        );
    }
}

fn red_profile_delta_ratios(
    previous: &RedIconHorizontalProfile,
    current: &RedIconHorizontalProfile,
) -> (f32, f32) {
    let max_len = previous.column_mass.len().max(current.column_mass.len());
    let mut l1 = 0u64;
    let mut max_delta = 0u32;
    for index in 0..max_len {
        let before = previous.column_mass.get(index).copied().unwrap_or(0);
        let after = current.column_mass.get(index).copied().unwrap_or(0);
        let delta = before.abs_diff(after);
        l1 += delta as u64;
        max_delta = max_delta.max(delta);
    }
    (
        l1 as f32 / previous.total_mass.max(1) as f32,
        max_delta as f32 / previous.max_column_mass.max(1) as f32,
    )
}

fn row_micro_feature_bounds(
    screenshot: &cranpose::RobotScreenshot,
    region: Bounds,
    predicate: fn([u8; 4]) -> bool,
    label: &str,
) -> PixelFeatureBounds {
    let crop = crop_screenshot_logical(screenshot, region.x, region.y, region.width, region.height)
        .unwrap_or_else(|| panic!("{label} feature crop is outside screenshot"));
    feature_pixel_bounds(&crop, predicate)
        .unwrap_or_else(|| panic!("{label} feature pixels not found in crop"))
}

fn feature_bounds_in_logical_region(
    screenshot: &cranpose::RobotScreenshot,
    region: Bounds,
    predicate: fn([u8; 4]) -> bool,
    label: &str,
) -> PixelFeatureBounds {
    let crop = crop_screenshot_logical(screenshot, region.x, region.y, region.width, region.height)
        .unwrap_or_else(|| panic!("{label} feature crop is outside screenshot"));
    feature_pixel_bounds(&crop, predicate)
        .unwrap_or_else(|| panic!("{label} feature pixels not found in crop"))
}

fn feature_pixel_bounds(
    screenshot: &cranpose::RobotScreenshot,
    predicate: fn([u8; 4]) -> bool,
) -> Option<PixelFeatureBounds> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut count = 0usize;
    let mut sum_x = 0u64;
    let mut sum_y = 0u64;
    for y in 0..screenshot.height {
        for x in 0..screenshot.width {
            let index = ((y * screenshot.width + x) * 4) as usize;
            let pixel = [
                screenshot.pixels[index],
                screenshot.pixels[index + 1],
                screenshot.pixels[index + 2],
                screenshot.pixels[index + 3],
            ];
            if predicate(pixel) {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                count += 1;
                sum_x += x as u64;
                sum_y += y as u64;
            }
        }
    }
    (count > 0).then_some(PixelFeatureBounds {
        min_x,
        max_x,
        min_y,
        max_y,
        count,
        sum_x,
        sum_y,
    })
}

fn red_icon_horizontal_profile(
    screenshot: &cranpose::RobotScreenshot,
) -> Option<RedIconHorizontalProfile> {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    let mut column_mass = vec![0u32; width];
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut count = 0usize;
    let mut sum_x = 0u64;
    let mut sum_y = 0u64;
    let mut weighted_sum_x = 0u64;
    let mut total_mass = 0u64;

    for y in 0..height {
        for (x, column) in column_mass.iter_mut().enumerate().take(width) {
            let index = (y * width + x) * 4;
            let pixel = [
                screenshot.pixels[index],
                screenshot.pixels[index + 1],
                screenshot.pixels[index + 2],
                screenshot.pixels[index + 3],
            ];
            let strength = red_icon_strength(pixel);
            if strength == 0 {
                continue;
            }
            min_x = min_x.min(x as u32);
            min_y = min_y.min(y as u32);
            max_x = max_x.max(x as u32);
            max_y = max_y.max(y as u32);
            count += 1;
            sum_x += x as u64;
            sum_y += y as u64;
            weighted_sum_x += x as u64 * strength as u64;
            total_mass += strength as u64;
            *column = column.saturating_add(strength);
        }
    }

    if count == 0 || total_mass == 0 {
        return None;
    }
    let max_column_mass = column_mass.iter().copied().max().unwrap_or(0);
    Some(RedIconHorizontalProfile {
        bounds: PixelFeatureBounds {
            min_x,
            max_x,
            min_y,
            max_y,
            count,
            sum_x,
            sum_y,
        },
        column_mass,
        total_mass,
        centroid_x: weighted_sum_x as f32 / total_mass as f32,
        max_column_mass,
    })
}

fn red_icon_strength(pixel: [u8; 4]) -> u32 {
    let red = pixel[0] as i32;
    let green = pixel[1] as i32;
    let blue = pixel[2] as i32;
    let excess = red - green.max(blue);
    if red > 125 && excess > 24 {
        excess as u32
    } else {
        0
    }
}

fn red_clear_icon_pixel(pixel: [u8; 4]) -> bool {
    pixel[0] > 170
        && pixel[1] < 150
        && pixel[2] < 160
        && pixel[0] > pixel[1].saturating_add(45)
        && pixel[0] > pixel[2].saturating_add(45)
}

fn blue_field_icon_pixel(pixel: [u8; 4]) -> bool {
    pixel[2] > 170 && pixel[0] < 130 && pixel[1] < 190 && pixel[2] > pixel[0].saturating_add(45)
}

fn blue_label_text_pixel(pixel: [u8; 4]) -> bool {
    pixel[0] < 120
        && pixel[1] < 155
        && pixel[2] > 95
        && pixel[1] > pixel[0].saturating_add(8)
        && pixel[2] > pixel[0].saturating_add(20)
}

fn row_micro_output_size(screenshot: &cranpose::RobotScreenshot, region: Bounds) -> (u32, u32) {
    let scale_x = screenshot.width as f32 / screenshot.logical_width.max(1.0);
    let scale_y = screenshot.height as f32 / screenshot.logical_height.max(1.0);
    (
        (region.width * scale_x).ceil().max(1.0) as u32,
        (region.height * scale_y).ceil().max(1.0) as u32,
    )
}

fn leetcodedaily_diagnostic_dir(name: &str) -> PathBuf {
    let dir = output_paths::diagnostic_path(name).join(std::process::id().to_string());
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", dir.display()));
    dir
}

fn save_row_micro_failure(
    step: usize,
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
) {
    let dir = leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-row-micro-stability");
    let before_path = dir.join(format!("step{step:02}_before.png"));
    let after_path = dir.join(format!("step{step:02}_after.png"));
    save_robot_screenshot_png(&before_path, before);
    save_robot_screenshot_png(&after_path, after);
    eprintln!(
        "row_micro_failure_paths step={step} before={} after={}",
        before_path.display(),
        after_path.display()
    );
}

fn save_workspace_strip_failure(
    step: usize,
    previous: &cranpose::RobotScreenshot,
    current: &cranpose::RobotScreenshot,
    previous_region: Bounds,
    actual_delta_y: f32,
) {
    let before = normalize_workspace_strip(previous, previous_region);
    let current_region = Bounds {
        y: previous_region.y + actual_delta_y,
        ..previous_region
    };
    let after = normalize_workspace_strip(current, current_region);

    let dir = leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-workspace-strip-stability");
    let before_path = dir.join(format!("step{step:02}_before.png"));
    let after_path = dir.join(format!("step{step:02}_after.png"));
    save_robot_screenshot_png(&before_path, &before);
    save_robot_screenshot_png(&after_path, &after);
    eprintln!(
        "workspace_strip_failure_paths step={step} before={} after={}",
        before_path.display(),
        after_path.display()
    );
}

fn save_workspace_top_band_failure(
    step: usize,
    previous: &cranpose::RobotScreenshot,
    current: &cranpose::RobotScreenshot,
    previous_region: Bounds,
    actual_delta_y: f32,
) {
    let before = normalize_workspace_region(previous, previous_region);
    let current_region = Bounds {
        y: previous_region.y + actual_delta_y,
        ..previous_region
    };
    let after = normalize_workspace_region(current, current_region);

    let dir = leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-workspace-top-band-stability");
    let before_path = dir.join(format!("step{step:02}_before.png"));
    let after_path = dir.join(format!("step{step:02}_after.png"));
    save_robot_screenshot_png(&before_path, &before);
    save_robot_screenshot_png(&after_path, &after);
    eprintln!(
        "workspace_top_band_failure_paths step={step} before={} after={}",
        before_path.display(),
        after_path.display()
    );
}

fn save_workspace_top_band_sample_if_requested(
    step: usize,
    sample: &WorkspaceTopBandStabilitySample,
) {
    if std::env::var_os(TOP_BAND_CAPTURE_ENV).is_none() {
        return;
    }
    let full = crop_screenshot_logical(
        &sample.screenshot,
        0.0,
        0.0,
        sample.screenshot.logical_width,
        sample.screenshot.logical_height,
    )
    .expect("top-band full screenshot crop should be inside screenshot");
    let band = normalize_workspace_region(&sample.screenshot, sample.region);

    let dir = leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-workspace-top-band-samples");
    let full_path = dir.join(format!("step{step:02}_full.png"));
    let band_path = dir.join(format!("step{step:02}_band.png"));
    save_robot_screenshot_png(&full_path, &full);
    save_robot_screenshot_png(&band_path, &band);
    eprintln!(
        "workspace_top_band_sample_paths step={step} full={} band={}",
        full_path.display(),
        band_path.display()
    );
}

fn save_row_micro_feature_failure(
    step: usize,
    label: &str,
    previous: &cranpose::RobotScreenshot,
    current: &cranpose::RobotScreenshot,
    previous_region: Bounds,
    current_region: Bounds,
) {
    let Some(before) = crop_screenshot_logical(
        previous,
        previous_region.x,
        previous_region.y,
        previous_region.width,
        previous_region.height,
    ) else {
        return;
    };
    let Some(after) = crop_screenshot_logical(
        current,
        current_region.x,
        current_region.y,
        current_region.width,
        current_region.height,
    ) else {
        return;
    };

    let dir = leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-row-micro-stability");
    let before_path = dir.join(format!("step{step:02}_{label}_before.png"));
    let after_path = dir.join(format!("step{step:02}_{label}_after.png"));
    save_robot_screenshot_png(&before_path, &before);
    save_robot_screenshot_png(&after_path, &after);
    eprintln!(
        "row_micro_feature_failure_paths step={step} label={label} before={} after={}",
        before_path.display(),
        after_path.display()
    );
}

fn bottom_clear_capture_dir() -> PathBuf {
    leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-bottom-clear-stability")
}

fn save_bottom_clear_sample_if_requested(step: usize, sample: &BottomClearStabilitySample) {
    if std::env::var_os(BOTTOM_CLEAR_CAPTURE_ENV).is_none() {
        return;
    }
    let dir = bottom_clear_capture_dir();
    let fixed_path = dir.join(format!("step{step:02}_fixed-bottom.png"));
    let clear_path = dir.join(format!("step{step:02}_clear-button.png"));
    save_robot_screenshot_png(&fixed_path, &sample.fixed_crop);
    save_robot_screenshot_png(&clear_path, &sample.clear_crop);
    println!(
        "bottom_clear_capture step={step} fixed={} clear={}",
        fixed_path.display(),
        clear_path.display()
    );
}

fn save_bottom_clear_failure(
    step: usize,
    previous: &BottomClearStabilitySample,
    current: &BottomClearStabilitySample,
) {
    let dir = bottom_clear_capture_dir();
    let previous_fixed_path = dir.join(format!("step{step:02}_previous_fixed-bottom.png"));
    let current_fixed_path = dir.join(format!("step{step:02}_current_fixed-bottom.png"));
    let previous_clear_path = dir.join(format!("step{step:02}_previous_clear-button.png"));
    let current_clear_path = dir.join(format!("step{step:02}_current_clear-button.png"));
    save_robot_screenshot_png(&previous_fixed_path, &previous.fixed_crop);
    save_robot_screenshot_png(&current_fixed_path, &current.fixed_crop);
    save_robot_screenshot_png(&previous_clear_path, &previous.clear_crop);
    save_robot_screenshot_png(&current_clear_path, &current.clear_crop);
    eprintln!(
        "bottom_clear_failure_paths step={step} previous_fixed={} current_fixed={} previous_clear={} current_clear={}",
        previous_fixed_path.display(),
        current_fixed_path.display(),
        previous_clear_path.display(),
        current_clear_path.display()
    );
}

fn capture_workspace_button_quality(
    robot: &cranpose::Robot,
    anchor_text: &str,
    label: &'static str,
    phase: &str,
) -> ButtonQualitySample {
    let viewport = stable_workspace_content_bounds(robot);
    let bounds = workspace_related_button_bounds_after_anchor(robot, anchor_text, label)
        .filter(|bounds| viewport.inset_contains(*bounds, BUTTON_QUALITY_SAFE_EDGE))
        .unwrap_or_else(|| {
            let all_bounds = workspace_button_bounds_exact(robot, label);
            fail_with_robot(
                robot,
                &format!(
                    "no visible workspace '{label}' button related to '{anchor_text}' found for quality phase {phase}; viewport=({:.1},{:.1},{:.1},{:.1}) all_bounds={:?}",
                    viewport.x, viewport.y, viewport.width, viewport.height, all_bounds,
                ),
            )
        });
    let screenshot = capture_visible_window_screenshot("button-quality");
    let crop_bounds = bounds
        .padded(BUTTON_QUALITY_SAFE_EDGE)
        .clipped_to(Bounds {
            x: 0.0,
            y: 0.0,
            width: screenshot.logical_width,
            height: screenshot.logical_height,
        })
        .unwrap_or_else(|| {
            fail_with_robot(
                robot,
                &format!(
                    "button quality crop outside screenshot: label={label} phase={phase} bounds=({:.1},{:.1},{:.1},{:.1}) screenshot={}x{} logical=({:.1},{:.1})",
                    bounds.x,
                    bounds.y,
                    bounds.width,
                    bounds.height,
                    screenshot.width,
                    screenshot.height,
                    screenshot.logical_width,
                    screenshot.logical_height,
                ),
            )
        });
    let crop = crop_screenshot_logical(
        &screenshot,
        crop_bounds.x,
        crop_bounds.y,
        crop_bounds.width,
        crop_bounds.height,
    )
    .expect("crop visible button quality region");
    let edge_energy = screenshot_edge_energy(&crop);
    let unique_rgb_count = screenshot_unique_rgb_count(&crop);
    let crop_path = save_button_quality_crop_if_requested(label, phase, &crop);
    println!(
        "button_quality label={label} phase={phase} bounds=({:.1},{:.1},{:.1},{:.1}) crop={}x{} edge_energy={edge_energy:.3} unique_rgb={unique_rgb_count} crop_path={}",
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        crop.width,
        crop.height,
        crop_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_string())
    );
    ButtonQualitySample {
        label,
        phase: phase.to_string(),
        bounds,
        edge_energy,
        unique_rgb_count,
        crop_path,
    }
}

fn capture_button_reference_sample(
    robot: &cranpose::Robot,
    tag: &'static str,
) -> ButtonReferenceSample {
    let semantics = robot
        .get_semantics()
        .unwrap_or_else(|err| fail_with_robot(robot, &format!("failed to get semantics: {err}")));
    let bounds = find_semantic_text_bounds(&semantics, tag).unwrap_or_else(|| {
        fail_with_robot(
            robot,
            &format!("button reference fixture not found: tag={tag}"),
        )
    });
    let screenshot = capture_visible_window_screenshot("button-reference");
    let crop_bounds = bounds
        .clipped_to(Bounds {
            x: 0.0,
            y: 0.0,
            width: screenshot.logical_width,
            height: screenshot.logical_height,
        })
        .unwrap_or_else(|| {
            fail_with_robot(
                robot,
                &format!(
                    "button reference crop outside screenshot: tag={tag} bounds=({:.1},{:.1},{:.1},{:.1}) screenshot={}x{} logical=({:.1},{:.1})",
                    bounds.x,
                    bounds.y,
                    bounds.width,
                    bounds.height,
                    screenshot.width,
                    screenshot.height,
                    screenshot.logical_width,
                    screenshot.logical_height,
                ),
            )
        });
    let crop = crop_screenshot_logical(
        &screenshot,
        crop_bounds.x,
        crop_bounds.y,
        crop_bounds.width,
        crop_bounds.height,
    )
    .expect("crop visible button reference region");
    let edge_energy = screenshot_edge_energy(&crop);
    let red_edge_energy = screenshot_red_strength_edge_energy(&crop);
    let unique_rgb_count = screenshot_unique_rgb_count(&crop);
    let red_profile = red_icon_horizontal_profile(&crop).unwrap_or_else(|| {
        let path = save_button_reference_crop("missing-red", tag, &crop);
        fail_with_robot(
            robot,
            &format!(
                "button reference red Clear icon pixels not found: tag={tag} bounds=({:.1},{:.1},{:.1},{:.1}) crop={}",
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                path.display(),
            ),
        )
    });
    let crop_path = save_button_reference_crop_if_requested(tag, &crop);
    println!(
        "button_reference tag={tag} bounds=({:.1},{:.1},{:.1},{:.1}) crop={}x{} edge_energy={edge_energy:.3} red_edge_energy={red_edge_energy:.3} unique_rgb={unique_rgb_count} red_width={} red_count={} red_max_column={} crop_path={}",
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        crop.width,
        crop.height,
        red_profile.bounds.width(),
        red_profile.bounds.count,
        red_profile.max_column_mass,
        crop_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_string())
    );
    ButtonReferenceSample {
        tag,
        bounds,
        edge_energy,
        red_edge_energy,
        unique_rgb_count,
        red_profile,
        crop,
        crop_path,
    }
}

fn assert_button_reference_matches_outside(robot: &cranpose::Robot) {
    log_top_isolated_layer_render_stats(robot, "button_reference", 0);
    let outside = capture_button_reference_sample(robot, BUTTON_REFERENCE_OUTSIDE_TAG);
    let inside = capture_button_reference_sample(robot, BUTTON_REFERENCE_INSIDE_TAG);
    let stats =
        screenshot_difference_stats(&outside.crop, &inside.crop, BUTTON_REFERENCE_DIFF_TOLERANCE)
            .expect("button reference crops should have the same size");
    let differing_ratio =
        stats.differing_pixels as f32 / (outside.crop.width * outside.crop.height).max(1) as f32;
    let min_edge_energy = outside.edge_energy * (1.0 - BUTTON_REFERENCE_MAX_EDGE_LOSS_RATIO);
    let min_red_edge_energy =
        outside.red_edge_energy * (1.0 - BUTTON_REFERENCE_MAX_RED_EDGE_LOSS_RATIO);
    let max_unique_rgb = outside.unique_rgb_count + BUTTON_REFERENCE_MAX_EXTRA_COLORS;
    let red_width_delta =
        inside.red_profile.bounds.width() as i32 - outside.red_profile.bounds.width() as i32;
    println!(
        "button_reference_compare differing_pixels={} differing_ratio={differing_ratio:.5} max_diff={} outside_bounds=({:.1},{:.1},{:.1},{:.1}) inside_bounds=({:.1},{:.1},{:.1},{:.1}) outside_edge={:.3} inside_edge={:.3} min_edge={min_edge_energy:.3} outside_red_edge={:.3} inside_red_edge={:.3} min_red_edge={min_red_edge_energy:.3} outside_unique={} inside_unique={} max_unique={} red_width_delta={red_width_delta}",
        stats.differing_pixels,
        stats.max_difference,
        outside.bounds.x,
        outside.bounds.y,
        outside.bounds.width,
        outside.bounds.height,
        inside.bounds.x,
        inside.bounds.y,
        inside.bounds.width,
        inside.bounds.height,
        outside.edge_energy,
        inside.edge_energy,
        outside.red_edge_energy,
        inside.red_edge_energy,
        outside.unique_rgb_count,
        inside.unique_rgb_count,
        max_unique_rgb,
    );

    let loses_global_edges = inside.edge_energy < min_edge_energy;
    let loses_red_edges = inside.red_edge_energy < min_red_edge_energy;
    let gains_interpolation_colors =
        inside.unique_rgb_count > max_unique_rgb && inside.edge_energy < outside.edge_energy;
    let differs_too_much = differing_ratio > BUTTON_REFERENCE_MAX_DIFFERING_RATIO;
    if loses_global_edges || loses_red_edges || gains_interpolation_colors || differs_too_much {
        let (outside_path, inside_path) =
            save_button_reference_failure("outside-vs-inside", &outside, &inside);
        fail_with_robot(
            robot,
            &format!(
                "workspace offscreen button is blurrier than the direct reference: differing_ratio={differing_ratio:.5} max_diff={} outside_edge={:.3} inside_edge={:.3} outside_red_edge={:.3} inside_red_edge={:.3} outside_unique={} inside_unique={} red_width_delta={red_width_delta} outside_crop={} inside_crop={}",
                stats.max_difference,
                outside.edge_energy,
                inside.edge_energy,
                outside.red_edge_energy,
                inside.red_edge_energy,
                outside.unique_rgb_count,
                inside.unique_rgb_count,
                outside_path.display(),
                inside_path.display(),
            ),
        );
    }
}

fn workspace_related_button_bounds_after_anchor(
    robot: &cranpose::Robot,
    anchor_text: &str,
    button_label: &str,
) -> Option<Bounds> {
    let anchor_bounds = workspace_text_bounds_exact(robot, anchor_text)?;
    workspace_button_bounds_exact(robot, button_label)
        .into_iter()
        .filter(|candidate| candidate.center_y() >= anchor_bounds.center_y() - 8.0)
        .min_by(|left, right| {
            let left_score = left.center_y() - anchor_bounds.center_y();
            let right_score = right.center_y() - anchor_bounds.center_y();
            left_score.total_cmp(&right_score)
        })
}

fn assert_button_quality_matches_baseline(
    baseline: &ButtonQualitySample,
    sample: &ButtonQualitySample,
) {
    let min_edge_energy = baseline.edge_energy * (1.0 - BUTTON_QUALITY_MAX_EDGE_LOSS_RATIO);
    let max_unique_rgb = baseline.unique_rgb_count + BUTTON_QUALITY_MAX_EXTRA_COLORS;
    let color_growth_with_edge_loss =
        sample.unique_rgb_count > max_unique_rgb && sample.edge_energy < baseline.edge_energy;
    if sample.edge_energy < min_edge_energy || color_growth_with_edge_loss {
        eprintln!(
            "FATAL: deep scrolled {label} button lost crispness: baseline_phase={} sample_phase={} baseline_bounds=({:.1},{:.1},{:.1},{:.1}) sample_bounds=({:.1},{:.1},{:.1},{:.1}) baseline_edge={:.3} sample_edge={:.3} min_edge={:.3} baseline_unique={} sample_unique={} max_unique={} baseline_crop={} sample_crop={}",
            baseline.phase,
            sample.phase,
            baseline.bounds.x,
            baseline.bounds.y,
            baseline.bounds.width,
            baseline.bounds.height,
            sample.bounds.x,
            sample.bounds.y,
            sample.bounds.width,
            sample.bounds.height,
            baseline.edge_energy,
            sample.edge_energy,
            min_edge_energy,
            baseline.unique_rgb_count,
            sample.unique_rgb_count,
            max_unique_rgb,
            baseline
                .crop_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string()),
            sample
                .crop_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string()),
            label = baseline.label,
        );
        std::process::exit(1);
    }
}

fn workspace_viewport_bounds(robot: &cranpose::Robot) -> Bounds {
    let window = stable_window_bounds();
    let (window_width, window_height) = app_window_size();
    let tag_bounds = robot
        .get_semantics()
        .ok()
        .and_then(|semantics| find_semantic_text_bounds(&semantics, VIEWPORT_TAG));
    if let Some(bounds) = tag_bounds {
        if bounds.x >= -1.0
            && bounds.y >= -1.0
            && bounds.x + bounds.width <= window_width as f32 + 1.0
            && bounds.width >= window_width as f32 * 0.5
            && bounds.height >= window_height as f32 * 0.25
        {
            if let Some(clipped) = bounds.clipped_to(window) {
                return clipped;
            }
        }
    }

    Bounds {
        x: window.x,
        y: WORKSPACE_VIEWPORT_FALLBACK_TOP,
        width: window.width,
        height: (window.height - WORKSPACE_VIEWPORT_FALLBACK_TOP).max(0.0),
    }
}

fn find_semantic_text_bounds(
    semantics: &[cranpose::SemanticElement],
    text: &str,
) -> Option<Bounds> {
    semantics
        .iter()
        .find_map(|element| find_semantic_text_bounds_in(element, text))
}

fn find_semantic_text_bounds_in(element: &cranpose::SemanticElement, text: &str) -> Option<Bounds> {
    if element.text.as_deref() == Some(text) {
        return Some(Bounds {
            x: element.bounds.x,
            y: element.bounds.y,
            width: element.bounds.width,
            height: element.bounds.height,
        });
    }
    element
        .children
        .iter()
        .find_map(|child| find_semantic_text_bounds_in(child, text))
}

fn workspace_button_bounds_exact(robot: &cranpose::Robot, label: &str) -> Vec<Bounds> {
    let semantics = robot
        .get_semantics()
        .unwrap_or_else(|err| fail_with_robot(robot, &format!("failed to get semantics: {err}")));
    let mut bounds = Vec::new();
    for root in &semantics {
        collect_button_bounds_exact(root, label, &mut bounds);
    }
    bounds
}

fn collect_button_bounds_exact(
    element: &cranpose::SemanticElement,
    label: &str,
    out: &mut Vec<Bounds>,
) -> bool {
    let mut matching_clickable_descendant = false;
    for child in &element.children {
        matching_clickable_descendant |= collect_button_bounds_exact(child, label, out);
    }

    let element_bounds = Bounds {
        x: element.bounds.x,
        y: element.bounds.y,
        width: element.bounds.width,
        height: element.bounds.height,
    };
    let matches = element.clickable
        && semantic_subtree_has_text_inside_bounds(element, label, element_bounds);
    if matches && !matching_clickable_descendant {
        out.push(element_bounds);
    }
    matches || matching_clickable_descendant
}

fn workspace_text_bounds_exact(robot: &cranpose::Robot, text: &str) -> Option<Bounds> {
    let viewport = workspace_viewport_bounds(robot);
    let semantics = robot.get_semantics().ok()?;
    let mut bounds = Vec::new();
    for root in &semantics {
        collect_text_bounds_exact(root, text, &mut bounds);
    }
    bounds
        .into_iter()
        .filter(|candidate| viewport.overlaps_x(*candidate))
        .min_by(|left, right| {
            let left_distance = if left.center_y() < viewport.y {
                viewport.y - left.center_y()
            } else if left.center_y() > viewport.y + viewport.height {
                left.center_y() - (viewport.y + viewport.height)
            } else {
                0.0
            };
            let right_distance = if right.center_y() < viewport.y {
                viewport.y - right.center_y()
            } else if right.center_y() > viewport.y + viewport.height {
                right.center_y() - (viewport.y + viewport.height)
            } else {
                0.0
            };
            left_distance.total_cmp(&right_distance)
        })
}

fn collect_text_bounds_exact(
    element: &cranpose::SemanticElement,
    text: &str,
    out: &mut Vec<Bounds>,
) {
    if element.text.as_deref() == Some(text) {
        out.push(Bounds {
            x: element.bounds.x,
            y: element.bounds.y,
            width: element.bounds.width,
            height: element.bounds.height,
        });
    }
    for child in &element.children {
        collect_text_bounds_exact(child, text, out);
    }
}

fn semantic_subtree_has_text_inside_bounds(
    element: &cranpose::SemanticElement,
    text: &str,
    bounds: Bounds,
) -> bool {
    if element.text.as_deref() == Some(text) {
        let text_bounds = Bounds {
            x: element.bounds.x,
            y: element.bounds.y,
            width: element.bounds.width,
            height: element.bounds.height,
        };
        return bounds.inset_contains(text_bounds, -2.0);
    }
    element
        .children
        .iter()
        .any(|child| semantic_subtree_has_text_inside_bounds(child, text, bounds))
}

fn screenshot_luma(pixel: &[u8]) -> i32 {
    (77 * pixel[0] as i32 + 150 * pixel[1] as i32 + 29 * pixel[2] as i32) / 256
}

fn screenshot_region_edge_energy(
    screenshot: &cranpose::RobotScreenshot,
    region: Bounds,
    label: &str,
) -> f32 {
    let crop = crop_screenshot_logical(
        screenshot,
        region.x,
        region.y,
        region.width.max(1.0),
        region.height.max(1.0),
    )
    .unwrap_or_else(|| panic!("{label} region is outside screenshot"));
    screenshot_edge_energy(&crop)
}

fn screenshot_region_unique_rgb_count(
    screenshot: &cranpose::RobotScreenshot,
    region: Bounds,
    label: &str,
) -> usize {
    let crop = crop_screenshot_logical(
        screenshot,
        region.x,
        region.y,
        region.width.max(1.0),
        region.height.max(1.0),
    )
    .unwrap_or_else(|| panic!("{label} region is outside screenshot"));
    screenshot_unique_rgb_count(&crop)
}

fn screenshot_edge_energy(screenshot: &cranpose::RobotScreenshot) -> f32 {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    let mut total = 0u64;
    let mut samples = 0u64;
    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) * 4;
            let current = screenshot_luma(&screenshot.pixels[index..index + 4]);
            if x + 1 < width {
                let right = (y * width + x + 1) * 4;
                total += (current - screenshot_luma(&screenshot.pixels[right..right + 4]))
                    .unsigned_abs() as u64;
                samples += 1;
            }
            if y + 1 < height {
                let down = ((y + 1) * width + x) * 4;
                total += (current - screenshot_luma(&screenshot.pixels[down..down + 4]))
                    .unsigned_abs() as u64;
                samples += 1;
            }
        }
    }
    if samples == 0 {
        0.0
    } else {
        total as f32 / samples as f32
    }
}

fn screenshot_red_strength_edge_energy(screenshot: &cranpose::RobotScreenshot) -> f32 {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    let mut total_variation = 0u64;
    let mut total_mass = 0u64;
    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) * 4;
            let current = red_icon_strength([
                screenshot.pixels[index],
                screenshot.pixels[index + 1],
                screenshot.pixels[index + 2],
                screenshot.pixels[index + 3],
            ]);
            total_mass += current as u64;
            if x + 1 < width {
                let right = (y * width + x + 1) * 4;
                let next = red_icon_strength([
                    screenshot.pixels[right],
                    screenshot.pixels[right + 1],
                    screenshot.pixels[right + 2],
                    screenshot.pixels[right + 3],
                ]);
                total_variation += current.abs_diff(next) as u64;
            }
            if y + 1 < height {
                let down = ((y + 1) * width + x) * 4;
                let next = red_icon_strength([
                    screenshot.pixels[down],
                    screenshot.pixels[down + 1],
                    screenshot.pixels[down + 2],
                    screenshot.pixels[down + 3],
                ]);
                total_variation += current.abs_diff(next) as u64;
            }
        }
    }
    if total_mass == 0 {
        0.0
    } else {
        total_variation as f32 / total_mass as f32
    }
}

fn screenshot_unique_rgb_count(screenshot: &cranpose::RobotScreenshot) -> usize {
    let mut colors = HashSet::new();
    for rgba in screenshot.pixels.as_chunks::<4>().0 {
        colors.insert([rgba[0], rgba[1], rgba[2]]);
    }
    colors.len()
}

fn assert_leetcodedaily_full_frame_color_health(
    robot: &cranpose::Robot,
    label: &str,
    screenshot: &cranpose::RobotScreenshot,
) {
    let health = screenshot_color_health(screenshot);
    println!(
        "leetcodedaily_full_frame_color_health label={} colorful_ratio={:.4} blue_cyan_ratio={:.4} dark_ratio={:.4} unique_rgb={}",
        label,
        health.colorful_ratio(),
        health.blue_cyan_ratio(),
        health.dark_ratio(),
        health.unique_rgb_count,
    );
    if health.colorful_ratio() < FULL_FRAME_MIN_COLORFUL_RATIO
        || health.blue_cyan_ratio() < FULL_FRAME_MIN_BLUE_CYAN_RATIO
        || health.dark_ratio() > FULL_FRAME_MAX_DARK_RATIO
        || health.unique_rgb_count < FULL_FRAME_MIN_UNIQUE_RGB_COUNT
    {
        let path = save_full_frame_color_health_failure(label, screenshot);
        fail_with_robot(
            robot,
            &format!(
                "LeetCodeDaily full frame lost expected color coverage: label={} colorful_ratio={:.4} min={:.4} blue_cyan_ratio={:.4} min={:.4} dark_ratio={:.4} max={:.4} unique_rgb={} min={} capture={}",
                label,
                health.colorful_ratio(),
                FULL_FRAME_MIN_COLORFUL_RATIO,
                health.blue_cyan_ratio(),
                FULL_FRAME_MIN_BLUE_CYAN_RATIO,
                health.dark_ratio(),
                FULL_FRAME_MAX_DARK_RATIO,
                health.unique_rgb_count,
                FULL_FRAME_MIN_UNIQUE_RGB_COUNT,
                path.display()
            ),
        );
    }
}

fn assert_initial_leetcodedaily_frame_health(robot: &cranpose::Robot) {
    let screenshot = capture_visible_window_screenshot("initial-frame-health");
    assert_leetcodedaily_full_frame_color_health(robot, "initial frame", &screenshot);
}

fn screenshot_color_health(screenshot: &cranpose::RobotScreenshot) -> ScreenshotColorHealth {
    let mut colors = HashSet::new();
    let mut colorful_pixels = 0usize;
    let mut blue_cyan_pixels = 0usize;
    let mut dark_pixels = 0usize;

    for rgba in screenshot.pixels.as_chunks::<4>().0 {
        let pixel = [rgba[0], rgba[1], rgba[2], rgba[3]];
        if pixel[3] == 0 {
            continue;
        }
        colors.insert([pixel[0], pixel[1], pixel[2]]);
        if colorful_full_frame_pixel(pixel) {
            colorful_pixels += 1;
        }
        if blue_cyan_full_frame_pixel(pixel) {
            blue_cyan_pixels += 1;
        }
        if screenshot_luma(rgba) < 90 {
            dark_pixels += 1;
        }
    }

    ScreenshotColorHealth {
        total_pixels: screenshot.pixels.len() / 4,
        colorful_pixels,
        blue_cyan_pixels,
        dark_pixels,
        unique_rgb_count: colors.len(),
    }
}

fn colorful_full_frame_pixel(pixel: [u8; 4]) -> bool {
    let max_channel = pixel[0].max(pixel[1]).max(pixel[2]);
    let min_channel = pixel[0].min(pixel[1]).min(pixel[2]);
    max_channel >= FULL_FRAME_COLORFUL_MIN_VALUE
        && max_channel.saturating_sub(min_channel) >= FULL_FRAME_COLORFUL_MIN_SPAN
}

fn blue_cyan_full_frame_pixel(pixel: [u8; 4]) -> bool {
    let red = pixel[0] as i32;
    let green = pixel[1] as i32;
    let blue = pixel[2] as i32;
    (blue >= 120 && green >= 80 && blue > red + 25) || (green >= 120 && blue >= 130 && red <= 120)
}

fn save_full_frame_color_health_failure(
    label: &str,
    screenshot: &cranpose::RobotScreenshot,
) -> PathBuf {
    let dir = leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-full-frame-color-health");
    let path = dir.join(format!("{}.png", sanitize_capture_name(label)));
    save_robot_screenshot_png(&path, screenshot);
    path
}

fn button_reference_capture_dir() -> PathBuf {
    leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-button-reference")
}

fn save_button_reference_crop(
    phase: &str,
    tag: &str,
    screenshot: &cranpose::RobotScreenshot,
) -> PathBuf {
    let dir = button_reference_capture_dir();
    let path = dir.join(format!(
        "{}_{}.png",
        sanitize_capture_name(phase),
        sanitize_capture_name(tag)
    ));
    save_robot_screenshot_png(&path, screenshot);
    path
}

fn save_button_reference_crop_if_requested(
    tag: &str,
    screenshot: &cranpose::RobotScreenshot,
) -> Option<PathBuf> {
    std::env::var_os(BUTTON_REFERENCE_CAPTURE_ENV)?;
    Some(save_button_reference_crop("sample", tag, screenshot))
}

fn save_button_reference_failure(
    phase: &str,
    outside: &ButtonReferenceSample,
    inside: &ButtonReferenceSample,
) -> (PathBuf, PathBuf) {
    let outside_path = outside
        .crop_path
        .clone()
        .unwrap_or_else(|| save_button_reference_crop(phase, outside.tag, &outside.crop));
    let inside_path = inside
        .crop_path
        .clone()
        .unwrap_or_else(|| save_button_reference_crop(phase, inside.tag, &inside.crop));
    (outside_path, inside_path)
}

fn save_button_quality_crop_if_requested(
    label: &str,
    phase: &str,
    screenshot: &cranpose::RobotScreenshot,
) -> Option<PathBuf> {
    std::env::var_os(BUTTON_QUALITY_CAPTURE_ENV)?;
    let dir = leetcodedaily_diagnostic_dir("cranpose-leetcodedaily-button-quality");
    let path = dir.join(format!(
        "{}_{}.png",
        sanitize_capture_name(label),
        sanitize_capture_name(phase)
    ));
    save_robot_screenshot_png(&path, screenshot);
    Some(path)
}

fn capture_visible_window_screenshot(label: &str) -> cranpose::RobotScreenshot {
    let (window_id, capture_id) = X11_CAPTURE_STATE.with(|state| state.borrow_mut().next_capture());
    let (logical_width, logical_height) = app_window_size();
    let path = output_paths::diagnostic_path(&format!(
        "cranpose-leetcodedaily-x11-{}-{}-{}.png",
        std::process::id(),
        capture_id,
        sanitize_capture_name(label)
    ));

    let path_string = path.to_string_lossy().into_owned();
    take_x11_screenshot(&window_id, &path_string);
    let image = image::open(&path)
        .unwrap_or_else(|err| panic!("failed to read X11 screenshot {}: {err}", path.display()))
        .to_rgba8();
    let _ = std::fs::remove_file(&path);

    cranpose::RobotScreenshot {
        width: image.width(),
        height: image.height(),
        logical_width: logical_width as f32,
        logical_height: logical_height as f32,
        pixels: image.into_raw(),
    }
}

fn sanitize_capture_name(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn save_robot_screenshot_png(path: &Path, screenshot: &cranpose::RobotScreenshot) {
    let image: RgbaImage = ImageBuffer::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    )
    .expect("valid screenshot dimensions");
    image
        .save(path)
        .unwrap_or_else(|err| panic!("failed to save {}: {err}", path.display()));
}

fn fail_with_robot(robot: &cranpose::Robot, message: &str) -> ! {
    eprintln!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
}

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
}

fn app_window_size() -> (u32, u32) {
    (
        env_u32(WINDOW_WIDTH_ENV).unwrap_or(APP_WIDTH),
        env_u32(WINDOW_HEIGHT_ENV).unwrap_or(APP_HEIGHT),
    )
}

fn run_leetcodedaily_perf_probe(robot: &cranpose::Robot) {
    let duration_secs = env_u64(PERF_DURATION_SECS_ENV).unwrap_or(5);
    let min_fps = env_f32(PERF_MIN_FPS_ENV).unwrap_or(120.0);
    let scroll_delta = env_f32(PERF_SCROLL_DELTA_ENV).unwrap_or(-8.0);

    scroll_workspace_text_into_view_between(
        robot,
        TARGET_TEXT,
        TARGET_MIN_CENTER_Y,
        TARGET_MAX_CENTER_Y,
        80,
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let start_stats = robot
        .fps_stats()
        .unwrap_or_else(|err| fail_with_robot(robot, &format!("failed to read FPS stats: {err}")));
    let start = Instant::now();
    let duration = Duration::from_secs(duration_secs);
    let mut render_stats = RenderStatsAccumulator::default();
    let mut iterations = 0u64;
    let mut direction = 1.0f32;

    while start.elapsed() < duration {
        if let Err(err) = robot.mouse_scroll(0.0, scroll_delta * direction) {
            fail_with_robot(robot, &format!("perf scroll failed: {err}"));
        }
        if robot.wait_for_idle().is_err() {
            std::thread::sleep(Duration::from_millis(2));
        }
        match robot.get_render_stats() {
            Ok(Some(stats)) => render_stats.record(stats),
            Ok(None) => {}
            Err(err) => fail_with_robot(robot, &format!("failed to read render stats: {err}")),
        }
        iterations = iterations.saturating_add(1);
        if iterations.is_multiple_of(80) {
            direction = -direction;
        }
    }

    let elapsed_secs = start.elapsed().as_secs_f32();
    let end_stats = robot
        .fps_stats()
        .unwrap_or_else(|err| fail_with_robot(robot, &format!("failed to read FPS stats: {err}")));
    let total_frames = end_stats
        .frame_count
        .saturating_sub(start_stats.frame_count);
    let fps = if elapsed_secs > 0.0 {
        total_frames as f32 / elapsed_secs
    } else {
        0.0
    };
    let avg_ms = if total_frames > 0 {
        elapsed_secs * 1000.0 / total_frames as f32
    } else {
        0.0
    };

    print_render_summary(PERF_SCENARIO_NAME, render_stats);
    println!(
        "PERF_FPS_SUMMARY scenario={} fps={:.1} avg_frame_ms={:.2} p95_frame_ms={:.2} p99_frame_ms={:.2} max_frame_ms={:.2} work_avg_ms={:.2} work_p95_ms={:.2} work_max_ms={:.2} missed_120hz={} missed_60hz={} stalls_50ms={} total_frames={} rolling_fps={:.1} rolling_avg_frame_ms={:.2} recompositions={} recompositions_per_second={}",
        PERF_SCENARIO_NAME,
        fps,
        avg_ms,
        end_stats.p95_ms,
        end_stats.p99_ms,
        end_stats.max_ms,
        end_stats.work_avg_ms,
        end_stats.work_p95_ms,
        end_stats.work_max_ms,
        end_stats.missed_120hz_budget,
        end_stats.missed_60hz_budget,
        end_stats.stalled_50ms_frames,
        total_frames,
        end_stats.fps,
        end_stats.avg_ms,
        end_stats.recompositions,
        end_stats.recomps_per_second,
    );
    println!(
        "PERF_SCENARIO_COMPLETE scenario={} iterations={}",
        PERF_SCENARIO_NAME, iterations
    );

    if fps < min_fps {
        fail_with_robot(
            robot,
            &format!("{PERF_SCENARIO_NAME} FPS must be at least {min_fps:.1}, got {fps:.1}"),
        );
    }
}

fn main() {
    env_logger::init();
    let (window_width, window_height) = app_window_size();
    let strip_only_target =
        std::env::var(STRIP_TARGET_ENV).unwrap_or_else(|_| "Rust Code".to_string());
    let strip_only = std::env::var_os(STRIP_ONLY_ENV).is_some();
    let active_viewport_only_target =
        std::env::var(ACTIVE_VIEWPORT_TARGET_ENV).unwrap_or_else(|_| "Rust Code".to_string());
    let active_viewport_only = std::env::var_os(ACTIVE_VIEWPORT_ONLY_ENV).is_some();
    let rigid_only_target =
        std::env::var(RIGID_TARGET_ENV).unwrap_or_else(|_| "Problem TLDR".to_string());
    let rigid_only = std::env::var_os(RIGID_ONLY_ENV).is_some();
    let top_band_only_target =
        std::env::var(TOP_BAND_TARGET_ENV).unwrap_or_else(|_| "Blog Template Preview".to_string());
    let top_band_only = std::env::var_os(TOP_BAND_ONLY_ENV).is_some();
    let bottom_clear_only = std::env::var_os(BOTTOM_CLEAR_ONLY_ENV).is_some();
    let button_reference_only = std::env::var_os(BUTTON_REFERENCE_ONLY_ENV).is_some();
    let extended = std::env::var_os(EXTENDED_ENV).is_some();
    let perf_only = std::env::var_os(PERF_ENV).is_some();

    if std::env::var_os(INTERACTIVE_ENV).is_some() {
        println!("=== Interactive LeetcodeDaily Full Layout ===");
        println!("{INTERACTIVE_ENV}=1, launching visible window");
        AppLauncher::new()
            .with_title("LeetcodeDaily Full Layout Interactive")
            .with_size(window_width, window_height)
            .with_fonts(LEETCODEDAILY_APP_FONTS)
            .with_headless(false)
            .run(LeetcodeDailyFullLayoutApp);
    }

    println!("=== Robot LeetcodeDaily Full Layout Scroll Stability ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(window_width, window_height)
        .with_fonts(LEETCODEDAILY_APP_FONTS)
        .with_headless(false)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();
            assert_initial_leetcodedaily_frame_health(&robot);

            if perf_only {
                run_leetcodedaily_perf_probe(&robot);
                robot.exit().expect("exit");
                return;
            }

            if button_reference_only {
                assert_button_reference_matches_outside(&robot);
                println!("\n=== Test Summary ===");
                println!(
                    "PASS: scrolled workspace Clear button matched the direct reference crispness"
                );
                robot.exit().expect("exit");
                return;
            }
            if strip_only {
                assert_workspace_strip_stable_under_micro_scroll(&robot, &strip_only_target);
                println!("\n=== Test Summary ===");
                println!("PASS: {strip_only_target} workspace strip stayed horizontally stable");
                robot.exit().expect("exit");
                return;
            }
            if active_viewport_only {
                assert_workspace_active_viewport_first_frame_matches_settled(
                    &robot,
                    &active_viewport_only_target,
                );
                println!("\n=== Test Summary ===");
                println!(
                    "PASS: {active_viewport_only_target} active viewport frame was immediate and crisp"
                );
                robot.exit().expect("exit");
                return;
            }
            if rigid_only {
                assert_workspace_rigid_picture_stable_during_active_scroll(
                    &robot,
                    &rigid_only_target,
                );
                println!("\n=== Test Summary ===");
                println!(
                    "PASS: {rigid_only_target} workspace visual anchors stayed horizontally stable during active scroll"
                );
                robot.exit().expect("exit");
                return;
            }
            if top_band_only {
                assert_workspace_top_band_stable_under_micro_scroll(&robot, &top_band_only_target);
                println!("\n=== Test Summary ===");
                println!(
                    "PASS: {top_band_only_target} workspace top band stayed horizontally stable"
                );
                robot.exit().expect("exit");
                return;
            }
            if bottom_clear_only {
                assert_bottom_clear_button_stable_under_one_px_scroll(&robot);
                println!("\n=== Test Summary ===");
                println!("PASS: bottom Clear stayed stable and crisp during active scroll");
                robot.exit().expect("exit");
                return;
            }

            assert_workspace_rigid_picture_stable_during_active_scroll(&robot, "Problem TLDR");
            assert_workspace_rigid_picture_stable_during_active_scroll(&robot, "Intuition");
            assert_workspace_rigid_picture_stable_during_active_scroll(&robot, "Approach");
            assert_workspace_strip_stable_under_micro_scroll(&robot, "Rust Code");
            assert_workspace_top_band_stable_under_micro_scroll(&robot, "Rust Runtime (ms)");
            assert_workspace_strip_stable_under_micro_scroll(&robot, "Blog Template Preview");
            if !extended {
                println!("\n=== Test Summary ===");
                println!("PASS: full LeetCode Daily workspace stayed horizontally stable");
                robot.exit().expect("exit");
                return;
            }

            assert_bottom_clear_button_stable_under_one_px_scroll(&robot);
            assert_rust_runtime_row_stable_under_micro_scroll(&robot);
            assert_deep_button_quality_matches_top(&robot);

            scroll_workspace_text_into_view_between(
                &robot,
                TARGET_TEXT,
                TARGET_MIN_CENTER_Y,
                TARGET_MAX_CENTER_Y,
                80,
            );
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let compare_ok = run_scroll_stability_capture(
                &robot,
                ScrollStabilityConfig {
                    window_title: WINDOW_TITLE,
                    output_name: "leetcodedaily_full_layout_scroll_stability",
                    file_prefix: "leetcodedaily_full_scroll",
                    target_text: TARGET_TEXT,
                    viewport_tag: Some(VIEWPORT_TAG),
                    window_width,
                    window_height,
                    scroll_steps: SCROLL_STEPS,
                    scroll_delta_y: SCROLL_DELTA_Y,
                    step_epsilon: STEP_EPSILON,
                    fallback_trim_top_px: 450,
                    fallback_trim_bottom_px: 70,
                    fallback_trim_left_px: 0,
                    fallback_trim_right_px: 0,
                    compare_search_offset_px: COMPARE_SEARCH_OFFSET_PX,
                    compare_max_adjacent_score: COMPARE_MAX_ADJACENT_SCORE,
                    compare_max_channel_delta: 0,
                    compare_stabilized_guard_px: COMPARE_STABILIZED_GUARD_PX,
                    compare_viewport_inset_px: COMPARE_VIEWPORT_INSET_PX,
                    render_stats_env: Some(RENDER_STATS_ENV),
                    active_frame: None,
                },
                None,
            );
            if !compare_ok {
                std::process::exit(1);
            }
            println!("\n=== Test Summary ===");
            println!("PASS: full LeetCode Daily layout stayed within the pixel-drift budget");
            robot.exit().expect("exit");
        })
        .run(LeetcodeDailyFullLayoutApp);
}

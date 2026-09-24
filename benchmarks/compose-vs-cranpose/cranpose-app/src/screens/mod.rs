pub mod feed;
pub mod heavy;
pub mod layers;
pub mod particles;
pub mod ticker;

use cranpose::prelude::*;
use cranpose_ui::text::{FontWeight, ParagraphStyle, SpanStyle, TextUnit};

/// Line height is explicit because the two frameworks read different vertical
/// metrics from the same font; 1.4 em makes both lay out identical lines.
pub fn text_style(size_sp: f32, color: Color, bold: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size_sp),
            font_weight: bold.then_some(FontWeight::BOLD),
            ..Default::default()
        },
        paragraph_style: ParagraphStyle {
            line_height: TextUnit::Em(1.4),
            ..Default::default()
        },
    }
}

pub fn clamped_text_options(max_lines: usize) -> TextOptions {
    TextOptions {
        overflow: TextOverflow::Ellipsis,
        max_lines: Some(max_lines),
        ..Default::default()
    }
}

#[composable]
pub fn TopBar(title: &'static str) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(56.0)
            .background(Color::from_rgb_u8(0x1E, 0x2A, 0x4A))
            .padding_horizontal(16.0),
        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                title,
                Modifier::empty(),
                text_style(20.0, Color::WHITE, true),
            );
        },
    );
}

/// Seconds since this screen's first frame, written once per frame from the
/// frame clock -- the counterpart of a `withFrameNanos` loop in Compose.
#[composable]
pub fn rememberFrameSeconds() -> MutableState<f32> {
    let seconds = rememberMutableStateOf(|| 0.0f32);
    LaunchedEffectAsync((), move |scope| {
        std::boxed::Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            let start = clock.next_frame().await;
            while scope.is_active() {
                let now = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                seconds.set(now.saturating_sub(start) as f32 / 1e9);
            }
        })
    });
    seconds
}

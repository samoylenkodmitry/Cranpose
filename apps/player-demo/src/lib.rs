// A Cranpose app that plays two clips through the platform's hardware decoder and shows each in a
// player surface: H.264 fitted into its box, and HEVC cropped to fill it. A line under each box
// gives the decoder's counts. The same app runs in a window of the desktop and on an Android
// phone.

use std::time::Duration;

use cranpose::{DecodedFrames, DecoderStats, VideoCodec, prelude::*};
use cranpose_ui::text::{FontWeight, TextUnit};

mod clip_feed;
#[cfg(not(target_arch = "wasm32"))]
mod feed_thread;
mod video_feed;

#[cfg(not(target_arch = "wasm32"))]
pub use feed_thread::FeedThread;
pub use video_feed::VerticalVideoFeed;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DemoTab {
    Decoders,
    Feed,
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn run_player_demo(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    AppLauncher::new()
        .with_title("Player demo")
        .with_size(1280, 720)
        .with_web_fill_viewport(true)
        .run_web(&canvas_id, PlayerDemo)
        .await
}

// The whole screen of the demo.
#[composable]
pub fn PlayerDemo() {
    let tab = rememberMutableStateOf(|| DemoTab::Decoders);
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.08, 0.1, 0.12, 1.0))
            .padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(12.0)),
                move || {
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        move || tab.set(DemoTab::Decoders),
                        || {
                            Text(
                                "Hardware Decoders",
                                Modifier::empty(),
                                sized(15.0, Some(FontWeight::BOLD)),
                            );
                        },
                    );
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        move || tab.set(DemoTab::Feed),
                        || {
                            Text(
                                "Vertical Video Feed",
                                Modifier::empty(),
                                sized(15.0, Some(FontWeight::BOLD)),
                            );
                        },
                    );
                },
            );
            match tab.value() {
                DemoTab::Decoders => Clips(),
                DemoTab::Feed => VerticalVideoFeed(),
            }
        },
    );
}

// The two clips with their title: H.264 fitted into its box at 120 pictures a second, and HEVC
// cropped to fill its box at 60. A parent that is a column lays them out.
#[composable]
pub fn Clips() {
    Text(
        "Hardware decoder, then PlayerSurface",
        Modifier::empty(),
        sized(28.0, Some(FontWeight::BOLD)),
    );
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(24.0)),
        || {
            Clip(
                "H.264 fitted, 120 pictures a second",
                VideoCodec::H264,
                120,
                ContentScale::Fit,
            );
            Clip(
                "HEVC cropped, 60 pictures a second",
                VideoCodec::Hevc,
                60,
                ContentScale::Crop,
            );
        },
    );
}

// A clip that plays as long as the box is composed, with a line of the decoder's counts below it.
#[composable]
fn Clip(title: &'static str, codec: VideoCodec, rate: u32, content_scale: ContentScale) {
    let frames = rememberMutableStateOf(|| None::<DecodedFrames>);
    let stats = rememberMutableStateOf(DecoderStats::default);
    let failure = rememberMutableStateOf(|| None::<String>);
    #[cfg(not(target_arch = "wasm32"))]
    DisposableEffect(codec, move |_| {
        let feed = match clip_feed::start(codec, rate) {
            Ok((feed, decoded)) => {
                frames.set(Some(decoded));
                Some(feed)
            }
            Err(error) => {
                failure.set(Some(error.to_string()));
                None
            }
        };
        DisposableEffectResult::new(move || drop(feed))
    });
    #[cfg(target_arch = "wasm32")]
    LaunchedEffect(codec, async move |_| {
        if let Err(error) =
            clip_feed::run_async(codec, rate, move |decoded| frames.set(Some(decoded))).await
        {
            failure.set(Some(error.to_string()));
        }
    });
    LaunchedEffect(frames.value(), async move |_| {
        let Some(decoded) = frames.value() else {
            return;
        };
        loop {
            delay(Duration::from_secs(1)).await;
            stats.set(decoded.stats());
        }
    });
    Column(
        Modifier::empty().weight(1.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            Text(
                title,
                Modifier::empty(),
                sized(22.0, Some(FontWeight::BOLD)),
            );
            if let Some(decoded) = frames.value() {
                PlayerSurface(
                    decoded,
                    Modifier::empty()
                        .fill_max_width()
                        .height(270.0)
                        .background(Color(0.0, 0.0, 0.0, 1.0)),
                    content_scale,
                );
            }
            let counts = stats.value();
            Text(
                failure.value().unwrap_or_else(|| {
                    format!(
                        "{} pictures, decode {:.2} ms, {} dropped",
                        counts.decoded,
                        counts.mean_decode.as_secs_f64() * 1000.0,
                        counts.dropped
                    )
                }),
                Modifier::empty(),
                sized(18.0, None),
            );
        },
    );
}

// The text style of the demos: the same color at every size.
pub fn sized(size: f32, weight: Option<FontWeight>) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.95, 0.96, 0.98, 1.0)),
            font_size: TextUnit::Sp(size),
            font_weight: weight,
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}

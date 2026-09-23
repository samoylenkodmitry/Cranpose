use std::time::{SystemTime, UNIX_EPOCH};

use cranpose::{
    composable,
    liquid::prelude::*,
    local_safe_area_insets, rememberMutableStateOf,
    text::{FontFamily, FontWeight, SpanStyle, TextStyle, TextUnit},
    widgets::{Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Text},
    Alignment, Brush, Color, Modifier, Rect,
};
use cranpose_ui::{PointerEventKind, PointerInputScope};

fn wall_millis() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after Unix epoch")
        .as_secs_f64()
        * 1000.0
}

fn save_touches(touches: &[serde_json::Value]) -> anyhow::Result<()> {
    let home = std::env::var("HOME")?;
    let directory = std::path::Path::new(&home).join("Documents");
    std::fs::create_dir_all(&directory)?;
    let first = touches[0]["wallMillis"]
        .as_f64()
        .expect("recorded timestamp") as u64;
    std::fs::write(
        directory.join(format!("cranpose-{first}-touches.json")),
        serde_json::to_vec(touches)?,
    )?;
    Ok(())
}

const TITLES: [&str; 4] = ["Discover", "Browse", "Saved", "Account"];

#[derive(serde::Deserialize)]
struct ReferenceContent {
    titles: [String; 4],
    icons: [usize; 4],
    accent: Option<[f32; 3]>,
    palette: Option<Vec<[f32; 3]>>,
}

static CONTENT: std::sync::OnceLock<ReferenceContent> = std::sync::OnceLock::new();

#[allow(dead_code)]
pub(crate) fn configure_reference_content(json: &str) -> anyhow::Result<()> {
    let content: ReferenceContent = serde_json::from_str(json)?;
    content.validate()?;
    CONTENT
        .set(content)
        .map_err(|_| anyhow::anyhow!("reference content is already initialized"))
}

impl ReferenceContent {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.icons.iter().all(|index| *index < 4),
            "invalid symbol index"
        );
        if let Some(palette) = &self.palette {
            anyhow::ensure!(!palette.is_empty(), "empty reference palette");
        }
        anyhow::ensure!(
            self.accent
                .iter()
                .chain(self.palette.iter().flatten())
                .flatten()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)),
            "reference colors must contain finite unit RGB channels"
        );
        Ok(())
    }

    fn active() -> &'static Self {
        CONTENT.get_or_init(|| {
            let content = std::env::var("REFERENCE_CONTENT").ok().map_or_else(
                || Self {
                    titles: TITLES.map(str::to_string),
                    icons: [0, 1, 2, 3],
                    accent: None,
                    palette: None,
                },
                |json| serde_json::from_str::<Self>(&json).expect("valid reference content"),
            );
            content.validate().expect("valid reference content");
            content
        })
    }

    fn colors() -> &'static [Color] {
        static COLORS: std::sync::OnceLock<Vec<Color>> = std::sync::OnceLock::new();
        COLORS.get_or_init(|| {
            Self::active().palette.as_ref().map_or_else(
                || RAINBOW.to_vec(),
                |colors| colors.iter().copied().map(Self::color).collect(),
            )
        })
    }

    fn color(rgb: [f32; 3]) -> Color {
        assert!(rgb.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        Color(rgb[0], rgb[1], rgb[2], 1.0)
    }
}
const RAINBOW: [Color; 7] = [
    Color(1.0, 0.23, 0.19, 1.0),
    Color(1.0, 0.58, 0.0, 1.0),
    Color(1.0, 0.80, 0.0, 1.0),
    Color(0.20, 0.78, 0.35, 1.0),
    Color(0.20, 0.68, 0.90, 1.0),
    Color(0.0, 0.48, 1.0, 1.0),
    Color(0.69, 0.32, 0.87, 1.0),
];

fn initial_destination() -> usize {
    std::env::var("REFERENCE_INITIAL_DESTINATION")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|index| *index < TITLES.len())
        .unwrap_or(0)
}

fn initial_tint_amount() -> GlassTintAmount {
    std::env::var("REFERENCE_MATERIAL_PROFILE")
        .ok()
        .map(|value| {
            GlassTintAmount::from_percent(value.parse().expect("numeric tint percentage"))
                .expect("valid tint percentage")
        })
        .unwrap_or_default()
}

#[composable]
fn TintControls(amount: cranpose_core::MutableState<GlassTintAmount>, top: f32) {
    let style = TextStyle {
        span_style: SpanStyle {
            color: Some(liquid_colors().label),
            font_size: TextUnit::Sp(18.0),
            ..Default::default()
        },
        ..Default::default()
    };
    Text(
        format!("Glass tint: {:.0}%", amount.get().percent()),
        Modifier::empty().offset(24.0, top + 16.0),
        style.clone(),
    );
    cranpose::widgets::Row(
        Modifier::empty().offset(24.0, top + 44.0),
        cranpose::widgets::RowSpec::default(),
        move || {
            for percent in [0.0, 20.0, 25.0, 50.0, 75.0, 100.0] {
                Text(
                    format!("{percent:.0}%"),
                    Modifier::empty()
                        .width(64.0)
                        .height(44.0)
                        .clickable(move |_| {
                            amount.set(
                                GlassTintAmount::from_percent(percent)
                                    .expect("valid tint percentage"),
                            );
                        }),
                    style.clone(),
                );
            }
        },
    );
}

#[composable]
pub(crate) fn LiquidTabReference(checkerboard: bool, dark: bool) {
    let scheme = if dark {
        SchemeMode::Dark
    } else {
        SchemeMode::Light
    };
    let content = ReferenceContent::active();
    let tint_amount = rememberMutableStateOf(initial_tint_amount);
    let mut typography = LiquidTypography::default();
    typography.caption1.span_style.font_family = Some(FontFamily::SansSerif);
    LiquidTheme(
        LiquidThemeSpec {
            scheme,
            accent: content
                .accent
                .map(ReferenceContent::color)
                .unwrap_or_else(|| Color::from_rgb_u8(0, if dark { 145 } else { 136 }, 255)),
            typography,
            glass_tint_amount: tint_amount.get(),
        },
        move || {
            let selected = rememberMutableStateOf(initial_destination);
            let colors = liquid_colors();
            let insets = local_safe_area_insets().current();
            BoxWithConstraints(
                Modifier::empty().fill_max_size().draw_behind(move |scope| {
                    scope.draw_rect(Brush::solid(if dark { Color::BLACK } else { Color::WHITE }));
                    if checkerboard {
                        let palette = ReferenceContent::colors();
                        let cell = 8.0;
                        for row in 0..(scope.size().height / cell).ceil() as usize {
                            for column in 0..(scope.size().width / cell).ceil() as usize {
                                let rect = Rect {
                                    x: column as f32 * cell,
                                    y: row as f32 * cell,
                                    width: cell,
                                    height: cell,
                                };
                                scope.draw_rect_at(
                                    rect,
                                    Brush::solid(palette[(column + row) % palette.len()]),
                                );
                                if (row + column) % 2 == 0 {
                                    scope.draw_rect_at(
                                        rect,
                                        Brush::solid(Color::WHITE.with_alpha(0.55)),
                                    );
                                }
                            }
                        }
                    }
                }),
                move |scope| {
                    let symbols = cranpose::remember(|| {
                        [
                            include_bytes!(
                                "../../../liquid-reference/reference-content/discover.png"
                            )
                            .as_slice(),
                            include_bytes!(
                                "../../../liquid-reference/reference-content/browse.png"
                            )
                            .as_slice(),
                            include_bytes!("../../../liquid-reference/reference-content/saved.png")
                                .as_slice(),
                            include_bytes!(
                                "../../../liquid-reference/reference-content/account.png"
                            )
                            .as_slice(),
                        ]
                        .map(|bytes| {
                            let image = image::load_from_memory(bytes)
                                .expect("native symbol PNG")
                                .into_rgba8();
                            let size = cranpose::Size::new(
                                image.width() as f32 / 3.0,
                                image.height() as f32 / 3.0,
                            );
                            let bitmap = cranpose::ImageBitmap::from_rgba8(
                                image.width(),
                                image.height(),
                                image.into_raw(),
                            )
                            .expect("native symbol pixels");
                            (cranpose::widgets::Painter::from_bitmap(bitmap), size)
                        })
                    })
                    .with(Clone::clone);
                    let width = scope.constraints().max_width;
                    let height = scope.constraints().max_height;
                    Box(
                        Modifier::empty().fill_max_size(),
                        BoxSpec::default().content_alignment(Alignment::CENTER),
                        move || {
                            Text(
                                content.titles[selected.get()].as_str(),
                                Modifier::empty(),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(colors.label),
                                        font_size: TextUnit::Sp(34.0),
                                        font_family: Some(FontFamily::SansSerif),
                                        font_weight: Some(FontWeight::BOLD),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );
                        },
                    );
                    TintControls(tint_amount, insets.top);
                    LiquidTabBar(
                        Modifier::empty()
                            .offset(21.0, height - (insets.bottom - 13.0).max(21.0) - 62.0)
                            .width(width - 42.0),
                        LiquidTabBarSpec::new((width - 50.0) / TITLES.len() as f32),
                        selected.get(),
                        move |index| selected.set(index),
                        move |tabs| {
                            let offsets = [
                                (-1.0 / 3.0, 1.0 / 6.0),
                                (-1.0 / 3.0, 5.0 / 6.0),
                                (-1.0 / 3.0, 1.0),
                                (1.0 / 6.0, 1.0),
                            ];
                            for (index, title) in content.titles.iter().enumerate() {
                                let symbol = content.icons[index];
                                let (painter, size) = symbols[symbol].clone();
                                let (x, y) = offsets[symbol];
                                tabs.push(
                                    LiquidTab::from_painter(painter, size, title)
                                        .with_icon_offset(x, y),
                                );
                            }
                        },
                    );
                    if std::env::var("REFERENCE_RECORDING").as_deref() == Ok("1") {
                        RecordingOverlay();
                    }
                },
            );
        },
    );
}

async fn record_pointer(scope: PointerInputScope) {
    scope
        .await_pointer_event_scope(|input| async move {
            let mut touches = Vec::new();
            loop {
                let event = input.await_pointer_event().await;
                let phase = match event.kind {
                    PointerEventKind::Down => {
                        touches.clear();
                        "Touch down"
                    }
                    PointerEventKind::Move => "Sliding",
                    PointerEventKind::Up => "Touch up",
                    PointerEventKind::Cancel => "Cancelled",
                    _ => continue,
                };
                touches.push(
                    serde_json::json!({"wallMillis": wall_millis(), "phase": phase,
                                "x": event.global_position.x, "y": event.global_position.y,
                                "platformMillis": event.time_ms,
                                "animationTimeNanos": event.animation_time_nanos}),
                );

                if matches!(event.kind, PointerEventKind::Up | PointerEventKind::Cancel) {
                    if let Err(error) = save_touches(&touches) {
                        eprintln!("touch trace: {error:#}");
                    }
                }
            }
        })
        .await;
}

#[composable]
fn RecordingOverlay() {
    let pulse = rememberMutableStateOf(|| 0u64);
    cranpose_core::LaunchedEffectAsync((), move |scope| {
        std::boxed::Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            while scope.is_active() {
                let now = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                pulse.set(now);
            }
        })
    });
    Box(
        Modifier::empty()
            .offset(16.0, 100.0)
            .size_points(128.0, 8.0)
            .draw_behind(move |scope| {
                let _ = pulse.get();
                let code = (0xB4u32 << 24) | ((wall_millis() as u64 & 0xFFFFFF) as u32);
                for bit in 0..32 {
                    let color = if (code >> (31 - bit)) & 1 == 1 {
                        Color::WHITE
                    } else {
                        Color::BLACK
                    };
                    scope.draw_rect_at(
                        Rect {
                            x: bit as f32 * 4.0,
                            y: 0.0,
                            width: 4.0,
                            height: 8.0,
                        },
                        Brush::solid(color),
                    );
                }
            }),
        BoxSpec::default(),
        || {},
    );
    Box(
        Modifier::empty()
            .fill_max_size()
            .pointer_input((), record_pointer),
        BoxSpec::default(),
        || {},
    );
}

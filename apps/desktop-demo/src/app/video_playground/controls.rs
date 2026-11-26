// Glass telemetry HUD, scenario/layout/effect bars, Media3 transport strip, and verified
// open-media license catalog drawer for the Video Playground showcase tab.

use std::time::Duration;

use cranpose::prelude::{
    rememberMuteButtonState, rememberPlayPauseButtonState, rememberProgressStateWithTickInterval,
    Player,
};
use cranpose_core::MutableState;
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    widgets::{FlowRow, FlowSpec},
    Alignment, BasicTextField, Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec,
    CornerRadii, LinearArrangement, Modifier, Path, Point, Row, RowSpec, ScrollState, Stroke, Text,
    TextStyle, VerticalAlignment,
};

use super::{
    catalog::{
        ActiveSource, CatalogEntry, DecoderConcurrency, LicenseKind, MediaOrigin, StressScenario,
        WallLayout, CATALOG,
    },
    telemetry::TelemetrySnapshot,
    wall::SurfaceEffects,
};

const BG_PANEL: Color = Color(0.06, 0.08, 0.13, 0.88);
const BG_CHIP: Color = Color(0.12, 0.16, 0.24, 0.92);
const BG_CHIP_ACTIVE: Color = Color(0.18, 0.42, 0.88, 0.96);
const TEXT_PRIMARY: Color = Color::from_rgb_u8(0xF3, 0xF6, 0xFC);
const TEXT_MUTED: Color = Color::from_rgb_u8(0x94, 0xA3, 0xB8);
const COLOR_DECODER: Color = Color::from_rgb_u8(0x38, 0xBD, 0xF8);
const COLOR_COMPOSITOR: Color = Color::from_rgb_u8(0x4A, 0xDE, 0x80);
const COLOR_WARN: Color = Color::from_rgb_u8(0xFB, 0xBF, 0x24);
const COLOR_RESTRICTED: Color = Color::from_rgb_u8(0xF8, 0x71, 0x71);

pub(super) fn styled_text(size: f32, color: Color, weight: Option<FontWeight>) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            font_weight: weight,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChipTone {
    Idle,
    Active,
}

impl ChipTone {
    const fn from_active(active: bool) -> Self {
        if active {
            Self::Active
        } else {
            Self::Idle
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HudDensity {
    Compact,
    Expanded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToggleState {
    Off,
    On,
}

impl ToggleState {
    const fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::On
        } else {
            Self::Off
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowSelection {
    Idle,
    Selected,
}

#[composable]
pub(super) fn ChipButton(
    label: impl Into<String>,
    tone: ChipTone,
    on_click: impl FnMut() + 'static,
) {
    let label = label.into();
    let (bg, text_color) = match tone {
        ChipTone::Active => (BG_CHIP_ACTIVE, Color::WHITE),
        ChipTone::Idle => (BG_CHIP, TEXT_PRIMARY),
    };
    Button(
        Modifier::empty()
            .height_in(28.0, f32::INFINITY)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(bg), CornerRadii::uniform(7.0));
            })
            .padding_each(9.0, 5.0, 9.0, 5.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(
                label.clone(),
                Modifier::empty(),
                styled_text(11.5, text_color, Some(FontWeight::MEDIUM)),
            );
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct HudLayoutConfig {
    pub(super) density: HudDensity,
    pub(super) controls_visible: bool,
    pub(super) controls_override: MutableState<Option<bool>>,
    pub(super) catalog_open: MutableState<bool>,
}

#[composable]
pub(super) fn TelemetryHudBar(
    scenario: StressScenario,
    snap: TelemetrySnapshot,
    source_title: String,
    hud: HudLayoutConfig,
    on_open_local_file: impl Fn() + Clone + 'static,
) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .draw_behind(|scope| {
                scope.draw_round_rect(Brush::solid(BG_PANEL), CornerRadii::uniform(10.0));
            })
            .padding_each(10.0, 6.0, 10.0, 6.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
        move || {
            TelemetryHudHeader(
                scenario,
                snap.clone(),
                source_title.clone(),
                hud,
                on_open_local_file.clone(),
            );
            TelemetryMetricsStrip(scenario, snap.clone(), hud.density);
        },
    );
}

#[composable]
fn TelemetryHudHeader(
    scenario: StressScenario,
    snap: TelemetrySnapshot,
    source_title: String,
    hud: HudLayoutConfig,
    on_open_local_file: impl Fn() + Clone + 'static,
) {
    let res_str = if snap.sample_size.0 > 0 {
        format!("{}x{}", snap.sample_size.0, snap.sample_size.1)
    } else {
        "probing".to_owned()
    };
    let subtitle = format!(
        "{source_title} · {res_str} · {} · Dec {}/{} · {} tiles",
        snap.color_summary,
        snap.active_decoders,
        scenario.tile_count(),
        snap.active_surfaces
    );
    FlowRow(
        Modifier::empty().fill_max_width(),
        FlowSpec::new()
            .main_axis_spacing(8.0)
            .cross_axis_spacing(5.0),
        move || {
            let sub = subtitle.clone();
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(1.0)),
                move || {
                    Text(
                        format!(
                            "Video Playground · {} ({})",
                            scenario.label(),
                            scenario.target_label()
                        ),
                        Modifier::empty(),
                        styled_text(13.0, TEXT_PRIMARY, Some(FontWeight::BOLD)),
                    );
                    Text(
                        sub.clone(),
                        Modifier::empty(),
                        styled_text(10.5, TEXT_MUTED, None),
                    );
                },
            );
            let open_local = on_open_local_file.clone();
            FlowRow(
                Modifier::empty(),
                FlowSpec::new()
                    .main_axis_spacing(5.0)
                    .cross_axis_spacing(4.0),
                move || {
                    let local_cb = open_local.clone();
                    ChipButton("Open Local 4K/8K", ChipTone::Idle, move || local_cb());
                    let is_cat = hud.catalog_open.value();
                    ChipButton(
                        if is_cat {
                            "Hide Media Catalog"
                        } else {
                            "Open Media Catalog"
                        },
                        ChipTone::from_active(is_cat),
                        move || hud.catalog_open.set(!is_cat),
                    );
                    let is_ctrl = hud.controls_visible;
                    ChipButton(
                        if is_ctrl {
                            "Hide Controls"
                        } else {
                            "Show Controls"
                        },
                        ChipTone::from_active(is_ctrl),
                        move || hud.controls_override.set(Some(!is_ctrl)),
                    );
                },
            );
        },
    );
}

#[composable]
fn TelemetryMetricsStrip(scenario: StressScenario, snap: TelemetrySnapshot, density: HudDensity) {
    let target_mpix = scenario.target_pixels_per_sec() as f32 / 1_000_000.0;
    let cpu_str = snap
        .cpu_percent
        .map_or_else(|| "N/A".to_owned(), |v| format!("{v:.0}%"));
    let rss_str = snap
        .process_rss_mb
        .map_or_else(|| "N/A".to_owned(), |v| format!("{v:.0} MB"));
    let copy_str = if snap.zero_copy_gpu {
        format!("0 B/s CPU · {:.0} us", snap.mean_import_us)
    } else {
        format!("{:.1} MB/s SW", snap.cpu_copy_mb_per_sec)
    };
    let drop_accent = if snap.decoder_dropped + snap.present_dropped > 0 {
        COLOR_WARN
    } else {
        COLOR_COMPOSITOR
    };
    let copy_accent = if snap.zero_copy_gpu {
        COLOR_COMPOSITOR
    } else {
        COLOR_WARN
    };

    FlowRow(
        Modifier::empty().fill_max_width(),
        FlowSpec::new()
            .main_axis_spacing(6.0)
            .cross_axis_spacing(5.0),
        move || {
            MetricCard(
                "Decoder FPS",
                format!("{:.1} fps", snap.decoder_fps),
                format!(
                    "{:.1}/{:.0} MP/s · {:.1} ms",
                    snap.decoded_mpix_per_sec, target_mpix, snap.mean_decode_ms
                ),
                COLOR_DECODER,
                density,
            );
            MetricCard(
                "Compositor FPS",
                format!("{:.1} fps", snap.compositor_fps),
                format!("{:.1} ms/frame", snap.compositor_frame_ms),
                COLOR_COMPOSITOR,
                density,
            );
            MetricCard(
                "Dropped Frames",
                format!("{} dec / {} ui", snap.decoder_dropped, snap.present_dropped),
                "Decoder vs present".to_owned(),
                drop_accent,
                density,
            );
            MetricCard(
                "GPU Video / RSS",
                format!("{:.1} MB GPU", snap.gpu_video_mb),
                format!("RSS {rss_str}"),
                TEXT_PRIMARY,
                density,
            );
            MetricCard(
                "CPU Utilization",
                cpu_str.clone(),
                "Process user+sys".to_owned(),
                TEXT_PRIMARY,
                density,
            );
            MetricCard(
                "Frame-Copy Overhead",
                copy_str.clone(),
                format!("Peak {:.0} us", snap.max_import_us),
                copy_accent,
                density,
            );
            if density == HudDensity::Expanded {
                FpsSparkline(
                    snap.decoder_history.clone(),
                    snap.compositor_history.clone(),
                );
            }
        },
    );
}

#[composable]
fn MetricCard(
    title: &'static str,
    value: String,
    detail: String,
    accent: Color,
    density: HudDensity,
) {
    let expanded = density == HudDensity::Expanded;
    Column(
        Modifier::empty()
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(0.09, 0.12, 0.19, 0.90)),
                    CornerRadii::uniform(7.0),
                );
            })
            .padding_each(8.0, 4.0, 8.0, 4.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(1.0)),
        move || {
            Text(title, Modifier::empty(), styled_text(9.5, TEXT_MUTED, None));
            Text(
                value.clone(),
                Modifier::empty(),
                styled_text(
                    if expanded { 12.5 } else { 11.5 },
                    accent,
                    Some(FontWeight::BOLD),
                ),
            );
            if expanded {
                Text(
                    detail.clone(),
                    Modifier::empty(),
                    styled_text(9.5, TEXT_MUTED, None),
                );
            }
        },
    );
}

#[composable]
fn FpsSparkline(decoder_history: Vec<f32>, compositor_history: Vec<f32>) {
    Box(
        Modifier::empty()
            .size_points(140.0, 42.0)
            .draw_behind(move |scope| {
                let sz = scope.size();
                scope.draw_round_rect(
                    Brush::solid(Color(0.08, 0.11, 0.17, 0.92)),
                    CornerRadii::uniform(7.0),
                );
                let max_val = decoder_history
                    .iter()
                    .chain(compositor_history.iter())
                    .copied()
                    .fold(60.0_f32, f32::max);
                draw_spark_series(
                    scope,
                    &compositor_history,
                    max_val,
                    sz.width,
                    sz.height,
                    COLOR_COMPOSITOR,
                );
                draw_spark_series(
                    scope,
                    &decoder_history,
                    max_val,
                    sz.width,
                    sz.height,
                    COLOR_DECODER,
                );
            })
            .padding_each(6.0, 2.0, 6.0, 2.0),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        || {
            Text(
                "Dec (blue) · Comp (green)",
                Modifier::empty(),
                styled_text(8.5, TEXT_MUTED, None),
            );
        },
    );
}

fn draw_spark_series(
    scope: &mut dyn cranpose_ui::DrawScope,
    values: &[f32],
    max_val: f32,
    w: f32,
    h: f32,
    color: Color,
) {
    if values.len() < 2 || max_val <= 0.0 {
        return;
    }
    let pad_x = 6.0;
    let pad_top = 14.0;
    let pad_bot = 5.0;
    let span_w = (w - pad_x * 2.0).max(1.0);
    let span_h = (h - pad_top - pad_bot).max(1.0);
    let step = span_w / ((values.len() - 1) as f32);
    let mut path = Path::new();
    for (i, &v) in values.iter().enumerate() {
        let x = pad_x + (i as f32) * step;
        let y = pad_top + span_h * (1.0 - (v / max_val).clamp(0.0, 1.0));
        if i == 0 {
            path.move_to(Point::new(x, y));
        } else {
            path.line_to(Point::new(x, y));
        }
    }
    scope.draw_path(
        &path,
        Brush::solid(color),
        cranpose_ui::DrawStyle::Stroke(Stroke::new(1.6)),
    );
}

#[composable]
pub(super) fn ScenarioAndEffectsToolbar(
    scenario: MutableState<StressScenario>,
    layout: MutableState<WallLayout>,
    concurrency: MutableState<DecoderConcurrency>,
    effects: MutableState<SurfaceEffects>,
    playing: MutableState<bool>,
    muted: MutableState<bool>,
) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .draw_behind(|scope| {
                scope.draw_round_rect(Brush::solid(BG_PANEL), CornerRadii::uniform(10.0));
            })
            .padding_each(10.0, 7.0, 10.0, 7.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
        move || {
            ScenarioPlaybackRow(scenario, playing, muted);
            LayoutAndEffectsRow(layout, concurrency, effects);
        },
    );
}

#[composable]
fn ScenarioPlaybackRow(
    scenario: MutableState<StressScenario>,
    playing: MutableState<bool>,
    muted: MutableState<bool>,
) {
    let cur_scenario = scenario.value();
    let is_playing = playing.value();
    let is_muted = muted.value();
    FlowRow(
        Modifier::empty().fill_max_width(),
        FlowSpec::new()
            .main_axis_spacing(6.0)
            .cross_axis_spacing(5.0),
        move || {
            for s in StressScenario::ALL {
                ChipButton(
                    s.label(),
                    ChipTone::from_active(cur_scenario == s),
                    move || {
                        scenario.set(s);
                    },
                );
            }
            ChipButton(
                if is_playing {
                    "Pause Wall"
                } else {
                    "Play Wall"
                },
                ChipTone::from_active(is_playing),
                move || playing.set(!is_playing),
            );
            ChipButton(
                if is_muted {
                    "Unmute Focus"
                } else {
                    "Mute Focus"
                },
                ChipTone::from_active(!is_muted),
                move || muted.set(!is_muted),
            );
        },
    );
}

#[composable]
fn LayoutAndEffectsRow(
    layout: MutableState<WallLayout>,
    concurrency: MutableState<DecoderConcurrency>,
    effects: MutableState<SurfaceEffects>,
) {
    let cur_layout = layout.value();
    let cur_conc = concurrency.value();
    let cur_fx = effects.value();
    FlowRow(
        Modifier::empty().fill_max_width(),
        FlowSpec::new()
            .main_axis_spacing(6.0)
            .cross_axis_spacing(5.0),
        move || {
            for l in WallLayout::ALL {
                ChipButton(
                    format!("Layout: {}", l.label()),
                    ChipTone::from_active(cur_layout == l),
                    move || {
                        layout.set(l);
                    },
                );
            }
            for c in DecoderConcurrency::PRESETS {
                ChipButton(c.label(), ChipTone::from_active(cur_conc == c), move || {
                    concurrency.set(c);
                });
            }
            EffectToggleChip(
                "Motion",
                ToggleState::from_enabled(cur_fx.animate),
                move || {
                    effects.set(SurfaceEffects {
                        animate: !cur_fx.animate,
                        ..cur_fx
                    });
                },
            );
            EffectToggleChip(
                "Clip",
                ToggleState::from_enabled(cur_fx.rounded_clip),
                move || {
                    effects.set(SurfaceEffects {
                        rounded_clip: !cur_fx.rounded_clip,
                        ..cur_fx
                    });
                },
            );
            EffectToggleChip(
                "3D",
                ToggleState::from_enabled(cur_fx.transform_3d),
                move || {
                    effects.set(SurfaceEffects {
                        transform_3d: !cur_fx.transform_3d,
                        ..cur_fx
                    });
                },
            );
            EffectToggleChip("Blur", ToggleState::from_enabled(cur_fx.blur), move || {
                effects.set(SurfaceEffects {
                    blur: !cur_fx.blur,
                    ..cur_fx
                });
            });
            EffectToggleChip(
                "Alpha",
                ToggleState::from_enabled(cur_fx.alpha_wave),
                move || {
                    effects.set(SurfaceEffects {
                        alpha_wave: !cur_fx.alpha_wave,
                        ..cur_fx
                    });
                },
            );
            ChipButton(
                cur_fx.shader.label(),
                ChipTone::from_active(cur_fx.shader != super::shaders::VideoShaderPreset::None),
                move || {
                    effects.set(SurfaceEffects {
                        shader: cur_fx.shader.next(),
                        ..cur_fx
                    });
                },
            );
        },
    );
}

#[composable]
fn EffectToggleChip(name: &'static str, state: ToggleState, on_toggle: impl FnMut() + 'static) {
    let (label, tone) = match state {
        ToggleState::On => ("On", ChipTone::Active),
        ToggleState::Off => ("Off", ChipTone::Idle),
    };
    ChipButton(format!("{name}: {label}"), tone, on_toggle);
}

#[composable]
pub(super) fn FocusedTransportBar(player: Player, tile_idx: usize, source_license: &'static str) {
    let play_pause = rememberPlayPauseButtonState(&player);
    let mute = rememberMuteButtonState(&player);
    let progress = rememberProgressStateWithTickInterval(&player, Duration::from_millis(200));
    let frac = progress.fraction().unwrap_or(0.0).clamp(0.0, 1.0);
    let pos_s = progress.current_position().as_secs_f32();
    let dur_s = progress.duration().map_or(0.0, |d| d.as_secs_f32());

    Row(
        Modifier::empty()
            .fill_max_width()
            .draw_behind(|scope| {
                scope.draw_round_rect(Brush::solid(BG_PANEL), CornerRadii::uniform(10.0));
            })
            .padding_each(12.0, 6.0, 12.0, 6.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(10.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let pp = play_pause.clone();
            ChipButton(
                if pp.show_play() {
                    format!("Play Tile {}", tile_idx + 1)
                } else {
                    format!("Pause Tile {}", tile_idx + 1)
                },
                ChipTone::from_active(!pp.show_play()),
                move || pp.on_click(),
            );
            let m = mute.clone();
            ChipButton(
                if m.is_muted() {
                    format!("Unmute Tile {}", tile_idx + 1)
                } else {
                    format!("Mute Tile {}", tile_idx + 1)
                },
                ChipTone::from_active(!m.is_muted()),
                move || m.on_click(),
            );
            Text(
                format!(
                    "Focused Tile #{} · {pos_s:.1}s / {dur_s:.1}s ({:.0}%) · License: {source_license}",
                    tile_idx + 1,
                    frac * 100.0
                ),
                Modifier::empty().weight(1.0),
                styled_text(11.0, TEXT_MUTED, None),
            );
        },
    );
}

#[composable]
pub(super) fn OpenMediaCatalogPanel(
    active_source: MutableState<ActiveSource>,
    custom_url_state: TextFieldState,
    catalog_open: MutableState<bool>,
    on_open_local_file: impl Fn() + Clone + 'static,
) {
    let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(|s| *s);

    Column(
        Modifier::empty()
            .fill_max_width()
            .height(290.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(0.05, 0.07, 0.12, 0.96)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(12.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Curated Open-Media Sources & License Audit (Stream on-demand or open local 4K/8K/120fps files — non-commercial datasets are never bundled)",
                Modifier::empty(),
                styled_text(12.0, TEXT_PRIMARY, Some(FontWeight::SEMI_BOLD)),
            );

            // Custom URL input + Open Local File row.
            let open_local = on_open_local_file.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Box(
                        Modifier::empty()
                            .weight(1.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.11, 0.14, 0.21, 1.0)),
                                    CornerRadii::uniform(8.0),
                                );
                            })
                            .padding_each(10.0, 8.0, 10.0, 8.0),
                        BoxSpec::default(),
                        move || {
                            BasicTextField(
                                custom_url_state,
                                Modifier::empty()
                                    .fill_max_width()
                                    .content_description("Custom video stream URL"),
                                styled_text(12.0, TEXT_PRIMARY, None),
                            );
                        },
                    );
                    ChipButton("Load Custom URL", ChipTone::Idle, move || {
                        let text = custom_url_state.text().trim().to_owned();
                        if !text.is_empty() {
                            active_source.set(ActiveSource::CustomUrl(text));
                            catalog_open.set(false);
                        }
                    });
                    let local_cb = open_local.clone();
                    ChipButton("Browse Local Video", ChipTone::Idle, move || local_cb());
                },
            );

            Column(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .vertical_scroll(scroll, false),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    let current = active_source.value();
                    for entry in CATALOG {
                        let selection = if current == ActiveSource::Catalog(entry.id) {
                            RowSelection::Selected
                        } else {
                            RowSelection::Idle
                        };
                        CatalogRow(entry, selection, active_source, catalog_open);
                    }
                },
            );
        },
    );
}

#[composable]
fn CatalogRow(
    entry: &'static CatalogEntry,
    selection: RowSelection,
    active_source: MutableState<ActiveSource>,
    catalog_open: MutableState<bool>,
) {
    let badge_color = match entry.license_kind {
        LicenseKind::PermissiveCommercial => COLOR_COMPOSITOR,
        LicenseKind::ShareAlikeCommercial => COLOR_DECODER,
        LicenseKind::NonCommercialOnly => COLOR_RESTRICTED,
        LicenseKind::VerifyPerFile => COLOR_WARN,
    };
    let row_bg = match selection {
        RowSelection::Selected => Color(0.11, 0.20, 0.36, 0.92),
        RowSelection::Idle => Color(0.09, 0.12, 0.18, 0.88),
    };
    let play_tone = match selection {
        RowSelection::Selected => ChipTone::Active,
        RowSelection::Idle => ChipTone::Idle,
    };

    Row(
        Modifier::empty()
            .fill_max_width()
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(row_bg), CornerRadii::uniform(8.0));
            })
            .padding_each(10.0, 8.0, 10.0, 8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(2.0)),
                move || {
                    Text(
                        format!("{} — {} [{}]", entry.title, entry.provider, entry.license),
                        Modifier::empty(),
                        styled_text(12.0, TEXT_PRIMARY, Some(FontWeight::SEMI_BOLD)),
                    );
                    Text(
                        format!(
                            "{} · {} · {}",
                            entry.license_kind.badge(),
                            entry.spec,
                            entry.note
                        ),
                        Modifier::empty(),
                        styled_text(11.0, badge_color, None),
                    );
                },
            );
            match entry.origin {
                MediaOrigin::BuiltInOffline | MediaOrigin::DirectStream { .. } => {
                    ChipButton(format!("Play {}", entry.title), play_tone, move || {
                        active_source.set(ActiveSource::Catalog(entry.id));
                        catalog_open.set(false);
                    });
                }
                MediaOrigin::ExternalPortal { page_url } => {
                    let uri_handler = cranpose_services::local_uri_handler().current();
                    ChipButton(
                        format!("Visit {}", entry.title),
                        ChipTone::Idle,
                        move || {
                            if let Err(err) = uri_handler.open_uri(page_url) {
                                log::warn!("video playground: open url {page_url} failed: {err}");
                            }
                        },
                    );
                }
            }
        },
    );
}

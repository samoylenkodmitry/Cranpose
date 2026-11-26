// Interactive multi-video wall engine (1 to 64 simultaneously playing surfaces) with lazy
// zero-recomposition GraphicsLayer animations: morphing rounded/circle clipping, real-time blur,
// transparency, 3D perspective transforms, custom WGSL shaders, and live layout switching.

use std::{sync::Arc, time::Duration};

use cranpose::{
    prelude::{AudioRole, ContentScale, Player, RepeatMode},
    rememberPlayerDecodedFrames, video_surface_supported, PlayerSurfaceIntake,
};
use cranpose_core::{mutableStateOf, with_key, MutableState};
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle, TextUnit},
    Alignment, Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Brush, Color, Column,
    ColumnSpec, CornerRadii, GraphicsLayer, Image, LinearArrangement, Modifier, Painter,
    RoundedCornerShape, Row, RowSpec, Text, TextStyle,
};
use cranpose_ui_graphics::{LayerShape, RenderEffect, TileMode};
use web_time::Instant;

use super::{
    catalog::{build_media_item, ActiveSource, BuiltinClipPack, StressScenario, WallLayout},
    shaders::{self, VideoShaderPreset},
    telemetry::{FrameTransferMode, TelemetryCounters},
    transport::player_status_message,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WallPlayback {
    Playing,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AudioOutput {
    Audible,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PlaybackInit {
    pub(super) playback: WallPlayback,
    pub(super) audio: AudioOutput,
    pub(super) focused_tile: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BadgeDensity {
    Full,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TileFocus {
    Normal,
    Focused,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SurfaceEffects {
    pub(super) animate: bool,
    pub(super) rounded_clip: bool,
    pub(super) transform_3d: bool,
    pub(super) blur: bool,
    pub(super) alpha_wave: bool,
    pub(super) shader: VideoShaderPreset,
}

impl Default for SurfaceEffects {
    fn default() -> Self {
        Self {
            animate: false,
            rounded_clip: true,
            transform_3d: false,
            blur: false,
            alpha_wave: false,
            shader: VideoShaderPreset::None,
        }
    }
}

#[derive(Clone)]
pub(super) struct DecoderSlot {
    pub(super) player: Player,
    pub(super) painter: MutableState<Option<Painter>>,
}

impl PartialEq for DecoderSlot {
    fn eq(&self, other: &Self) -> bool {
        self.player.id() == other.player.id() && self.painter == other.painter
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct DecoderRig {
    pub(super) generation: u64,
    pub(super) slots: Vec<DecoderSlot>,
}

impl DecoderRig {
    pub(super) fn empty() -> Self {
        Self {
            generation: 0,
            slots: Vec::new(),
        }
    }

    pub(super) fn build(
        generation: u64,
        decoder_count: usize,
        source: &ActiveSource,
        pack: &BuiltinClipPack,
        init: PlaybackInit,
    ) -> Self {
        let count = decoder_count.max(1);
        let focused_slot = init.focused_tile % count;
        let muted = init.audio == AudioOutput::Muted;
        let playing = init.playback == WallPlayback::Playing;
        let mut slots = Vec::with_capacity(count);
        for slot_idx in 0..count {
            let role = if slot_idx == focused_slot {
                AudioRole::Primary
            } else {
                AudioRole::Muted
            };
            let Ok(player) = Player::builder().with_audio_role(role).build() else {
                break;
            };
            let item = build_media_item(source, pack, slot_idx);
            player.set_media_item(item);
            player.set_repeat_mode(RepeatMode::One);
            player.set_muted(muted || slot_idx != focused_slot);
            // Stagger start offsets across multi-tile walls so adjacent tiles show distinct motion,
            // while single-tile showcase playback always begins cleanly at 0:00.
            let stagger_ms = if count > 1 {
                u64::try_from((slot_idx * 310) % 2400).unwrap_or(0)
            } else {
                0
            };
            player.prepare();
            if stagger_ms > 0 {
                player.seek_to(Duration::from_millis(stagger_ms));
            }
            if playing {
                player.play();
            } else {
                player.pause();
            }
            slots.push(DecoderSlot {
                player,
                painter: mutableStateOf(None),
            });
        }
        Self { generation, slots }
    }

    pub(super) fn release_all(&self) {
        for slot in &self.slots {
            slot.player.release();
        }
    }

    pub(super) fn set_playing(&self, mode: WallPlayback) {
        for slot in &self.slots {
            match mode {
                WallPlayback::Playing => slot.player.play(),
                WallPlayback::Paused => slot.player.pause(),
            }
        }
    }

    pub(super) fn set_audio_focus(&self, focused_tile: usize, audio: AudioOutput) {
        if self.slots.is_empty() {
            return;
        }
        let target = focused_tile % self.slots.len();
        let muted = audio == AudioOutput::Muted;
        for (idx, slot) in self.slots.iter().enumerate() {
            slot.player.set_muted(muted || idx != target);
        }
    }

    pub(super) fn players(&self) -> Vec<Player> {
        self.slots.iter().map(|s| s.player.clone()).collect()
    }
}

// Hidden per-decoder intake pumps: each active decoder converts its zero-copy GPU VideoFrame
// once via `PlayerSurfaceIntake` and publishes its `Painter` handle for any wall tile to draw.
#[composable]
pub(super) fn DecoderPumpHost(
    rig: DecoderRig,
    counters: Arc<TelemetryCounters>,
    transfer: FrameTransferMode,
) {
    for (idx, slot) in rig.slots.into_iter().enumerate() {
        let key = (rig.generation, idx);
        let counters = Arc::clone(&counters);
        with_key(&key, || {
            DecoderSlotPump(slot, counters, transfer);
        });
    }
}

#[composable]
fn DecoderSlotPump(
    slot: DecoderSlot,
    counters: Arc<TelemetryCounters>,
    transfer: FrameTransferMode,
) {
    let player = slot.player;
    let shared_painter = slot.painter;
    let Some(frames) = rememberPlayerDecodedFrames(&player) else {
        if shared_painter.value().is_some() {
            shared_painter.set(None);
        }
        return;
    };
    let counters = Arc::clone(&counters);
    PlayerSurfaceIntake(
        frames,
        Modifier::empty().size_points(0.0, 0.0),
        ContentScale::Crop,
        move |queue, presenter| {
            let Some(frame) = queue.latest_frame() else {
                return;
            };
            let t0 = Instant::now();
            // Inspect metadata before `present` consumes `frame` into the GPU converter.
            let size = frame.display_size();
            let color = frame.color();
            let result = presenter.present(frame, None);
            let elapsed = t0.elapsed();
            match result {
                Ok(()) => {
                    if let Some(painter) = presenter.current_painter() {
                        shared_painter.set(Some(painter));
                    }
                    player.notify_rendered_first_frame();
                    counters.record_presented_meta(size, color, elapsed, transfer);
                }
                Err(_) => {
                    counters.record_present_drop();
                }
            }
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct WallStageState {
    pub(super) scenario: MutableState<StressScenario>,
    pub(super) layout: MutableState<WallLayout>,
    pub(super) rig: MutableState<DecoderRig>,
    pub(super) effects: MutableState<SurfaceEffects>,
    pub(super) phase: MutableState<f32>,
    pub(super) focused_tile: MutableState<usize>,
}

#[composable]
pub(super) fn VideoWallStage(stage: WallStageState, modifier: Modifier) {
    BoxWithConstraints(
        modifier
            .clip_to_bounds()
            .background(Color::from_rgb_u8(0x07, 0x0A, 0x10)),
        move |scope| {
            // Read every reactive state inside BoxWithConstraints so SlotId(0) invalidates
            // whenever scenario, layout, decoder rig, or effects change under unchanged bounds.
            let scenario = stage.scenario.value();
            let layout = stage.layout.value();
            let slots = stage.rig.value().slots;
            let effects = stage.effects.value();
            let tile_count = scenario.tile_count();
            let (cols, rows) = scenario.grid();
            let max_w = scope.max_width().0.max(160.0);
            let max_h = scope.max_height().0.max(120.0);
            match layout {
                WallLayout::Spotlight if tile_count > 1 => {
                    SpotlightLayout(
                        tile_count,
                        slots,
                        effects,
                        stage.phase,
                        stage.focused_tile,
                        max_w,
                        max_h,
                    );
                }
                WallLayout::Carousel3d if tile_count > 1 => {
                    Carousel3dLayout(
                        tile_count,
                        slots,
                        effects,
                        stage.phase,
                        stage.focused_tile,
                        max_w,
                        max_h,
                    );
                }
                _ => {
                    GridWallLayout(
                        GridLayoutSpec {
                            cols,
                            rows,
                            wave_mode: layout == WallLayout::MosaicWave,
                            stage_w: max_w,
                            stage_h: max_h,
                        },
                        slots,
                        effects,
                        stage.phase,
                        stage.focused_tile,
                    );
                }
            }
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct GridLayoutSpec {
    cols: usize,
    rows: usize,
    wave_mode: bool,
    stage_w: f32,
    stage_h: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct TileMatrixSpec {
    cols: usize,
    rows: usize,
    visible: usize,
    gap: f32,
    pad: f32,
    tile_w: f32,
    tile_h: f32,
    wave_mode: bool,
    carousel: bool,
    compact_badges: bool,
}

#[composable]
fn TileMatrix(
    spec: TileMatrixSpec,
    slots: Vec<DecoderSlot>,
    effects: SurfaceEffects,
    phase: MutableState<f32>,
    focused_tile: MutableState<usize>,
) {
    Column(
        Modifier::empty().fill_max_size().padding(spec.pad),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(spec.gap)),
        move || {
            for r in 0..spec.rows {
                let row_slots = slots.clone();
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(spec.gap)),
                    move || {
                        for c in 0..spec.cols {
                            let tile_idx = r * spec.cols + c;
                            if tile_idx >= spec.visible {
                                continue;
                            }
                            let slot = row_slots.get(tile_idx % row_slots.len().max(1)).cloned();
                            let motion = TileMotion {
                                tile_idx,
                                cols: spec.cols,
                                rows: spec.rows,
                                wave_mode: spec.wave_mode,
                                carousel: spec.carousel,
                                tile_w: spec.tile_w,
                                tile_h: spec.tile_h,
                                compact_badges: spec.compact_badges,
                            };
                            with_key(&tile_idx, || {
                                VideoWallTile(slot, motion, effects, phase, focused_tile);
                            });
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn GridWallLayout(
    spec: GridLayoutSpec,
    slots: Vec<DecoderSlot>,
    effects: SurfaceEffects,
    phase: MutableState<f32>,
    focused_tile: MutableState<usize>,
) {
    let GridLayoutSpec {
        cols,
        rows,
        wave_mode,
        stage_w,
        stage_h,
    } = spec;
    let gap = if cols >= 8 {
        4.0
    } else if cols >= 4 {
        6.0
    } else {
        10.0
    };
    let pad = if cols >= 8 { 4.0 } else { 8.0 };
    let usable_w = (stage_w - pad * 2.0 - gap * (cols.saturating_sub(1) as f32)).max(24.0);
    let usable_h = (stage_h - pad * 2.0 - gap * (rows.saturating_sub(1) as f32)).max(24.0);
    let matrix = TileMatrixSpec {
        cols,
        rows,
        visible: cols * rows,
        gap,
        pad,
        tile_w: (usable_w / (cols as f32)).floor().max(24.0),
        tile_h: (usable_h / (rows as f32)).floor().max(24.0),
        wave_mode,
        carousel: false,
        compact_badges: cols >= 8,
    };
    TileMatrix(matrix, slots, effects, phase, focused_tile);
}

#[composable]
fn Carousel3dLayout(
    tile_count: usize,
    slots: Vec<DecoderSlot>,
    effects: SurfaceEffects,
    phase: MutableState<f32>,
    focused_tile: MutableState<usize>,
    stage_w: f32,
    stage_h: f32,
) {
    let visible = tile_count.min(12);
    let cols = if visible <= 4 { visible } else { 4 };
    let rows = visible.div_ceil(cols);
    let gap = 12.0;
    let matrix = TileMatrixSpec {
        cols,
        rows,
        visible,
        gap,
        pad: 16.0,
        tile_w: ((stage_w - 32.0 - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(60.0),
        tile_h: ((stage_h - 32.0 - gap * (rows.saturating_sub(1) as f32)) / rows as f32).max(52.0),
        wave_mode: true,
        carousel: true,
        compact_badges: false,
    };
    let forced_3d = SurfaceEffects {
        transform_3d: true,
        rounded_clip: true,
        ..effects
    };
    TileMatrix(matrix, slots, forced_3d, phase, focused_tile);
}

#[composable]
fn SpotlightLayout(
    tile_count: usize,
    slots: Vec<DecoderSlot>,
    effects: SurfaceEffects,
    phase: MutableState<f32>,
    focused_tile: MutableState<usize>,
    stage_w: f32,
    stage_h: f32,
) {
    let active = focused_tile.value().min(tile_count.saturating_sub(1));
    let hero_slot = slots.get(active % slots.len().max(1)).cloned();
    let strip_h = 92.0_f32.min((stage_h * 0.24).max(56.0));
    let hero_w = (stage_w - 20.0).max(160.0);
    let hero_h = (stage_h - strip_h - 28.0).max(120.0);
    let strip_count = tile_count.min(8);
    let strip_w =
        ((hero_w - 6.0 * (strip_count.saturating_sub(1) as f32)) / strip_count as f32).max(40.0);

    Column(
        Modifier::empty().fill_max_size().padding(10.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            let hero_motion = TileMotion {
                tile_idx: active,
                cols: 1,
                rows: 1,
                wave_mode: false,
                carousel: false,
                tile_w: hero_w,
                tile_h: hero_h,
                compact_badges: false,
            };
            VideoWallTile(hero_slot.clone(), hero_motion, effects, phase, focused_tile);

            let strip_slots = slots.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    for i in 0..strip_count {
                        let slot = strip_slots.get(i % strip_slots.len().max(1)).cloned();
                        let motion = TileMotion {
                            tile_idx: i,
                            cols: strip_count,
                            rows: 1,
                            wave_mode: false,
                            carousel: false,
                            tile_w: strip_w,
                            tile_h: strip_h,
                            compact_badges: true,
                        };
                        with_key(&i, || {
                            VideoWallTile(slot, motion, effects, phase, focused_tile);
                        });
                    }
                },
            );
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct TileMotion {
    tile_idx: usize,
    cols: usize,
    rows: usize,
    wave_mode: bool,
    carousel: bool,
    tile_w: f32,
    tile_h: f32,
    compact_badges: bool,
}

#[composable]
fn VideoWallTile(
    slot: Option<DecoderSlot>,
    motion: TileMotion,
    effects: SurfaceEffects,
    phase: MutableState<f32>,
    focused_tile: MutableState<usize>,
) {
    let idx = motion.tile_idx;
    let focus = if focused_tile.value() == idx {
        TileFocus::Focused
    } else {
        TileFocus::Normal
    };
    let density = if motion.compact_badges {
        BadgeDensity::Compact
    } else {
        BadgeDensity::Full
    };
    let accent = tile_accent(idx);

    let outer_modifier = Modifier::empty()
        .size_points(motion.tile_w, motion.tile_h)
        .graphics_layer(move || build_tile_transform_layer(motion, effects, phase.value(), focus))
        .clickable(move |_| {
            focused_tile.set(idx);
        })
        .content_description(format!("Video tile {}", idx + 1));

    let border_color = match focus {
        TileFocus::Focused => Color::from_rgb_u8(0x60, 0xA5, 0xFA),
        TileFocus::Normal => Color(1.0, 1.0, 1.0, 0.14),
    };

    Box(outer_modifier, BoxSpec::default(), move || {
        // Keep rounded/morphing clip on an axis-aligned inner layer so it stays anti-aliased
        // under 3D perspective transforms and uses the direct image shader clip when flat.
        let inner_clip = Modifier::empty()
            .fill_max_size()
            .graphics_layer(move || build_tile_clip_layer(motion, effects, phase.value()))
            .draw_behind(move |scope| {
                let radius = if effects.rounded_clip { 12.0 } else { 3.0 };
                scope.draw_round_rect(
                    Brush::solid(Color::from_rgb_u8(0x0E, 0x13, 0x1E)),
                    CornerRadii::uniform(radius),
                );
                scope.draw_round_rect(
                    Brush::solid(Color(accent.r(), accent.g(), accent.b(), 0.22)),
                    CornerRadii::uniform(radius),
                );
            });
        let slot_for_tile = slot.clone();
        let content_scale = if motion.cols == 1 {
            ContentScale::Fit
        } else {
            ContentScale::Crop
        };
        Box(
            inner_clip,
            BoxSpec::new().content_alignment(Alignment::BOTTOM_START),
            move || {
                // Isolate per-frame video `Painter` reads to a leaf composable so decoded frames do
                // not recompose the outer tile box, clickable modifier, or graphics layer.
                TileVideoSurface(slot_for_tile.clone(), idx, accent, density, content_scale);
                if density == BadgeDensity::Full || focus == TileFocus::Focused {
                    TileOverlayPill(idx, focus, border_color);
                }
            },
        );
    });
}

#[composable]
fn TileVideoSurface(
    slot: Option<DecoderSlot>,
    idx: usize,
    accent: Color,
    density: BadgeDensity,
    content_scale: ContentScale,
) {
    let painter = slot.as_ref().and_then(|s| s.painter.value());
    if let Some(painter) = painter {
        Image(
            painter,
            None,
            Modifier::empty().fill_max_size(),
            Alignment::CENTER,
            content_scale,
            1.0,
            None,
        );
    } else {
        let player = slot.map(|s| s.player);
        TileFallbackBackdrop(player, idx, accent, density);
    }
}

#[composable]
fn TileFallbackBackdrop(player: Option<Player>, idx: usize, accent: Color, density: BadgeDensity) {
    let fallback_color = Color(
        (accent.r() + 0.5).min(1.0),
        (accent.g() + 0.5).min(1.0),
        (accent.b() + 0.5).min(1.0),
        0.92,
    );
    let (status, text_color) = if !video_surface_supported() {
        (
            format!("Tile {} · CPU framebuffer mode", idx + 1),
            fallback_color,
        )
    } else if let Some(active) = player.as_ref() {
        let (msg, color) = player_status_message(active, &format!("Tile {}", idx + 1));
        if color == Color::from_rgb_u8(0xF8, 0x71, 0x71) {
            (msg, color)
        } else {
            (msg, fallback_color)
        }
    } else {
        (
            format!("Tile {} · Initializing decoder...", idx + 1),
            fallback_color,
        )
    };
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            if density == BadgeDensity::Full {
                Text(
                    status.clone(),
                    Modifier::empty().padding(8.0),
                    TextStyle {
                        span_style: SpanStyle {
                            color: Some(text_color),
                            font_size: TextUnit::Sp(11.5),
                            font_weight: Some(FontWeight::MEDIUM),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
            }
        },
    );
}

#[composable]
fn TileOverlayPill(idx: usize, focus: TileFocus, border_color: Color) {
    let is_focused = focus == TileFocus::Focused;
    let label = if is_focused {
        format!("#{} LIVE · AUDIO", idx + 1)
    } else {
        format!("#{} GPU", idx + 1)
    };
    let pill_bg = if is_focused {
        Color(0.08, 0.22, 0.45, 0.78)
    } else {
        Color(0.03, 0.05, 0.09, 0.62)
    };
    Box(
        Modifier::empty()
            .padding(6.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(border_color), CornerRadii::uniform(7.0));
                scope.draw_round_rect(Brush::solid(pill_bg), CornerRadii::uniform(6.0));
            })
            .padding_each(7.0, 3.0, 7.0, 3.0),
        BoxSpec::default(),
        move || {
            Text(
                label.clone(),
                Modifier::empty(),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(Color::from_rgb_u8(0xE5, 0xED, 0xFF)),
                        font_size: TextUnit::Sp(10.0),
                        font_weight: Some(FontWeight::SEMI_BOLD),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
        },
    );
}

fn tile_wave(motion: TileMotion, effects: SurfaceEffects, t: f32) -> (f32, f32, f32) {
    let col = motion.tile_idx % motion.cols.max(1);
    let row = motion.tile_idx / motion.cols.max(1);
    let tile_phase = (col as f32) * 0.55 + (row as f32) * 0.45;
    let wave = if effects.animate {
        (t * 1.9 + tile_phase).sin()
    } else {
        0.0
    };
    let cos_wave = if effects.animate {
        (t * 1.3 + tile_phase).cos()
    } else {
        0.0
    };
    (tile_phase, wave, cos_wave)
}

fn build_tile_clip_layer(motion: TileMotion, effects: SurfaceEffects, t: f32) -> GraphicsLayer {
    let (_, wave, _) = tile_wave(motion, effects, t);
    let min_side = motion.tile_w.min(motion.tile_h);
    let corner_radius = if effects.rounded_clip {
        if motion.wave_mode && effects.animate {
            let morph = 0.5 + 0.5 * wave;
            (6.0 + morph * (min_side * 0.34)).min(min_side * 0.48)
        } else {
            (min_side * 0.14).clamp(6.0, 26.0)
        }
    } else {
        2.0
    };
    GraphicsLayer {
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(corner_radius)),
        clip: true,
        ..GraphicsLayer::DEFAULT
    }
}

fn build_tile_transform_layer(
    motion: TileMotion,
    effects: SurfaceEffects,
    t: f32,
    focus: TileFocus,
) -> GraphicsLayer {
    let is_focused = focus == TileFocus::Focused;
    let col = motion.tile_idx % motion.cols.max(1);
    let (tile_phase, wave, cos_wave) = tile_wave(motion, effects, t);

    let (rot_x, rot_y, rot_z, scale) = if motion.carousel {
        let norm_x = if motion.cols > 1 {
            (col as f32 / (motion.cols - 1) as f32) * 2.0 - 1.0
        } else {
            0.0
        };
        let ry = norm_x * 26.0 + wave * 12.0;
        let rx = cos_wave * 9.0;
        let sc = 0.90 + 0.08 * (1.0 - norm_x.abs()) + if is_focused { 0.04 } else { 0.0 };
        (rx, ry, norm_x * 2.5, sc)
    } else if effects.transform_3d {
        let amp = if motion.wave_mode { 18.0 } else { 9.0 };
        let rx = cos_wave * amp * 0.7;
        let ry = wave * amp;
        let rz = if motion.wave_mode { wave * 3.5 } else { 0.0 };
        let sc = if motion.wave_mode {
            0.93 + 0.06 * (0.5 + 0.5 * wave)
        } else {
            0.985 + if is_focused { 0.015 } else { 0.0 }
        };
        (rx, ry, rz, sc)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };

    let alpha = if effects.alpha_wave {
        0.58 + 0.42 * (0.5 + 0.5 * (t * 2.1 + tile_phase).sin())
    } else {
        1.0
    };

    // Real-time GPU effect: WGSL RuntimeShader takes priority when active; otherwise live Gaussian
    // blur when enabled (applied selectively on non-focused tiles or pulsing smoothly).
    let render_effect = if effects.shader != VideoShaderPreset::None {
        shaders::render_effect(effects.shader, t, 1.0, tile_phase)
    } else if effects.blur {
        let radius = if is_focused && motion.cols > 1 {
            0.0
        } else if effects.animate {
            2.0 + 4.5 * (0.5 + 0.5 * wave)
        } else {
            4.5
        };
        (radius > 0.1).then(|| RenderEffect::blur_xy(radius, radius, TileMode::Clamp))
    } else {
        None
    };

    GraphicsLayer {
        alpha,
        scale,
        rotation_x: rot_x,
        rotation_y: rot_y,
        rotation_z: rot_z,
        camera_distance: 10.0,
        render_effect,
        ..GraphicsLayer::DEFAULT
    }
}

fn tile_accent(idx: usize) -> Color {
    const PALETTE: [Color; 6] = [
        Color::from_rgb_u8(0x3B, 0x82, 0xF6),
        Color::from_rgb_u8(0x8B, 0x5C, 0xF6),
        Color::from_rgb_u8(0x10, 0xB9, 0x81),
        Color::from_rgb_u8(0xF5, 0x9E, 0x0B),
        Color::from_rgb_u8(0xEC, 0x48, 0x99),
        Color::from_rgb_u8(0x06, 0xB6, 0xD4),
    ];
    PALETTE[idx % PALETTE.len()]
}

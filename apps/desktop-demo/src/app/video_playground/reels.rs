// TikTok-style snap-scrolling vertical video gallery powered by VerticalPager, bounded
// PlayerPool preloading (max 2 decoders), live GPU telemetry intake, tap-to-play/pause,
// floating action rail, and interactive seek scrubber.

use std::sync::Arc;

use cranpose::{
    prelude::{
        rememberMuteButtonState, rememberPlayPauseButtonState, rememberPlaybackSpeedState,
        rememberPlayerPool, ContentScale, LoadControl, MediaItem, Player, PlayerPool,
        PlayerPoolConfig, RepeatMode,
    },
    rememberPlayerDecodedFrames, video_surface_supported, PlayerSurfaceIntake,
};
use cranpose_core::{
    remember, rememberMutableStateOf, DisposableEffect, DisposableEffectResult, LaunchedEffect,
    MutableState,
};
use cranpose_ui::{
    composable,
    text::FontWeight,
    widgets::{rememberPagerState, PagerSpec, PagerState, VerticalPager},
    Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer, Image,
    LinearArrangement, Modifier, Painter, RoundedCornerShape, Row, RowSpec, Text,
    VerticalAlignment,
};
use cranpose_ui_graphics::{LayerShape, RenderEffect, TileMode};
use web_time::Instant;

use super::{
    catalog::{build_media_item, entry_by_id, ActiveSource, BuiltinClipPack, REEL_FEED_IDS},
    controls::{styled_text, ChipButton, ChipTone},
    shaders::{self, VideoShaderPreset},
    telemetry::{FrameTransferMode, TelemetryCounters},
    transport::{format_speed, next_playback_speed, player_status_message, VideoSeekScrubber},
    wall::SurfaceEffects,
};

const BG_CARD: Color = Color::from_rgb_u8(0x06, 0x09, 0x10);
const BG_OVERLAY: Color = Color(0.04, 0.06, 0.11, 0.80);
const BG_RAIL: Color = Color(0.04, 0.06, 0.11, 0.72);
const TEXT_PRIMARY: Color = Color::from_rgb_u8(0xF3, 0xF6, 0xFC);
const TEXT_MUTED: Color = Color::from_rgb_u8(0xB4, 0xC2, 0xD8);
const ACCENT_REEL: Color = Color::from_rgb_u8(0x38, 0xBD, 0xF8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReelScaleMode {
    Fit,
    Fill,
}

impl ReelScaleMode {
    const fn content_scale(self) -> ContentScale {
        match self {
            Self::Fit => ContentScale::Fit,
            Self::Fill => ContentScale::Crop,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Fit => "Fit",
            Self::Fill => "Fill",
        }
    }

    const fn toggle(self) -> Self {
        match self {
            Self::Fit => Self::Fill,
            Self::Fill => Self::Fit,
        }
    }
}

#[derive(Clone, PartialEq)]
struct ReelFeedEntry {
    key: String,
    catalog_id: Option<&'static str>,
    title: String,
    provider: String,
    spec: String,
    license: &'static str,
    item: MediaItem,
}

fn build_reel_feed(active_source: &ActiveSource, pack: &BuiltinClipPack) -> Vec<ReelFeedEntry> {
    let mut entries = Vec::with_capacity(REEL_FEED_IDS.len() + 1);
    match active_source {
        ActiveSource::CustomUrl(url) => {
            entries.push(ReelFeedEntry {
                key: format!("reel-custom-{url}"),
                catalog_id: None,
                title: "Custom Stream".to_owned(),
                provider: "User URL".to_owned(),
                spec: url.clone(),
                license: "User-supplied stream URL",
                item: build_media_item(active_source, pack, 0),
            });
        }
        ActiveSource::LocalUri { uri, label } => {
            entries.push(ReelFeedEntry {
                key: format!("reel-local-{uri}"),
                catalog_id: None,
                title: label.clone(),
                provider: "Local Video".to_owned(),
                spec: "Local file playback".to_owned(),
                license: "Local user file",
                item: build_media_item(active_source, pack, 0),
            });
        }
        ActiveSource::Catalog(_) => {}
    }
    for (idx, &id) in REEL_FEED_IDS.iter().enumerate() {
        let meta = entry_by_id(id);
        let source = ActiveSource::Catalog(id);
        entries.push(ReelFeedEntry {
            key: format!("reel-{id}"),
            catalog_id: Some(id),
            title: meta.title.to_owned(),
            provider: meta.provider.to_owned(),
            spec: meta.spec.to_owned(),
            license: meta.license,
            item: build_media_item(&source, pack, idx),
        });
    }
    entries
}

#[derive(Clone, PartialEq)]
pub(super) struct ReelsStageConfig {
    pub(super) active_source: MutableState<ActiveSource>,
    pub(super) effects: MutableState<SurfaceEffects>,
    pub(super) phase: MutableState<f32>,
    pub(super) muted: MutableState<bool>,
    pub(super) playing: MutableState<bool>,
    pub(super) active_reel_player: MutableState<Option<Player>>,
    pub(super) clip_pack: BuiltinClipPack,
    pub(super) counters: Arc<TelemetryCounters>,
    pub(super) transfer: FrameTransferMode,
}

#[composable]
pub(super) fn ReelsGalleryStage(config: ReelsStageConfig, modifier: Modifier) {
    let entries = remember_reel_entries(&config.active_source.value(), &config.clip_pack);
    let pool = rememberPlayerPool(PlayerPoolConfig {
        max_concurrent_players: 2,
        load_control: LoadControl::FEED,
        preload_ahead: 1,
        preload_behind: 0,
    });
    let pool_version = rememberMutableStateOf(|| 0_u64);
    let scale_mode = rememberMutableStateOf(|| ReelScaleMode::Fit);
    let pager = rememberPagerState(0);
    let current_page = pager.current_page().min(entries.len().saturating_sub(1));

    ReelsPoolCoordinator(&config, &entries, &pool, pager, current_page, pool_version);

    let count = entries.len();
    let pager_spec = PagerSpec {
        beyond_viewport_page_count: 1,
        ..PagerSpec::default()
    };

    Box(
        modifier.clip_to_bounds().background(BG_CARD),
        BoxSpec::default(),
        move || {
            let page_entries = entries.clone();
            let page_pool = pool.clone();
            let page_counters = Arc::clone(&config.counters);
            let page_effects = config.effects;
            let page_phase = config.phase;
            let page_muted = config.muted;
            let page_transfer = config.transfer;
            VerticalPager(
                Modifier::empty().fill_max_size(),
                pager,
                pager_spec,
                count,
                move |page| {
                    if let Some(entry) = page_entries.get(page).cloned() {
                        ReelCard(ReelCardProps {
                            entry,
                            page,
                            total: count,
                            pool: page_pool.clone(),
                            pool_version,
                            pager,
                            effects: page_effects,
                            phase: page_phase,
                            muted: page_muted,
                            scale_mode,
                            counters: Arc::clone(&page_counters),
                            transfer: page_transfer,
                        });
                    }
                },
            );
        },
    );
}

#[composable]
fn remember_reel_entries(cur_source: &ActiveSource, pack: &BuiltinClipPack) -> Vec<ReelFeedEntry> {
    let base = remember({
        let pack = pack.clone();
        let default_src = ActiveSource::Catalog(REEL_FEED_IDS[0]);
        move || build_reel_feed(&default_src, &pack)
    })
    .with(Clone::clone);
    if matches!(
        cur_source,
        ActiveSource::CustomUrl(_) | ActiveSource::LocalUri { .. }
    ) {
        build_reel_feed(cur_source, pack)
    } else {
        base
    }
}

#[composable]
fn ReelsPoolCoordinator(
    config: &ReelsStageConfig,
    entries: &[ReelFeedEntry],
    pool: &PlayerPool,
    pager: PagerState,
    current_page: usize,
    pool_version: MutableState<u64>,
) {
    let cur_source = config.active_source.value();
    if let ActiveSource::Catalog(selected_id) = cur_source {
        let target_idx = entries
            .iter()
            .position(|e| e.catalog_id == Some(selected_id));
        LaunchedEffect(selected_id, async move |_| {
            if let Some(idx) = target_idx {
                if pager.current_page() != idx {
                    pager.scroll_to_page(idx);
                }
            }
        });
    }

    let is_muted = config.muted.value();
    let mute_pool = pool.clone();
    LaunchedEffect(is_muted, async move |_| {
        mute_pool.set_user_muted(is_muted);
    });

    let sync_pool = pool.clone();
    let sync_items: Vec<(String, MediaItem)> = entries
        .iter()
        .map(|entry| (entry.key.clone(), entry.item.clone()))
        .collect();
    let active_player_state = config.active_reel_player;
    let is_playing = config.playing.value();
    LaunchedEffect((current_page, is_playing), async move |_| {
        if let Err(err) = sync_pool.update_feed_window(&sync_items, current_page) {
            log::warn!("video playground reels: pool window update failed: {err}");
            return;
        }
        let active = sync_items
            .get(current_page)
            .and_then(|(key, _)| sync_pool.player_for(key));
        if let Some(player) = &active {
            player.set_repeat_mode(RepeatMode::One);
            if is_playing {
                player.play();
            } else {
                player.pause();
            }
        }
        active_player_state.set(active);
        pool_version.set(pool_version.value().saturating_add(1));
    });

    DisposableEffect((), move |_| {
        DisposableEffectResult::new(move || {
            active_player_state.set(None);
        })
    });
}

#[derive(Clone, PartialEq)]
struct ReelCardProps {
    entry: ReelFeedEntry,
    page: usize,
    total: usize,
    pool: PlayerPool,
    pool_version: MutableState<u64>,
    pager: PagerState,
    effects: MutableState<SurfaceEffects>,
    phase: MutableState<f32>,
    muted: MutableState<bool>,
    scale_mode: MutableState<ReelScaleMode>,
    counters: Arc<TelemetryCounters>,
    transfer: FrameTransferMode,
}

#[composable]
fn ReelCard(props: ReelCardProps) {
    let _version = props.pool_version.value();
    let player = props.pool.player_for(&props.entry.key);
    let effects = props.effects.value();
    let scale = props.scale_mode.value();

    Box(
        Modifier::empty()
            .fill_max_size()
            .background(BG_CARD)
            .content_description(format!("Reel {} {}", props.page + 1, props.entry.title)),
        BoxSpec::default(),
        move || {
            if let Some(active) = player.clone() {
                ReelVideoCanvas(
                    active.clone(),
                    props.entry.title.clone(),
                    effects,
                    props.phase,
                    scale,
                    Arc::clone(&props.counters),
                    props.transfer,
                );
                ReelOverlayChrome(active, &props);
            } else {
                ReelPreloadShutter(&props.entry, props.page, props.total);
            }
        },
    );
}

#[composable]
fn ReelVideoCanvas(
    player: Player,
    title: String,
    effects: SurfaceEffects,
    phase: MutableState<f32>,
    scale_mode: ReelScaleMode,
    counters: Arc<TelemetryCounters>,
    transfer: FrameTransferMode,
) {
    let painter_state = rememberMutableStateOf(|| None::<Painter>);
    let play_pause = rememberPlayPauseButtonState(&player);
    let tap_pp = play_pause.clone();

    if let Some(frames) = rememberPlayerDecodedFrames(&player) {
        let intake_player = player.clone();
        let intake_counters = Arc::clone(&counters);
        PlayerSurfaceIntake(
            frames,
            Modifier::empty().size_points(0.0, 0.0),
            scale_mode.content_scale(),
            move |queue, presenter| {
                let Some(frame) = queue.latest_frame() else {
                    return;
                };
                let t0 = Instant::now();
                let size = frame.display_size();
                let color = frame.color();
                match presenter.present(frame, None) {
                    Ok(()) => {
                        if let Some(painter) = presenter.current_painter() {
                            painter_state.set(Some(painter));
                        }
                        intake_player.notify_rendered_first_frame();
                        intake_counters.record_presented_meta(size, color, t0.elapsed(), transfer);
                    }
                    Err(_) => intake_counters.record_present_drop(),
                }
            },
        );
    }

    let video_modifier = Modifier::empty()
        .fill_max_size()
        .graphics_layer(move || build_reel_layer(effects, phase.value()))
        .clickable(move |_| tap_pp.on_click());

    Box(
        video_modifier,
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            if let Some(painter) = painter_state.value() {
                Image(
                    painter,
                    None,
                    Modifier::empty().fill_max_size(),
                    Alignment::CENTER,
                    scale_mode.content_scale(),
                    1.0,
                    None,
                );
                if play_pause.show_play() {
                    Box(
                        Modifier::empty()
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.04, 0.06, 0.11, 0.78)),
                                    CornerRadii::uniform(12.0),
                                );
                            })
                            .padding_each(18.0, 10.0, 18.0, 10.0),
                        BoxSpec::default(),
                        || {
                            Text(
                                "PAUSED · Tap to resume",
                                Modifier::empty(),
                                styled_text(14.0, TEXT_PRIMARY, Some(FontWeight::BOLD)),
                            );
                        },
                    );
                }
            } else {
                let (status, color) = if video_surface_supported() {
                    player_status_message(&player, &title)
                } else {
                    (format!("{title} · CPU framebuffer mode"), TEXT_MUTED)
                };
                Text(
                    status,
                    Modifier::empty().padding(16.0),
                    styled_text(13.0, color, Some(FontWeight::MEDIUM)),
                );
            }
        },
    );
}

#[composable]
fn ReelOverlayChrome(player: Player, props: &ReelCardProps) {
    ReelCounterBadge(props.page, props.total);
    ReelActionRail(player.clone(), props);
    ReelBottomMetadata(player, &props.entry);
}

#[composable]
fn ReelCounterBadge(page: usize, total: usize) {
    Box(
        Modifier::empty().fill_max_size().padding(12.0),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            Box(
                Modifier::empty()
                    .draw_behind(|scope| {
                        scope.draw_round_rect(Brush::solid(BG_OVERLAY), CornerRadii::uniform(8.0));
                    })
                    .padding_each(10.0, 5.0, 10.0, 5.0),
                BoxSpec::default(),
                move || {
                    Text(
                        format!("Reel {} / {} · Swipe up/down", page + 1, total),
                        Modifier::empty(),
                        styled_text(11.0, ACCENT_REEL, Some(FontWeight::SEMI_BOLD)),
                    );
                },
            );
        },
    );
}

#[composable]
fn ReelActionRail(player: Player, props: &ReelCardProps) {
    let play_pause = rememberPlayPauseButtonState(&player);
    let mute_state = rememberMuteButtonState(&player);
    let speed_state = rememberPlaybackSpeedState(&player);
    let cur_fx = props.effects.value();
    let cur_scale = props.scale_mode.value();
    let cur_speed = speed_state.playback_speed();
    let page = props.page;
    let total = props.total;
    let pager = props.pager;
    let rail_effects = props.effects;
    let rail_muted = props.muted;
    let rail_scale = props.scale_mode;

    Box(
        Modifier::empty()
            .fill_max_size()
            .padding_each(12.0, 12.0, 12.0, 88.0),
        BoxSpec::new().content_alignment(Alignment::CENTER_END),
        move || {
            let col_pp = play_pause.clone();
            let col_mute = mute_state.clone();
            let sp = speed_state.clone();
            Column(
                Modifier::empty()
                    .draw_behind(|scope| {
                        scope.draw_round_rect(Brush::solid(BG_RAIL), CornerRadii::uniform(12.0));
                    })
                    .padding(6.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    if page > 0 {
                        ChipButton("Prev", ChipTone::Idle, move || {
                            pager.animate_scroll_to_page(page - 1);
                        });
                    }
                    if page + 1 < total {
                        ChipButton("Next", ChipTone::Idle, move || {
                            pager.animate_scroll_to_page(page + 1);
                        });
                    }
                    let pp = col_pp.clone();
                    ChipButton(
                        if pp.show_play() { "Play" } else { "Pause" },
                        ChipTone::from_active(!pp.show_play()),
                        move || pp.on_click(),
                    );
                    let m = col_mute.clone();
                    ChipButton(
                        if m.is_muted() { "Unmute" } else { "Mute" },
                        ChipTone::from_active(!m.is_muted()),
                        move || {
                            let next = !m.is_muted();
                            rail_muted.set(next);
                            m.on_click();
                        },
                    );
                    let speed_btn = sp.clone();
                    ChipButton(
                        format_speed(cur_speed),
                        ChipTone::from_active((cur_speed - 1.0).abs() > 0.05),
                        move || speed_btn.update_playback_speed(next_playback_speed(cur_speed)),
                    );
                    ChipButton(
                        cur_scale.label(),
                        ChipTone::from_active(cur_scale == ReelScaleMode::Fill),
                        move || rail_scale.set(cur_scale.toggle()),
                    );
                    ChipButton(
                        cur_fx.shader.label(),
                        ChipTone::from_active(cur_fx.shader != VideoShaderPreset::None),
                        move || {
                            rail_effects.set(SurfaceEffects {
                                shader: cur_fx.shader.next(),
                                ..cur_fx
                            });
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn ReelBottomMetadata(player: Player, entry: &ReelFeedEntry) {
    let title = entry.title.clone();
    let meta_line = format!("{} · {} · {}", entry.provider, entry.spec, entry.license);
    Box(
        Modifier::empty().fill_max_size().padding(10.0),
        BoxSpec::new().content_alignment(Alignment::BOTTOM_START),
        move || {
            let scrub_player = player.clone();
            let col_title = title.clone();
            let col_meta = meta_line.clone();
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .draw_behind(|scope| {
                        scope.draw_round_rect(Brush::solid(BG_OVERLAY), CornerRadii::uniform(10.0));
                    })
                    .padding_each(12.0, 8.0, 12.0, 8.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    let row_title = col_title.clone();
                    let row_meta = col_meta.clone();
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::SpaceBetween)
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        move || {
                            let text_title = row_title.clone();
                            let text_meta = row_meta.clone();
                            Column(
                                Modifier::empty().weight(1.0),
                                ColumnSpec::new()
                                    .vertical_arrangement(LinearArrangement::SpacedBy(1.0)),
                                move || {
                                    Text(
                                        text_title.clone(),
                                        Modifier::empty(),
                                        styled_text(13.5, TEXT_PRIMARY, Some(FontWeight::BOLD)),
                                    );
                                    Text(
                                        text_meta.clone(),
                                        Modifier::empty(),
                                        styled_text(10.5, TEXT_MUTED, None),
                                    );
                                },
                            );
                        },
                    );
                    VideoSeekScrubber(scrub_player.clone(), Modifier::empty().fill_max_width());
                },
            );
        },
    );
}

#[composable]
fn ReelPreloadShutter(entry: &ReelFeedEntry, page: usize, total: usize) {
    let title = entry.title.clone();
    let spec = format!("Reel {} / {} · {}", page + 1, total, entry.spec);
    Box(
        Modifier::empty().fill_max_size().background(BG_CARD),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            let col_title = title.clone();
            let col_spec = spec.clone();
            Column(
                Modifier::empty().padding(20.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    Text(
                        col_title.clone(),
                        Modifier::empty(),
                        styled_text(18.0, TEXT_PRIMARY, Some(FontWeight::BOLD)),
                    );
                    Text(
                        col_spec.clone(),
                        Modifier::empty(),
                        styled_text(12.0, TEXT_MUTED, None),
                    );
                },
            );
        },
    );
}

fn build_reel_layer(effects: SurfaceEffects, t: f32) -> GraphicsLayer {
    let render_effect = if effects.shader != VideoShaderPreset::None {
        shaders::render_effect(effects.shader, t, 1.0, 0.0)
    } else if effects.blur {
        Some(RenderEffect::blur_xy(4.0, 4.0, TileMode::Clamp))
    } else {
        None
    };
    GraphicsLayer {
        shape: if effects.rounded_clip {
            LayerShape::Rounded(RoundedCornerShape::uniform(12.0))
        } else {
            LayerShape::Rectangle
        },
        clip: effects.rounded_clip,
        render_effect,
        ..GraphicsLayer::DEFAULT
    }
}

// Video Playground showcase tab: real-time multi-video GPU wall (1 to 64 live surfaces),
// TikTok-style scrollable vertical Reels gallery, stress-test throughput scenarios, optional
// animated clipping/blur/3D/alpha/WGSL shaders, independent telemetry HUD, interactive seek
// controls, curated CORS-verified open-media license catalog, and local 4K/8K file picker.

use std::{sync::Arc, time::Duration};

use cranpose::prelude::{delay, Player};
use cranpose_core::{
    remember, DisposableEffect, DisposableEffectResult, LaunchedEffect, MutableState,
};
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    composable, BoxWithConstraints, BoxWithConstraintsScope, Color, Column, ColumnSpec,
    LinearArrangement, Modifier,
};

use super::{
    super::ticking::Ticking,
    catalog::{
        ActiveSource, BuiltinClipPack, DecoderConcurrency, StressScenario, WallLayout,
        DEFAULT_SOURCE_ID,
    },
    controls::{
        HudDensity, HudLayoutConfig, OpenMediaCatalogPanel, ScenarioAndEffectsToolbar,
        TelemetryHudBar,
    },
    reels::{ReelsGalleryStage, ReelsStageConfig},
    telemetry::{FrameTransferMode, TelemetryCounters, TelemetrySampler, TelemetrySnapshot},
    transport::FocusedTransportBar,
    wall::{
        AudioOutput, DecoderPumpHost, DecoderRig, PlaybackInit, SurfaceEffects, VideoWallStage,
        WallPlayback, WallStageState,
    },
};

#[derive(Clone, Copy, PartialEq)]
struct PlaygroundState {
    scenario: MutableState<StressScenario>,
    layout: MutableState<WallLayout>,
    concurrency: MutableState<DecoderConcurrency>,
    effects: MutableState<SurfaceEffects>,
    active_source: MutableState<ActiveSource>,
    playing: MutableState<bool>,
    muted: MutableState<bool>,
    focused_tile: MutableState<usize>,
    catalog_open: MutableState<bool>,
    controls_override: MutableState<Option<bool>>,
    phase: MutableState<f32>,
    rig: MutableState<DecoderRig>,
    rig_generation: MutableState<u64>,
    active_reel_player: MutableState<Option<Player>>,
    telemetry: MutableState<TelemetrySnapshot>,
    custom_url: TextFieldState,
}

impl PlaygroundState {
    fn new() -> Self {
        Self {
            scenario: cranpose_core::mutableStateOf(StressScenario::CinematicHero),
            layout: cranpose_core::mutableStateOf(WallLayout::Grid),
            concurrency: cranpose_core::mutableStateOf(DecoderConcurrency::Auto),
            effects: cranpose_core::mutableStateOf(SurfaceEffects::default()),
            active_source: cranpose_core::mutableStateOf(ActiveSource::Catalog(DEFAULT_SOURCE_ID)),
            playing: cranpose_core::mutableStateOf(true),
            muted: cranpose_core::mutableStateOf(true),
            focused_tile: cranpose_core::mutableStateOf(0_usize),
            catalog_open: cranpose_core::mutableStateOf(false),
            controls_override: cranpose_core::mutableStateOf(None::<bool>),
            phase: cranpose_core::mutableStateOf(0.0_f32),
            rig: cranpose_core::mutableStateOf(DecoderRig::empty()),
            rig_generation: cranpose_core::mutableStateOf(1_u64),
            active_reel_player: cranpose_core::mutableStateOf(None::<Player>),
            telemetry: cranpose_core::mutableStateOf(TelemetrySnapshot::default()),
            custom_url: TextFieldState::new(
                "https://dash.akamaized.net/akamai/bbb_30fps/bbb_30fps.mpd",
            ),
        }
    }
}

#[composable]
pub(crate) fn VideoPlaygroundTab() {
    let clip_pack = remember(BuiltinClipPack::new).with(Clone::clone);
    let state = remember(PlaygroundState::new).with(|s| *s);
    let counters = remember(TelemetryCounters::new).with(Arc::clone);
    let transfer = remember(|| {
        FrameTransferMode::from_zero_copy(cranpose_services::player_capabilities().zero_copy_gpu)
    })
    .with(|v| *v);

    let open_local = cranpose::rememberOpenFileLauncher("video-playground.open", move |picked| {
        if let Ok(Some(content)) = picked {
            let meta = content.metadata();
            state.active_source.set(ActiveSource::LocalUri {
                uri: local_media_uri(&meta.identifier),
                label: meta.name,
            });
            state.catalog_open.set(false);
        }
    });

    PlaygroundLifecycle(state, clip_pack.clone(), Arc::clone(&counters), transfer);
    // Keep per-decoder intake pumps outside BoxWithConstraints so SlotId(0) holds a single
    // stable root child across decoder rig rebuilds.
    DecoderPumpHost(state.rig.value(), Arc::clone(&counters), transfer);

    let launch_local = move || {
        open_local.launch(
            cranpose::FilePickerOptions::default()
                .with_title("Open 4K / 8K / 120fps Video")
                .with_filter(
                    cranpose::FileFilter::new(
                        "Video files",
                        &["mp4", "mkv", "mov", "webm", "m3u8", "mpd", "ts"],
                    )
                    .with_mime_types(&[
                        "video/*",
                        "application/vnd.apple.mpegurl",
                        "application/dash+xml",
                    ]),
                ),
        );
    };

    PlaygroundScaffold(state, clip_pack, counters, transfer, launch_local);
}

#[composable]
fn PlaygroundLifecycle(
    state: PlaygroundState,
    clip_pack: BuiltinClipPack,
    counters: Arc<TelemetryCounters>,
    transfer: FrameTransferMode,
) {
    let cur_scenario = state.scenario.value();
    let cur_concurrency = state.concurrency.value();
    let cur_source = state.active_source.value();
    let in_reels = state.layout.value() == WallLayout::Reels;
    let decoder_count = cur_concurrency.resolved(cur_scenario.tile_count());
    let rig_counters = Arc::clone(&counters);
    let rig_source = cur_source.clone();

    LaunchedEffect(
        (in_reels, cur_scenario, cur_concurrency, cur_source),
        async move |_| {
            state.rig.value().release_all();
            rig_counters.reset();
            let next_gen = state.rig_generation.value().wrapping_add(1);
            state.rig_generation.set(next_gen);
            if in_reels {
                state.rig.set(DecoderRig::empty());
                return;
            }
            let focus = state
                .focused_tile
                .value()
                .min(cur_scenario.tile_count().saturating_sub(1));
            state.focused_tile.set(focus);
            let init = PlaybackInit {
                playback: if state.playing.value() {
                    WallPlayback::Playing
                } else {
                    WallPlayback::Paused
                },
                audio: if state.muted.value() {
                    AudioOutput::Muted
                } else {
                    AudioOutput::Audible
                },
                focused_tile: focus,
            };
            let next = DecoderRig::build(next_gen, decoder_count, &rig_source, &clip_pack, init);
            state.rig.set(next);
        },
    );

    let is_playing = state.playing.value();
    let playback_mode = if is_playing {
        WallPlayback::Playing
    } else {
        WallPlayback::Paused
    };
    LaunchedEffect(playback_mode, async move |_| {
        state.rig.value().set_playing(playback_mode);
    });

    let cur_focus = state.focused_tile.value();
    let audio_output = if state.muted.value() {
        AudioOutput::Muted
    } else {
        AudioOutput::Audible
    };
    LaunchedEffect((cur_focus, audio_output), async move |_| {
        state.rig.value().set_audio_focus(cur_focus, audio_output);
    });

    // Release every hardware decoder immediately when leaving the tab so idle stays at 0 work.
    DisposableEffect((), move |_| {
        DisposableEffectResult::new(move || {
            state.rig.value().release_all();
            state.rig.set(DecoderRig::empty());
            state.active_reel_player.set(None);
        })
    });

    PlaygroundTelemetryLoop(
        state,
        counters,
        transfer,
        cur_scenario,
        state.layout.value(),
    );
}

#[composable]
fn PlaygroundTelemetryLoop(
    state: PlaygroundState,
    counters: Arc<TelemetryCounters>,
    transfer: FrameTransferMode,
    cur_scenario: StressScenario,
    layout: WallLayout,
) {
    let in_reels = layout == WallLayout::Reels;
    let motion_active = state.effects.value().animate || state.playing.value();
    let frame_counters = Arc::clone(&counters);
    Ticking(motion_active, move |dt| {
        frame_counters.record_compositor_frame();
        if state.effects.value().animate {
            state.phase.set(state.phase.value() + dt);
        }
    });

    LaunchedEffect((motion_active, cur_scenario, in_reels), async move |_| {
        if !motion_active {
            return;
        }
        let mut sampler = TelemetrySampler::new();
        loop {
            delay(Duration::from_millis(250)).await;
            let players = if in_reels {
                state.active_reel_player.value().into_iter().collect()
            } else {
                state.rig.value().players()
            };
            let surfaces = if in_reels {
                1
            } else {
                cur_scenario.tile_count()
            };
            let snapshot = sampler.sample(&counters, &players, surfaces, transfer);
            state.telemetry.set(snapshot);
        }
    });
}

#[composable]
fn PlaygroundScaffold(
    state: PlaygroundState,
    clip_pack: BuiltinClipPack,
    counters: Arc<TelemetryCounters>,
    transfer: FrameTransferMode,
    launch_local: impl Fn() + Clone + 'static,
) {
    BoxWithConstraints(
        Modifier::empty()
            .fill_max_size()
            .background(Color::from_rgb_u8(0x05, 0x07, 0x0D)),
        move |scope| {
            // Read all reactive tab state inside BoxWithConstraints so SlotId(0) tracks every
            // state change cleanly without stale non-reactive captures.
            let cur_scenario = state.scenario.value();
            let in_reels = state.layout.value() == WallLayout::Reels;
            let cur_source = state.active_source.value();
            let src_title = if in_reels {
                "Vertical Reels Gallery".to_owned()
            } else {
                cur_source.title()
            };
            let src_license = cur_source.license_summary();
            let compact = scope.max_width().0 < 720.0 || scope.max_height().0 < 620.0;
            let density = if compact {
                HudDensity::Compact
            } else {
                HudDensity::Expanded
            };
            let controls_visible = state.controls_override.value().unwrap_or(!compact);
            let hud = HudLayoutConfig {
                density,
                controls_visible,
                controls_override: state.controls_override,
                catalog_open: state.catalog_open,
                layout: state.layout,
            };
            let local_cb = launch_local.clone();
            let body_pack = clip_pack.clone();
            let body_counters = Arc::clone(&counters);
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .padding(if compact { 6.0 } else { 8.0 }),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    TelemetryHudBar(
                        cur_scenario,
                        state.telemetry.value(),
                        src_title.clone(),
                        hud,
                        local_cb.clone(),
                    );
                    if controls_visible {
                        ScenarioAndEffectsToolbar(
                            state.scenario,
                            state.layout,
                            state.concurrency,
                            state.effects,
                            state.playing,
                            state.muted,
                        );
                    }
                    if state.catalog_open.value() {
                        OpenMediaCatalogPanel(
                            state.active_source,
                            state.custom_url,
                            state.catalog_open,
                            local_cb.clone(),
                        );
                    }
                    PlaygroundStageBody(
                        state,
                        body_pack.clone(),
                        Arc::clone(&body_counters),
                        transfer,
                        src_license,
                    );
                },
            );
        },
    );
}

#[composable]
fn PlaygroundStageBody(
    state: PlaygroundState,
    clip_pack: BuiltinClipPack,
    counters: Arc<TelemetryCounters>,
    transfer: FrameTransferMode,
    src_license: &'static str,
) {
    if state.layout.value() == WallLayout::Reels {
        ReelsGalleryStage(
            ReelsStageConfig {
                active_source: state.active_source,
                effects: state.effects,
                phase: state.phase,
                muted: state.muted,
                playing: state.playing,
                active_reel_player: state.active_reel_player,
                clip_pack,
                counters,
                transfer,
            },
            Modifier::empty().fill_max_width().weight(1.0),
        );
        return;
    }
    let stage = WallStageState {
        scenario: state.scenario,
        layout: state.layout,
        rig: state.rig,
        effects: state.effects,
        phase: state.phase,
        focused_tile: state.focused_tile,
    };
    let cur_focus = state.focused_tile.value();
    let rig = state.rig.value();
    let focused_player = rig
        .slots
        .get(cur_focus % rig.slots.len().max(1))
        .map(|s| s.player.clone());
    VideoWallStage(stage, Modifier::empty().fill_max_width().weight(1.0));
    if let Some(player) = focused_player {
        FocusedTransportBar(player, cur_focus, src_license);
    }
}

fn local_media_uri(identifier: &str) -> String {
    if identifier.contains("://") {
        identifier.to_owned()
    } else {
        format!("file://{identifier}")
    }
}

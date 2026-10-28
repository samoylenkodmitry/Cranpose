//! The framework's diagnostic and ablation switches, each named by an
//! environment variable, in one struct: [`DIAGNOSTICS`].
//!
//! A switch is read once and cached until a [`crate::debug_toggles`]
//! override changes, because reading the environment walks the whole
//! variable array under a lock, and several switches sit on paths that run
//! once a frame or once a node. Android seeds the variables from
//! `debug.cranpose.*` system properties before the first read.
//!
//! Without the `diagnostics` feature every switch reads as unset, whatever
//! the environment holds, and the code behind it compiles out of the build.

use std::str::FromStr;

#[cfg(feature = "diagnostics")]
use crate::debug_toggles::DebugToggle;

/// One diagnostic or ablation switch, named by an environment variable.
#[cfg(feature = "diagnostics")]
pub struct Diagnostic(DebugToggle);

/// One diagnostic or ablation switch; this build leaves them out, so it
/// always reads as unset.
#[cfg(not(feature = "diagnostics"))]
pub struct Diagnostic(());

impl Diagnostic {
    #[cfg(feature = "diagnostics")]
    const fn new(name: &'static str) -> Self {
        Self(DebugToggle::new(name))
    }

    #[cfg(not(feature = "diagnostics"))]
    const fn new(_name: &'static str) -> Self {
        Self(())
    }

    /// Reads the switch's value through `read`; `None` while it is unset.
    pub fn with<R>(&self, read: impl FnOnce(Option<&str>) -> R) -> R {
        #[cfg(feature = "diagnostics")]
        {
            self.0.with(read)
        }
        #[cfg(not(feature = "diagnostics"))]
        {
            read(None)
        }
    }

    /// Whether the switch holds any value.
    pub fn is_set(&self) -> bool {
        self.with(|value| value.is_some())
    }

    /// Whether the switch is on: `1`, `true` or `yes`.
    pub fn flag(&self) -> bool {
        self.with(|value| matches!(value, Some("1" | "true" | "yes")))
    }

    /// Whether the switch holds exactly `expected`.
    pub fn equals(&self, expected: &str) -> bool {
        self.with(|value| value == Some(expected))
    }

    /// The switch parsed as `T`, when set and well formed.
    pub fn parse<T: FromStr>(&self) -> Option<T> {
        self.with(|value| value.and_then(|value| value.trim().parse().ok()))
    }

    /// The switch as a threshold in milliseconds: a finite number from zero
    /// up. Zero reports every frame.
    pub fn threshold_ms(&self) -> Option<f64> {
        self.parse::<f64>()
            .filter(|threshold| threshold.is_finite() && *threshold >= 0.0)
    }
}

/// Every diagnostic and ablation switch, by the environment variable that
/// names it. An unset switch leaves the framework as it ships.
pub struct Diagnostics {
    /// `CRANPOSE_A11Y_SYNC`: Android publishes the accessibility tree on
    /// the UI thread (`1`) or off it (`0`).
    pub a11y_sync: Diagnostic,
    /// `CRANPOSE_ABLATE`: the comma-separated renderer stages to leave out.
    pub ablate: Diagnostic,
    /// `CRANPOSE_ADPF`: `0` turns off Android's performance hint session.
    pub adpf: Diagnostic,
    /// `CRANPOSE_ASSERT_STATE_CHAIN`: checks every state record chain.
    pub assert_state_chain: Diagnostic,
    /// `CRANPOSE_ASYNC_HAPTICS`: `0`, `false`, `off` or `no` plays Android
    /// haptics on the calling thread.
    pub async_haptics: Diagnostic,
    /// `CRANPOSE_BACKGROUND_PIPELINES`: `0` or `1` turns compiling
    /// pipelines on background threads off or on.
    pub background_pipelines: Diagnostic,
    /// `COMPOSE_DEBUG_MODIFIERS`: logs modifier chains.
    pub compose_debug_modifiers: Diagnostic,
    /// `COMPOSE_DEBUG_SLOT_TABLE`: logs the slot table.
    pub compose_debug_slot_table: Diagnostic,
    /// `CRANPOSE_CORE_PIN`: `0` keeps the render threads off the fast cores.
    pub core_pin: Diagnostic,
    /// `CRANPOSE_DEBUG_SCOPE_LABELS`: labels recompose scopes in traces.
    pub debug_scope_labels: Diagnostic,
    /// `CRANPOSE_DESKTOP_FRAME_TELEMETRY`: logs desktop frames over the
    /// default threshold.
    pub desktop_frame_telemetry: Diagnostic,
    /// `CRANPOSE_DESKTOP_FRAME_TELEMETRY_MS`: logs desktop frames over this
    /// many milliseconds.
    pub desktop_frame_telemetry_ms: Diagnostic,
    /// `CRANPOSE_DISABLE_NAV_SNAP`: the navigation bar does not snap.
    pub disable_nav_snap: Diagnostic,
    /// `CRANPOSE_DISABLE_SCROLL_TRANSLATE`: a scroll rebuilds its layers in
    /// place of moving them.
    pub disable_scroll_translate: Diagnostic,
    /// `CRANPOSE_DISABLE_STRUCTURAL_DIRT`: the shell does not mark the
    /// parents of structural changes.
    pub disable_structural_dirt: Diagnostic,
    /// `CRANPOSE_ENCODE_DELAY_MS`: waits this many milliseconds before each
    /// frame's encode on the present thread.
    pub encode_delay_ms: Diagnostic,
    /// `CRANPOSE_FRAME_LEAD_TENTHS`: pins the frame lead, in tenths of a
    /// frame period.
    pub frame_lead_tenths: Diagnostic,
    /// `CRANPOSE_FRAME_STAGE_TELEMETRY`: logs the shell's frame stages over
    /// the default threshold.
    pub frame_stage_telemetry: Diagnostic,
    /// `CRANPOSE_FRAME_STAGE_TELEMETRY_MS`: logs the shell's frame stages
    /// over this many milliseconds.
    pub frame_stage_telemetry_ms: Diagnostic,
    /// `CRANPOSE_GLASS_MATERIAL_FOLDS`: `1` or `0` forces the glass
    /// material folds on or off.
    pub glass_material_folds: Diagnostic,
    /// `CRANPOSE_GPU_FENCE_PROFILE`: profiles GPU fences.
    pub gpu_fence_profile: Diagnostic,
    /// `CRANPOSE_GPU_PASS_TIMING`: times GPU passes with timestamp queries.
    pub gpu_pass_timing: Diagnostic,
    /// `CRANPOSE_GPU_SHADOW_CACHE_DIAG`: logs the shadow cache.
    pub gpu_shadow_cache_diag: Diagnostic,
    /// `CRANPOSE_GPU_STAGE_DIAG`: logs the renderer's frame stages.
    pub gpu_stage_diag: Diagnostic,
    /// `CRANPOSE_GPU_STATS`: collects and prints the renderer's frame stats.
    pub gpu_stats: Diagnostic,
    /// `CRANPOSE_LAYOUT_INVALIDATION_DISPATCH_DIAG`: logs layout
    /// invalidations (`all` for every one).
    pub layout_invalidation_dispatch_diag: Diagnostic,
    /// `CRANPOSE_LAYOUT_MEASURE_TELEMETRY`: logs measure passes over the
    /// default threshold.
    pub layout_measure_telemetry: Diagnostic,
    /// `CRANPOSE_LAYOUT_MEASURE_TELEMETRY_MS`: logs measure passes over this
    /// many milliseconds.
    pub layout_measure_telemetry_ms: Diagnostic,
    /// `CRANPOSE_LAYOUT_REPASS_SCHEDULE_DIAG`: logs layout repass
    /// scheduling (`all` for every one).
    pub layout_repass_schedule_diag: Diagnostic,
    /// `CRANPOSE_LAZY_CACHE_TELEMETRY`: logs the lazy list item cache.
    pub lazy_cache_telemetry: Diagnostic,
    /// `CRANPOSE_LAZY_ITEM_TELEMETRY`: logs lazy list item work.
    pub lazy_item_telemetry: Diagnostic,
    /// `CRANPOSE_LAZY_MEASURE_TELEMETRY`: logs lazy list measures.
    pub lazy_measure_telemetry: Diagnostic,
    /// `CRANPOSE_LOCATION_KEY_DIAGNOSTICS`: logs location key collisions.
    pub location_key_diagnostics: Diagnostic,
    /// `CRANPOSE_NATIVE_TRACE`: prints native window events.
    pub native_trace: Diagnostic,
    /// `CRANPOSE_NATIVE_WINDOW_TIMING`: prints native window timings.
    pub native_window_timing: Diagnostic,
    /// `CRANPOSE_NO_BACKDROP_CACHE`: draws backdrops without their cache.
    pub no_backdrop_cache: Diagnostic,
    /// `CRANPOSE_NO_EFFECT_DOMAINS`: effects sample their whole sources.
    pub no_effect_domains: Diagnostic,
    /// `CRANPOSE_NO_FILL_CACHE`: draws fills without the opaque prefix
    /// cache.
    pub no_fill_cache: Diagnostic,
    /// `CRANPOSE_NO_INTERIORS_FIRST`: draws shape interiors in paint order.
    pub no_interiors_first: Diagnostic,
    /// `CRANPOSE_NO_SHADER_FLATTEN`: compiles shaders without flattening.
    pub no_shader_flatten: Diagnostic,
    /// `CRANPOSE_NO_SHADER_FOLD`: compiles shaders without folding
    /// constants.
    pub no_shader_fold: Diagnostic,
    /// `CRANPOSE_NO_SHADER_SPECIALIZATION`: draws with the general shaders.
    pub no_shader_specialization: Diagnostic,
    /// `CRANPOSE_PACING_DIAG`: logs the desktop frame pacing.
    pub pacing_diag: Diagnostic,
    /// `CRANPOSE_PIPELINE_DISK_CACHE`: `0` or `1` turns the pipeline disk
    /// cache off or on.
    pub pipeline_disk_cache: Diagnostic,
    /// `CRANPOSE_PRESENT_THREAD`: chooses whether Android presents on a
    /// thread of its own.
    pub present_thread: Diagnostic,
    /// `CRANPOSE_PROBE_DRAW_PASSES`: draws the first this many draw passes.
    pub probe_draw_passes: Diagnostic,
    /// `CRANPOSE_PROBE_PASSES`: encodes the first this many passes.
    pub probe_passes: Diagnostic,
    /// `CRANPOSE_RECOMPOSE_SCOPE_TELEMETRY_MS`: logs recompose scopes over
    /// this many milliseconds.
    pub recompose_scope_telemetry_ms: Diagnostic,
    /// `CRANPOSE_RECOMP_DIAG`: prints the recomposition rate.
    pub recomp_diag: Diagnostic,
    /// `CRANPOSE_RENDER_PHASE_DIRTY_DIAG`: logs what the render phase finds
    /// dirty.
    pub render_phase_dirty_diag: Diagnostic,
    /// `CRANPOSE_ROBOT_BUTTON_HELPER_DIAG`: prints the robot's button
    /// lookups.
    pub robot_button_helper_diag: Diagnostic,
    /// `CRANPOSE_SCENE_UPDATE_DIAG`: logs scene updates.
    pub scene_update_diag: Diagnostic,
    /// `CRANPOSE_SHAPE_VARIANTS`: `0` draws shapes with the general
    /// pipeline.
    pub shape_variants: Diagnostic,
    /// `CRANPOSE_SKIP_SHADOWS`: leaves shadows out.
    pub skip_shadows: Diagnostic,
    /// `CRANPOSE_STRUCTURAL_DIAG`: logs structural child syncs.
    pub structural_diag: Diagnostic,
    /// `CRANPOSE_SUBCOMPOSE_TELEMETRY`: logs subcompositions.
    pub subcompose_telemetry: Diagnostic,
    /// `CRANPOSE_SURVIVE_GPU_ERRORS`: `0` stops on a GPU validation error.
    pub survive_gpu_errors: Diagnostic,
    /// `CRANPOSE_TEXT_ATLAS_FALLBACK_DIAG`: logs text that misses the glyph
    /// atlas.
    pub text_atlas_fallback_diag: Diagnostic,
    /// `CRANPOSE_TEXT_LAYOUT_TELEMETRY`: logs text layouts.
    pub text_layout_telemetry: Diagnostic,
    /// `CRANPOSE_UPDATE_STAGE_TELEMETRY_MS`: logs the shell's update stages
    /// over this many milliseconds.
    pub update_stage_telemetry_ms: Diagnostic,
    /// `CRANPOSE_VALIDATE_SLOTS`: validates the slot table after each
    /// composition.
    pub validate_slots: Diagnostic,
    /// `CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY`: logs the renderer's stages
    /// over the default threshold.
    pub wgpu_render_stage_telemetry: Diagnostic,
    /// `CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY_MS`: logs the renderer's stages
    /// over this many milliseconds.
    pub wgpu_render_stage_telemetry_ms: Diagnostic,
}

/// The framework's diagnostic and ablation switches.
pub static DIAGNOSTICS: Diagnostics = Diagnostics {
    a11y_sync: Diagnostic::new("CRANPOSE_A11Y_SYNC"),
    ablate: Diagnostic::new("CRANPOSE_ABLATE"),
    adpf: Diagnostic::new("CRANPOSE_ADPF"),
    assert_state_chain: Diagnostic::new("CRANPOSE_ASSERT_STATE_CHAIN"),
    async_haptics: Diagnostic::new("CRANPOSE_ASYNC_HAPTICS"),
    background_pipelines: Diagnostic::new("CRANPOSE_BACKGROUND_PIPELINES"),
    compose_debug_modifiers: Diagnostic::new("COMPOSE_DEBUG_MODIFIERS"),
    compose_debug_slot_table: Diagnostic::new("COMPOSE_DEBUG_SLOT_TABLE"),
    core_pin: Diagnostic::new("CRANPOSE_CORE_PIN"),
    debug_scope_labels: Diagnostic::new("CRANPOSE_DEBUG_SCOPE_LABELS"),
    desktop_frame_telemetry: Diagnostic::new("CRANPOSE_DESKTOP_FRAME_TELEMETRY"),
    desktop_frame_telemetry_ms: Diagnostic::new("CRANPOSE_DESKTOP_FRAME_TELEMETRY_MS"),
    disable_nav_snap: Diagnostic::new("CRANPOSE_DISABLE_NAV_SNAP"),
    disable_scroll_translate: Diagnostic::new("CRANPOSE_DISABLE_SCROLL_TRANSLATE"),
    disable_structural_dirt: Diagnostic::new("CRANPOSE_DISABLE_STRUCTURAL_DIRT"),
    encode_delay_ms: Diagnostic::new("CRANPOSE_ENCODE_DELAY_MS"),
    frame_lead_tenths: Diagnostic::new("CRANPOSE_FRAME_LEAD_TENTHS"),
    frame_stage_telemetry: Diagnostic::new("CRANPOSE_FRAME_STAGE_TELEMETRY"),
    frame_stage_telemetry_ms: Diagnostic::new("CRANPOSE_FRAME_STAGE_TELEMETRY_MS"),
    glass_material_folds: Diagnostic::new("CRANPOSE_GLASS_MATERIAL_FOLDS"),
    gpu_fence_profile: Diagnostic::new("CRANPOSE_GPU_FENCE_PROFILE"),
    gpu_pass_timing: Diagnostic::new("CRANPOSE_GPU_PASS_TIMING"),
    gpu_shadow_cache_diag: Diagnostic::new("CRANPOSE_GPU_SHADOW_CACHE_DIAG"),
    gpu_stage_diag: Diagnostic::new("CRANPOSE_GPU_STAGE_DIAG"),
    gpu_stats: Diagnostic::new("CRANPOSE_GPU_STATS"),
    layout_invalidation_dispatch_diag: Diagnostic::new(
        "CRANPOSE_LAYOUT_INVALIDATION_DISPATCH_DIAG",
    ),
    layout_measure_telemetry: Diagnostic::new("CRANPOSE_LAYOUT_MEASURE_TELEMETRY"),
    layout_measure_telemetry_ms: Diagnostic::new("CRANPOSE_LAYOUT_MEASURE_TELEMETRY_MS"),
    layout_repass_schedule_diag: Diagnostic::new("CRANPOSE_LAYOUT_REPASS_SCHEDULE_DIAG"),
    lazy_cache_telemetry: Diagnostic::new("CRANPOSE_LAZY_CACHE_TELEMETRY"),
    lazy_item_telemetry: Diagnostic::new("CRANPOSE_LAZY_ITEM_TELEMETRY"),
    lazy_measure_telemetry: Diagnostic::new("CRANPOSE_LAZY_MEASURE_TELEMETRY"),
    location_key_diagnostics: Diagnostic::new("CRANPOSE_LOCATION_KEY_DIAGNOSTICS"),
    native_trace: Diagnostic::new("CRANPOSE_NATIVE_TRACE"),
    native_window_timing: Diagnostic::new("CRANPOSE_NATIVE_WINDOW_TIMING"),
    no_backdrop_cache: Diagnostic::new("CRANPOSE_NO_BACKDROP_CACHE"),
    no_effect_domains: Diagnostic::new("CRANPOSE_NO_EFFECT_DOMAINS"),
    no_fill_cache: Diagnostic::new("CRANPOSE_NO_FILL_CACHE"),
    no_interiors_first: Diagnostic::new("CRANPOSE_NO_INTERIORS_FIRST"),
    no_shader_flatten: Diagnostic::new("CRANPOSE_NO_SHADER_FLATTEN"),
    no_shader_fold: Diagnostic::new("CRANPOSE_NO_SHADER_FOLD"),
    no_shader_specialization: Diagnostic::new("CRANPOSE_NO_SHADER_SPECIALIZATION"),
    pacing_diag: Diagnostic::new("CRANPOSE_PACING_DIAG"),
    pipeline_disk_cache: Diagnostic::new("CRANPOSE_PIPELINE_DISK_CACHE"),
    present_thread: Diagnostic::new("CRANPOSE_PRESENT_THREAD"),
    probe_draw_passes: Diagnostic::new("CRANPOSE_PROBE_DRAW_PASSES"),
    probe_passes: Diagnostic::new("CRANPOSE_PROBE_PASSES"),
    recompose_scope_telemetry_ms: Diagnostic::new("CRANPOSE_RECOMPOSE_SCOPE_TELEMETRY_MS"),
    recomp_diag: Diagnostic::new("CRANPOSE_RECOMP_DIAG"),
    render_phase_dirty_diag: Diagnostic::new("CRANPOSE_RENDER_PHASE_DIRTY_DIAG"),
    robot_button_helper_diag: Diagnostic::new("CRANPOSE_ROBOT_BUTTON_HELPER_DIAG"),
    scene_update_diag: Diagnostic::new("CRANPOSE_SCENE_UPDATE_DIAG"),
    shape_variants: Diagnostic::new("CRANPOSE_SHAPE_VARIANTS"),
    skip_shadows: Diagnostic::new("CRANPOSE_SKIP_SHADOWS"),
    structural_diag: Diagnostic::new("CRANPOSE_STRUCTURAL_DIAG"),
    subcompose_telemetry: Diagnostic::new("CRANPOSE_SUBCOMPOSE_TELEMETRY"),
    survive_gpu_errors: Diagnostic::new("CRANPOSE_SURVIVE_GPU_ERRORS"),
    text_atlas_fallback_diag: Diagnostic::new("CRANPOSE_TEXT_ATLAS_FALLBACK_DIAG"),
    text_layout_telemetry: Diagnostic::new("CRANPOSE_TEXT_LAYOUT_TELEMETRY"),
    update_stage_telemetry_ms: Diagnostic::new("CRANPOSE_UPDATE_STAGE_TELEMETRY_MS"),
    validate_slots: Diagnostic::new("CRANPOSE_VALIDATE_SLOTS"),
    wgpu_render_stage_telemetry: Diagnostic::new("CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY"),
    wgpu_render_stage_telemetry_ms: Diagnostic::new("CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY_MS"),
};

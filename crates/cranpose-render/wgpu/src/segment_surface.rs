//! Retained-segment surface caching under rigid similarity motion.
//!
//! The command-replay machinery already proves, per frame and per retained
//! span, that a stretch of content moved by exactly one similarity transform
//! (uniform scale + rotation about a shared center) with byte-verified
//! content stability. The replay path exploits that on the CPU side — the
//! shapes convert once and re-draw from retained GPU buffers — but it still
//! SUBMITS every member quad every frame: on the mega scene ~14k retained
//! shapes cost ~0.79 Mpx of translucent SrcOver read-modify-write per frame
//! on a tiler. This module removes the fragments: a qualifying span renders
//! ONCE into a pooled offscreen surface (in its capture space) and each
//! following frame draws as ONE textured quad under the span's per-frame
//! transform.
//!
//! ## Why compositing the flattened span preserves z order exactly
//!
//! Every retained-span pipeline blends SrcOver, and SrcOver over
//! premultiplied alpha is ASSOCIATIVE: for any draws A, B and destination D,
//! `over(A, over(B, D)) == over(over(A, B), D)`. The capture pass renders
//! the span's members (in their exact order) onto a transparent surface with
//! the ordinary `ALPHA_BLENDING` state, which accumulates exactly
//! `over(A_n, ... over(A_1, transparent))` — the premultiplied flattening of
//! the span. Compositing that surface with `PREMULTIPLIED_ALPHA_BLENDING`
//! at the span's own position in the fused pass therefore produces the same
//! blending result as drawing the members inline, REGARDLESS of what dynamic
//! content sits below or above the span — including dynamic spans whose
//! footprints overlap the segment's. The planner already emits each maximal
//! consecutive retained stretch as its own `SegmentBatchPlan::Retained`
//! batch at its exact z position between the dynamic batches, and the
//! composite quad is drawn at precisely that point in the pass, so strict
//! interleave holds BY CONSTRUCTION and admission needs no footprint
//! constraint. The only divergence from analytic redraw is resampling under
//! rotation/scale and the 8-bit quantization of the intermediate surface
//! (the same class of quantization Compose's own layer surfaces have) —
//! bounded and asserted by the parity suite, never a reordering.
//!
//! ## Economics (why admission is content-conditional)
//!
//! Per frame, the direct path pays roughly
//! `member_px = Σ member-quad pixels` of SrcOver RMW, where every fragment
//! also evaluates the arc/rect SDF, its AA band and (when present) the
//! gradient + dither chain. The cached path pays `surface_px = capture-rect
//! pixels` of one bilinear fetch + SrcOver RMW, once per frame, plus a
//! recapture (`member_px` worth of the direct cost, into the offscreen) on
//! invalidation. A thin ring inscribed in its bounding box has
//! `surface_px / member_px ≈ 2·r / (π·band)` — for the mega rings that is
//! roughly 2-3× more pixels touched — but each surface pixel runs a flat
//! textured-blit fragment where each member pixel runs the SDF shape shader,
//! which the arc-mesh work measured as the dominant per-pixel cost on the
//! watch's Adreno 702. The default admission ratio (`surface_px ≤ 3 ×
//! member_px`, env-overridable) encodes that fragment-cost asymmetry; the
//! watch A/B, not this comment, decides the flip. Segments whose geometry
//! makes the surface strictly larger than that (sparse content in a huge
//! box) stay on the direct path.
//!
//! Two other Cranpose apps that win from this, content-conditionally: any
//! watch face or dashboard with rotating complication rings (bezel ticks,
//! rotating gauges) whose ring content is retained and rigid, and any data
//! visualization that spins or zooms a static radial gauge/dial under user
//! input. Nothing here consults the app: admission reads only the span's
//! own measured geometry, churn and transform.
//!
//! ## Recolor churn
//!
//! Recolor patches (twinkles) rewrite a member's 16-byte paint record; a
//! cached surface of that member is stale the moment the patch lands, so a
//! patched segment recaptures IN THE SAME FRAME (the capture pass encodes
//! after the staged-upload flush that carries the patch, before the fused
//! pass that samples it). A segment recoloring every frame would therefore
//! recapture every frame and pay `member_px + surface_px` — strictly worse
//! than direct. Measured on the command-feed parity scene (the synthetic
//! mega boss, 120 frames): ring segments recolor on 0/119 frames while the
//! twinkle segment recolors on 108/119 frames (~200 records per recoloring
//! frame) — the distribution is fully bimodal, so any threshold between
//! ~0.05 and ~0.9 separates the classes. The default admits a segment only
//! when it recolored on at most 1/8 of recently observed frames
//! (`CRANPOSE_SEGMENT_SURFACE_RECOLOR_RATE`), which admits every ring and
//! rejects the twinkle field with a wide margin on both sides.
//!
//! Kill switch: DEFAULT ON, earned by measurement — Pixel Watch 3 mega
//! scene, alternating pairs off-charger: ON 59.01/58.53 fps vs OFF
//! 53.73/52.37 (+5.3/+6.2, ten times the protocol noise floor), with
//! engagement proven live (captures + composites counting, ~2.1k churn
//! rejections keeping the twinkle field direct, zero economics
//! rejections at the default ratio) and the full scene visually intact.
//! `CRANPOSE_SEGMENT_SURFACE=0` disables (property
//! `debug.cranpose.segment_surface`). The watch A/B earns any flip.

use crate::offscreen::OffscreenTarget;

/// How many segment captures one frame may encode. Also sizes the reserved
/// capture-transform slots appended to the replay transform buffer and the
/// capture viewport-uniform slots. Excess captures defer to later frames
/// (the segments draw direct meanwhile), which staggers capture cost across
/// frames instead of spiking one.
pub(crate) const SEGMENT_CAPTURE_SLOTS: u32 = 8;

/// Uniform stride for the capture viewport slots — the strictest
/// `min_uniform_buffer_offset_alignment` any backend reports.
pub(crate) const SEGMENT_CAPTURE_UNIFORM_STRIDE: u64 = 256;

/// Total cached-surface byte budget. Mega-scale ring segments measure
/// ~0.3-0.5 MB each at watch resolution, so this holds a whole scene of
/// them with room while staying far below the layer-surface cache's 128 MiB.
const MAX_SEGMENT_SURFACE_BYTES: u64 = 32 * 1024 * 1024;

/// Per-entry byte cap: a segment whose padded capture rect exceeds this
/// would also fail the economics gate for any plausible member set; the cap
/// just fails it before a texture is sized.
const MAX_SEGMENT_SURFACE_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

/// Frames a key must be observed before its churn window is trusted. Below
/// this the segment draws direct — a segment that dies young never pays a
/// capture — unless the waiver (`waiver_ratio`) admits a never-recolored
/// candidate whose capture pays for itself within about one frame.
const ADMISSION_WARMUP_FRAMES: u32 = 8;

/// An entry unseen for this many frames is dropped in the periodic sweep.
const ENTRY_IDLE_EVICT_FRAMES: u64 = 600;

/// Transparent padding around the capture rect: one texel of guard band for
/// bilinear sampling plus one for the conservative snap. Member quads
/// already cover their own AA footprint, so ink never reaches the pad.
const CAPTURE_PAD_PX: f32 = 2.0;

/// Master switch, read once per frame (`begin_frame`): default OFF, `=1`
/// opts in. Mirrored from `debug.cranpose.segment_surface` on Android. Read
/// per frame, not latched, so the parity harness can flip it between arms.
fn segment_surface_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_SEGMENT_SURFACE").as_deref() != Some("0")
}

fn env_f32(name: &'static str, default: f32) -> f32 {
    crate::debug_toggles::debug_toggle(name)
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(default)
}

/// As [`env_f32`], except zero is a meaningful value (a kill switch), not a
/// malformed one.
fn env_f32_zero_ok(name: &'static str, default: f32) -> f32 {
    crate::debug_toggles::debug_toggle(name)
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(default)
}

/// Per-frame snapshot of the module's tunables.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SegmentSurfaceConfig {
    pub enabled: bool,
    /// Admit while `surface_px <= cost_ratio * member_px` — see the module
    /// economics comment for the derivation of the default.
    pub cost_ratio: f32,
    /// During warmup only: a candidate never observed to recolor may be
    /// admitted immediately while `surface_px <= waiver_ratio * member_px`
    /// — a strictly tighter bar than `cost_ratio`, payback within about
    /// one frame. At or below zero the waiver is off and warmup admits
    /// nothing, as before.
    pub waiver_ratio: f32,
    /// Admit while the key's recolored-frame rate over its observation
    /// window stays at or below this. Default sits far inside the measured
    /// bimodal gap (rings 0.0, twinkles ~0.9).
    pub recolor_rate: f32,
    /// Recapture when the span's scale drifts this far from the captured
    /// scale (resampling quality bound, not a correctness bound — the quad
    /// is drawn under the CURRENT transform either way).
    pub scale_eps: f32,
}

impl SegmentSurfaceConfig {
    pub(crate) fn load() -> Self {
        Self {
            enabled: segment_surface_enabled(),
            cost_ratio: env_f32("CRANPOSE_SEGMENT_SURFACE_COST_RATIO", 3.0),
            waiver_ratio: env_f32_zero_ok("CRANPOSE_SEGMENT_SURFACE_WAIVER_RATIO", 1.0),
            recolor_rate: env_f32("CRANPOSE_SEGMENT_SURFACE_RECOLOR_RATE", 0.125),
            scale_eps: env_f32("CRANPOSE_SEGMENT_SURFACE_SCALE_EPS", 0.05),
        }
    }

    #[cfg(test)]
    fn test_default() -> Self {
        Self {
            enabled: true,
            cost_ratio: 3.0,
            waiver_ratio: 1.0,
            recolor_rate: 0.125,
            scale_eps: 0.05,
        }
    }
}

/// A 2D affine map `p -> L·p + t` in device pixels. Similarity transforms
/// compose into these; keeping the general form means a span whose pivot
/// moved between capture and now (layer translation) still composites
/// exactly, with no recapture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Affine2 {
    /// Row-major linear part: `[[l00, l01], [l10, l11]]`.
    pub l: [[f32; 2]; 2],
    pub t: [f32; 2],
}

impl Affine2 {
    /// The similarity `p -> c + R·s·(p - c)` with `rot = (cos, sin)`.
    pub(crate) fn from_similarity(center: [f32; 2], rot: [f32; 2], scale: f32) -> Self {
        let (cos, sin) = (rot[0], rot[1]);
        let l = [[cos * scale, -sin * scale], [sin * scale, cos * scale]];
        Self {
            l,
            t: [
                center[0] - (l[0][0] * center[0] + l[0][1] * center[1]),
                center[1] - (l[1][0] * center[0] + l[1][1] * center[1]),
            ],
        }
    }

    pub(crate) fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        [
            self.l[0][0] * p[0] + self.l[0][1] * p[1] + self.t[0],
            self.l[1][0] * p[0] + self.l[1][1] * p[1] + self.t[1],
        ]
    }

    /// `self ∘ other` — apply `other` first.
    pub(crate) fn compose(&self, other: &Self) -> Self {
        let a = &self.l;
        let b = &other.l;
        Self {
            l: [
                [
                    a[0][0] * b[0][0] + a[0][1] * b[1][0],
                    a[0][0] * b[0][1] + a[0][1] * b[1][1],
                ],
                [
                    a[1][0] * b[0][0] + a[1][1] * b[1][0],
                    a[1][0] * b[0][1] + a[1][1] * b[1][1],
                ],
            ],
            t: self.apply(other.t),
        }
    }

    pub(crate) fn invert(&self) -> Option<Self> {
        let det = self.l[0][0] * self.l[1][1] - self.l[0][1] * self.l[1][0];
        if !det.is_finite() || det.abs() <= f32::EPSILON {
            return None;
        }
        let inv_det = 1.0 / det;
        let l = [
            [self.l[1][1] * inv_det, -self.l[0][1] * inv_det],
            [-self.l[1][0] * inv_det, self.l[0][0] * inv_det],
        ];
        Some(Self {
            l,
            t: [
                -(l[0][0] * self.t[0] + l[0][1] * self.t[1]),
                -(l[1][0] * self.t[0] + l[1][1] * self.t[1]),
            ],
        })
    }

    pub(crate) fn is_identity_for_sampling(&self) -> bool {
        self.l == [[1.0, 0.0], [0.0, 1.0]] && self.t == [0.0, 0.0]
    }

    pub(crate) fn is_integer_translation_for_sampling(&self) -> bool {
        self.l == [[1.0, 0.0], [0.0, 1.0]]
            && self
                .t
                .iter()
                .all(|value| value.is_finite() && value.fract() == 0.0)
    }
}

/// Integer-snapped, padded capture rect in capture device space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CaptureRect {
    pub origin: [f32; 2],
    pub width: u32,
    pub height: u32,
}

impl CaptureRect {
    pub(crate) fn byte_size(&self) -> u64 {
        self.width as u64 * self.height as u64 * 8
    }
}

/// Snaps the union of member AABBs (already transformed to capture space)
/// to whole device pixels with the transparent guard pad. Integer origin is
/// what makes the identity composite land texel-exact.
pub(crate) fn snap_capture_rect(
    min: [f32; 2],
    max: [f32; 2],
    max_texture_dim: u32,
) -> Option<CaptureRect> {
    if !(min[0].is_finite() && min[1].is_finite() && max[0].is_finite() && max[1].is_finite()) {
        return None;
    }
    if max[0] <= min[0] || max[1] <= min[1] {
        return None;
    }
    let x0 = (min[0] - CAPTURE_PAD_PX).floor();
    let y0 = (min[1] - CAPTURE_PAD_PX).floor();
    let x1 = (max[0] + CAPTURE_PAD_PX).ceil();
    let y1 = (max[1] + CAPTURE_PAD_PX).ceil();
    let width = (x1 - x0) as i64;
    let height = (y1 - y0) as i64;
    if width <= 0 || height <= 0 {
        return None;
    }
    if width > max_texture_dim as i64 || height > max_texture_dim as i64 {
        return None;
    }
    Some(CaptureRect {
        origin: [x0, y0],
        width: width as u32,
        height: height as u32,
    })
}

/// The capture identity of one retained span's shape range. `slot` is the
/// renderer replay-slot id; split pieces of one segment share the slot and
/// address disjoint ranges, so the range is part of the identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SegmentSurfaceKey {
    pub slot: u32,
    pub first_shape: u32,
    pub shape_count: u32,
}

/// Sliding 64-frame churn window for one key. A frame's bit is set when the
/// key received at least one recolor patch that frame.
#[derive(Clone, Copy, Debug, Default)]
struct AdmissionTrack {
    window: u64,
    observed: u32,
    last_noted_frame: u64,
}

impl AdmissionTrack {
    fn note_frame(&mut self, frame: u64, recolored: bool) {
        if self.last_noted_frame == frame && self.observed > 0 {
            // Same frame re-noted (a span drawn twice per frame does not
            // exist today, but the guard keeps the window honest if one
            // ever does): only an upgrade to "recolored" sticks.
            if recolored {
                self.window |= 1;
            }
            return;
        }
        self.last_noted_frame = frame;
        self.window = (self.window << 1) | u64::from(recolored);
        self.observed = self.observed.saturating_add(1);
    }

    fn warm(&self) -> bool {
        self.observed >= ADMISSION_WARMUP_FRAMES
    }

    fn has_recolored(&self) -> bool {
        self.window != 0
    }

    fn recolor_rate(&self) -> f32 {
        let considered = self.observed.min(64);
        if considered == 0 {
            return 0.0;
        }
        let mask = if considered >= 64 {
            u64::MAX
        } else {
            (1u64 << considered) - 1
        };
        (self.window & mask).count_ones() as f32 / considered as f32
    }
}

/// One cached segment surface.
pub(crate) struct SegmentSurfaceEntry {
    pub texture: OffscreenTarget,
    /// The slot capture epoch the surface was rendered from — a released
    /// and re-captured slot id can never be composited through a stale
    /// surface.
    pub capture_epoch: u64,
    /// The span transform baked into the surface: capture space is the
    /// span's shapes under exactly this similarity.
    pub cap_center: [f32; 2],
    pub cap_rot: [f32; 2],
    pub cap_scale: f32,
    pub rect: CaptureRect,
    last_seen_frame: u64,
}

/// The per-frame lookup maps are keyed by small POD keys and hit on every
/// retained item every frame; std's SipHash showed up at ~0.2 ms/frame on
/// the watch profile. FxHasher is the crate's convention for exactly this.
type FxMap<K, V> =
    std::collections::HashMap<K, V, std::hash::BuildHasherDefault<cranpose_ui_graphics::FxHasher>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SegmentSurfaceStats {
    pub captures: u64,
    pub composites: u64,
    pub dirty_recaptures: u64,
    pub rejected_churn: u64,
    pub rejected_economics: u64,
    pub rejected_capacity: u64,
    pub evictions: u64,
    pub waived_captures: u64,
}

/// A capture the frame must encode for a `Composite` decision: the claimed
/// per-frame capture slot and the measured geometry the entry is installed
/// with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CapturePlan {
    pub index: u32,
    pub rect: CaptureRect,
    pub member_px: f32,
}

/// What the prepare arm decided for one retained item this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SegmentSurfaceDecision {
    /// Draw the member quads as before.
    Direct,
    /// Composite the cached surface (fresh from this frame's capture pass
    /// when `capture` is `Some`).
    Composite { capture: Option<CapturePlan> },
}

/// The renderer-side cache. GPU resources (offscreen textures, the capture
/// viewport-uniform buffer) are owned here; the capture-transform slots live
/// in the replay transform buffer, which the renderer sizes with
/// [`SEGMENT_CAPTURE_SLOTS`] extra strides.
#[derive(Default)]
pub(crate) struct SegmentSurfaceCache {
    entries: FxMap<SegmentSurfaceKey, SegmentSurfaceEntry>,
    admission: FxMap<SegmentSurfaceKey, AdmissionTrack>,
    bytes: u64,
    frame: u64,
    config: Option<SegmentSurfaceConfig>,
    /// Capture slots claimed this frame, across all partitions.
    capture_cursor: u32,
    capture_uniforms: Option<CaptureUniforms>,
    /// This frame's recolor-patched shape indices per slot, memoized on the
    /// FIRST partition that plans (the prepare arms drain the renderer's
    /// parked patch list, so later partitions could no longer read it).
    dirty_by_slot: FxMap<u32, Vec<u32>>,
    dirty_ready: bool,
    pub(crate) stats: SegmentSurfaceStats,
}

pub(crate) struct CaptureUniforms {
    pub buffer: wgpu::Buffer,
    pub bind_groups: Vec<wgpu::BindGroup>,
}

impl SegmentSurfaceCache {
    /// Once per frame: refresh config, advance the frame counter, sweep.
    pub(crate) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.capture_cursor = 0;
        self.dirty_ready = false;
        let config = SegmentSurfaceConfig::load();
        if !config.enabled {
            if !self.entries.is_empty() {
                self.clear();
            }
            self.admission.clear();
            self.config = Some(config);
            return;
        }
        self.config = Some(config);
        // Cheap periodic sweep: entries and admission tracks the scene
        // stopped producing decay away instead of pinning VRAM.
        if self.frame.is_multiple_of(64) {
            let frame = self.frame;
            let bytes = &mut self.bytes;
            let stats = &mut self.stats;
            self.entries.retain(|_, entry| {
                let keep = frame.wrapping_sub(entry.last_seen_frame) < ENTRY_IDLE_EVICT_FRAMES;
                if !keep {
                    *bytes = bytes.saturating_sub(entry.rect.byte_size());
                    stats.evictions += 1;
                }
                keep
            });
            self.admission
                .retain(|_, track| frame.wrapping_sub(track.last_noted_frame) < 128);
        }
    }

    pub(crate) fn config(&self) -> SegmentSurfaceConfig {
        self.config.unwrap_or_else(SegmentSurfaceConfig::load)
    }

    /// Test-only frame advance that keeps an explicit config instead of
    /// reloading it from the environment (which would disable the cache
    /// and clear the admission state mid-test).
    #[cfg(test)]
    fn advance_frame_for_tests(&mut self, config: SegmentSurfaceConfig) {
        self.frame = self.frame.wrapping_add(1);
        self.capture_cursor = 0;
        self.dirty_ready = false;
        self.config = Some(config);
    }

    pub(crate) fn enabled(&self) -> bool {
        self.config().enabled
    }

    pub(crate) fn clear(&mut self) {
        self.stats.evictions += self.entries.len() as u64;
        self.entries.clear();
        self.bytes = 0;
    }

    /// Drops every entry captured from `slot` — called when the renderer
    /// releases the replay slot (segment death).
    pub(crate) fn drop_slot(&mut self, slot: u32) {
        let bytes = &mut self.bytes;
        let stats = &mut self.stats;
        self.entries.retain(|key, entry| {
            let keep = key.slot != slot;
            if !keep {
                *bytes = bytes.saturating_sub(entry.rect.byte_size());
                stats.evictions += 1;
            }
            keep
        });
        self.admission.retain(|key, _| key.slot != slot);
    }

    pub(crate) fn entry(&self, key: &SegmentSurfaceKey) -> Option<&SegmentSurfaceEntry> {
        self.entries.get(key)
    }

    /// Memoizes this frame's recolor-patch coverage from the renderer's
    /// parked patch list; `patches` is consulted only on the frame's first
    /// call (later partitions reuse the memo — see `dirty_by_slot`).
    pub(crate) fn ensure_dirty_map<'a>(&mut self, patches: impl Iterator<Item = (u32, u32)> + 'a) {
        if self.dirty_ready {
            return;
        }
        self.dirty_ready = true;
        self.dirty_by_slot.clear();
        for (slot, shape_index) in patches {
            self.dirty_by_slot
                .entry(slot)
                .or_default()
                .push(shape_index);
        }
    }

    /// Whether any of this frame's recolor patches lands inside the key's
    /// shape range.
    pub(crate) fn range_dirty(&self, slot: u32, first: u32, last: u32) -> bool {
        self.dirty_by_slot
            .get(&slot)
            .is_some_and(|indices| indices.iter().any(|index| *index >= first && *index < last))
    }

    /// Decides one retained item's path this frame and updates the churn
    /// window. `capture_epoch` is the slot's current epoch, `dirty` whether
    /// a recolor patch touches the range this frame, `scale` the span's
    /// current similarity scale, and `plan` a closure that measures the
    /// capture-space geometry ONLY when a capture is actually wanted —
    /// `(rect, member_px)` of the padded capture rect and the member-quad
    /// pixel sum.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decide(
        &mut self,
        key: SegmentSurfaceKey,
        capture_epoch: u64,
        dirty: bool,
        scale: f32,
        plan: impl FnOnce() -> Option<(CaptureRect, f32)>,
    ) -> SegmentSurfaceDecision {
        let config = self.config();
        if !config.enabled {
            return SegmentSurfaceDecision::Direct;
        }
        let frame = self.frame;
        let track = self.admission.entry(key).or_default();
        track.note_frame(frame, dirty);
        let warm = track.warm();
        let rate = track.recolor_rate();
        // Warmup waiver: at bootstrap frame rates the warmup window is
        // wall-SECONDS during which every frame redraws the whole scene
        // direct, so a candidate with no recolor history may capture
        // before the window fills when the capture pays for itself within
        // about one frame (the tighter `waiver_ratio` bar below). Waste is
        // bounded: a churny key has no recolor history at bootstrap and
        // can waive at most once — its first recolor invalidates the entry
        // in that same frame, within ~2 frames its rate crosses the churn
        // gate and the key leaves the cache — and `SEGMENT_CAPTURE_SLOTS`
        // caps any one frame's capture cost.
        let waived = !warm
            && !track.has_recolored()
            && config.waiver_ratio > 0.0
            && !self.entries.contains_key(&key);

        // A live entry serves the frame unless something invalidates it.
        let (stale, drifted) = match self.entries.get_mut(&key) {
            Some(entry) if entry.capture_epoch == capture_epoch => {
                let drift = if entry.cap_scale > f32::EPSILON {
                    (scale / entry.cap_scale - 1.0).abs()
                } else {
                    f32::INFINITY
                };
                if !dirty && drift <= config.scale_eps {
                    entry.last_seen_frame = frame;
                    self.stats.composites += 1;
                    return SegmentSurfaceDecision::Composite { capture: None };
                }
                (dirty, drift > config.scale_eps)
            }
            Some(_) => (true, false),
            None => (false, false),
        };

        // Anything below needs a (re)capture. Gate on churn first: a stale
        // entry whose segment now churns must LEAVE the cache, not
        // recapture forever. A waived candidate falls through to buy in at
        // the tighter bar.
        if (!warm && !waived) || rate > config.recolor_rate {
            if stale || drifted {
                self.remove(&key);
            }
            if warm {
                self.stats.rejected_churn += 1;
            }
            return SegmentSurfaceDecision::Direct;
        }
        if self.capture_cursor >= SEGMENT_CAPTURE_SLOTS {
            // No capture slot left this frame. A merely drifted entry still
            // composites (quality bound, not correctness); a stale one must
            // not be sampled and its range draws direct.
            if stale {
                self.remove(&key);
                self.stats.rejected_capacity += 1;
                return SegmentSurfaceDecision::Direct;
            }
            if drifted && let Some(entry) = self.entries.get_mut(&key) {
                entry.last_seen_frame = frame;
                self.stats.composites += 1;
                return SegmentSurfaceDecision::Composite { capture: None };
            }
            self.stats.rejected_capacity += 1;
            return SegmentSurfaceDecision::Direct;
        }

        let Some((rect, member_px)) = plan() else {
            self.remove(&key);
            return SegmentSurfaceDecision::Direct;
        };
        let byte_size = rect.byte_size();
        let surface_px = rect.width as f32 * rect.height as f32;
        let member_px_priced = member_px.is_finite() && member_px > 0.0;
        let admit_ratio = if waived {
            config.waiver_ratio
        } else {
            config.cost_ratio
        };
        if byte_size > MAX_SEGMENT_SURFACE_ENTRY_BYTES
            || !member_px_priced
            || surface_px > admit_ratio * member_px
        {
            self.remove(&key);
            self.stats.rejected_economics += 1;
            return SegmentSurfaceDecision::Direct;
        }
        let existing_bytes = self
            .entries
            .get(&key)
            .map(|entry| entry.rect.byte_size())
            .unwrap_or(0);
        if self.bytes.saturating_sub(existing_bytes) + byte_size > MAX_SEGMENT_SURFACE_BYTES {
            self.remove(&key);
            self.stats.rejected_capacity += 1;
            return SegmentSurfaceDecision::Direct;
        }
        let capture_index = self.capture_cursor;
        self.capture_cursor += 1;
        self.stats.captures += 1;
        if waived {
            self.stats.waived_captures += 1;
        }
        // Always-on engagement proof for device logs, mirroring the
        // [command-replay] health line: captures are rare (one per admitted
        // segment per invalidation), so one warn per capture is bounded by
        // the mechanism itself, and an on-watch A/B arm that never prints
        // this line provably measured a vacuous mechanism.
        log::warn!(
            "[segment-surface] capture #{}: composites {} dirty {} rejected churn {} econ {} cap {} evictions {} waived {}",
            self.stats.captures,
            self.stats.composites,
            self.stats.dirty_recaptures,
            self.stats.rejected_churn,
            self.stats.rejected_economics,
            self.stats.rejected_capacity,
            self.stats.evictions,
            self.stats.waived_captures,
        );
        if stale && dirty {
            self.stats.dirty_recaptures += 1;
        }
        self.stats.composites += 1;
        SegmentSurfaceDecision::Composite {
            capture: Some(CapturePlan {
                index: capture_index,
                rect,
                member_px,
            }),
        }
    }

    /// Installs (or replaces) the entry a `Composite { capture: Some }`
    /// decision promised.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn install_entry(
        &mut self,
        key: SegmentSurfaceKey,
        capture_epoch: u64,
        cap_center: [f32; 2],
        cap_rot: [f32; 2],
        cap_scale: f32,
        rect: CaptureRect,
        texture: OffscreenTarget,
    ) {
        if let Some(previous) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(previous.rect.byte_size());
        }
        self.bytes += rect.byte_size();
        self.entries.insert(
            key,
            SegmentSurfaceEntry {
                texture,
                capture_epoch,
                cap_center,
                cap_rot,
                cap_scale,
                rect,
                last_seen_frame: self.frame,
            },
        );
    }

    pub(crate) fn remove(&mut self, key: &SegmentSurfaceKey) {
        if let Some(entry) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.rect.byte_size());
            self.stats.evictions += 1;
        }
    }

    /// Takes an existing entry's texture for size-matched reuse during
    /// recapture (avoids an offscreen-pool round trip for the common
    /// same-size recapture).
    pub(crate) fn take_texture_for_recapture(
        &mut self,
        key: &SegmentSurfaceKey,
        rect: &CaptureRect,
    ) -> Option<OffscreenTarget> {
        let matches = self
            .entries
            .get(key)
            .map(|entry| entry.rect.width == rect.width && entry.rect.height == rect.height)
            .unwrap_or(false);
        if !matches {
            return None;
        }
        let entry = self.entries.remove(key)?;
        self.bytes = self.bytes.saturating_sub(entry.rect.byte_size());
        Some(entry.texture)
    }

    pub(crate) fn capture_uniforms(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> &CaptureUniforms {
        self.capture_uniforms.get_or_insert_with(|| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Segment Capture Uniform Buffer"),
                size: SEGMENT_CAPTURE_SLOTS as u64 * SEGMENT_CAPTURE_UNIFORM_STRIDE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_groups = (0..SEGMENT_CAPTURE_SLOTS)
                .map(|index| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Segment Capture Uniform Bind Group"),
                        layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &buffer,
                                offset: index as u64 * SEGMENT_CAPTURE_UNIFORM_STRIDE,
                                size: std::num::NonZeroU64::new(16),
                            }),
                        }],
                    })
                })
                .collect();
            CaptureUniforms {
                buffer,
                bind_groups,
            }
        })
    }

    pub(crate) fn capture_uniform_bind_group(&self, index: u32) -> Option<&wgpu::BindGroup> {
        self.capture_uniforms
            .as_ref()
            .and_then(|u| u.bind_groups.get(index as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn similarity(center: [f32; 2], angle: f32, scale: f32) -> Affine2 {
        Affine2::from_similarity(center, [angle.cos(), angle.sin()], scale)
    }

    #[test]
    fn affine_similarity_matches_pointwise_definition() {
        let c = [204.0, 204.0];
        let t = similarity(c, 0.37, 1.02);
        let p = [150.0, 90.0];
        let (sin, cos) = 0.37f32.sin_cos();
        let dx = p[0] - c[0];
        let dy = p[1] - c[1];
        let expected = [
            c[0] + (dx * cos - dy * sin) * 1.02,
            c[1] + (dx * sin + dy * cos) * 1.02,
        ];
        let got = t.apply(p);
        assert!((got[0] - expected[0]).abs() < 1e-3 && (got[1] - expected[1]).abs() < 1e-3);
    }

    #[test]
    fn compose_with_inverse_is_identity() {
        let a = similarity([204.0, 204.0], 0.9, 1.1);
        let b = similarity([200.0, 210.0], -0.3, 0.97);
        let e = a.compose(&b.invert().expect("invertible"));
        let round_trip = e.compose(&b);
        for p in [[0.0, 0.0], [408.0, 0.0], [123.4, 321.0]] {
            let via = round_trip.apply(p);
            let direct = a.apply(p);
            assert!(
                (via[0] - direct[0]).abs() < 1e-2 && (via[1] - direct[1]).abs() < 1e-2,
                "compose/invert drifted: {via:?} vs {direct:?}"
            );
        }
    }

    #[test]
    fn identity_detection_is_tight() {
        let c = [204.0, 204.0];
        let id = similarity(c, 0.0, 1.0);
        assert!(id.compose(&id.invert().unwrap()).is_identity_for_sampling());
        assert!(!similarity(c, 0.01, 1.0).is_identity_for_sampling());
        assert!(!similarity(c, 0.0, 1.001).is_identity_for_sampling());
        assert!(
            !Affine2 {
                l: [[1.0, 0.0], [0.0, 1.0]],
                t: [1.0 / 128.0, 0.0],
            }
            .is_identity_for_sampling()
        );
    }

    #[test]
    fn integer_translation_is_texel_exact_for_sampling() {
        let translated = Affine2 {
            l: [[1.0, 0.0], [0.0, 1.0]],
            t: [3.0, -2.0],
        };
        assert!(translated.is_integer_translation_for_sampling());
        assert!(
            !Affine2 {
                l: [[1.0, 0.0], [0.0, 1.0]],
                t: [3.25, -2.0],
            }
            .is_integer_translation_for_sampling()
        );
        assert!(!similarity([204.0, 204.0], 0.01, 1.0).is_integer_translation_for_sampling());
    }

    #[test]
    fn snap_rect_pads_and_snaps_to_integers() {
        let rect = snap_capture_rect([10.4, 20.6], [110.2, 90.1], 4096).expect("rect");
        assert_eq!(rect.origin, [8.0, 18.0]);
        assert_eq!((rect.width, rect.height), (105, 75));
        assert!(snap_capture_rect([5.0, 5.0], [5.0, 5.0], 4096).is_none());
        assert!(snap_capture_rect([0.0, 0.0], [9000.0, 10.0], 4096).is_none());
    }

    #[test]
    fn churn_window_separates_rings_from_twinkles() {
        // The measured distribution: rings recolor never, twinkles on ~91%
        // of frames. Both must classify correctly well before the window
        // saturates.
        let mut ring = AdmissionTrack::default();
        let mut twinkle = AdmissionTrack::default();
        for frame in 1..=16u64 {
            ring.note_frame(frame, false);
            twinkle.note_frame(frame, frame % 11 != 0);
        }
        assert!(ring.warm() && ring.recolor_rate() == 0.0);
        assert!(twinkle.warm() && twinkle.recolor_rate() > 0.5);
    }

    #[test]
    fn decide_admits_only_warm_clean_segments_that_pay() {
        let mut cache = SegmentSurfaceCache::default();
        // Waiver held shut: the warmup count itself is what this test
        // pins; the waiver's own admissions are pinned below.
        let config = SegmentSurfaceConfig {
            waiver_ratio: 0.0,
            ..SegmentSurfaceConfig::test_default()
        };
        let key = SegmentSurfaceKey {
            slot: 3,
            first_shape: 0,
            shape_count: 420,
        };
        let rect = CaptureRect {
            origin: [0.0, 0.0],
            width: 300,
            height: 300,
        };
        // Cold: direct, no capture, until the observation that completes
        // the warmup window.
        for _ in 0..ADMISSION_WARMUP_FRAMES - 1 {
            cache.advance_frame_for_tests(config);
            let decision = cache.decide(key, 7, false, 1.0, || Some((rect, 40_000.0)));
            assert_eq!(decision, SegmentSurfaceDecision::Direct);
        }
        // Warm and clean: capture.
        cache.advance_frame_for_tests(config);
        let decision = cache.decide(key, 7, false, 1.0, || Some((rect, 40_000.0)));
        assert_eq!(
            decision,
            SegmentSurfaceDecision::Composite {
                capture: Some(CapturePlan {
                    index: 0,
                    rect,
                    member_px: 40_000.0,
                })
            }
        );
        // Economics: a sparse segment in the same box is rejected.
        let sparse = SegmentSurfaceKey {
            slot: 4,
            first_shape: 0,
            shape_count: 3,
        };
        for _ in 0..ADMISSION_WARMUP_FRAMES + 1 {
            cache.advance_frame_for_tests(config);
            let _ = cache.decide(sparse, 9, false, 1.0, || Some((rect, 2_000.0)));
        }
        assert!(cache.stats.rejected_economics > 0);
    }

    #[test]
    fn decide_rejects_per_frame_recolor_churn() {
        let mut cache = SegmentSurfaceCache::default();
        let key = SegmentSurfaceKey {
            slot: 5,
            first_shape: 0,
            shape_count: 220,
        };
        let rect = CaptureRect {
            origin: [0.0, 0.0],
            width: 160,
            height: 160,
        };
        for _ in 0..24 {
            cache.advance_frame_for_tests(SegmentSurfaceConfig::test_default());
            let decision = cache.decide(key, 11, true, 1.0, || Some((rect, 30_000.0)));
            assert_eq!(decision, SegmentSurfaceDecision::Direct);
        }
        assert_eq!(cache.stats.captures, 0);
        assert!(cache.stats.rejected_churn > 0);
    }

    #[test]
    fn warmup_waiver_admits_a_paying_static_candidate_immediately() {
        let mut cache = SegmentSurfaceCache::default();
        let key = SegmentSurfaceKey {
            slot: 6,
            first_shape: 0,
            shape_count: 420,
        };
        let rect = CaptureRect {
            origin: [0.0, 0.0],
            width: 300,
            height: 300,
        };
        // 90k surface px against 120k member px: the capture pays for
        // itself within one frame, and the key has never recolored — the
        // waiver must not wait out the warmup window.
        cache.advance_frame_for_tests(SegmentSurfaceConfig::test_default());
        let mut decision = cache.decide(key, 7, false, 1.0, || Some((rect, 120_000.0)));
        if decision == SegmentSurfaceDecision::Direct {
            cache.advance_frame_for_tests(SegmentSurfaceConfig::test_default());
            decision = cache.decide(key, 7, false, 1.0, || Some((rect, 120_000.0)));
        }
        assert_eq!(
            decision,
            SegmentSurfaceDecision::Composite {
                capture: Some(CapturePlan {
                    index: 0,
                    rect,
                    member_px: 120_000.0,
                })
            },
            "a paying, never-recolored candidate must capture by the second frame"
        );
        assert_eq!(cache.stats.waived_captures, 1);
    }

    #[test]
    fn warmup_waiver_skips_a_recolored_candidate() {
        let mut cache = SegmentSurfaceCache::default();
        let key = SegmentSurfaceKey {
            slot: 6,
            first_shape: 0,
            shape_count: 420,
        };
        let rect = CaptureRect {
            origin: [0.0, 0.0],
            width: 300,
            height: 300,
        };
        // One recolor on the first observed frame disqualifies the key
        // from waiving for the rest of the warmup window, paying
        // economics notwithstanding.
        for frame in 0..ADMISSION_WARMUP_FRAMES - 1 {
            cache.advance_frame_for_tests(SegmentSurfaceConfig::test_default());
            let decision = cache.decide(key, 7, frame == 0, 1.0, || Some((rect, 120_000.0)));
            assert_eq!(decision, SegmentSurfaceDecision::Direct);
        }
        assert_eq!(cache.stats.captures, 0);
        assert_eq!(cache.stats.waived_captures, 0);
    }

    #[test]
    fn warmup_waiver_respects_its_economics_bar() {
        let mut cache = SegmentSurfaceCache::default();
        let key = SegmentSurfaceKey {
            slot: 6,
            first_shape: 0,
            shape_count: 420,
        };
        let rect = CaptureRect {
            origin: [0.0, 0.0],
            width: 300,
            height: 300,
        };
        // 90k surface px against 40k member px sits between the bars:
        // over the waiver's 1.0, under the steady-state 3.0 — direct
        // through warmup, admitted once warm.
        for _ in 0..ADMISSION_WARMUP_FRAMES - 1 {
            cache.advance_frame_for_tests(SegmentSurfaceConfig::test_default());
            let decision = cache.decide(key, 7, false, 1.0, || Some((rect, 40_000.0)));
            assert_eq!(decision, SegmentSurfaceDecision::Direct);
        }
        cache.advance_frame_for_tests(SegmentSurfaceConfig::test_default());
        let decision = cache.decide(key, 7, false, 1.0, || Some((rect, 40_000.0)));
        assert_eq!(
            decision,
            SegmentSurfaceDecision::Composite {
                capture: Some(CapturePlan {
                    index: 0,
                    rect,
                    member_px: 40_000.0,
                })
            }
        );
        assert_eq!(cache.stats.waived_captures, 0);
    }

    #[test]
    fn zero_waiver_ratio_disables_the_waiver() {
        let mut cache = SegmentSurfaceCache::default();
        let config = SegmentSurfaceConfig {
            waiver_ratio: 0.0,
            ..SegmentSurfaceConfig::test_default()
        };
        let key = SegmentSurfaceKey {
            slot: 6,
            first_shape: 0,
            shape_count: 420,
        };
        let rect = CaptureRect {
            origin: [0.0, 0.0],
            width: 300,
            height: 300,
        };
        // The candidate would waive on economics; the kill switch must
        // hold warmup admission exactly where it was.
        for _ in 0..ADMISSION_WARMUP_FRAMES - 1 {
            cache.advance_frame_for_tests(config);
            let decision = cache.decide(key, 7, false, 1.0, || Some((rect, 120_000.0)));
            assert_eq!(decision, SegmentSurfaceDecision::Direct);
        }
        cache.advance_frame_for_tests(config);
        let decision = cache.decide(key, 7, false, 1.0, || Some((rect, 120_000.0)));
        assert!(matches!(
            decision,
            SegmentSurfaceDecision::Composite { capture: Some(_) }
        ));
        assert_eq!(cache.stats.waived_captures, 0);
    }
}

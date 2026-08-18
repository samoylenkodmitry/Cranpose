//! Display clip region: framework-level culling of pixels the display
//! physically never shows.
//!
//! The surface has a VISIBLE REGION — the part of it the panel actually
//! displays. Everything outside that region is cullable for any app and
//! any layout, because no layout can make an invisible pixel visible. The
//! mechanism is fully general: the region's COMPLEMENT is tessellated
//! into a conservative occluder mesh, drawn first into a small transient
//! depth attachment on the full-frame pass (depth write on, color writes
//! off, no discard — early-Z/LRZ eligible), and every content pipeline
//! runs a depth-tested variant (compare `Less`, write off). Content emits
//! clip z 0.5 in those variants, the occluder writes 0.0, the clear is
//! 1.0 — so occluded pixels fail the test before the fragment shader
//! runs.
//!
//! [`DisplayVisibleRegion`] names the region; providers plug in as
//! variants plus a [`tessellate_complement`] arm, without touching the
//! cull machinery. The first provider is the round display: Android's
//! `AConfiguration` screenRound yields [`DisplayVisibleRegion::InscribedCircle`].
//! Future providers — display cutouts/insets reported by the platform, or
//! an explicitly declared clip — are new variants. A rectangular display
//! is [`DisplayVisibleRegion::Full`]: the mechanism is structurally inert,
//! zero cost, and rendering is bitwise identical to a renderer without
//! this capability.

use std::borrow::Cow;

/// The region of the surface the display physically shows. The renderer
/// may refuse to shade anything outside it.
///
/// This is PLATFORM (or otherwise host-declared) truth about the display,
/// never derived from app content — apps cannot invent one for their own
/// scene; they get the cull for free on any layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisplayVisibleRegion {
    /// The whole surface is visible (every rectangular display). The cull
    /// machinery never engages: no depth attachment, no occluder, no
    /// pipeline variants — bitwise-identical rendering.
    #[default]
    Full,
    /// Only the circle inscribed in the surface rect is visible — the
    /// round-display panel shape (center at the surface midpoint, radius
    /// `min(width, height) / 2`).
    InscribedCircle,
}

impl DisplayVisibleRegion {
    /// Whether the region leaves anything to cull at all.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cullable(self) -> bool {
        self != Self::Full
    }
}

/// Depth format of the display-clip attachment: universally supported,
/// 2 bytes per pixel (~333 KB transient at 408²), and with
/// `LoadOp::Clear` + `StoreOp::Discard` it lives and dies in GMEM on tiled
/// GPUs without ever touching main memory.
pub(crate) const DISPLAY_CLIP_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;

/// The clear value of the display-clip depth attachment (far plane).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const DISPLAY_CLIP_DEPTH_CLEAR: f32 = 1.0;

/// Depth state every CONTENT pipeline uses in its display-clip variant:
/// test `Less` against the occluder (which wrote 0.0), never write.
/// `None` for the ordinary no-depth variant, which stays byte-identical
/// to the pipelines that existed before the cull.
pub(crate) fn content_depth_state(depth: bool) -> Option<wgpu::DepthStencilState> {
    depth.then(|| wgpu::DepthStencilState {
        format: DISPLAY_CLIP_DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    })
}

/// The exact clip-position tail every framework vertex stage emits today.
/// [`with_content_z`] rewrites it in the display-clip pipeline variants;
/// the ordinary variants compile the untouched text, so their output
/// cannot drift by construction.
const FLAT_Z_TAIL: &str = "(x, y, 0.0, 1.0);";
const MID_Z_TAIL: &str = "(x, y, 0.5, 1.0);";

/// Rewrites a framework vertex stage's emitted clip z from 0.0 to the
/// display-clip content depth 0.5 — only for the depth pipeline variant.
/// 0.5 keeps a wide, unambiguous margin between content and both the
/// occluder (0.0) and the clear (1.0), which is what lets conservative
/// early-Z / LRZ hardware reject occluded fragments confidently. (Runtime
/// user shaders are NOT rewritten; their conventional z 0.0 still fails
/// `0.0 < 0.0` against the occluder, so they cull correctly, just without
/// the margin.)
///
/// Exact-text substitution, same discipline as `shape_shader_source`: a
/// drifted literal makes this a silent no-op, so the debug assertion (and
/// a unit test below) pin the pattern.
pub(crate) fn with_content_z(source: Cow<'static, str>, depth: bool) -> Cow<'static, str> {
    if !depth {
        return source;
    }
    debug_assert!(
        source.contains(FLAT_Z_TAIL),
        "vertex stage no longer emits `{FLAT_Z_TAIL}`; the display-clip z substitution missed"
    );
    Cow::Owned(source.replace(FLAT_Z_TAIL, MID_Z_TAIL))
}

/// The occluder's own shader: positions arrive pre-baked in NDC, z is the
/// near plane 0.0, and the fragment stage is trivial — no discard, no
/// texture reads — so the draw is early-Z friendly and the pipeline masks
/// off every color write.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const OCCLUDER_SHADER: &str = "\
@vertex
fn mask_vs(@location(0) position: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4<f32>(position, 0.0, 1.0);
}

@fragment
fn mask_fs() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
";

/// Triangles per corner fan of the inscribed-circle tessellation. Four
/// corners × 8 = 32 triangles per frame — the entire per-frame geometry
/// cost of that region's cull.
#[cfg(not(target_arch = "wasm32"))]
const SEGMENTS_PER_CORNER: usize = 8;

/// Conservative tessellations are built against the region inflated by
/// this margin, so f32 vertex rounding and rasterization snapping can
/// never push an occluder edge over a pixel the panel actually shows. The
/// cost is a sub-pixel band of cullable pixels left unculled —
/// conservative in the safe direction.
#[cfg(not(target_arch = "wasm32"))]
const SAFETY_PX: f64 = 0.5;

/// The tessellated complement of a visible region for one surface size.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct ComplementMesh {
    /// Triangle-list vertices, NDC xy.
    pub(crate) vertices: Vec<[f32; 2]>,
    /// Approximate pixels the occluder rejects per full-screen layer of
    /// overdraw: the area outside the visible region. (A tessellation may
    /// under-cover that area; this is a log figure, not an accounting
    /// one.)
    pub(crate) masked_px: u64,
}

/// Tessellates the COMPLEMENT of `region` on a `width`×`height` surface
/// into the occluder mesh the depth pre-pass draws.
///
/// THE CONTRACT every region arm must uphold: the mesh is CONSERVATIVE —
/// every triangle lies strictly outside the visible region, so the
/// occluder can never cover a pixel whose center the display shows.
/// Under-coverage is the only permitted error (unculled cullable pixels
/// cost fill, never correctness). An arm that cannot guarantee this for a
/// given size returns `None` and the caller leaves the cull off.
///
/// [`DisplayVisibleRegion::Full`] has an empty complement and always
/// returns `None`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn tessellate_complement(
    region: DisplayVisibleRegion,
    width: u32,
    height: u32,
) -> Option<ComplementMesh> {
    match region {
        DisplayVisibleRegion::Full => None,
        DisplayVisibleRegion::InscribedCircle => {
            tessellate_inscribed_circle_complement(width, height)
        }
    }
}

/// The inscribed-circle arm of [`tessellate_complement`]: for each of the
/// four corners, a triangle fan from the corner point to a polyline
/// CIRCUMSCRIBED about the inscribed circle (every chord tangent to
/// radius `r + SAFETY_PX`, vertices at `r_safe / cos(Δθ/2)`), clamped to
/// the corner's tangent cone. Every triangle therefore lies strictly
/// outside the circle.
///
/// The construction is verified numerically before it is accepted: the
/// distance from the circle center to every triangle must exceed `r`. A
/// failure returns `None` — fail-safe for exotic surface sizes, per the
/// contract above.
#[cfg(not(target_arch = "wasm32"))]
fn tessellate_inscribed_circle_complement(width: u32, height: u32) -> Option<ComplementMesh> {
    use std::f64::consts::PI;

    if width < 16 || height < 16 {
        return None;
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let (cx, cy) = (w / 2.0, h / 2.0);
    let r = w.min(h) / 2.0;
    let r_safe = r + SAFETY_PX;

    // Corner order pairs each corner with the quadrant of the circle that
    // faces it, in atan2-normalized-to-[0, 2π) angles (y grows down, so
    // e.g. the bottom-right corner faces the (+x, +y) quadrant [0, π/2]).
    let corners = [
        (w, h, 0.0),              // bottom-right: quadrant [0, π/2]
        (0.0, h, PI / 2.0),       // bottom-left:  quadrant [π/2, π]
        (0.0, 0.0, PI),           // top-left:     quadrant [π, 3π/2]
        (w, 0.0, 3.0 * PI / 2.0), // top-right:    quadrant [3π/2, 2π]
    ];

    let mut triangles: Vec<[[f64; 2]; 3]> = Vec::with_capacity(4 * SEGMENTS_PER_CORNER);
    for (px, py, quadrant_start) in corners {
        let (dx, dy) = (px - cx, py - cy);
        let d = dx.hypot(dy);
        if d <= r_safe {
            // The corner itself is (numerically) inside the safe circle:
            // nothing invisible to occlude here.
            continue;
        }
        let mut phi = dy.atan2(dx);
        if phi < 0.0 {
            phi += 2.0 * PI;
        }
        // Tangent cone: the corner can only "see" (and thus safely fan to)
        // arc points within ±acos(r_safe/d) of its own direction. For a
        // square surface that is the whole quadrant; for elongated ones it
        // shrinks, deliberately under-covering the side bands.
        let beta = (r_safe / d).acos();
        let theta_lo = quadrant_start.max(phi - beta);
        let theta_hi = (quadrant_start + PI / 2.0).min(phi + beta);
        if theta_hi - theta_lo < 1e-6 {
            continue;
        }
        let step = (theta_hi - theta_lo) / SEGMENTS_PER_CORNER as f64;
        // Chord between consecutive vertices at radius r_v stays at
        // distance r_v·cos(step/2) = r_safe from the center: tangent to
        // the safe circle, strictly outside the real one.
        let r_v = r_safe / (step / 2.0).cos();
        let vertex_at = |theta: f64| [cx + r_v * theta.cos(), cy + r_v * theta.sin()];
        for i in 0..SEGMENTS_PER_CORNER {
            let a = vertex_at(theta_lo + step * i as f64);
            let b = vertex_at(theta_lo + step * (i + 1) as f64);
            triangles.push([[px, py], a, b]);
        }
    }
    if triangles.is_empty() {
        return None;
    }

    // Conservative-containment proof, run once per surface size: the
    // circle center must be strictly farther than r from every triangle.
    // (The center is inside the circle; if it were inside a triangle the
    // distance would be 0 and this rejects the whole mesh.)
    for triangle in &triangles {
        if distance_point_to_triangle([cx, cy], triangle) <= r {
            log::warn!(
                "[display-clip] occluder verification failed at {width}x{height}; cull stays off"
            );
            return None;
        }
    }

    let to_ndc = |[x, y]: [f64; 2]| [(x / w * 2.0 - 1.0) as f32, (1.0 - y / h * 2.0) as f32];
    let vertices = triangles
        .iter()
        .flat_map(|t| t.iter().copied().map(to_ndc))
        .collect();
    let masked_px = (w * h - PI * r * r).max(0.0).round() as u64;
    Some(ComplementMesh {
        vertices,
        masked_px,
    })
}

/// Distance from `p` to the closed triangle `t` (0 when inside).
#[cfg(not(target_arch = "wasm32"))]
fn distance_point_to_triangle(p: [f64; 2], t: &[[f64; 2]; 3]) -> f64 {
    // Inside test via consistent edge orientation.
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let d0 = cross(t[0], t[1], p);
    let d1 = cross(t[1], t[2], p);
    let d2 = cross(t[2], t[0], p);
    let has_neg = d0 < 0.0 || d1 < 0.0 || d2 < 0.0;
    let has_pos = d0 > 0.0 || d1 > 0.0 || d2 > 0.0;
    if !(has_neg && has_pos) {
        return 0.0;
    }
    distance_point_to_segment(p, t[0], t[1])
        .min(distance_point_to_segment(p, t[1], t[2]))
        .min(distance_point_to_segment(p, t[2], t[0]))
}

#[cfg(not(target_arch = "wasm32"))]
fn distance_point_to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let (apx, apy) = (p[0] - a[0], p[1] - a[1]);
    let len_sq = abx * abx + aby * aby;
    let t = if len_sq > 0.0 {
        ((apx * abx + apy * aby) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (apx - t * abx, apy - t * aby);
    dx.hypot(dy)
}

/// Whether a pixel center is inside the visible region — the reference
/// predicate the parity suite tests the GPU path against, defined here so
/// the tests stay parameterized by region rather than hard-coding any one
/// shape.
#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub fn pixel_is_visible(
    region: DisplayVisibleRegion,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
) -> bool {
    match region {
        DisplayVisibleRegion::Full => true,
        DisplayVisibleRegion::InscribedCircle => {
            let dx = (f64::from(x) + 0.5) - f64::from(width) / 2.0;
            let dy = (f64::from(y) + 0.5) - f64::from(height) / 2.0;
            (dx * dx + dy * dy).sqrt() < f64::from(width.min(height)) / 2.0
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// Recovers a vertex's pixel position from its NDC form.
    fn from_ndc([x, y]: [f32; 2], width: u32, height: u32) -> [f64; 2] {
        [
            (f64::from(x) + 1.0) / 2.0 * f64::from(width),
            (1.0 - f64::from(y)) / 2.0 * f64::from(height),
        ]
    }

    fn triangles_of(mesh: &ComplementMesh, width: u32, height: u32) -> Vec<[[f64; 2]; 3]> {
        mesh.vertices
            .chunks_exact(3)
            .map(|t| {
                [
                    from_ndc(t[0], width, height),
                    from_ndc(t[1], width, height),
                    from_ndc(t[2], width, height),
                ]
            })
            .collect()
    }

    fn point_in_triangle(p: [f64; 2], t: &[[f64; 2]; 3]) -> bool {
        distance_point_to_triangle(p, t) == 0.0
    }

    /// The full region has an empty complement: the mechanism must be
    /// structurally inert for every rectangular display.
    #[test]
    fn full_region_tessellates_to_nothing() {
        assert!(tessellate_complement(DisplayVisibleRegion::Full, 408, 408).is_none());
    }

    /// THE tessellation contract, checked at pixel granularity for every
    /// cullable region: no pixel whose center is visible may be covered
    /// by any occluder triangle — for square, odd, and elongated
    /// surfaces.
    #[test]
    fn occluder_never_covers_a_visible_pixel() {
        for region in [DisplayVisibleRegion::InscribedCircle] {
            for (width, height) in [
                (408u32, 408u32),
                (407, 407),
                (466, 466),
                (320, 290),
                (480, 360),
                (1000, 200),
                (64, 64),
            ] {
                let mesh = tessellate_complement(region, width, height)
                    .unwrap_or_else(|| panic!("{region:?} must tessellate at {width}x{height}"));
                let triangles = triangles_of(&mesh, width, height);
                for y in 0..height {
                    for x in 0..width {
                        if !pixel_is_visible(region, width, height, x, y) {
                            continue;
                        }
                        let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
                        for triangle in &triangles {
                            assert!(
                                !point_in_triangle(p, triangle),
                                "{region:?} occluder covers visible pixel ({x}, {y}) \
                                 at {width}x{height}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The occluder must actually be worth drawing: on a square surface
    /// the inscribed-circle tessellation covers nearly all of the
    /// invisible corner region.
    #[test]
    fn inscribed_circle_occluder_covers_most_of_the_invisible_region() {
        let region = DisplayVisibleRegion::InscribedCircle;
        let (width, height) = (408u32, 408u32);
        let mesh = tessellate_complement(region, width, height).expect("mesh must build");
        let triangles = triangles_of(&mesh, width, height);
        let mut invisible = 0u64;
        let mut covered = 0u64;
        for y in 0..height {
            for x in 0..width {
                if pixel_is_visible(region, width, height, x, y) {
                    continue;
                }
                invisible += 1;
                let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
                if triangles.iter().any(|t| point_in_triangle(p, t)) {
                    covered += 1;
                }
            }
        }
        assert!(
            covered as f64 >= invisible as f64 * 0.9,
            "occluder covers {covered} of {invisible} invisible pixels — the cull would be hollow"
        );
    }

    /// Pins the exact-text z substitution against shader drift, for every
    /// vertex stage that draws inside the fused pass.
    #[test]
    fn content_z_substitution_matches_every_fused_pass_vertex_stage() {
        for (name, source) in [
            ("shape", crate::shaders::SHADER),
            // The trimmed solid entries are appended to the shape source
            // under `CRANPOSE_SOLID_TRIM_VARYINGS`; their z tails must take
            // the same rewrite so the trimmed depth pipelines cull too.
            ("shape_solid_trim", crate::shaders::SOLID_TRIM_APPENDIX),
            ("image", crate::shaders::IMAGE_SHADER),
            ("glyph_atlas", crate::shaders::GLYPH_ATLAS_SHADER),
            ("fullscreen_quad", crate::shaders::FULLSCREEN_QUAD_VS),
        ] {
            assert!(
                source.contains(FLAT_Z_TAIL),
                "{name} no longer emits `{FLAT_Z_TAIL}`; display-clip z substitution would no-op"
            );
            let substituted = with_content_z(Cow::Borrowed(source), true);
            assert!(
                !substituted.contains(FLAT_Z_TAIL) && substituted.contains(MID_Z_TAIL),
                "{name} substitution failed"
            );
            assert_eq!(
                with_content_z(Cow::Borrowed(source), false),
                source,
                "{name} flat variant must be the untouched text"
            );
        }
    }
}

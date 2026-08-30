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
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cullable(self) -> bool {
        self != Self::Full
    }
}

pub(crate) const DISPLAY_CLIP_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const DISPLAY_CLIP_DEPTH_CLEAR: f32 = 1.0;

pub(crate) fn content_depth_state(depth: bool) -> Option<wgpu::DepthStencilState> {
    depth.then(|| wgpu::DepthStencilState {
        format: DISPLAY_CLIP_DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    })
}

const FLAT_Z_TAIL: &str = "(x, y, 0.0, 1.0);";
const MID_Z_TAIL: &str = "(x, y, 0.5, 1.0);";

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

#[cfg(not(target_arch = "wasm32"))]
const SEGMENTS_PER_CORNER: usize = 8;

#[cfg(not(target_arch = "wasm32"))]
const SAFETY_PX: f64 = 0.5;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct ComplementMesh {
    pub(crate) vertices: Vec<[f32; 2]>,
    pub(crate) masked_px: u64,
}

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

    let corners = [
        (w, h, 0.0),
        (0.0, h, PI / 2.0),
        (0.0, 0.0, PI),
        (w, 0.0, 3.0 * PI / 2.0),
    ];

    let mut triangles: Vec<[[f64; 2]; 3]> = Vec::with_capacity(4 * SEGMENTS_PER_CORNER);
    for (px, py, quadrant_start) in corners {
        let (dx, dy) = (px - cx, py - cy);
        let d = dx.hypot(dy);
        if d <= r_safe {
            continue;
        }
        let mut phi = dy.atan2(dx);
        if phi < 0.0 {
            phi += 2.0 * PI;
        }
        let beta = (r_safe / d).acos();
        let theta_lo = quadrant_start.max(phi - beta);
        let theta_hi = (quadrant_start + PI / 2.0).min(phi + beta);
        if theta_hi - theta_lo < 1e-6 {
            continue;
        }
        let step = (theta_hi - theta_lo) / SEGMENTS_PER_CORNER as f64;
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

#[cfg(not(target_arch = "wasm32"))]
fn distance_point_to_triangle(p: [f64; 2], t: &[[f64; 2]; 3]) -> f64 {
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

    fn from_ndc([x, y]: [f32; 2], width: u32, height: u32) -> [f64; 2] {
        [
            (f64::from(x) + 1.0) / 2.0 * f64::from(width),
            (1.0 - f64::from(y)) / 2.0 * f64::from(height),
        ]
    }

    fn triangles_of(mesh: &ComplementMesh, width: u32, height: u32) -> Vec<[[f64; 2]; 3]> {
        mesh.vertices
            .as_chunks::<3>()
            .0
            .iter()
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

    #[test]
    fn full_region_tessellates_to_nothing() {
        assert!(tessellate_complement(DisplayVisibleRegion::Full, 408, 408).is_none());
    }

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

    #[test]
    fn content_z_substitution_matches_every_fused_pass_vertex_stage() {
        for (name, source) in [
            ("shape", crate::shaders::SHADER),
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

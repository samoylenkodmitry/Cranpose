//! The light of a hologram's objects: one shader for every object of every hologram, so the scene
//! builds one pipeline for all of them.
//!
//! An object adds light to what lies behind it and hides nothing (`Material::ShadedGlow`), so the
//! objects need no order among themselves and leave no depth that could hide the panel's words.
//! The light comes from four parts, each a value of the shader:
//! - the rim: a surface that turns away from the eye glows, as the edge of a glass does;
//! - the fill: a surface that faces the eye glows a little;
//! - the edges: the edges of a facet or a face glow, as the texture coordinates of the mesh mark
//!   them (`hologram_meshes.rs`), with a core of at least one pixel and a soft fall around it;
//! - the halo: light that is brightest where the surface faces the eye and fades to nothing at its
//!   outline, so a ball of it shows as a soft round glow from every side, with no turn to the eye.
//!
//! A light that fades along the mesh's `v` makes a beam or a cone, and lines across the mesh's `y`
//! that move with time make the scan of a projector. Every part ends at exact zero, so the space
//! around an object stays black.

use std::sync::{Arc, OnceLock};

use cranpose::{
    Color,
    space::{Material, Shaded},
};

static HOLOGRAM: OnceLock<Arc<str>> = OnceLock::new();

const HOLOGRAM_WGSL: &str = "
fn shade(surface: ShadedSurface) -> vec4<f32> {
    let color = vec3<f32>(shaded_param(0u), shaded_param(1u), shaded_param(2u)) * shaded_param(3u);
    let facing = clamp(dot(surface.normal, surface.to_eye), 0.0, 1.0);
    let turned = 1.0 - facing;
    var light = shaded_param(4u) * turned * turned * turned + shaded_param(5u) * facing;
    let uv = surface.uv;
    let cell = fract(uv * max(shaded_param(13u), 1.0));
    let quad = min(min(cell.x, 1.0 - cell.x), min(cell.y, 1.0 - cell.y));
    let facet = min(min(uv.x, uv.y), 1.0 - uv.x - uv.y);
    let band = min(uv.y, 1.0 - uv.y);
    let kind = shaded_param(8u);
    let distance = select(select(quad, facet, kind > 1.5), band, kind > 2.5);
    let pixels = distance / max(fwidth(distance), 1.0e-6);
    let soft = 1.0 - smoothstep(0.0, max(shaded_param(7u), 1.0e-4), distance);
    let core = clamp(1.5 - pixels, 0.0, 1.0);
    light += select(0.0, shaded_param(6u) * max(soft * soft, core), kind > 0.5);
    let halo = shaded_param(9u);
    light += select(0.0, pow(facing, max(halo, 1.0e-3)), halo > 0.0);
    let fade = 1.0 - uv.y;
    light *= mix(1.0, fade * fade, shaded_param(10u));
    let lines = shaded_param(11u);
    let scan = 0.7 + 0.3 * sin(surface.position.y * lines + shaded_param(12u));
    light *= select(1.0, scan, lines > 0.0);
    return vec4<f32>(color * light, 0.0);
}
";

// Which edges of a mesh glow.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Edges {
    None,
    // The four edges of each face that runs from (0, 0) to (1, 1), or of each cell of a grid on
    // the face
    Quads,
    // The three edges of each facet of `hologram_meshes::crystal` and its kin
    Facets,
    // The two edges of a band at v = 0 and v = 1
    Band,
}

// How one object glows. Strength scales all of it: a hidden page and an object far from its
// choice glow less.
#[derive(Clone, Copy)]
pub(crate) struct Glow {
    pub(crate) color: Color,
    pub(crate) strength: f32,
    pub(crate) rim: f32,
    pub(crate) fill: f32,
    pub(crate) edges: (Edges, f32, f32),
    pub(crate) halo: f32,
    pub(crate) fade: f32,
    // The density of the scan lines and their phase
    pub(crate) lines: (f32, f32),
    // The cells of the grid along each side of a face, whose edges glow as the face's do
    pub(crate) cells: f32,
}

impl Glow {
    // Glass of `color`: a bright rim, a faint fill and no edges.
    pub(crate) const fn glass(color: Color) -> Self {
        Self {
            color,
            strength: 1.0,
            rim: 1.1,
            fill: 0.06,
            edges: (Edges::None, 0.0, 0.0),
            halo: 0.0,
            fade: 0.0,
            lines: (0.0, 0.0),
            cells: 1.0,
        }
    }

    // Glass whose `edges` glow by `strength`, with a soft fall of `width` in texture units.
    pub(crate) const fn edged(self, edges: Edges, strength: f32, width: f32) -> Self {
        Self {
            edges: (edges, strength, width),
            ..self
        }
    }

    // A soft round glow of `color` that fades with `power` toward its outline.
    pub(crate) const fn halo(color: Color, power: f32) -> Self {
        Self {
            color,
            strength: 1.0,
            rim: 0.0,
            fill: 0.0,
            edges: (Edges::None, 0.0, 0.0),
            halo: power,
            fade: 0.0,
            lines: (0.0, 0.0),
            cells: 1.0,
        }
    }

    pub(crate) const fn times(self, strength: f32) -> Self {
        Self {
            strength: self.strength * strength,
            ..self
        }
    }

    pub(crate) const fn faded(self) -> Self {
        Self { fade: 1.0, ..self }
    }

    pub(crate) const fn scanned(self, density: f32, phase: f32) -> Self {
        Self {
            lines: (density, phase),
            ..self
        }
    }

    // Faces whose edges of `Edges::Quads` mark a grid of `cells` along each side.
    pub(crate) const fn gridded(self, cells: f32) -> Self {
        Self { cells, ..self }
    }

    pub(crate) fn material(&self) -> Material {
        let Color(red, green, blue, _) = self.color;
        let (edges, edge, width) = self.edges;
        let kind = match edges {
            Edges::None => 0.0,
            Edges::Quads => 1.0,
            Edges::Facets => 2.0,
            Edges::Band => 3.0,
        };
        let mut params = [0.0; Shaded::PARAMS];
        params[..14].copy_from_slice(&[
            red,
            green,
            blue,
            self.strength,
            self.rim,
            self.fill,
            edge,
            width,
            kind,
            self.halo,
            self.fade,
            self.lines.0,
            self.lines.1,
            self.cells,
        ]);
        Material::ShadedGlow(Shaded {
            shader: Arc::clone(HOLOGRAM.get_or_init(|| Arc::from(HOLOGRAM_WGSL))),
            params,
        })
    }
}

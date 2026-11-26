//! The meshes of a hologram's objects, in units that the scene scales: gems and crystals, rings,
//! flowers of petals, arcs, capsules, arrows and the parts of the projector's stage.
//!
//! Every mesh lies in the axes of a volume's content: x to the right, y down and z away from the
//! viewer. Its texture coordinates say where its edges lie, as the hologram's shader reads them
//! (`hologram_material.rs`): a facet of a gem runs from `(0, 0)` to `(1, 0)` and `(0, 1)`, so each
//! of its three edges lies where one of `u`, `v` and `1 - u - v` is zero. A face of a quad runs
//! from `(0, 0)` to `(1, 1)`. A tube, a ring and a band run along `u`. A scene makes each mesh
//! once and draws it at every size.

use std::f32::consts::{PI, TAU};

use cranpose::space::Mesh;

// Positions, texture coordinates, normals and triangles, as a mesh is made of them.
#[derive(Default)]
struct Parts {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl Parts {
    fn vertex(&mut self, position: [f32; 3], uv: [f32; 2], normal: [f32; 3]) -> u32 {
        let index = u32::try_from(self.positions.len()).expect("a hologram's mesh has few points");
        self.positions.push(position);
        self.uvs.push(uv);
        self.normals.push(normalized(normal));
        index
    }

    // A flat triangle whose corners take the texture's `(0, 0)`, `(1, 0)` and `(0, 1)`.
    fn facet(&mut self, [a, b, c]: [[f32; 3]; 3]) {
        let normal = cross(sub(b, a), sub(c, a));
        let first = self.vertex(a, [0.0, 0.0], normal);
        self.vertex(b, [1.0, 0.0], normal);
        self.vertex(c, [0.0, 1.0], normal);
        self.indices.extend([first, first + 1, first + 2]);
    }

    // A grid of `columns` by `rows` cells whose points `point` gives from `(u, v)` with the normal
    // there. `wrap` joins the last column to the first, as around a tube.
    fn grid(
        &mut self,
        (columns, rows): (u32, u32),
        point: impl Fn(f32, f32) -> ([f32; 3], [f32; 3]),
    ) {
        let first = u32::try_from(self.positions.len()).expect("a hologram's mesh has few points");
        for row in 0..=rows {
            let v = row as f32 / rows as f32;
            for column in 0..=columns {
                let u = column as f32 / columns as f32;
                let (position, normal) = point(u, v);
                self.vertex(position, [u, v], normal);
            }
        }
        let stride = columns + 1;
        for row in 0..rows {
            for column in 0..columns {
                let at = first + row * stride + column;
                self.indices.extend([
                    at,
                    at + 1,
                    at + stride,
                    at + 1,
                    at + stride + 1,
                    at + stride,
                ]);
            }
        }
    }

    fn into_mesh(self) -> Mesh {
        Mesh::new(self.positions, self.uvs, Some(self.normals), self.indices)
            .expect("a hologram's mesh is well formed")
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length > 0.0 {
        v.map(|c| c / length)
    } else {
        [0.0, 0.0, -1.0]
    }
}

// A crystal of `sides` facets around: a point at the top at `top`, a girdle of radius one half at
// `y = 0` and a point at the bottom at `bottom`, with flat facets. Up is -y.
pub(crate) fn crystal(sides: u32, top: f32, bottom: f32) -> Mesh {
    let mut parts = Parts::default();
    let girdle = |side: u32| {
        let angle = side as f32 / sides as f32 * TAU;
        [0.5 * angle.sin(), 0.0, -0.5 * angle.cos()]
    };
    let (up, down) = ([0.0, -top, 0.0], [0.0, bottom, 0.0]);
    for side in 0..sides {
        let (a, b) = (girdle(side), girdle(side + 1));
        parts.facet([up, b, a]);
        parts.facet([down, a, b]);
    }
    parts.into_mesh()
}

// A ring of radius one about the y axis, lying flat, with a tube of radius `thickness`.
pub(crate) fn ring(thickness: f32) -> Mesh {
    tube(64, (0.0, TAU), thickness)
}

// A tube of radius `thickness` along the circle of radius one in the x-z plane, from the angle
// `from` to `to` in `columns` steps. The angle 0 lies nearest the viewer, at `(0, 0, -1)`. The
// tube runs along `u`.
fn tube(columns: u32, (from, to): (f32, f32), thickness: f32) -> Mesh {
    let mut parts = Parts::default();
    parts.grid((columns, 6), |u, v| {
        let (sin_a, cos_a) = (from + (to - from) * u).sin_cos();
        let (sin_t, cos_t) = (v * TAU).sin_cos();
        let reach = 1.0 + thickness * cos_t;
        (
            [reach * sin_a, thickness * sin_t, -reach * cos_a],
            [cos_t * sin_a, sin_t, -cos_t * cos_a],
        )
    });
    parts.into_mesh()
}

// Several flat rings of radius one and less about the y axis, as the projector's disc shows them:
// bands of `width` at each of `radii`.
pub(crate) fn rings(radii: &[f32], width: f32) -> Mesh {
    let mut parts = Parts::default();
    for &radius in radii {
        parts.grid((64, 1), |u, v| {
            let (sin, cos) = (u * TAU).sin_cos();
            let reach = radius - width / 2.0 + width * v;
            ([reach * sin, 0.0, -reach * cos], [0.0, -1.0, 0.0])
        });
    }
    parts.into_mesh()
}

// A flower of `petals` petals of `length` about the y axis, open upward, with a small cup in the
// middle. A petal rises by `rise` times its length over the square of the way out, so a large
// rise closes the flower into a bud. Each petal's sides lie at `u = 0` and `u = 1`.
pub(crate) fn flower(petals: u32, length: f32, rise: f32) -> Mesh {
    let mut parts = Parts::default();
    // The cup: a shallow bowl of radius 0.2
    parts.grid((24, 3), |u, v| {
        let (sin, cos) = (u * TAU).sin_cos();
        let reach = 0.2 * v;
        let y = -0.08 * v * v;
        ([reach * sin, y, -reach * cos], [0.0, -1.0, 0.0])
    });
    for petal in 0..petals {
        let angle = petal as f32 / petals as f32 * TAU;
        let (sin, cos) = angle.sin_cos();
        let width = 0.8 * TAU / petals.max(5) as f32;
        parts.grid((2, 8), |u, v| {
            // Along the petal from the middle outward, it rises as a lotus petal does
            let reach = 0.1 + length * v;
            let rise = rise * length * v * v;
            let half = width * (PI * v).sin().powf(0.7) * reach * 0.5;
            let across = (u - 0.5) * 2.0 * half;
            let (x, z) = (reach * sin + across * cos, -reach * cos + across * sin);
            ([x, -rise, z], [-sin * 0.6, -1.0, cos * 0.6])
        });
    }
    parts.into_mesh()
}

// A tube of radius `thickness` along the arc of radius one in the x-z plane from the angle
// `-sweep / 2` to `sweep / 2`, whose middle lies nearest the viewer at `(0, 0, -1)`. The tube runs
// along `u`.
pub(crate) fn arc(sweep: f32, thickness: f32) -> Mesh {
    tube(48, (-sweep / 2.0, sweep / 2.0), thickness)
}

// A capsule along x from -0.5 to 0.5 with round ends, of radius `radius`: a tube about the x axis
// with half balls at its ends.
pub(crate) fn capsule(radius: f32) -> Mesh {
    let mut parts = Parts::default();
    let straight = (0.5 - radius).max(0.0);
    // The profile goes from the left pole to the right one: half a turn of the left end, the
    // straight part and half a turn of the right end
    parts.grid((24, 32), |u, v| {
        let around = u * TAU;
        let (sin_r, cos_r) = around.sin_cos();
        let (x, reach, along) = if v < 0.25 {
            let polar = v / 0.25 * PI / 2.0;
            (
                -straight - radius * polar.cos(),
                radius * polar.sin(),
                -polar.cos(),
            )
        } else if v > 0.75 {
            let polar = (v - 0.75) / 0.25 * PI / 2.0;
            (
                straight + radius * polar.sin(),
                radius * polar.cos(),
                polar.sin(),
            )
        } else {
            (-straight + (v - 0.25) / 0.5 * 2.0 * straight, radius, 0.0)
        };
        let side = (1.0 - along * along).max(0.0).sqrt();
        (
            [x, reach * sin_r, -reach * cos_r],
            [along, side * sin_r, -side * cos_r],
        )
    });
    parts.into_mesh()
}

// A prism of a triangle that points to +x, one unit long and one unit high, `depth` deep.
pub(crate) fn arrow(depth: f32) -> Mesh {
    let mut parts = Parts::default();
    let (front, back) = (-depth / 2.0, depth / 2.0);
    let corners = [[0.5, 0.0], [-0.5, -0.5], [-0.5, 0.5]];
    let at = |[x, y]: [f32; 2], z: f32| [x, y, z];
    parts.facet([
        at(corners[0], front),
        at(corners[2], front),
        at(corners[1], front),
    ]);
    parts.facet([
        at(corners[0], back),
        at(corners[1], back),
        at(corners[2], back),
    ]);
    for side in 0..3 {
        let (a, b) = (corners[side], corners[(side + 1) % 3]);
        parts.facet([at(a, front), at(b, front), at(a, back)]);
        parts.facet([at(b, back), at(a, back), at(b, front)]);
    }
    parts.into_mesh()
}

// A cone open at both ends about the y axis, from radius `bottom` at `y = 0` to `top` at
// `y = -1`. `v` runs from the bottom to the top.
pub(crate) fn cone(bottom: f32, top: f32) -> Mesh {
    let mut parts = Parts::default();
    let slope = bottom - top;
    parts.grid((48, 4), |u, v| {
        let (sin, cos) = (u * TAU).sin_cos();
        let reach = bottom + (top - bottom) * v;
        ([reach * sin, -v, -reach * cos], [sin, slope, -cos])
    });
    parts.into_mesh()
}

// A band that winds `turns` times about the y axis on the cylinder of radius 1, from `y = 0.5`
// down to `y = -0.5`, `width` high. Its normals point out of the cylinder. Its edges lie at
// `v = 0` and `v = 1`, and it runs along `u`.
pub(crate) fn helix(turns: u32, width: f32) -> Mesh {
    let mut parts = Parts::default();
    // Twelve columns a turn keep the band round
    let sweep = TAU * turns as f32;
    parts.grid((12 * turns.max(1), 1), |u, v| {
        let (sin, cos) = (sweep * u).sin_cos();
        let y = 0.5 - u + (v - 0.5) * width;
        ([sin, y, -cos], [sin, 0.0, -cos])
    });
    parts.into_mesh()
}

// `count` small crystals spread through the shell between radii `inner` and `outer` about the y
// axis, from `y = -0.5` to `0.5`, each of size `size`, beside the two ends of the x axis: one mesh,
// so they draw at once. The spread comes from a fixed sequence, so every scene shows the same field.
pub(crate) fn sparks(count: u32, (inner, outer): (f32, f32), size: f32) -> Mesh {
    let mut parts = Parts::default();
    for index in 0..count {
        let at = index as f32;
        // A hash of the index spreads the sparks with no rows or columns. Every other one stands
        // on the left, and each lies within a quarter turn of its side, so none floats over the
        // middle.
        let side = if index % 2 == 0 {
            TAU / 4.0
        } else {
            -TAU / 4.0
        };
        let angle = side + (scatter(at, 1.0) - 0.5) * 0.9;
        let reach = inner + (outer - inner) * scatter(at, 2.0);
        let y = scatter(at, 3.0) - 0.5;
        let (sin, cos) = angle.sin_cos();
        let middle = [reach * sin, y, -reach * cos];
        let point = |dx: f32, dy: f32, dz: f32| {
            [
                middle[0] + dx * size,
                middle[1] + dy * size,
                middle[2] + dz * size,
            ]
        };
        let (up, down) = (point(0.0, -1.0, 0.0), point(0.0, 1.0, 0.0));
        let around = [
            point(0.6, 0.0, 0.0),
            point(0.0, 0.0, 0.6),
            point(-0.6, 0.0, 0.0),
            point(0.0, 0.0, -0.6),
        ];
        for side in 0..4 {
            let (a, b) = (around[side], around[(side + 1) % 4]);
            parts.facet([up, a, b]);
            parts.facet([down, b, a]);
        }
    }
    parts.into_mesh()
}

// A number from 0 to 1 that changes with no pattern from one `index` to the next, one for each
// `seed`.
fn scatter(index: f32, seed: f32) -> f32 {
    ((index * 12.989_8 + seed * 78.233).sin() * 43_758.547)
        .fract()
        .abs()
}

// A sheet from `x = -0.5` to `0.5` and `y = -0.5` to `0.5` facing the viewer, whose ends bend
// away from the viewer by `bend` at `x = ±0.5`. Its four edges lie at the edges of its texture.
pub(crate) fn sheet(bend: f32) -> Mesh {
    let mut parts = Parts::default();
    parts.grid((24, 1), |u, v| {
        let x = u - 0.5;
        // The slope of z = 4 bend x², as the normal leans with it
        let slope = 8.0 * bend * x;
        ([x, v - 0.5, 4.0 * bend * x * x], [slope, 0.0, -1.0])
    });
    parts.into_mesh()
}

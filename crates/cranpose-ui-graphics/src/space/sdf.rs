//! Meshes from signed distance fields: the surface where a field is zero,
//! found on a grid by surface nets.

use std::f32::consts::{PI, TAU};

use super::{Mesh, MeshError, Vec3};

/// The corners of a grid cell, as offsets along x, y and z.
const CORNERS: [[usize; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0],
    [1, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [0, 1, 1],
    [1, 1, 1],
];

/// The edges of a grid cell, as pairs of its corners.
const EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [2, 3],
    [4, 5],
    [6, 7],
    [0, 2],
    [1, 3],
    [4, 6],
    [5, 7],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

/// For each axis, the step along a grid edge, and the four cells around the
/// edge, as offsets back from its start, in turn around the edge.
const AROUND_EDGES: [([usize; 3], [[usize; 3]; 4]); 3] = [
    ([1, 0, 0], [[0, 1, 1], [0, 0, 1], [0, 0, 0], [0, 1, 0]]),
    ([0, 1, 0], [[1, 0, 1], [1, 0, 0], [0, 0, 0], [0, 0, 1]]),
    ([0, 0, 1], [[1, 1, 0], [0, 1, 0], [0, 0, 0], [1, 0, 0]]),
];

/// A field sampled on the points of a grid of `cells` cells along each side
/// of the box from `low` with cells of `step`.
struct Samples {
    values: Vec<f32>,
    cells: usize,
    low: Vec3,
    step: Vec3,
}

impl Samples {
    fn of(field: &impl Fn(Vec3) -> f32, (low, high): (Vec3, Vec3), cells: usize) -> Self {
        let side = cells as f32;
        let step = Vec3::new(
            (high.x - low.x) / side,
            (high.y - low.y) / side,
            (high.z - low.z) / side,
        );
        let points = cells + 1;
        let mut values = Vec::with_capacity(points * points * points);
        for k in 0..points {
            for j in 0..points {
                for i in 0..points {
                    values.push(field(Self::place(low, step, [i, j, k])));
                }
            }
        }
        Self {
            values,
            cells,
            low,
            step,
        }
    }

    fn place(low: Vec3, step: Vec3, [i, j, k]: [usize; 3]) -> Vec3 {
        Vec3::new(
            low.x + step.x * i as f32,
            low.y + step.y * j as f32,
            low.z + step.z * k as f32,
        )
    }

    fn point(&self, at: [usize; 3]) -> Vec3 {
        Self::place(self.low, self.step, at)
    }

    fn value(&self, [i, j, k]: [usize; 3]) -> f32 {
        let points = self.cells + 1;
        self.values[(k * points + j) * points + i]
    }

    /// Where the surface crosses the cell at `cell`: the mean of the points
    /// where it crosses the cell's edges, or `None` when it misses the cell.
    fn cell_point(&self, [i, j, k]: [usize; 3]) -> Option<Vec3> {
        let corner = |index: usize| {
            let [di, dj, dk] = CORNERS[index];
            [i + di, j + dj, k + dk]
        };
        let (mut sum, mut count) = (Vec3::ZERO, 0.0);
        for [a, b] in EDGES {
            let (va, vb) = (self.value(corner(a)), self.value(corner(b)));
            if (va < 0.0) == (vb < 0.0) {
                continue;
            }
            let (pa, pb) = (self.point(corner(a)), self.point(corner(b)));
            sum = sum + pa + (pb - pa) * (va / (va - vb));
            count += 1.0;
        }
        (count > 0.0).then(|| sum * (1.0 / count))
    }
}

impl Mesh {
    /// The surface where `field` is zero within the box from `bounds.0` to
    /// `bounds.1`: a signed distance field, negative inside the shape,
    /// sampled on a grid of `resolution` cells along each side of the box.
    /// The normals follow the field's gradient, and the texture wraps the
    /// shape as it wraps [`Mesh::sphere`], seen from the middle of the box:
    /// its middle faces the viewer, its left and right edges meet at the
    /// back and its top and bottom edges are the poles. A field that changes
    /// gives a shape that changes: build the mesh again for each new shape.
    pub fn from_sdf(
        field: impl Fn(Vec3) -> f32,
        bounds: (Vec3, Vec3),
        resolution: u32,
    ) -> Result<Self, MeshError> {
        let cells = resolution.max(2) as usize;
        let samples = Samples::of(&field, bounds, cells);
        let middle = (bounds.0 + bounds.1) * 0.5;
        let reach = samples.step.x.min(samples.step.y).min(samples.step.z) * 0.5;
        let mut net = Net::default();
        let mut cell_vertices = vec![u32::MAX; cells * cells * cells];
        for k in 0..cells {
            for j in 0..cells {
                for i in 0..cells {
                    if let Some(point) = samples.cell_point([i, j, k]) {
                        cell_vertices[(k * cells + j) * cells + i] =
                            net.vertex(point, gradient(&field, point, reach), middle);
                    }
                }
            }
        }
        let cell = |[i, j, k]: [usize; 3]| cell_vertices[(k * cells + j) * cells + i];
        // Each grid edge the surface crosses joins the four cells around it.
        for (step, around) in AROUND_EDGES {
            for k in 0..cells {
                for j in 0..cells {
                    for i in 0..cells {
                        let start = [i, j, k];
                        let back = |axis: usize| around.iter().any(|offset| offset[axis] > 0);
                        if (0..3).any(|axis| back(axis) && start[axis] == 0) {
                            continue;
                        }
                        let end = [i + step[0], j + step[1], k + step[2]];
                        let cells_around =
                            around.map(|[di, dj, dk]| cell([i - di, j - dj, k - dk]));
                        net.join(&samples, (start, end), cells_around);
                    }
                }
            }
        }
        Self::new(net.positions, net.uvs, Some(net.normals), net.indices)
    }
}

/// The vertices and triangles of a surface net as it grows.
#[derive(Default)]
struct Net {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    /// For each vertex, the copy of it whose texture coordinate lies one
    /// turn on, for the triangles that cross the seam at the back.
    turned: Vec<u32>,
}

impl Net {
    fn vertex(&mut self, point: Vec3, normal: Vec3, middle: Vec3) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push([point.x, point.y, point.z]);
        self.normals.push([normal.x, normal.y, normal.z]);
        self.uvs.push(wrapped_uv(point - middle));
        self.turned.push(u32::MAX);
        index
    }

    /// Two triangles over the four cells around the grid edge from `edge.0`
    /// to `edge.1`, when the surface crosses it.
    fn join(&mut self, samples: &Samples, edge: ([usize; 3], [usize; 3]), around: [u32; 4]) {
        let (from, to) = (samples.value(edge.0), samples.value(edge.1));
        if (from < 0.0) == (to < 0.0) || around.contains(&u32::MAX) {
            return;
        }
        // The quad turns to face the outside, where the field grows.
        let [a, b, c, d] = if from < 0.0 {
            around
        } else {
            [around[0], around[3], around[2], around[1]]
        };
        self.triangle([a, b, c]);
        self.triangle([a, c, d]);
    }

    /// A triangle; one that crosses the texture's seam takes the copies of
    /// its corners on the far side of the seam.
    fn triangle(&mut self, corners: [u32; 3]) {
        let us = corners.map(|corner| self.uvs[corner as usize][0]);
        let crosses = us.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            - us.iter().copied().fold(f32::INFINITY, f32::min)
            > 0.5;
        for (corner, u) in corners.into_iter().zip(us) {
            let index = if crosses && u < 0.5 {
                self.turned_copy(corner)
            } else {
                corner
            };
            self.indices.push(index);
        }
    }

    fn turned_copy(&mut self, corner: u32) -> u32 {
        let at = corner as usize;
        if self.turned[at] == u32::MAX {
            self.turned[at] = self.positions.len() as u32;
            self.positions.push(self.positions[at]);
            self.normals.push(self.normals[at]);
            let [u, v] = self.uvs[at];
            self.uvs.push([u + 1.0, v]);
            self.turned.push(u32::MAX);
        }
        self.turned[at]
    }
}

/// The texture coordinate of the point in `direction` from a shape's
/// middle, as [`Mesh::sphere`] lays its texture.
fn wrapped_uv(direction: Vec3) -> [f32; 2] {
    let Some(direction) = direction.normalized() else {
        return [0.5, 0.5];
    };
    [
        direction.x.atan2(-direction.z) / TAU + 0.5,
        (-direction.y).clamp(-1.0, 1.0).acos() / PI,
    ]
}

/// The direction the field grows fastest at `point`, from steps of `reach`.
fn gradient(field: &impl Fn(Vec3) -> f32, point: Vec3, reach: f32) -> Vec3 {
    let along = |axis: Vec3| field(point + axis * reach) - field(point - axis * reach);
    Vec3::new(
        along(Vec3::new(1.0, 0.0, 0.0)),
        along(Vec3::new(0.0, 1.0, 0.0)),
        along(Vec3::new(0.0, 0.0, 1.0)),
    )
    .normalized()
    .unwrap_or(Vec3::new(0.0, 0.0, -1.0))
}

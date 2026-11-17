// How far the chocolate has slid down the wall, and the sheet of mesh that
// carries the words down by it. The words are laid on the sheet, so a
// pointer finds the letter it shows over through the mesh, however far the
// letter has slid.

use std::sync::Arc;

use cranpose_ui::{
    space::{Mesh, TextureEdges},
    Point, Size,
};

use super::{grid_mesh, walk::ease};

// The sag is smooth, so a coarse grid follows it.
const SHEET_COLUMNS: usize = 64;
const SHEET_ROWS: usize = 40;
// In units.
pub(super) const WARM_REACH: f32 = 105.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Warmth {
    pub strength: f32,
    pub at: Point,
}

impl Warmth {
    pub(super) fn at(self, point: Point, u: f32) -> f32 {
        if self.strength <= 0.0 {
            return 0.0;
        }
        let reach = u * WARM_REACH;
        let (dx, dy) = ((point.x - self.at.x) / reach, (point.y - self.at.y) / reach);
        self.strength * (-(dx * dx + dy * dy) * 0.5).exp()
    }
}

pub(super) fn melt_at(progress: f32) -> f32 {
    ease(0.02, 1.06, progress).powf(0.9)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sag {
    pub melt: f32,
    pub warmth: Warmth,
    pub u: f32,
    pub split: f32,
}

impl Sag {
    pub(super) fn at(&self, point: Point) -> f32 {
        let (x, y) = (point.x / self.u, point.y / self.u);
        // The paragraph slides more evenly than the headline.
        let unevenness = 1.35 - 0.73 * ease(-60.0, 70.0, y - self.split / self.u);
        let slide = self.melt * (15.0 + 0.030 * y) * (1.0 + unevenness * wave(x, y));
        self.u * (slide + 9.0 * self.warmth.at(point, self.u))
    }

    pub(super) fn reaching(&self, x: f32, bottom: f32) -> f32 {
        (0..3).fold(bottom, |y, _| bottom - self.at(Point::new(x, y)))
    }
}

fn wave(x: f32, y: f32) -> f32 {
    0.26 * (x * 0.0207 + 1.3 + 0.5 * (y * 0.0110).sin()).sin()
        + 0.16 * (x * 0.0493 + 4.1 + 0.5 * (y * 0.0170 + 2.0).sin()).sin()
        + 0.045 * (x * 0.1210 + 0.7 + 0.3 * (y * 0.02).sin()).sin()
}

pub(super) struct Sheet {
    size: Size,
    depth: f32,
    made: Option<(Sag, Arc<Mesh>)>,
    uvs: Vec<[f32; 2]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl Sheet {
    pub(super) fn new(size: Size, depth: f32) -> Self {
        let (columns, rows) = (SHEET_COLUMNS + 1, SHEET_ROWS + 1);
        let uvs = (0..rows)
            .flat_map(|row| {
                (0..columns).map(move |column| {
                    [
                        column as f32 / SHEET_COLUMNS as f32,
                        row as f32 / SHEET_ROWS as f32,
                    ]
                })
            })
            .collect();
        Self {
            size,
            depth,
            made: None,
            uvs,
            normals: vec![[0.0, 0.0, -1.0]; columns * rows],
            indices: grid_mesh::quads(columns, rows),
        }
    }

    pub(super) fn mesh(&mut self, sag: Sag) -> Arc<Mesh> {
        if let Some((made, mesh)) = &self.made {
            if *made == sag {
                return Arc::clone(mesh);
            }
        }
        let positions = self
            .uvs
            .iter()
            .map(|&[u, v]| {
                let point = Point::new(u * self.size.width, v * self.size.height);
                [point.x, point.y + sag.at(point), self.depth]
            })
            .collect();
        let mesh = Arc::new(
            Mesh::new(
                positions,
                self.uvs.clone(),
                Some(self.normals.clone()),
                self.indices.clone(),
            )
            .expect("a grid of two or more points each way makes whole triangles")
            .with_texture_edges(TextureEdges::Outline),
        );
        self.made = Some((sag, Arc::clone(&mesh)));
        mesh
    }
}

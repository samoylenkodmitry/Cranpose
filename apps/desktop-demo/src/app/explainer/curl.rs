// A postage stamp torn free along its perforations and curling up from its
// far corner. The curled paper is a developable sheet: it bends only along
// one direction, the peel, which runs from the stamp's top left corner to
// its bottom right one. Its whole shape follows from the angle the paper
// turns through along that direction, so a profile walked once gives every
// point of the stamp.
//
// A stamp's mesh is made for a lift in steps of `1 / LEVELS`, once each, and
// shared by every stamp of that size: a stamp that lifts picks the mesh of
// its lift and makes none.

use std::{cell::RefCell, sync::Arc};

use cranpose_ui::{
    space::{Mesh, TextureEdges},
    Size,
};

// The direction the stamp peels along, of length one: from its top left
// corner toward its bottom right one, which rises first.
pub(super) const PEEL: [f32; 2] = [0.83, 0.557_77];
// How far the free corner turns at full lift, in radians.
const MAX_CURL: f32 = 2.75;
// The lifts a mesh is made for: this many steps from flat to full.
const LEVELS: usize = 48;
// The mesh's points across and down.
const COLUMNS: usize = 22;
const ROWS: usize = 16;
// The steps the profile is walked in.
const PROFILE_STEPS: usize = 256;

// How the paper bends at `lift`: how much of it stays glued down along
// the peel, and how far its free corner turns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bend {
    pub attached: f32,
    pub turn: f32,
    pub diagonal: f32,
}

impl Bend {
    pub(super) fn at(lift: f32, size: Size) -> Self {
        let lift = lift.clamp(0.0, 1.0);
        let diagonal = size.width * PEEL[0] + size.height * PEEL[1];
        Self {
            attached: diagonal * (0.96 + (0.47 - 0.96) * lift.powf(0.75)),
            turn: MAX_CURL * lift,
            diagonal,
        }
    }

    // The angle the paper has turned through `s` along the peel.
    pub(super) fn angle(&self, s: f32) -> f32 {
        let a = ((s - self.attached) / (self.diagonal - self.attached).max(1.0e-3)).clamp(0.0, 1.0);
        self.turn * (0.3 * a + 0.7 * a * a * a)
    }
}

// The meshes of one stamp size, made as the lifts are first asked for.
pub(super) struct CurlMeshes {
    size: Size,
    levels: RefCell<Vec<Option<Arc<Mesh>>>>,
}

impl CurlMeshes {
    pub(super) fn new(size: Size) -> Self {
        Self {
            size,
            levels: RefCell::new(vec![None; LEVELS + 1]),
        }
    }

    // The level a lift is drawn at.
    pub(super) fn level(lift: f32) -> usize {
        ((lift.clamp(0.0, 1.0) * LEVELS as f32).round() as usize).min(LEVELS)
    }

    // The lift of a level.
    pub(super) fn lift(level: usize) -> f32 {
        level as f32 / LEVELS as f32
    }

    // The stamp's mesh at `lift`, in content pixels.
    pub(super) fn mesh(&self, lift: f32) -> Arc<Mesh> {
        let level = Self::level(lift);
        if let Some(mesh) = &self.levels.borrow()[level] {
            return Arc::clone(mesh);
        }
        let mesh = Arc::new(curled(self.size, Bend::at(Self::lift(level), self.size)));
        self.levels.borrow_mut()[level] = Some(Arc::clone(&mesh));
        mesh
    }
}

// A stamp `size` big bent as `bend` says, its face toward the viewer, who
// looks along +z.
fn curled(size: Size, bend: Bend) -> Mesh {
    // Where the paper `s` along the peel lies: how far along the peel and
    // how high, found by walking the profile.
    let step = bend.diagonal / PROFILE_STEPS as f32;
    let mut profile = Vec::with_capacity(PROFILE_STEPS + 1);
    let (mut along, mut up) = (0.0f32, 0.0f32);
    profile.push((along, up));
    for index in 0..PROFILE_STEPS {
        let angle = bend.angle((index as f32 + 0.5) * step);
        along += angle.cos() * step;
        up += angle.sin() * step;
        profile.push((along, up));
    }
    let place = |s: f32| {
        let at = (s / step).clamp(0.0, PROFILE_STEPS as f32);
        let index = (at.floor() as usize).min(PROFILE_STEPS - 1);
        let share = at - index as f32;
        let (a, b) = (profile[index], profile[index + 1]);
        (a.0 + (b.0 - a.0) * share, a.1 + (b.1 - a.1) * share)
    };
    let mut positions = Vec::with_capacity(COLUMNS * ROWS);
    let mut normals = Vec::with_capacity(COLUMNS * ROWS);
    let mut uvs = Vec::with_capacity(COLUMNS * ROWS);
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let uv = [
                column as f32 / (COLUMNS - 1) as f32,
                row as f32 / (ROWS - 1) as f32,
            ];
            let (x, y) = (uv[0] * size.width, uv[1] * size.height);
            let s = x * PEEL[0] + y * PEEL[1];
            let (along, up) = place(s);
            let shift = along - s;
            positions.push([x + PEEL[0] * shift, y + PEEL[1] * shift, -up]);
            let angle = bend.angle(s);
            // The face turns about the axis across the peel, toward the
            // viewer.
            normals.push([PEEL[0] * angle.sin(), PEEL[1] * angle.sin(), -angle.cos()]);
            uvs.push(uv);
        }
    }
    let mut indices = Vec::with_capacity((COLUMNS - 1) * (ROWS - 1) * 6);
    for row in 0..ROWS - 1 {
        for column in 0..COLUMNS - 1 {
            let at = (row * COLUMNS + column) as u32;
            let below = at + COLUMNS as u32;
            indices.extend_from_slice(&[at, at + 1, below, below, at + 1, below + 1]);
        }
    }
    Mesh::new(positions, uvs, Some(normals), indices)
        .expect("a grid of two or more points each way makes whole triangles")
        .with_texture_edges(TextureEdges::Outline)
}

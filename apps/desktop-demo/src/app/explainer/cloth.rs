// A sheet of cloth that hangs from clips: a coarse grid of particles held
// together by distance constraints and stepped with position based
// dynamics, smoothed with a cubic B-spline into the finer mesh that is drawn
// and that the pointer is cast against.

use cranpose_ui::{
    space::{Mesh, TextureEdges, Vec3},
    Size,
};

// Steps of the simulation each second. Frames take as many steps as fit.
const STEPS_PER_SECOND: f32 = 120.0;
// The most steps one frame takes: a frame that comes late does not stall
// the next one catching up.
const MOST_STEPS_A_FRAME: u32 = 4;
const SOLVER_ITERATIONS: u32 = 6;
// Gravity, in content pixels a second squared.
const GRAVITY: f32 = 2600.0;
// The share of its speed a particle keeps each step.
const KEEP_SPEED: f32 = 0.992;
const STRETCH_STIFFNESS: f32 = 1.0;
const SHEAR_STIFFNESS: f32 = 0.6;
const BEND_STIFFNESS: f32 = 0.18;
// How close the cloth comes to the wall behind it.
const WALL_GAP: f32 = 3.0;
// The mesh has this many points between two particles along each side.
const SMOOTHING: usize = 2;

// Where a cloth hangs and how much of it there is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ClothHang {
    // The content's size: the cloth laid flat.
    pub content: Size,
    // Where the top edge's first clip holds it, in scene pixels; the
    // viewer looks along +z.
    pub top_left: Vec3,
    // How deep the wall behind the cloth lies: the cloth stays in front.
    pub wall: f32,
    // How far the clips span along the rod. Less than the content's width
    // gathers the cloth into folds between the clips.
    pub span: f32,
    // How many clips hold the top edge, two or more.
    pub clips: usize,
    // Particles along the cloth's width and height.
    pub columns: usize,
    pub rows: usize,
}

impl ClothHang {
    // The hang's numbers bit for bit: equal bits make the same cloth.
    pub(super) fn bits(&self) -> [u32; 10] {
        [
            self.content.width.to_bits(),
            self.content.height.to_bits(),
            self.top_left.x.to_bits(),
            self.top_left.y.to_bits(),
            self.top_left.z.to_bits(),
            self.wall.to_bits(),
            self.span.to_bits(),
            self.clips as u32,
            self.columns as u32,
            self.rows as u32,
        ]
    }
}

// A push of air on the cloth: a gentle breeze and the gust a button sends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Air {
    // How strong the breeze is, in pixels a second squared.
    pub breeze: f32,
    // The gust, in pixels a second squared, toward the viewer and to the
    // right; zero for none.
    pub gust: f32,
}

#[derive(Clone, Copy)]
struct Link {
    a: u32,
    b: u32,
    rest: f32,
    stiffness: f32,
}

pub(super) struct Cloth {
    hang: ClothHang,
    positions: Vec<Vec3>,
    previous: Vec<Vec3>,
    // Zero for a particle a clip holds, one for a free one.
    freedom: Vec<f32>,
    links: Vec<Link>,
    // Seconds of simulation the steps have not taken yet.
    owed: f32,
    time: f32,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Cloth {
    pub(super) fn new(hang: ClothHang) -> Self {
        let (columns, rows) = (hang.columns.max(2), hang.rows.max(2));
        let hang = ClothHang {
            columns,
            rows,
            clips: hang.clips.max(2),
            ..hang
        };
        let rest_x = hang.content.width / (columns - 1) as f32;
        let rest_y = hang.content.height / (rows - 1) as f32;
        let clip_columns = clip_columns(&hang);
        let mut positions = Vec::with_capacity(columns * rows);
        let mut freedom = Vec::with_capacity(columns * rows);
        for row in 0..rows {
            for column in 0..columns {
                let x = hang.top_left.x + hang.span * column as f32 / (columns - 1) as f32;
                // Pleats alternate toward and away from the wall between the
                // clips, so the gathered cloth folds the way a curtain does.
                let between = pleat(&clip_columns, column);
                let z = hang.top_left.z - 6.0 - between * 16.0;
                positions.push(Vec3::new(x, hang.top_left.y + rest_y * row as f32, z));
                let held = row == 0 && clip_columns.contains(&column);
                freedom.push(if held { 0.0 } else { 1.0 });
            }
        }
        let links = links((columns, rows), (rest_x, rest_y));
        let (uvs, indices) = fine_grid(columns, rows);
        Self {
            hang,
            previous: positions.clone(),
            positions,
            freedom,
            links,
            owed: 0.0,
            time: 0.0,
            uvs,
            indices,
        }
    }

    // Moves the cloth `seconds` on under `air`.
    pub(super) fn advance(&mut self, seconds: f32, air: Air) {
        let step = 1.0 / STEPS_PER_SECOND;
        self.owed = (self.owed + seconds).min(step * MOST_STEPS_A_FRAME as f32);
        while self.owed >= step {
            self.owed -= step;
            self.time += step;
            self.step(step, air);
        }
    }

    fn step(&mut self, seconds: f32, air: Air) {
        let height = self.hang.content.height.max(1.0);
        let top = self.hang.top_left.y;
        let time = self.time;
        for ((position, previous), &free) in self
            .positions
            .iter_mut()
            .zip(&mut self.previous)
            .zip(&self.freedom)
        {
            if free == 0.0 {
                *previous = *position;
                continue;
            }
            let down = ((position.y - top) / height).clamp(0.0, 1.0);
            let push = air_push(*position, time, down, air);
            let speed = (*position - *previous) * KEEP_SPEED;
            let acceleration = Vec3::new(push.x, GRAVITY + push.y, push.z);
            *previous = *position;
            *position = *position + speed + acceleration * (seconds * seconds);
        }
        let wall = self.hang.wall - WALL_GAP;
        for _ in 0..SOLVER_ITERATIONS {
            for link in &self.links {
                let (a, b) = (link.a as usize, link.b as usize);
                let (free_a, free_b) = (self.freedom[a], self.freedom[b]);
                let total = free_a + free_b;
                if total == 0.0 {
                    continue;
                }
                let delta = self.positions[b] - self.positions[a];
                let length = delta.length();
                if length <= f32::EPSILON {
                    continue;
                }
                let correction = delta * ((length - link.rest) / length * link.stiffness / total);
                self.positions[a] = self.positions[a] + correction * free_a;
                self.positions[b] = self.positions[b] - correction * free_b;
            }
            for position in &mut self.positions {
                position.z = position.z.min(wall);
            }
        }
    }

    // The cloth as it hangs now, smoothed into a finer mesh whose texture
    // coordinates lay the content on it flat.
    pub(super) fn mesh(&self) -> Mesh {
        let (columns, rows) = (self.hang.columns, self.hang.rows);
        let (fine_columns, fine_rows) = fine_size(columns, rows);
        // Rows of the coarse grid smoothed along x first, then each fine
        // column along y.
        let mut across = Vec::with_capacity(fine_columns * rows);
        for row in 0..rows {
            let line = &self.positions[row * columns..(row + 1) * columns];
            for column in 0..fine_columns {
                across.push(spline_at(line, 1, column as f32 / SMOOTHING as f32));
            }
        }
        let mut positions = Vec::with_capacity(fine_columns * fine_rows);
        for row in 0..fine_rows {
            for column in 0..fine_columns {
                let point = spline_at(
                    &across[column..],
                    fine_columns,
                    row as f32 / SMOOTHING as f32,
                );
                positions.push(point);
            }
        }
        let normals = grid_normals(&positions, fine_columns, fine_rows);
        Mesh::new(
            positions
                .iter()
                .map(|point| [point.x, point.y, point.z])
                .collect(),
            self.uvs.clone(),
            Some(normals),
            self.indices.clone(),
        )
        .expect("a grid of two or more points each way makes whole triangles")
        .with_texture_edges(TextureEdges::Outline)
    }
}

// The springs between the cloth's points: along each row and column, across
// each cell and over every second point, which resists bending.
fn links((columns, rows): (usize, usize), (rest_x, rest_y): (f32, f32)) -> Vec<Link> {
    let index = |column: usize, row: usize| (row * columns + column) as u32;
    let mut links = Vec::new();
    let mut link = |a: (usize, usize), b: (usize, usize), rest: f32, stiffness: f32| {
        links.push(Link {
            a: index(a.0, a.1),
            b: index(b.0, b.1),
            rest,
            stiffness,
        });
    };
    let diagonal = (rest_x * rest_x + rest_y * rest_y).sqrt();
    for row in 0..rows {
        for column in 0..columns {
            if column + 1 < columns {
                link((column, row), (column + 1, row), rest_x, STRETCH_STIFFNESS);
            }
            if row + 1 < rows {
                link((column, row), (column, row + 1), rest_y, STRETCH_STIFFNESS);
            }
            if column + 1 < columns && row + 1 < rows {
                link(
                    (column, row),
                    (column + 1, row + 1),
                    diagonal,
                    SHEAR_STIFFNESS,
                );
                link(
                    (column + 1, row),
                    (column, row + 1),
                    diagonal,
                    SHEAR_STIFFNESS,
                );
            }
            if column + 2 < columns {
                link(
                    (column, row),
                    (column + 2, row),
                    rest_x * 2.0,
                    BEND_STIFFNESS,
                );
            }
            if row + 2 < rows {
                link(
                    (column, row),
                    (column, row + 2),
                    rest_y * 2.0,
                    BEND_STIFFNESS,
                );
            }
        }
    }
    links
}

// The texture coordinates and the triangles of the fine grid the cloth is
// drawn with.
fn fine_grid(columns: usize, rows: usize) -> (Vec<[f32; 2]>, Vec<u32>) {
    let (fine_columns, fine_rows) = fine_size(columns, rows);
    let mut uvs = Vec::with_capacity(fine_columns * fine_rows);
    for row in 0..fine_rows {
        for column in 0..fine_columns {
            uvs.push([
                column as f32 / (fine_columns - 1) as f32,
                row as f32 / (fine_rows - 1) as f32,
            ]);
        }
    }
    let mut indices = Vec::with_capacity((fine_columns - 1) * (fine_rows - 1) * 6);
    for row in 0..fine_rows - 1 {
        for column in 0..fine_columns - 1 {
            let top_left = (row * fine_columns + column) as u32;
            let bottom_left = top_left + fine_columns as u32;
            indices.extend([
                top_left,
                bottom_left,
                top_left + 1,
                top_left + 1,
                bottom_left,
                bottom_left + 1,
            ]);
        }
    }
    (uvs, indices)
}

// The columns of the top row the clips hold, evenly spread from the first
// column to the last.
fn clip_columns(hang: &ClothHang) -> Vec<usize> {
    (0..hang.clips)
        .map(|clip| {
            let share = clip as f32 / (hang.clips - 1) as f32;
            (share * (hang.columns - 1) as f32).round() as usize
        })
        .collect()
}

// From -1 to 1 between two clips, zero at each clip: which way the cloth
// between them first folds.
fn pleat(clips: &[usize], column: usize) -> f32 {
    let Some(next) = clips.iter().position(|&clip| clip >= column) else {
        return 0.0;
    };
    if next == 0 {
        return 0.0;
    }
    let (from, to) = (clips[next - 1], clips[next]);
    let share = (column - from) as f32 / (to - from).max(1) as f32;
    let sign = if next % 2 == 0 { 1.0 } else { -1.0 };
    // Pleats of uneven depth, as a hand hangs them.
    let depth = 0.45 + 0.55 * ((next as f32 * 12.9898).sin() * 43_758.547).fract().abs();
    sign * depth * (share * std::f32::consts::PI).sin()
}

// The air's push on a particle at `point`, `down` of the way from the top
// edge to the bottom: a breeze that wanders over the cloth and a gust.
fn air_push(point: Vec3, time: f32, down: f32, air: Air) -> Vec3 {
    let wander = (point.x * 0.0041 + time * 1.3).sin() * 0.6
        + (point.x * 0.0073 - point.y * 0.0052 + time * 2.1).sin() * 0.4;
    let reach = down.sqrt();
    let breeze = air.breeze * wander * reach;
    let gust_wave = 0.65 + 0.35 * (point.x * 0.006 - time * 5.0).sin();
    let gust = air.gust * reach * gust_wave;
    Vec3::new(gust * 0.55, -gust * 0.1, -breeze - gust)
}

fn fine_size(columns: usize, rows: usize) -> (usize, usize) {
    ((columns - 1) * SMOOTHING + 1, (rows - 1) * SMOOTHING + 1)
}

// The uniform cubic B-spline through `points[0]`, `points[stride]`, ... at
// `at`, measured in points. Points past either end are mirrored through
// the end point, so the curve starts and ends on the end points.
fn spline_at(points: &[Vec3], stride: usize, at: f32) -> Vec3 {
    let count = points.len().div_ceil(stride);
    let last = count as isize - 1;
    let point = |index: isize| -> Vec3 {
        if index < 0 {
            points[0] * 2.0 - points[(-index) as usize * stride]
        } else if index > last {
            points[last as usize * stride] * 2.0 - points[(2 * last - index) as usize * stride]
        } else {
            points[index as usize * stride]
        }
    };
    let base = (at.floor() as isize).min(last);
    let t = at - base as f32;
    let (t2, t3) = (t * t, t * t * t);
    let weights = [
        (1.0 - t).powi(3) / 6.0,
        (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0,
        (-3.0 * t3 + 3.0 * t2 + 3.0 * t + 1.0) / 6.0,
        t3 / 6.0,
    ];
    let mut sum = Vec3::ZERO;
    for (offset, weight) in (-1..=2).zip(weights) {
        sum = sum + point(base + offset) * weight;
    }
    sum
}

// Each point's normal from its neighbours across and down the grid, facing
// the viewer, who looks along +z.
fn grid_normals(positions: &[Vec3], columns: usize, rows: usize) -> Vec<[f32; 3]> {
    let at = |column: usize, row: usize| positions[row * columns + column];
    let mut normals = Vec::with_capacity(positions.len());
    for row in 0..rows {
        for column in 0..columns {
            let across = at((column + 1).min(columns - 1), row) - at(column.saturating_sub(1), row);
            let down = at(column, (row + 1).min(rows - 1)) - at(column, row.saturating_sub(1));
            let normal = down
                .cross(across)
                .normalized()
                .unwrap_or(Vec3::new(0.0, 0.0, -1.0));
            normals.push([normal.x, normal.y, normal.z]);
        }
    }
    normals
}

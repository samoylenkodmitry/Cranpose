// The water over the pool's floor: the swell and the ripples, and the floor
// as it shows through them. The water bends light, so every point of it
// shows a floor point a little off; how far grows with the depth and fades
// up the wall under the coping. The floor's mesh is a flat grid whose
// vertices stand where they show and whose texture coordinates are the
// floor points seen there, so the pointer lands on the word it sees with no
// map of its own. The mesh's normals carry the ripples' slope to the shader.

use cranpose_ui::{space::Mesh, Point, Size};

use super::{
    grid_mesh,
    ripples::{Disturbance, Ripples},
    swell::{Swell, SwellGrid},
    whole,
};

// The grid's cell, in pixels, unless the stage is so large that this
// would take more points than `MOST_POINTS`.
const CELL: f32 = 12.0;
const MOST_POINTS: f32 = 12_000.0;
// Pixels of shift per unit of slope at 1.5 metres, for a 1:1 look.
const REFRACTION: f32 = 50.0;
// The clock starts where the swell already looks settled.
const FIRST_CLOCK: f32 = 11.3;

// Where the pool's parts lie down the stage, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PoolShape {
    pub size: Size,
    // The size of the look: 1 on a 1600 by 1000 stage.
    pub look: f32,
    // The coping's lower edge, which is the waterline.
    pub waterline: f32,
    // Where the wall under the coping meets the floor.
    pub wall_foot: f32,
}

impl PoolShape {
    // Metres of water over `point`, as light and refraction take them.
    fn depth(&self, (x, y): (f32, f32)) -> f32 {
        let across = 0.38 * x / self.size.width
            + 0.62 * (y - self.wall_foot) / (self.size.height - self.wall_foot).max(1.0);
        0.8 + 1.35 * across.clamp(-0.05, 1.1)
    }

    // Pixels of shift per unit of slope at `point`: more in deep water,
    // none at the waterline.
    fn bend(&self, point: (f32, f32)) -> f32 {
        let on_floor = ((point.1 - self.waterline) / (self.wall_foot - self.waterline).max(1.0))
            .clamp(0.0, 1.0);
        REFRACTION * self.look * (self.depth(point) / 1.5).sqrt() * on_floor
    }
}

#[derive(Debug)]
pub(super) struct Water {
    shape: PoolShape,
    columns: usize,
    rows: usize,
    cell: f32,
    clock: f32,
    swell: Swell,
    grid: SwellGrid,
    ripples: Ripples,
    indices: Vec<u32>,
}

impl Water {
    pub(super) fn new(shape: PoolShape) -> Self {
        let size = shape.size;
        let cell = CELL.max((size.width * size.height / MOST_POINTS).sqrt());
        let columns = whole::count((size.width / cell).ceil()) + 1;
        let rows = whole::count((size.height / cell).ceil()) + 1;
        let swell = Swell::new();
        let mut grid = SwellGrid::default();
        swell.lay_grid(&mut grid, (columns, rows), cell, shape.look);
        let mut indices = Vec::with_capacity((columns - 1) * (rows - 1) * 6);
        let stride = grid_mesh::vertex(columns);
        for row in 0..rows - 1 {
            for column in 0..columns - 1 {
                let top_left = grid_mesh::vertex(row * columns + column);
                let bottom_left = top_left + stride;
                // Diagonals alternate, so the bend has no grain.
                if (row + column) % 2 == 0 {
                    indices.extend([
                        top_left,
                        bottom_left,
                        top_left + 1,
                        top_left + 1,
                        bottom_left,
                        bottom_left + 1,
                    ]);
                } else {
                    indices.extend([
                        top_left,
                        bottom_left + 1,
                        top_left + 1,
                        top_left,
                        bottom_left,
                        bottom_left + 1,
                    ]);
                }
            }
        }
        let mut water = Self {
            shape,
            columns,
            rows,
            cell,
            clock: FIRST_CLOCK,
            swell,
            grid,
            ripples: Ripples::new((columns, rows), cell, shape.look, shape.waterline),
            indices,
        };
        water
            .swell
            .tick_grid(&mut water.grid, cell, (water.clock, shape.look));
        water
    }

    pub(super) fn shape(&self) -> PoolShape {
        self.shape
    }

    pub(super) fn clock(&self) -> f32 {
        self.clock
    }

    pub(super) fn advance(&mut self, seconds: f32) {
        self.clock += seconds;
        self.ripples.advance(seconds);
        self.swell
            .tick_grid(&mut self.grid, self.cell, (self.clock, self.shape.look));
    }

    // Touches the water at `at`, a point of the stage, `delay` seconds from
    // now; the coping is dry.
    pub(super) fn disturb(&mut self, at: Point, kind: Disturbance, delay: f32) {
        if at.y > self.shape.waterline {
            self.ripples.disturb((at.x, at.y), kind, delay);
        }
    }

    // The floor as it shows through the water now, over the stage's size.
    pub(super) fn floor_mesh(&self) -> Mesh {
        let size = self.shape.size;
        let points = self.columns * self.rows;
        let (mut positions, mut uvs, mut normals) = (
            Vec::with_capacity(points),
            Vec::with_capacity(points),
            Vec::with_capacity(points),
        );
        let still = self.ripples.still();
        for row in 0..self.rows {
            let y = (row as f32 * self.cell).min(size.height);
            for column in 0..self.columns {
                let x = (column as f32 * self.cell).min(size.width);
                let swell = self.swell.grid_slope(&self.grid, (column, row));
                let ripple = if still {
                    (0.0, 0.0)
                } else {
                    self.ripples.slope(column, row)
                };
                // Steep ripples bend less than their slope, or words would
                // tear.
                let soften = 1.0 / (1.0 + 3.0 * (ripple.0 * ripple.0 + ripple.1 * ripple.1).sqrt());
                let bend = self.shape.bend((x, y));
                let seen = (
                    x + bend * (swell.0 + ripple.0 * soften),
                    y + bend * (swell.1 + ripple.1 * soften),
                );
                positions.push([x / size.width, y / size.height, 0.0]);
                uvs.push([
                    (seen.0 / size.width).clamp(0.0, 1.0),
                    (seen.1 / size.height).clamp(0.0, 1.0),
                ]);
                let length = (ripple.0 * ripple.0 + ripple.1 * ripple.1 + 1.0).sqrt();
                normals.push([ripple.0 / length, ripple.1 / length, -1.0 / length]);
            }
        }
        Mesh::new(positions, uvs, Some(normals), self.indices.clone())
            .expect("the floor's grid is whole")
    }
}

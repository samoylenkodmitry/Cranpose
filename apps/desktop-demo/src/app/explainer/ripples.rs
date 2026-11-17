// Ripples on the pool: a damped wave equation, height and velocity, on a
// coarse grid of the water, stepped at a fixed rate. A nine-point Laplacian
// keeps the rings round. The coping at the top is a mirror: the rows above
// the waterline stay still and the water never reads them. The other three
// sides soak the waves up. Each frame takes a few steps at most and none
// while the water is still, so the cost of a frame has a bound.

use std::f32::consts::TAU;

use super::whole;

// Steps a second; a frame takes as many as fit.
const STEPS_PER_SECOND: f32 = 60.0;
// A late frame does not stall the next one catching up.
const MOST_STEPS_A_FRAME: u32 = 3;
// (c dt / dx)^2: waves cross about 300 pixels a second on 12 pixel cells.
const WAVE_SPEED: f32 = 0.17;
// The velocity kept per step.
const DAMPING: f32 = 0.995;
// Velocity diffusion: it kills ringing at the grid's own scale.
const VISCOSITY: f32 = 0.006;
const MOST_SOURCES: usize = 40;
// How long the water stays awake after something touched it.
const AWAKE_SECONDS: f32 = 6.0;
// How many cells along the sides and the bottom soak waves up.
const BEACH_CELLS: f32 = 5.0;

// What touches the water, at a point of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Disturbance {
    // A typed letter or a press on words: one damped push and pull, as
    // strong as `strength` says.
    Drop { strength: f32 },
    // A deleted letter: a small ring that closes in.
    Dimple,
    // A press on bare floor: a stronger, slower drop.
    Splash,
    // The pointer moving through the water: a single push.
    Wake { strength: f32 },
}

#[derive(Clone, Copy, Debug)]
struct Source {
    at: (f32, f32),
    kind: Disturbance,
    // Seconds since it started.
    age: f32,
}

// A push on the water for one step: a bump at `at`, or a ring of `ring`
// pixels round it, `width` pixels wide.
#[derive(Clone, Copy, Debug)]
struct Force {
    at: (f32, f32),
    strength: f32,
    width: f32,
    ring: f32,
}

impl Force {
    fn at(&self, x: f32, y: f32) -> f32 {
        let distance = ((x - self.at.0).powi(2) + (y - self.at.1).powi(2)).sqrt();
        let e = (distance - self.ring) / self.width;
        if e.abs() >= 3.5 {
            return 0.0;
        }
        let profile = if self.ring > 0.0 {
            1.0 - 2.0 * e * e
        } else {
            1.0 - e * e
        };
        self.strength * profile * (-e * e).exp()
    }
}

impl Source {
    // The force of the step that starts now, once it has started, `look` scaling its size;
    // `None` once the source is spent.
    fn force(&self, look: f32) -> Option<Force> {
        let t = self.age;
        let bump = |strength: f32, width: f32| Force {
            at: self.at,
            strength,
            width: width * look,
            ring: 0.0,
        };
        let swing = |period: f32, length: f32| (TAU * t / period).sin() * (1.0 - t / length);
        match self.kind {
            Disturbance::Drop { strength } => {
                (t < 0.3).then(|| bump(-9.0 * strength * swing(0.2, 0.3), 24.0))
            }
            Disturbance::Splash => (t < 0.5).then(|| bump(-14.0 * swing(0.26, 0.5), 30.0)),
            Disturbance::Dimple => (t < 0.15).then(|| Force {
                at: self.at,
                strength: -1.2 * (TAU * t / 0.15).sin(),
                width: 18.0 * look,
                ring: 54.0 * look,
            }),
            Disturbance::Wake { strength } => {
                (t < 1.0 / STEPS_PER_SECOND).then(|| bump(strength, 24.0))
            }
        }
    }
}

// Ripples on a grid of `columns` by `rows` square cells `cell` pixels
// across, with the first row at the top of the water.
#[derive(Debug)]
pub(super) struct Ripples {
    columns: usize,
    rows: usize,
    cell: f32,
    look: f32,
    // The first row in the water: the ones above it are coping.
    wet_row: usize,
    height: Vec<f32>,
    velocity: Vec<f32>,
    next_height: Vec<f32>,
    next_velocity: Vec<f32>,
    // How much each cell keeps of its waves per step: less on the beaches.
    keep: Vec<f32>,
    sources: Vec<Source>,
    forces: Vec<Force>,
    owed: f32,
    awake: f32,
}

impl Ripples {
    pub(super) fn new(
        (columns, rows): (usize, usize),
        cell: f32,
        look: f32,
        waterline: f32,
    ) -> Self {
        let cells = columns * rows;
        let beach = |from_edge: f32| {
            let share = (1.0 - from_edge / BEACH_CELLS).clamp(0.0, 1.0);
            1.0 - 0.3 * share * share
        };
        let keep = (0..cells)
            .map(|index| {
                let (column, row) = ((index % columns) as f32, (index / columns) as f32);
                let edge = column
                    .min(columns as f32 - 1.0 - column)
                    .min(rows as f32 - 1.0 - row);
                beach(edge)
            })
            .collect();
        Self {
            columns,
            rows,
            cell,
            look,
            wet_row: whole::count((waterline / cell).ceil()).min(rows.saturating_sub(1)),
            height: vec![0.0; cells],
            velocity: vec![0.0; cells],
            next_height: vec![0.0; cells],
            next_velocity: vec![0.0; cells],
            keep,
            sources: Vec::with_capacity(MOST_SOURCES),
            forces: Vec::with_capacity(MOST_SOURCES),
            owed: 0.0,
            awake: 0.0,
        }
    }

    // Touches the water at `at` `delay` seconds from now.
    pub(super) fn disturb(&mut self, at: (f32, f32), kind: Disturbance, delay: f32) {
        if self.sources.len() >= MOST_SOURCES {
            return;
        }
        self.sources.push(Source {
            at,
            kind,
            age: -delay,
        });
        self.awake = self.awake.max(AWAKE_SECONDS + delay);
    }

    // Whether no wave moves: the grid is all zero then.
    pub(super) fn still(&self) -> bool {
        self.awake <= 0.0
    }

    pub(super) fn advance(&mut self, seconds: f32) {
        if self.still() {
            return;
        }
        let step = 1.0 / STEPS_PER_SECOND;
        self.owed = (self.owed + seconds).min(step * MOST_STEPS_A_FRAME as f32);
        while self.owed >= step {
            self.owed -= step;
            self.step(step);
        }
        self.awake -= seconds;
        if self.still() {
            self.height.fill(0.0);
            self.velocity.fill(0.0);
            self.sources.clear();
        }
    }

    fn step(&mut self, seconds: f32) {
        let look = self.look;
        self.forces.clear();
        self.forces.extend(
            self.sources
                .iter()
                .filter(|source| source.age >= 0.0)
                .filter_map(|source| source.force(look)),
        );
        for source in &mut self.sources {
            source.age += seconds;
        }
        self.sources
            .retain(|source| source.age < 0.0 || source.force(look).is_some());

        let (columns, rows, wet_row) = (self.columns, self.rows, self.wet_row);
        let laplacian = |field: &[f32], column: usize, row: usize| {
            let left = column.saturating_sub(1);
            let right = (column + 1).min(columns - 1);
            // The water never reads the coping: its top row mirrors itself.
            let up = row.saturating_sub(1).max(wet_row) * columns;
            let down = (row + 1).min(rows - 1) * columns;
            let here = row * columns;
            let edges = field[here + left]
                + field[here + right]
                + field[up + column]
                + field[down + column];
            let corners =
                field[up + left] + field[up + right] + field[down + left] + field[down + right];
            (4.0 * edges + corners - 20.0 * field[here + column]) / 6.0
        };
        for row in wet_row..rows {
            for column in 0..columns {
                let index = row * columns + column;
                let mut velocity = (self.velocity[index]
                    + WAVE_SPEED * laplacian(&self.height, column, row)
                    + VISCOSITY * laplacian(&self.velocity, column, row))
                    * DAMPING;
                velocity *= self.keep[index];
                self.next_velocity[index] = velocity;
                self.next_height[index] = (self.height[index] + velocity) * self.keep[index];
            }
        }
        for force in &self.forces {
            let reach = force.ring + 3.5 * force.width;
            let cells = |from: f32, to: f32, count: usize| {
                let first = whole::count((from / self.cell).floor());
                let last = whole::count((to / self.cell).ceil()).min(count - 1);
                first..=last
            };
            for row in cells(force.at.1 - reach, force.at.1 + reach, rows) {
                if row < wet_row {
                    continue;
                }
                for column in cells(force.at.0 - reach, force.at.0 + reach, columns) {
                    let index = row * columns + column;
                    let push = force.at(column as f32 * self.cell, row as f32 * self.cell);
                    self.next_velocity[index] += push;
                    self.next_height[index] += push;
                }
            }
        }
        std::mem::swap(&mut self.height, &mut self.next_height);
        std::mem::swap(&mut self.velocity, &mut self.next_velocity);
    }

    // The slope of the water at grid point (`column`, `row`), in pixels.
    pub(super) fn slope(&self, column: usize, row: usize) -> (f32, f32) {
        let (columns, rows) = (self.columns, self.rows);
        let at = |column: usize, row: usize| self.height[row.max(self.wet_row) * columns + column];
        let (left, right) = (column.saturating_sub(1), (column + 1).min(columns - 1));
        let (up, down) = (row.saturating_sub(1), (row + 1).min(rows - 1));
        (
            (at(right, row) - at(left, row)) / ((right - left).max(1) as f32 * self.cell),
            (at(column, down) - at(column, up)) / ((down - up).max(1) as f32 * self.cell),
        )
    }
}

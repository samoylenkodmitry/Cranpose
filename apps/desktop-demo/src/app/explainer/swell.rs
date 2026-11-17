// The swell of the pool: plane waves from all round. Short ones spread over
// the golden angle and grow steeper as they get longer; three long, slow
// ones carry most of the slope, and only the short ones feel the gusts that
// make some patches of water livelier than others. The same table bends the
// floor on the CPU (see `water.rs`) and lights it in `pool.wgsl`, so the
// shader's copy is written from it.

use std::{f32::consts::TAU, fmt::Write};

const WAVES: usize = 20;
const SHORT_WAVES: usize = 17;
// How fast the gusts drift, as a share of the water's clock.
const GUST_RATE: f32 = 0.11;

#[derive(Clone, Copy, Debug)]
struct Wave {
    // Unit direction of travel on the floor.
    direction: (f32, f32),
    // Radians per pixel of a 1:1 look.
    number: f32,
    // Radians a second.
    speed: f32,
    phase: f32,
    // The slope at the crest.
    slope: f32,
    // 1 for a wave the gusts swell, 0 for one they leave be.
    gusty: f32,
}

// The waves' phases along one row and one column of a fixed grid: a wave's
// cosine at a grid point is then two products, the row's and the column's.
#[derive(Clone, Debug, Default)]
pub(super) struct SwellGrid {
    columns: usize,
    rows: usize,
    // Per wave and column: the cosine and sine of the phase across.
    across: Vec<(f32, f32)>,
    // Per wave and row: the same down, with the time in it.
    down: Vec<(f32, f32)>,
    // Per column and per row: the gusts' two phases across and down.
    gust_across: Vec<[(f32, f32); 2]>,
    gust_down: Vec<[(f32, f32); 2]>,
}

// The pool's swell.
#[derive(Clone, Debug)]
pub(super) struct Swell {
    waves: [Wave; WAVES],
}

// A number from 0 to 1 that stays with `i`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the fraction narrows to the f32 the tables keep"
)]
fn seeded(i: usize) -> f32 {
    let x = ((i as f64) * 127.1 + 311.7).sin() * 43_758.545_3;
    (x - x.floor()) as f32
}

impl Swell {
    pub(super) fn new() -> Self {
        const LONG_HEADINGS: [f32; 3] = [2.2, 0.5, 4.0];
        let waves = std::array::from_fn(|i| {
            let long = i >= SHORT_WAVES;
            let share = i as f32 / (SHORT_WAVES - 1) as f32;
            let length = if long {
                430.0 + 130.0 * (i - SHORT_WAVES) as f32
            } else {
                40.0 * 6.2f32.powf(share) * (0.92 + 0.16 * seeded(i))
            };
            let heading = if long {
                LONG_HEADINGS[i - SHORT_WAVES]
            } else {
                i as f32 * 2.399_96 + 0.5 * seeded(i + 40)
            };
            let number = TAU / length;
            // Deep water: the speed of a wave grows with its length.
            let depth_rate = if long { 300.0 } else { 520.0 };
            Wave {
                direction: (heading.cos(), heading.sin()),
                number,
                speed: (depth_rate * number).sqrt() * (0.9 + 0.2 * seeded(i + 9)),
                phase: seeded(i + 80) * TAU,
                slope: if long { 0.052 } else { 0.0052 + 0.0125 * share },
                gusty: if long { 0.0 } else { 1.0 },
            }
        });
        Self { waves }
    }

    // Lays out `grid` for `columns` by `rows` points `spacing` pixels apart
    // from the origin, for a look `look` times as large as 1:1.
    pub(super) fn lay_grid(
        &self,
        grid: &mut SwellGrid,
        (columns, rows): (usize, usize),
        spacing: f32,
        look: f32,
    ) {
        grid.columns = columns;
        grid.rows = rows;
        grid.across.clear();
        for wave in &self.waves {
            grid.across.extend((0..columns).map(|column| {
                let x = column as f32 * spacing / look;
                cos_sin(wave.direction.0 * x * wave.number)
            }));
        }
        grid.gust_across.clear();
        grid.gust_across.extend((0..columns).map(|column| {
            let x = column as f32 * spacing / look;
            [cos_sin(x * 0.0061), cos_sin(-x * 0.0027)]
        }));
        grid.down.resize(WAVES * rows, (0.0, 0.0));
        grid.gust_down.resize(rows, [(0.0, 0.0); 2]);
    }

    // Moves `grid`'s rows to `clock` seconds, for a look `look` times as
    // large as 1:1.
    pub(super) fn tick_grid(&self, grid: &mut SwellGrid, spacing: f32, (clock, look): (f32, f32)) {
        let rows = grid.rows;
        for (wave, down) in self.waves.iter().zip(grid.down.chunks_exact_mut(rows)) {
            for (row, cell) in down.iter_mut().enumerate() {
                let y = row as f32 * spacing / look;
                let phase = wave.direction.1 * y * wave.number + wave.phase - wave.speed * clock;
                *cell = cos_sin(phase);
            }
        }
        let at = clock * GUST_RATE;
        for (row, cell) in grid.gust_down.iter_mut().enumerate() {
            let y = row as f32 * spacing / look;
            *cell = [
                cos_sin(y * 0.0032 + at),
                cos_sin(y * 0.0083 - at * 0.7 + 1.3),
            ];
        }
    }

    // The slope at point (`column`, `row`) of `grid`, in pixels.
    pub(super) fn grid_slope(&self, grid: &SwellGrid, (column, row): (usize, usize)) -> (f32, f32) {
        // sin(a + b) and cos(a + b) from the halves across and down.
        let sin_sum =
            |(cos_a, sin_a): (f32, f32), (cos_b, sin_b): (f32, f32)| sin_a * cos_b + cos_a * sin_b;
        let [first_across, second_across] = grid.gust_across[column];
        let [first_down, second_down] = grid.gust_down[row];
        let gusts =
            1.0 + 0.42 * sin_sum(first_across, first_down) * sin_sum(second_across, second_down);
        let (columns, rows) = (grid.columns, grid.rows);
        self.waves
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(x, y), (index, wave)| {
                let (cos_a, sin_a) = grid.across[index * columns + column];
                let (cos_b, sin_b) = grid.down[index * rows + row];
                let crest = wave.slope * (1.0 + (gusts - 1.0) * wave.gusty);
                let height = crest * (cos_a * cos_b - sin_a * sin_b);
                (x + wave.direction.0 * height, y + wave.direction.1 * height)
            })
    }

    // The table as WGSL: `SWELL_WAVES`, and per wave `SWELL_TRAVEL` (the
    // direction times the wave number, the speed and the phase) and
    // `SWELL_CREST` (the direction times the slope, and the gusts' share).
    pub(super) fn wgsl(&self) -> String {
        let mut travel = String::new();
        let mut crest = String::new();
        for wave in &self.waves {
            let (dx, dy) = wave.direction;
            writeln!(
                travel,
                "    vec4<f32>({:.7}, {:.7}, {:.7}, {:.7}),",
                dx * wave.number,
                dy * wave.number,
                wave.speed,
                wave.phase
            )
            .expect("a String takes every write");
            writeln!(
                crest,
                "    vec4<f32>({:.7}, {:.7}, {:.1}, 0.0),",
                dx * wave.slope,
                dy * wave.slope,
                wave.gusty
            )
            .expect("a String takes every write");
        }
        format!(
            "const SWELL_WAVES: u32 = {WAVES}u;\n\
             const SWELL_GUST_RATE: f32 = {GUST_RATE};\n\
             const SWELL_TRAVEL: array<vec4<f32>, {WAVES}> = array<vec4<f32>, {WAVES}>(\n{travel});\n\
             const SWELL_CREST: array<vec4<f32>, {WAVES}> = array<vec4<f32>, {WAVES}>(\n{crest});\n"
        )
    }
}

// The cosine and the sine of `angle`, in the order the tables keep them.
fn cos_sin(angle: f32) -> (f32, f32) {
    let (sin, cos) = angle.sin_cos();
    (cos, sin)
}

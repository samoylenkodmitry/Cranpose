use crate::{
    mesh::{MeshData, mix, normalize, srgb},
    noise::fbm,
};

pub const RIVER_HALF_WIDTH: f32 = 6.5;

#[derive(Clone, Copy, Debug)]
pub struct Terrain {
    seed: u32,
    pub half_size: f32,
    pub resolution: u32,
    pub water_level: f32,
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Terrain {
    pub fn new(seed: u32) -> Self {
        Self {
            seed,
            half_size: 130.0,
            resolution: 220,
            water_level: 0.0,
        }
    }

    pub fn river_center(&self, x: f32) -> f32 {
        let phase = self.seed as f32 * 0.37;
        16.0 * (x * 0.022 + phase).sin() + 7.0 * (x * 0.057 + 1.3 + phase).sin()
    }

    pub fn river_distance(&self, x: f32, z: f32) -> f32 {
        (z - self.river_center(x)).abs()
    }

    pub fn height(&self, x: f32, z: f32) -> f32 {
        let distance = self.river_distance(x, z);
        let away = smoothstep(4.0, 40.0, distance);
        let hills = fbm(x * 0.011, z * 0.011, 5, self.seed) * 9.0 * away
            + fbm(x * 0.05, z * 0.05, 3, self.seed + 3) * 0.9;
        let banks = (distance / 55.0).min(1.6).powf(1.4) * 7.0;
        let edge = smoothstep(70.0, 125.0, x.abs().max(z.abs()));
        let mountains = edge * (26.0 + fbm(x * 0.02, z * 0.02, 4, self.seed + 11) * 16.0);
        let land = 1.1 + hills.max(-2.0) + banks + mountains;
        let channel = smoothstep(RIVER_HALF_WIDTH * 2.2, RIVER_HALF_WIDTH * 0.5, distance)
            * (1.0 - smoothstep(85.0, 112.0, x.abs()));
        let bed = -1.8 + distance / RIVER_HALF_WIDTH * 0.7;
        land + (bed - land) * channel
    }

    pub fn normal(&self, x: f32, z: f32) -> [f32; 3] {
        let step = 0.5;
        let dx = self.height(x + step, z) - self.height(x - step, z);
        let dz = self.height(x, z + step) - self.height(x, z - step);
        normalize([-dx, 2.0 * step, -dz])
    }

    pub fn slope(&self, x: f32, z: f32) -> f32 {
        1.0 - self.normal(x, z)[1]
    }

    fn color(&self, x: f32, z: f32, height: f32, slope: f32) -> [f32; 4] {
        let variation = fbm(x * 0.06, z * 0.06, 3, self.seed + 5) * 0.5 + 0.5;
        let meadow = fbm(x * 0.025, z * 0.025, 3, self.seed + 33);
        let grass = mix(srgb(0.27, 0.47, 0.14), srgb(0.42, 0.6, 0.2), variation);
        let grass = mix(
            grass,
            srgb(0.6, 0.63, 0.27),
            smoothstep(0.15, 0.5, meadow) * 0.6,
        );
        let sand = mix(
            srgb(0.36, 0.33, 0.24),
            srgb(0.78, 0.72, 0.52),
            smoothstep(-0.6, 0.4, height),
        );
        let ground = mix(sand, grass, smoothstep(0.5, 1.2, height));
        let rock = mix(srgb(0.4, 0.38, 0.36), srgb(0.55, 0.53, 0.5), variation);
        let ground = mix(ground, rock, smoothstep(0.22, 0.4, slope));
        mix(
            ground,
            srgb(0.93, 0.94, 0.97),
            smoothstep(26.0, 34.0, height + variation * 3.0),
        )
    }

    pub fn mesh(&self) -> MeshData {
        grid(self.half_size, self.resolution, |x, z| {
            let height = self.height(x, z);
            let normal = self.normal(x, z);
            (
                [x, height, z],
                normal,
                self.color(x, z, height, 1.0 - normal[1]),
            )
        })
    }

    pub fn water_mesh(&self, cells: u32) -> MeshData {
        grid(self.half_size, cells, |x, z| {
            ([x, self.water_level, z], [0.0, 1.0, 0.0], [1.0; 4])
        })
    }
}

fn grid(
    half_size: f32,
    cells: u32,
    vertex: impl Fn(f32, f32) -> ([f32; 3], [f32; 3], [f32; 4]),
) -> MeshData {
    let step = half_size * 2.0 / cells as f32;
    let mut mesh = MeshData::default();
    for row in 0..=cells {
        for column in 0..=cells {
            let (position, normal, color) = vertex(
                -half_size + column as f32 * step,
                -half_size + row as f32 * step,
            );
            mesh.vertex(position, normal, color);
        }
    }
    let width = cells + 1;
    for row in 0..cells {
        for column in 0..cells {
            let a = row * width + column;
            let b = a + 1;
            let c = a + width;
            let d = c + 1;
            mesh.indices.extend([a, c, b, b, c, d]);
        }
    }
    mesh
}

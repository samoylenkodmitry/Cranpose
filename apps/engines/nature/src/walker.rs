use crate::terrain::Terrain;

const EYE_HEIGHT: f32 = 1.7;
const WALK_SPEED: f32 = 4.5;
const RUN_SPEED: f32 = 10.0;
const WADING_DEPTH: f32 = 0.9;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stride {
    pub forward: f32,
    pub right: f32,
    pub run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walker {
    pub position: [f32; 3],
    pub stamina: f32,
}

impl Walker {
    pub fn start(terrain: &Terrain) -> Self {
        let x = -terrain.half_size * 0.7;
        let z = terrain.river_center(x) + 14.0;
        Self {
            position: [x, terrain.height(x, z), z],
            stamina: 1.0,
        }
    }

    pub fn step(&mut self, terrain: &Terrain, stride: Stride, yaw: f32, seconds: f32) {
        let (sin, cos) = yaw.sin_cos();
        let mut direction = [
            cos * stride.forward - sin * stride.right,
            sin * stride.forward + cos * stride.right,
        ];
        let length = (direction[0] * direction[0] + direction[1] * direction[1]).sqrt();
        let running = stride.run && self.stamina > 0.0 && length > 0.0;
        self.stamina = if running {
            (self.stamina - seconds * 0.12).max(0.0)
        } else {
            (self.stamina + seconds * 0.08).min(1.0)
        };
        if length > 0.0 {
            direction = [direction[0] / length, direction[1] / length];
            let wading = if self.position[1] < terrain.water_level {
                0.5
            } else {
                1.0
            };
            let speed = if running { RUN_SPEED } else { WALK_SPEED } * wading;
            let limit = terrain.half_size - 8.0;
            self.position[0] =
                (self.position[0] + direction[0] * speed * seconds).clamp(-limit, limit);
            self.position[2] =
                (self.position[2] + direction[1] * speed * seconds).clamp(-limit, limit);
        }
        let ground = terrain
            .height(self.position[0], self.position[2])
            .max(terrain.water_level - WADING_DEPTH);
        self.position[1] += (ground - self.position[1]) * (seconds * 14.0).min(1.0);
    }

    pub fn eye(&self) -> [f32; 3] {
        [
            self.position[0],
            self.position[1] + EYE_HEIGHT,
            self.position[2],
        ]
    }

    pub fn look_at(&self, yaw: f32, pitch: f32) -> [f32; 3] {
        let eye = self.eye();
        [
            eye[0] + yaw.cos() * pitch.cos(),
            eye[1] + pitch.sin(),
            eye[2] + yaw.sin() * pitch.cos(),
        ]
    }
}

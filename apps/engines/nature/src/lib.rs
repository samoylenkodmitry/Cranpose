mod mesh;
mod noise;
mod plants;
mod terrain;
mod valley;
mod walker;

pub use mesh::{MeshData, srgb};
pub use terrain::{RIVER_HALF_WIDTH, Terrain};
pub use valley::{Placement, PlantGroup, Valley};
pub use walker::{Stride, Walker};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    pub toward: [f32; 3],
    pub color: [f32; 3],
    pub strength: f32,
}

pub fn sun(time_of_day: f32) -> Sun {
    let angle = time_of_day.clamp(0.0, 1.0) * std::f32::consts::PI;
    let height = angle.sin() * 0.95 + 0.04;
    let toward = mesh::normalize([angle.cos(), height, 0.35]);
    let warmth = (toward[1] / 0.45).clamp(0.0, 1.0);
    let low = srgb(1.0, 0.58, 0.32);
    let high = srgb(1.0, 0.97, 0.92);
    let color = mesh::mix(low, high, warmth);
    Sun {
        toward,
        color: [color[0], color[1], color[2]],
        strength: (toward[1] * 3.0).clamp(0.05, 1.0),
    }
}

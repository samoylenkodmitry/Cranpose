#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

pub fn srgb(r: f32, g: f32, b: f32) -> [f32; 4] {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    [linear(r), linear(g), linear(b), 1.0]
}

pub(crate) fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

pub(crate) fn scale(color: [f32; 4], by: f32) -> [f32; 4] {
    [color[0] * by, color[1] * by, color[2] * by, color[3]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / length, v[1] / length, v[2] / length]
}

impl MeshData {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub(crate) fn vertex(&mut self, position: [f32; 3], normal: [f32; 3], color: [f32; 4]) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push(position);
        self.normals.push(normal);
        self.colors.push(color);
        index
    }

    pub(crate) fn flat_triangle(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], color: [f32; 4]) {
        let normal = normalize(cross(sub(b, a), sub(c, a)));
        let first = self.vertex(a, normal, color);
        self.vertex(b, normal, color);
        self.vertex(c, normal, color);
        self.indices.extend([first, first + 1, first + 2]);
    }

    pub(crate) fn double_sided_triangle(
        &mut self,
        a: [f32; 3],
        b: [f32; 3],
        c: [f32; 3],
        colors: [[f32; 4]; 3],
    ) {
        let normal = normalize(cross(sub(b, a), sub(c, a)));
        let back = [-normal[0], -normal[1], -normal[2]];
        let first = self.vertex(a, normal, colors[0]);
        self.vertex(b, normal, colors[1]);
        self.vertex(c, normal, colors[2]);
        self.vertex(a, back, colors[0]);
        self.vertex(c, back, colors[2]);
        self.vertex(b, back, colors[1]);
        self.indices
            .extend([first, first + 1, first + 2, first + 3, first + 4, first + 5]);
    }
}

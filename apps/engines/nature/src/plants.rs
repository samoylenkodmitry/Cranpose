use std::f32::consts::TAU;

use crate::{
    mesh::{MeshData, mix, normalize, scale, srgb},
    noise::Rng,
};

const BARK: [f32; 3] = [0.36, 0.25, 0.16];

fn color(rgb: [f32; 3]) -> [f32; 4] {
    srgb(rgb[0], rgb[1], rgb[2])
}

fn ring(radius: f32, height: f32, sides: u32, turn: f32) -> Vec<[f32; 3]> {
    (0..sides)
        .map(|side| {
            let angle = side as f32 / sides as f32 * TAU + turn;
            [angle.cos() * radius, height, angle.sin() * radius]
        })
        .collect()
}

fn prism(
    mesh: &mut MeshData,
    bottom: f32,
    top: f32,
    radius: f32,
    taper: f32,
    sides: u32,
    tint: [f32; 4],
) {
    let low = ring(radius, bottom, sides, 0.0);
    let high = ring(radius * taper, top, sides, 0.0);
    for side in 0..sides as usize {
        let next = (side + 1) % sides as usize;
        mesh.flat_triangle(low[side], high[side], low[next], tint);
        mesh.flat_triangle(low[next], high[side], high[next], tint);
    }
}

fn cone(
    mesh: &mut MeshData,
    base: f32,
    height: f32,
    radius: f32,
    sides: u32,
    turn: f32,
    tint: [f32; 4],
) {
    let rim = ring(radius, base, sides, turn);
    let tip = [0.0, base + height, 0.0];
    let center = [0.0, base, 0.0];
    for side in 0..sides as usize {
        let next = (side + 1) % sides as usize;
        mesh.flat_triangle(rim[side], tip, rim[next], tint);
        mesh.flat_triangle(rim[next], center, rim[side], scale(tint, 0.6));
    }
}

fn icosphere(
    center: [f32; 3],
    radius: f32,
    squash: f32,
    rng: &mut Rng,
    mut tint: impl FnMut(&mut Rng) -> [f32; 4],
    mesh: &mut MeshData,
) {
    let t = (1.0 + 5.0_f32.sqrt()) / 2.0;
    let mut points: Vec<[f32; 3]> = [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ]
    .into_iter()
    .map(normalize)
    .collect();
    let faces: [[usize; 3]; 20] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let midpoint = |a: usize, b: usize, points: &mut Vec<[f32; 3]>| {
        let (p, q) = (points[a], points[b]);
        points.push(normalize([
            (p[0] + q[0]) / 2.0,
            (p[1] + q[1]) / 2.0,
            (p[2] + q[2]) / 2.0,
        ]));
        points.len() - 1
    };
    let mut fine = Vec::with_capacity(80);
    for [a, b, c] in faces {
        let ab = midpoint(a, b, &mut points);
        let bc = midpoint(b, c, &mut points);
        let ca = midpoint(c, a, &mut points);
        fine.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
    }
    let jitter: Vec<f32> = points.iter().map(|_| rng.range(0.82, 1.12)).collect();
    let place = |index: usize| {
        let p = points[index];
        let r = radius * jitter[index];
        [
            center[0] + p[0] * r,
            center[1] + p[1] * r * squash,
            center[2] + p[2] * r,
        ]
    };
    for [a, b, c] in fine {
        let shade = tint(rng);
        mesh.flat_triangle(place(a), place(b), place(c), shade);
    }
}

pub fn pine(seed: u64) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut mesh = MeshData::default();
    prism(&mut mesh, 0.0, 1.4, 0.2, 0.7, 6, color(BARK));
    let greens = [[0.1, 0.3, 0.15], [0.12, 0.35, 0.17], [0.15, 0.4, 0.19]];
    for (tier, green) in greens.into_iter().enumerate() {
        let tier = tier as f32;
        cone(
            &mut mesh,
            1.0 + tier * 1.35,
            2.4 - tier * 0.25,
            1.7 - tier * 0.42,
            9,
            rng.range(0.0, TAU),
            scale(color(green), rng.range(0.9, 1.1)),
        );
    }
    mesh
}

pub fn broadleaf(seed: u64) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut mesh = MeshData::default();
    prism(&mut mesh, 0.0, 2.6, 0.24, 0.65, 6, color(BARK));
    let light = color([0.38, 0.6, 0.2]);
    let dark = color([0.2, 0.42, 0.13]);
    for (center, radius) in [
        ([0.0, 3.6, 0.0], 1.8),
        ([0.9, 3.0, 0.4], 1.3),
        ([-0.7, 3.2, -0.6], 1.35),
    ] {
        icosphere(
            center,
            radius,
            0.85,
            &mut rng,
            |rng| mix(dark, light, rng.next()),
            &mut mesh,
        );
    }
    mesh
}

pub fn bush(seed: u64) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut mesh = MeshData::default();
    let light = color([0.32, 0.55, 0.2]);
    let dark = color([0.16, 0.36, 0.12]);
    icosphere(
        [0.0, 0.45, 0.0],
        0.8,
        0.7,
        &mut rng,
        |rng| mix(dark, light, rng.next()),
        &mut mesh,
    );
    mesh
}

pub fn rock(seed: u64) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut mesh = MeshData::default();
    let light = color([0.58, 0.56, 0.52]);
    let dark = color([0.36, 0.35, 0.33]);
    icosphere(
        [0.0, 0.15, 0.0],
        0.7,
        0.6,
        &mut rng,
        |rng| mix(dark, light, rng.next()),
        &mut mesh,
    );
    mesh
}

pub fn flowers(seed: u64, petal: [f32; 3]) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut mesh = MeshData::default();
    let stem = color([0.24, 0.48, 0.16]);
    let heart = color([0.98, 0.82, 0.25]);
    for _ in 0..7 {
        let (x, z) = (rng.range(-0.55, 0.55), rng.range(-0.55, 0.55));
        let height = rng.range(0.28, 0.5);
        let lean = [rng.range(-0.05, 0.05), 0.0, rng.range(-0.05, 0.05)];
        let top = [x + lean[0], height, z + lean[2]];
        mesh.double_sided_triangle([x - 0.015, 0.0, z], [x + 0.015, 0.0, z], top, [stem; 3]);
        let size = rng.range(0.07, 0.11);
        let turn = rng.range(0.0, TAU);
        let tint = scale(color(petal), rng.range(0.85, 1.1));
        for leaf in 0..5 {
            let a = turn + leaf as f32 / 5.0 * TAU;
            let b = a + TAU / 10.0;
            let c = a - TAU / 10.0;
            let tip = [
                top[0] + a.cos() * size,
                top[1] + 0.02,
                top[2] + a.sin() * size,
            ];
            let left = [
                top[0] + b.cos() * size * 0.45,
                top[1] + 0.01,
                top[2] + b.sin() * size * 0.45,
            ];
            let right = [
                top[0] + c.cos() * size * 0.45,
                top[1] + 0.01,
                top[2] + c.sin() * size * 0.45,
            ];
            mesh.double_sided_triangle(top, left, tip, [tint; 3]);
            mesh.double_sided_triangle(top, tip, right, [tint; 3]);
        }
        let dot = size * 0.3;
        mesh.double_sided_triangle(
            [top[0] - dot, top[1] + 0.025, top[2]],
            [top[0] + dot, top[1] + 0.025, top[2] - dot],
            [top[0], top[1] + 0.025, top[2] + dot],
            [heart; 3],
        );
    }
    mesh
}

pub fn blades(
    seed: u64,
    count: u32,
    height: (f32, f32),
    base: [f32; 3],
    tip: [f32; 3],
) -> MeshData {
    let mut rng = Rng::new(seed);
    let mut mesh = MeshData::default();
    let (base, tip) = (color(base), color(tip));
    for _ in 0..count {
        let (x, z) = (rng.range(-0.18, 0.18), rng.range(-0.18, 0.18));
        let angle = rng.range(0.0, TAU);
        let width = rng.range(0.03, 0.05);
        let tall = rng.range(height.0, height.1);
        let lean = rng.range(0.05, 0.3) * tall;
        let (dx, dz) = (angle.cos() * width, angle.sin() * width);
        let top = [x + angle.sin() * lean, tall, z - angle.cos() * lean];
        mesh.double_sided_triangle(
            [x - dx, 0.0, z - dz],
            [x + dx, 0.0, z + dz],
            top,
            [base, base, mix(tip, base, rng.range(0.0, 0.3))],
        );
    }
    mesh
}

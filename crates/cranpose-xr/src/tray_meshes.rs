//! The meshes of a tray and of the controls on it, made from their sizes.
//!
//! A slab is a box with rounded corners seen from the front and a rounded front edge, so the light
//! catches its rim. A slab whose corner radius is half its width and height is a puck. The corners
//! keep their radius at every size, so each size has a mesh of its own. A tray makes a mesh when a
//! control first lays out or changes its size, never in a frame.

use std::f32::consts::FRAC_PI_2;

use cranpose::space::Mesh;

// The steps of a quarter of a rounded corner, and of the rounded front edge
const CORNER_STEPS: usize = 8;
const EDGE_STEPS: usize = 4;

// A slab of `size` dp (width, height, depth) centered at the origin, in the axes of a volume's
// content: x to the right, y down and z away from the viewer, so its front face lies at
// `-depth / 2`. Its corners seen from the front have the radius `corner`, and its front edge is
// rounded with the radius `edge`. The texture lies flat across the front.
pub(crate) fn slab(size: [f32; 3], corner: f32, edge: f32) -> Mesh {
    let [width, height, depth] = size.map(|side| side.max(0.01));
    let corner = corner.clamp(0.0, width.min(height) / 2.0);
    let edge = edge.clamp(0.0, corner.min(depth / 2.0));
    let (front, back) = (-depth / 2.0, depth / 2.0);
    let outline = outline(width, height, corner);
    let points = outline.len();
    let rings = EDGE_STEPS + 3;
    let mut positions = Vec::with_capacity(points * rings + 2);
    let mut normals = Vec::with_capacity(positions.capacity());
    let uv = |x: f32, y: f32| [x / width + 0.5, y / height + 0.5];
    // The rounded front edge, from the front face to the side
    for step in 0..=EDGE_STEPS {
        let angle = step as f32 / EDGE_STEPS as f32 * FRAC_PI_2;
        let (sin, cos) = angle.sin_cos();
        let reach = edge * sin - edge;
        for &([x, y], [dx, dy]) in &outline {
            positions.push([x + dx * reach, y + dy * reach, front + edge * (1.0 - cos)]);
            normals.push([dx * sin, dy * sin, -cos]);
        }
    }
    // The side back to the back face, and the back face's own ring
    for normal_z in [0.0, 1.0] {
        for &([x, y], [dx, dy]) in &outline {
            positions.push([x, y, back]);
            normals.push(if normal_z > 0.0 {
                [0.0, 0.0, 1.0]
            } else {
                [dx, dy, 0.0]
            });
        }
    }
    let front_center = positions.len();
    positions.push([0.0, 0.0, front]);
    normals.push([0.0, 0.0, -1.0]);
    let back_center = positions.len();
    positions.push([0.0, 0.0, back]);
    normals.push([0.0, 0.0, 1.0]);
    let uvs = positions.iter().map(|&[x, y, _]| uv(x, y)).collect();
    let mut indices = Vec::with_capacity(points * 6 * (rings + 1));
    let index = |at: usize| u32::try_from(at).expect("a slab has few points");
    let at = |ring: usize, point: usize| index(ring * points + point % points);
    // The front edge's rings and the side, then the fans of the front and the back face
    for ring in 0..=EDGE_STEPS {
        for point in 0..points {
            let (a, b) = (at(ring, point), at(ring, point + 1));
            let (c, d) = (at(ring + 1, point + 1), at(ring + 1, point));
            indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    let back_ring = EDGE_STEPS + 2;
    for point in 0..points {
        indices.extend_from_slice(&[index(front_center), at(0, point + 1), at(0, point)]);
        indices.extend_from_slice(&[
            index(back_center),
            at(back_ring, point),
            at(back_ring, point + 1),
        ]);
    }
    Mesh::new(positions, uvs, Some(normals), indices).expect("a slab has whole triangles")
}

// The points of the slab's outline seen from the front, inset by the front edge's radius at the
// front, with the outward direction at each: four quarter circles about the corners' centers, which
// straight sides join.
fn outline(width: f32, height: f32, corner: f32) -> Vec<([f32; 2], [f32; 2])> {
    let (half_x, half_y) = (width / 2.0 - corner, height / 2.0 - corner);
    // The corners from the top right, clockwise as the viewer sees them: y points down
    let centers = [
        [half_x, -half_y],
        [half_x, half_y],
        [-half_x, half_y],
        [-half_x, -half_y],
    ];
    let mut points = Vec::with_capacity(4 * (CORNER_STEPS + 1));
    for (index, [cx, cy]) in centers.into_iter().enumerate() {
        let start = -FRAC_PI_2 + index as f32 * FRAC_PI_2;
        for step in 0..=CORNER_STEPS {
            let angle = start + step as f32 / CORNER_STEPS as f32 * FRAC_PI_2;
            let (dy, dx) = angle.sin_cos();
            points.push(([cx + dx * corner, cy + dy * corner], [dx, dy]));
        }
    }
    points
}

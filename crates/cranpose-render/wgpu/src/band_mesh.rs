use bytemuck::{Pod, Zeroable};

use crate::render::{SHAPE_KIND_ARC, SHAPE_KIND_STROKE, ShapeData};

/// Texels of slack around a band so every pixel the shape shader
/// anti-aliases lies inside the mesh.
pub(crate) const BAND_MESH_MARGIN: f32 = 1.0;
/// How far a chord of the outer polygon may fall inside the true circle,
/// which sets the segment count; the outer vertices ride out to keep the
/// whole circle covered.
const BAND_MESH_OVERSHOOT: f32 = 2.0;
const BAND_MESH_MIN_SEGMENTS: usize = 4;
const BAND_MESH_MAX_SEGMENTS: usize = 64;
/// A band whose bounding quad is smaller than this draws as the quad: the
/// pixels a mesh would save cost less than its vertices.
pub(crate) const BAND_MESH_MIN_QUAD_PIXELS: f32 = 512.0;

/// One vertex of a shape mesh: where it sits, its place in the shape's
/// rect, and the shape record it draws.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct MeshVertex {
    pub(crate) position: [f32; 2],
    pub(crate) uv: [f32; 2],
    pub(crate) shape_index: u32,
}

impl MeshVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32];

    pub(crate) fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// An annular band in device pixels: a stroked circle or an arc band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Band {
    pub(crate) center: [f32; 2],
    pub(crate) inner: f32,
    pub(crate) outer: f32,
    pub(crate) start: f32,
    pub(crate) sweep: f32,
}

fn axis_aligned_quad(shape: &ShapeData) -> bool {
    let [left, top, right, top2] = shape.quad01;
    let [left2, bottom, right2, bottom2] = shape.quad23;
    top2 == top
        && left2 == left
        && right2 == right
        && bottom2 == bottom
        && left < right
        && top < bottom
}

fn plain_solid(shape: &ShapeData) -> bool {
    shape.brush_type == 0
        && !(shape.clip_rect[2] > 0.0 && shape.clip_rect[3] > 0.0)
        && shape.rect[2] > 0.0
        && shape.rect[3] > 0.0
        && axis_aligned_quad(shape)
}

fn arc_band(shape: &ShapeData) -> Option<Band> {
    let center = [shape.arc_params[0], shape.arc_params[1]];
    let start = shape.arc_params[2];
    let sweep = shape.arc_params[3];
    let outer = shape.stroke_params[2];
    let inner = shape.stroke_params[3];
    let finite = center.iter().all(|value| value.is_finite())
        && start.is_finite()
        && sweep.is_finite()
        && outer.is_finite()
        && inner.is_finite();
    (finite && outer > 0.0 && sweep > 0.0).then_some(Band {
        center,
        inner: inner.max(0.0),
        outer,
        start,
        sweep,
    })
}

fn stroked_circle_band(shape: &ShapeData) -> Option<Band> {
    let [x, y, width, height] = shape.rect;
    if width.to_bits() != height.to_bits() {
        return None;
    }
    let [r0, r1, r2, r3] = shape.radii;
    if r0.to_bits() != r1.to_bits() || r0.to_bits() != r2.to_bits() || r0.to_bits() != r3.to_bits()
    {
        return None;
    }
    let stroke_width = shape.stroke_params[0];
    if !r0.is_finite() || r0 <= 0.0 || !stroke_width.is_finite() || stroke_width <= 0.0 {
        return None;
    }
    let geometry_half = (width - stroke_width) * 0.5;
    if (r0 - geometry_half).abs() > 0.01 {
        return None;
    }
    let outer = geometry_half + stroke_width * 0.5;
    (outer > 0.0).then_some(Band {
        center: [x + width * 0.5, y + height * 0.5],
        inner: (geometry_half - stroke_width * 0.5).max(0.0),
        outer,
        start: 0.0,
        sweep: cranpose_ui_graphics::TAU,
    })
}

/// The band a shape record draws, when it is a plain solid stroked circle
/// or arc band whose quad is worth replacing.
pub(crate) fn band_of(shape: &ShapeData) -> Option<Band> {
    if !plain_solid(shape) || shape.rect[2] * shape.rect[3] < BAND_MESH_MIN_QUAD_PIXELS {
        return None;
    }
    match shape.stroke_params[1].max(0.0) as u32 & 3 {
        SHAPE_KIND_ARC => arc_band(shape),
        SHAPE_KIND_STROKE => stroked_circle_band(shape),
        _ => None,
    }
}

fn clip_polygon_axis(
    input: &[[f32; 2]],
    axis: usize,
    bound: f32,
    keep_at_most: bool,
    output: &mut Vec<[f32; 2]>,
) {
    output.clear();
    let inside = |point: [f32; 2]| {
        if keep_at_most {
            point[axis] <= bound
        } else {
            point[axis] >= bound
        }
    };
    let intersect = |a: [f32; 2], b: [f32; 2]| {
        let (p, q) = if (b[0], b[1]) < (a[0], a[1]) {
            (b, a)
        } else {
            (a, b)
        };
        let t = (bound - p[axis]) / (q[axis] - p[axis]);
        let mut point = [0.0f32; 2];
        point[axis] = bound;
        point[1 - axis] = p[1 - axis] + t * (q[1 - axis] - p[1 - axis]);
        point
    };
    for (index, &current) in input.iter().enumerate() {
        let previous = input[(index + input.len() - 1) % input.len()];
        match (inside(previous), inside(current)) {
            (true, true) => output.push(current),
            (true, false) => output.push(intersect(previous, current)),
            (false, true) => {
                output.push(intersect(previous, current));
                output.push(current);
            }
            (false, false) => {}
        }
    }
}

/// The angular range a band's mesh covers: the whole circle, or the sweep
/// padded so the round caps and the anti-aliased ends stay inside.
fn angular_range(band: &Band, ring_half: f32) -> (f32, f32) {
    let tau = cranpose_ui_graphics::TAU;
    if band.sweep >= tau {
        return (0.0, tau);
    }
    let mid = (band.outer + band.inner) * 0.5;
    let pad = if ring_half < mid {
        (ring_half / mid).asin() + 0.05
    } else {
        std::f32::consts::PI
    };
    let padded = band.sweep + pad + pad;
    if padded >= tau {
        (0.0, tau)
    } else {
        (band.start - pad, padded)
    }
}

enum SegmentGeometry {
    Shared,
    Fan(std::ops::Range<usize>),
    Empty,
}

/// The scratch a band tessellation needs, kept between bands so a frame of
/// thousands of bands allocates nothing per band.
#[derive(Default)]
pub(crate) struct BandTessellator {
    boundaries: Vec<([f32; 2], [f32; 2])>,
    polygon: Vec<[f32; 2]>,
    scratch: Vec<[f32; 2]>,
    fan_points: Vec<[f32; 2]>,
    geometry: Vec<SegmentGeometry>,
    boundary_used: Vec<bool>,
    boundary_vertices: Vec<[u32; 2]>,
}

/// Appends the triangles of `band` clipped to the shape's quad; returns
/// whether anything was appended.
pub(crate) fn emit_band(
    tessellator: &mut BandTessellator,
    shape: &ShapeData,
    shape_index: u32,
    band: &Band,
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
) -> bool {
    let [cx, cy] = band.center;
    let mid = (band.outer + band.inner) * 0.5;
    let ring_half = ((band.outer - band.inner) * 0.5).max(0.0) + BAND_MESH_MARGIN;
    let outer = mid + ring_half;
    let inner = (mid - ring_half).max(0.0);
    let tau = cranpose_ui_graphics::TAU;
    let (range_start, range) = angular_range(band, ring_half);
    let closed = range >= tau;
    let step_limit = (2.0 * (outer / (outer + BAND_MESH_OVERSHOOT)).acos())
        .clamp(tau / BAND_MESH_MAX_SEGMENTS as f32, tau / 6.0);
    let segments = ((range / step_limit).ceil() as usize)
        .clamp(BAND_MESH_MIN_SEGMENTS, BAND_MESH_MAX_SEGMENTS);
    let step = range / segments as f32;
    let outer_vertex_radius = outer / (step * 0.5).cos();
    let boundary_count = if closed { segments } else { segments + 1 };

    let BandTessellator {
        boundaries,
        polygon,
        scratch,
        fan_points,
        geometry,
        boundary_used,
        boundary_vertices,
    } = tessellator;
    boundaries.clear();
    boundaries.extend((0..boundary_count).map(|index| {
        let (sin, cos) = (range_start + step * index as f32).sin_cos();
        (
            [cx + cos * inner, cy + sin * inner],
            [
                cx + cos * outer_vertex_radius,
                cy + sin * outer_vertex_radius,
            ],
        )
    }));

    let quad_min = [shape.quad01[0], shape.quad01[1]];
    let quad_max = [shape.quad23[2], shape.quad23[3]];
    geometry.clear();
    fan_points.clear();
    boundary_used.clear();
    boundary_used.resize(boundary_count, false);
    for index in 0..segments {
        let next = (index + 1) % boundary_count;
        let (inner_a, outer_a) = boundaries[index];
        let (inner_b, outer_b) = boundaries[next];
        polygon.clear();
        polygon.extend_from_slice(&[inner_a, outer_a, outer_b, inner_b]);
        clip_polygon_axis(polygon, 0, quad_min[0], false, scratch);
        clip_polygon_axis(scratch, 0, quad_max[0], true, polygon);
        clip_polygon_axis(polygon, 1, quad_min[1], false, scratch);
        clip_polygon_axis(scratch, 1, quad_max[1], true, polygon);
        scratch.clear();
        for &point in polygon.iter() {
            if scratch.last() != Some(&point) {
                scratch.push(point);
            }
        }
        while scratch.len() > 1 && scratch.first() == scratch.last() {
            scratch.pop();
        }
        geometry.push(if scratch.len() < 3 {
            SegmentGeometry::Empty
        } else if scratch[..] == [inner_a, outer_a, outer_b, inner_b] {
            boundary_used[index] = true;
            boundary_used[next] = true;
            SegmentGeometry::Shared
        } else {
            let start = fan_points.len();
            fan_points.extend_from_slice(scratch);
            SegmentGeometry::Fan(start..fan_points.len())
        });
    }

    let push_vertex = |vertices: &mut Vec<MeshVertex>, position: [f32; 2]| -> u32 {
        let index = vertices.len() as u32;
        vertices.push(MeshVertex {
            position,
            uv: [
                (position[0] - shape.rect[0]) / shape.rect[2],
                (position[1] - shape.rect[1]) / shape.rect[3],
            ],
            shape_index,
        });
        index
    };
    boundary_vertices.clear();
    boundary_vertices.resize(boundary_count, [0u32; 2]);
    for (index, used) in boundary_used.iter().enumerate() {
        if *used {
            let (inner, outer) = boundaries[index];
            boundary_vertices[index] = [push_vertex(vertices, inner), push_vertex(vertices, outer)];
        }
    }
    let start_len = indices.len();
    for (index, segment) in geometry.iter().enumerate() {
        match segment {
            SegmentGeometry::Empty => {}
            SegmentGeometry::Shared => {
                let next = (index + 1) % boundary_count;
                let [in_a, out_a] = boundary_vertices[index];
                let [in_b, out_b] = boundary_vertices[next];
                indices.extend_from_slice(&[in_a, out_a, out_b, in_a, out_b, in_b]);
            }
            SegmentGeometry::Fan(range) => {
                let points = &fan_points[range.clone()];
                let base = vertices.len() as u32;
                for &point in points {
                    push_vertex(vertices, point);
                }
                for offset in 1..points.len() as u32 - 1 {
                    indices.extend_from_slice(&[base, base + offset, base + offset + 1]);
                }
            }
        }
    }
    indices.len() > start_len
}

/// Appends the shape's quad as two triangles.
pub(crate) fn emit_quad(
    shape: &ShapeData,
    shape_index: u32,
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    let corners = [
        ([shape.quad01[0], shape.quad01[1]], [0.0, 0.0]),
        ([shape.quad01[2], shape.quad01[3]], [1.0, 0.0]),
        ([shape.quad23[0], shape.quad23[1]], [0.0, 1.0]),
        ([shape.quad23[2], shape.quad23[3]], [1.0, 1.0]),
    ];
    for (position, uv) in corners {
        vertices.push(MeshVertex {
            position,
            uv,
            shape_index,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
}

/// The pixels a shape's quad rasterizes.
pub(crate) fn quad_area(shape: &ShapeData) -> f64 {
    let corners = [
        [shape.quad01[0] as f64, shape.quad01[1] as f64],
        [shape.quad01[2] as f64, shape.quad01[3] as f64],
        [shape.quad23[0] as f64, shape.quad23[1] as f64],
        [shape.quad23[2] as f64, shape.quad23[3] as f64],
    ];
    triangle_area(corners[0], corners[1], corners[2])
        + triangle_area(corners[2], corners[1], corners[3])
}

fn triangle_area(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
}

/// The pixels the triangles of `indices` rasterize.
pub(crate) fn triangles_area(vertices: &[MeshVertex], indices: &[u32]) -> f64 {
    indices
        .chunks_exact(3)
        .map(|triangle| {
            let point = |index: u32| {
                let position = vertices[index as usize].position;
                [position[0] as f64, position[1] as f64]
            };
            triangle_area(point(triangle[0]), point(triangle[1]), point(triangle[2]))
        })
        .sum()
}

/// A batch's meshes: every shape as triangles, bands as their band and the
/// rest as their quad, in one vertex and one index list.
pub(crate) struct BatchMesh {
    pub(crate) vertices: Vec<MeshVertex>,
    pub(crate) indices: Vec<u32>,
    pub(crate) fill_pixels: f64,
}

/// Meshes a batch when at least one of its shapes is a band; `None` means
/// every shape is a quad and the quad path draws the batch as it is.
pub(crate) fn mesh_batch(
    tessellator: &mut BandTessellator,
    shapes: &[ShapeData],
) -> Option<BatchMesh> {
    let bands: Vec<Option<Band>> = shapes.iter().map(band_of).collect();
    if bands.iter().all(Option::is_none) {
        return None;
    }
    let mut mesh = BatchMesh {
        vertices: Vec::with_capacity(shapes.len() * 4),
        indices: Vec::with_capacity(shapes.len() * 6),
        fill_pixels: 0.0,
    };
    for (index, (shape, band)) in shapes.iter().zip(&bands).enumerate() {
        let start = mesh.indices.len();
        let meshed = band.as_ref().is_some_and(|band| {
            emit_band(
                tessellator,
                shape,
                index as u32,
                band,
                &mut mesh.vertices,
                &mut mesh.indices,
            )
        });
        if meshed {
            mesh.fill_pixels += triangles_area(&mesh.vertices, &mesh.indices[start..]);
        } else if band.is_none() {
            emit_quad(shape, index as u32, &mut mesh.vertices, &mut mesh.indices);
            mesh.fill_pixels += quad_area(shape);
        }
    }
    Some(mesh)
}

/// The quad pixels of every shape, for a batch drawn without a mesh.
pub(crate) fn quad_fill_pixels(shapes: &[ShapeData]) -> f64 {
    shapes.iter().map(quad_area).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(rect: [f32; 4], radii: f32, stroke: [f32; 4], arc: [f32; 4]) -> ShapeData {
        let [x, y, width, height] = rect;
        ShapeData {
            rect,
            radii: [radii; 4],
            gradient_params: [0.0; 4],
            clip_rect: [0.0; 4],
            stroke_params: stroke,
            arc_params: arc,
            quad01: [x, y, x + width, y],
            quad23: [x, y + height, x + width, y + height],
            color: [1.0; 4],
            brush_type: 0,
            gradient_start: 0,
            gradient_count: 0,
            gradient_tile_mode: 0,
        }
    }

    fn stroked_circle(center: [f32; 2], radius: f32, stroke_width: f32) -> ShapeData {
        let outer = radius + stroke_width * 0.5;
        record(
            [
                center[0] - outer,
                center[1] - outer,
                2.0 * outer,
                2.0 * outer,
            ],
            radius,
            [stroke_width, SHAPE_KIND_STROKE as f32, 0.0, 0.0],
            [0.0; 4],
        )
    }

    fn arc(center: [f32; 2], inner: f32, outer: f32, start: f32, sweep: f32) -> ShapeData {
        record(
            [
                center[0] - outer,
                center[1] - outer,
                2.0 * outer,
                2.0 * outer,
            ],
            0.0,
            [0.0, SHAPE_KIND_ARC as f32, outer, inner],
            [center[0], center[1], start, sweep],
        )
    }

    fn inside_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
        let sign = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
            (p[0] - r[0]) * (q[1] - r[1]) - (q[0] - r[0]) * (p[1] - r[1])
        };
        let d1 = sign(p, a, b);
        let d2 = sign(p, b, c);
        let d3 = sign(p, c, a);
        let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(negative && positive)
    }

    fn mesh_covers(vertices: &[MeshVertex], indices: &[u32], point: [f32; 2]) -> bool {
        indices.chunks_exact(3).any(|triangle| {
            inside_triangle(
                point,
                vertices[triangle[0] as usize].position,
                vertices[triangle[1] as usize].position,
                vertices[triangle[2] as usize].position,
            )
        })
    }

    /// Whether the shape shader gives `point` any coverage: within half a
    /// pixel of the band radially and, for an arc, within its sweep. The
    /// quad path only ever shades pixel centers inside the quad, so the
    /// caller checks those.
    fn shader_covers(band: &Band, point: [f32; 2]) -> bool {
        let dx = point[0] - band.center[0];
        let dy = point[1] - band.center[1];
        let radius = (dx * dx + dy * dy).sqrt();
        let mid = (band.outer + band.inner) * 0.5;
        let half = (band.outer - band.inner) * 0.5;
        if (radius - mid).abs() >= half + 0.5 {
            return false;
        }
        if band.sweep >= cranpose_ui_graphics::TAU {
            return true;
        }
        let tau = cranpose_ui_graphics::TAU;
        let angle = (dy.atan2(dx) - band.start).rem_euclid(tau);
        angle <= band.sweep
    }

    fn assert_mesh_covers_shader(shape: &ShapeData) {
        let band = band_of(shape).expect("a band");
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut tessellator = BandTessellator::default();
        assert!(emit_band(
            &mut tessellator,
            shape,
            0,
            &band,
            &mut vertices,
            &mut indices
        ));
        let [x, y, width, height] = shape.rect;
        let mut missed = Vec::new();
        for py in (y.floor() as i32)..((y + height).ceil() as i32) {
            for px in (x.floor() as i32)..((x + width).ceil() as i32) {
                let point = [px as f32 + 0.5, py as f32 + 0.5];
                let inside_quad =
                    point[0] >= x && point[0] < x + width && point[1] >= y && point[1] < y + height;
                if inside_quad
                    && shader_covers(&band, point)
                    && !mesh_covers(&vertices, &indices, point)
                {
                    missed.push(point);
                }
            }
        }
        assert!(
            missed.is_empty(),
            "the mesh of {band:?} misses {} covered pixels, first {:?}",
            missed.len(),
            &missed[..missed.len().min(5)]
        );
    }

    #[test]
    fn a_stroked_circle_mesh_covers_every_pixel_the_shader_shades() {
        for (center, radius, width) in [
            ([240.0, 240.0], 200.0, 4.0),
            ([100.3, 77.7], 61.0, 9.0),
            ([300.0, 120.0], 33.0, 1.0),
        ] {
            assert_mesh_covers_shader(&stroked_circle(center, radius, width));
        }
    }

    #[test]
    fn an_arc_band_mesh_covers_every_pixel_the_shader_shades() {
        for (inner, outer, start, sweep) in [
            (188.0, 200.0, 0.3, 1.2),
            (50.0, 70.0, 4.0, 3.0),
            (0.0, 90.0, 1.0, 0.4),
            (120.0, 130.0, 2.5, 5.9),
        ] {
            assert_mesh_covers_shader(&arc([240.0, 240.0], inner, outer, start, sweep));
        }
    }

    #[test]
    fn a_band_mesh_rasterizes_a_fraction_of_the_quad() {
        let shape = stroked_circle([240.0, 240.0], 200.0, 4.0);
        let mesh = mesh_batch(
            &mut BandTessellator::default(),
            std::slice::from_ref(&shape),
        )
        .expect("a mesh");
        assert!(
            mesh.fill_pixels < quad_area(&shape) * 0.25,
            "mesh {} against quad {}",
            mesh.fill_pixels,
            quad_area(&shape)
        );
    }

    #[test]
    fn small_bands_and_gradients_and_clipped_bands_keep_their_quad() {
        let small = stroked_circle([20.0, 20.0], 10.0, 2.0);
        assert!(band_of(&small).is_none());
        let mut gradient = stroked_circle([240.0, 240.0], 200.0, 4.0);
        gradient.brush_type = 1;
        assert!(band_of(&gradient).is_none());
        let mut clipped = stroked_circle([240.0, 240.0], 200.0, 4.0);
        clipped.clip_rect = [0.0, 0.0, 100.0, 100.0];
        assert!(band_of(&clipped).is_none());
        assert!(mesh_batch(&mut BandTessellator::default(), &[small, gradient, clipped]).is_none());
    }
}

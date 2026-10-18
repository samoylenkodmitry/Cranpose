use std::{
    f32::consts::PI,
    fmt,
    sync::{OnceLock, atomic::Ordering},
};

use portable_atomic::AtomicU64;

use super::math::Vec3;

static NEXT_MESH_ID: AtomicU64 = AtomicU64::new(1);

/// Why the parts given to [`Mesh::new`] do not make a mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// The texture coordinates or the normals do not have one entry for
    /// each position.
    AttributeCountMismatch,
    /// The number of indices is not a multiple of three.
    IndicesNotTriangles,
    /// An index names a vertex past the last one.
    IndexOutOfRange,
    /// The mesh has no triangle.
    Empty,
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AttributeCountMismatch => {
                "texture coordinates and normals need one entry for each position"
            }
            Self::IndicesNotTriangles => "the index count is not a multiple of three",
            Self::IndexOutOfRange => "an index names a vertex past the last one",
            Self::Empty => "the mesh has no triangle",
        })
    }
}

impl std::error::Error for MeshError {}

/// Where a ray meets a mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshHit {
    /// How far along the ray the hit is, in units of the ray's direction.
    pub distance: f32,
    /// The index of the triangle that the ray meets.
    pub triangle: u32,
    /// The texture coordinate at the hit.
    pub uv: [f32; 2],
}

/// Triangles in 3D that a composable can be drawn on.
///
/// Each vertex has a position, a normal and a texture coordinate. A
/// composable drawn on the mesh shows its content point `(x, y)` where the
/// texture coordinate is `(x / width, y / height)`: `(0, 0)` is the content's
/// top left corner and `(1, 1)` its bottom right.
///
/// ```
/// use cranpose_ui_graphics::space::{Mesh, Vec3};
///
/// let sphere = Mesh::sphere(32, 16);
/// // The middle of the texture faces the viewer.
/// let front = sphere
///     .point_at([0.5, 0.5])
///     .expect("the sphere covers its texture");
/// assert!((front - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-3);
/// ```
///
/// Two meshes are equal when they are the same mesh: a mesh does not change
/// once made.
pub struct Mesh {
    id: u64,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    texture_plane: OnceLock<Option<TexturePlane>>,
    bounds: OnceLock<(Vec3, Vec3)>,
}

impl PartialEq for Mesh {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl fmt::Debug for Mesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mesh")
            .field("id", &self.id)
            .field("vertices", &self.positions.len())
            .field("triangles", &self.triangle_count())
            .finish()
    }
}

/// The plane that carries texture coordinates onto a mesh: `(u, v)` lies at
/// `origin + u_axis * u + v_axis * v`. A flat mesh lies in it; a curved one
/// touches it where the texture's middle lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexturePlane {
    /// Where `(0, 0)` lies.
    pub origin: Vec3,
    /// The step from `(0, 0)` to `(1, 0)`.
    pub u_axis: Vec3,
    /// The step from `(0, 0)` to `(0, 1)`.
    pub v_axis: Vec3,
}

impl Mesh {
    /// A mesh from its vertices and the indices of its triangles' corners,
    /// three to a triangle. Without `normals`, each vertex takes the mean
    /// direction of the triangles around it.
    pub fn new(
        positions: Vec<[f32; 3]>,
        uvs: Vec<[f32; 2]>,
        normals: Option<Vec<[f32; 3]>>,
        indices: Vec<u32>,
    ) -> Result<Self, MeshError> {
        if uvs.len() != positions.len()
            || normals
                .as_ref()
                .is_some_and(|normals| normals.len() != positions.len())
        {
            return Err(MeshError::AttributeCountMismatch);
        }
        if !indices.len().is_multiple_of(3) {
            return Err(MeshError::IndicesNotTriangles);
        }
        if indices.is_empty() {
            return Err(MeshError::Empty);
        }
        if indices
            .iter()
            .any(|&index| index as usize >= positions.len())
        {
            return Err(MeshError::IndexOutOfRange);
        }
        let normals = normals.unwrap_or_else(|| smooth_normals(&positions, &indices));
        Ok(Self::assembled(positions, uvs, normals, indices))
    }

    /// The unit square from `(0, 0, 0)` to `(1, 1, 0)`, facing the viewer,
    /// with the texture coordinate equal to the position. Scaled by a
    /// composable's size it shows the composable as it is laid out.
    pub fn plane() -> Self {
        let mut parts = MeshParts::default();
        parts.quad(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        parts.into_mesh()
    }

    /// The unit cube from `(-0.5, -0.5, -0.5)` to `(0.5, 0.5, 0.5)`. Each
    /// face shows the whole texture, upright when seen from outside; the top
    /// and bottom faces have the texture's top toward the back.
    pub fn cuboid() -> Self {
        let mut parts = MeshParts::default();
        let (x, y, z) = (
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let h = 0.5;
        parts.quad(Vec3::new(-h, -h, -h), x, y);
        parts.quad(Vec3::new(h, -h, h), -x, y);
        parts.quad(Vec3::new(-h, -h, h), -z, y);
        parts.quad(Vec3::new(h, -h, -h), z, y);
        parts.quad(Vec3::new(-h, -h, h), x, -z);
        parts.quad(Vec3::new(-h, h, -h), x, z);
        parts.into_mesh()
    }

    /// The sphere of radius one about the origin, with `segments` slices
    /// around and `rings` from top to bottom. The texture wraps around it
    /// once: its left and right edges meet at the back, its top and bottom
    /// edges are the poles, and its middle faces the viewer.
    pub fn sphere(segments: u32, rings: u32) -> Self {
        let (segments, rings) = (segments.max(3), rings.max(2));
        let columns = segments + 1;
        let mut positions = Vec::with_capacity((columns * (rings + 1)) as usize);
        let mut uvs = Vec::with_capacity(positions.capacity());
        for ring in 0..=rings {
            let v = ring as f32 / rings as f32;
            let (sin_polar, cos_polar) = (v * PI).sin_cos();
            for segment in 0..=segments {
                let u = segment as f32 / segments as f32;
                let (sin_around, cos_around) = ((u - 0.5) * 2.0 * PI).sin_cos();
                positions.push([sin_polar * sin_around, -cos_polar, -sin_polar * cos_around]);
                uvs.push([u, v]);
            }
        }
        let mut indices = Vec::with_capacity((segments * rings * 6) as usize);
        for ring in 0..rings {
            for segment in 0..segments {
                let top_left = ring * columns + segment;
                let bottom_left = top_left + columns;
                indices.extend([
                    top_left,
                    top_left + 1,
                    bottom_left,
                    bottom_left,
                    top_left + 1,
                    bottom_left + 1,
                ]);
            }
        }
        let normals = positions.clone();
        Self::assembled(positions, uvs, normals, indices)
    }

    /// A number that no other mesh made in this process has. Renderers keep
    /// a mesh's buffers under it.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The vertex positions.
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// The vertex normals, one for each position, each of length one.
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    /// The vertex texture coordinates, one for each position.
    pub fn uvs(&self) -> &[[f32; 2]] {
        &self.uvs
    }

    /// The vertex indices of the triangles' corners, three to a triangle.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// The nearest place in front of `origin` where the ray along
    /// `direction` meets a triangle, from either side.
    pub fn cast(&self, origin: Vec3, direction: Vec3) -> Option<MeshHit> {
        let mut nearest: Option<MeshHit> = None;
        for triangle in 0..self.triangle_count() {
            let [a, b, c] = self.corners(triangle);
            let Some((distance, weights)) = ray_triangle(origin, direction, [a, b, c]) else {
                continue;
            };
            if nearest.is_some_and(|hit| hit.distance <= distance) {
                continue;
            }
            nearest = Some(MeshHit {
                distance,
                triangle,
                uv: self.uv_at(triangle, weights),
            });
        }
        nearest
    }

    /// Where the ray meets the plane of `triangle`, as a texture coordinate
    /// carried on from the triangle's corners: it lies outside the triangle
    /// when the ray misses it. `None` when the ray runs along the plane or
    /// meets it behind `origin`, or for a triangle that does not exist.
    pub fn cast_plane(&self, origin: Vec3, direction: Vec3, triangle: u32) -> Option<[f32; 2]> {
        if triangle >= self.triangle_count() {
            return None;
        }
        let [a, b, c] = self.corners(triangle);
        let normal = (b - a).cross(c - a);
        let along = normal.dot(direction);
        if along.abs() <= f32::EPSILON * normal.length() * direction.length() {
            return None;
        }
        let distance = normal.dot(a - origin) / along;
        if distance <= 0.0 {
            return None;
        }
        let point = origin + direction * distance;
        let weights = barycentric(point, [a, b, c], normal)?;
        Some(self.uv_at(triangle, weights))
    }

    /// The position where the texture coordinate `uv` lies on the mesh, from
    /// the first triangle that covers it; `None` when none does.
    pub fn point_at(&self, uv: [f32; 2]) -> Option<Vec3> {
        const EDGE: f32 = 1.0e-4;
        let point = Vec3::new(uv[0], uv[1], 0.0);
        (0..self.triangle_count()).find_map(|triangle| {
            let [ia, ib, ic] = self.triangle_indices(triangle);
            let corner = |index: usize| {
                let [u, v] = self.uvs[index];
                Vec3::new(u, v, 0.0)
            };
            let flat = [corner(ia), corner(ib), corner(ic)];
            let normal = (flat[1] - flat[0]).cross(flat[2] - flat[0]);
            let [wa, wb, wc] = barycentric(point, flat, normal)?;
            (wa >= -EDGE && wb >= -EDGE && wc >= -EDGE).then(|| {
                let position = |index: usize| Vec3::from_array(self.positions[index]);
                position(ia) * wa + position(ib) * wb + position(ic) * wc
            })
        })
    }

    /// The smallest box along the axes that holds every vertex: its lowest
    /// and highest corners.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        *self.bounds.get_or_init(|| {
            let mut low = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
            let mut high = -low;
            for &[x, y, z] in &self.positions {
                low = Vec3::new(low.x.min(x), low.y.min(y), low.z.min(z));
                high = Vec3::new(high.x.max(x), high.y.max(y), high.z.max(z));
            }
            (low, high)
        })
    }

    /// The plane that carries texture coordinates onto the mesh, worked out
    /// once: the plane of a flat mesh, or the one that touches a curved mesh
    /// at the texture's middle. `None` when the texture's middle does not lie
    /// on the mesh, or the mesh stretches it to a line there.
    pub fn texture_plane(&self) -> Option<TexturePlane> {
        *self.texture_plane.get_or_init(|| {
            self.flat_texture_plane()
                .or_else(|| self.tangent_texture_plane())
        })
    }

    fn flat_texture_plane(&self) -> Option<TexturePlane> {
        let origin = self.point_at([0.0, 0.0])?;
        let u_axis = self.point_at([1.0, 0.0])? - origin;
        let v_axis = self.point_at([0.0, 1.0])? - origin;
        let plane = TexturePlane {
            origin,
            u_axis,
            v_axis,
        };
        let tolerance = 1.0e-3 * (u_axis.length() + v_axis.length());
        let lies_flat = [[1.0, 1.0], [0.5, 0.5], [0.25, 0.75]].iter().all(|&uv| {
            self.point_at(uv)
                .is_some_and(|point| (point - plane.at(uv)).length() <= tolerance)
        });
        (lies_flat && u_axis.cross(v_axis).length() > tolerance * tolerance).then_some(plane)
    }

    fn tangent_texture_plane(&self) -> Option<TexturePlane> {
        const STEP: f32 = 1.0 / 64.0;
        let middle = self.point_at([0.5, 0.5])?;
        let u_axis = (self.point_at([0.5 + STEP, 0.5])? - middle) * (1.0 / STEP);
        let v_axis = (self.point_at([0.5, 0.5 + STEP])? - middle) * (1.0 / STEP);
        (u_axis.cross(v_axis).length() > f32::EPSILON).then(|| TexturePlane {
            origin: middle - u_axis * 0.5 - v_axis * 0.5,
            u_axis,
            v_axis,
        })
    }

    fn triangle_count(&self) -> u32 {
        (self.indices.len() / 3) as u32
    }

    fn triangle_indices(&self, triangle: u32) -> [usize; 3] {
        let first = triangle as usize * 3;
        [0, 1, 2].map(|corner| self.indices[first + corner] as usize)
    }

    fn corners(&self, triangle: u32) -> [Vec3; 3] {
        self.triangle_indices(triangle)
            .map(|index| Vec3::from_array(self.positions[index]))
    }

    fn uv_at(&self, triangle: u32, [wa, wb, wc]: [f32; 3]) -> [f32; 2] {
        let [a, b, c] = self.triangle_indices(triangle).map(|index| self.uvs[index]);
        [
            a[0] * wa + b[0] * wb + c[0] * wc,
            a[1] * wa + b[1] * wb + c[1] * wc,
        ]
    }

    pub(super) fn assembled(
        positions: Vec<[f32; 3]>,
        uvs: Vec<[f32; 2]>,
        normals: Vec<[f32; 3]>,
        indices: Vec<u32>,
    ) -> Self {
        Self {
            id: NEXT_MESH_ID.fetch_add(1, Ordering::Relaxed),
            positions,
            normals,
            uvs,
            indices,
            texture_plane: OnceLock::new(),
            bounds: OnceLock::new(),
        }
    }
}

impl TexturePlane {
    /// Where `uv` lies in the plane.
    pub fn at(&self, [u, v]: [f32; 2]) -> Vec3 {
        self.origin + self.u_axis * u + self.v_axis * v
    }
}

#[derive(Default)]
struct MeshParts {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl MeshParts {
    /// A square face with the texture's top left corner at `top_left`, its
    /// x along `right` and its y along `down`, facing the side from which
    /// `right` turns clockwise into `down`.
    fn quad(&mut self, top_left: Vec3, right: Vec3, down: Vec3) {
        let base = self.positions.len() as u32;
        let normal = down.cross(right).normalized().unwrap_or(Vec3::ZERO);
        for (corner, uv) in [
            (top_left, [0.0, 0.0]),
            (top_left + right, [1.0, 0.0]),
            (top_left + down, [0.0, 1.0]),
            (top_left + right + down, [1.0, 1.0]),
        ] {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv);
        }
        self.indices
            .extend([base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }

    fn into_mesh(self) -> Mesh {
        Mesh::assembled(self.positions, self.uvs, self.normals, self.indices)
    }
}

/// Normals that follow the triangles around each vertex, weighted by their
/// areas.
pub(super) fn smooth_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut sums = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let corner = |index: u32| Vec3::from_array(positions[index as usize]);
        let (a, b, c) = (
            corner(triangle[0]),
            corner(triangle[1]),
            corner(triangle[2]),
        );
        let face = (b - a).cross(c - a);
        for &index in triangle {
            sums[index as usize] = sums[index as usize] + face;
        }
    }
    sums.into_iter()
        .map(|sum| {
            sum.normalized()
                .unwrap_or(Vec3::new(0.0, 0.0, -1.0))
                .to_array()
        })
        .collect()
}

/// Where the ray meets the triangle, from either side: the distance along
/// `direction` and the corners' weights at the hit.
fn ray_triangle(origin: Vec3, direction: Vec3, [a, b, c]: [Vec3; 3]) -> Option<(f32, [f32; 3])> {
    let (edge_b, edge_c) = (b - a, c - a);
    let p = direction.cross(edge_c);
    let determinant = edge_b.dot(p);
    if determinant.abs() <= f32::EPSILON * edge_b.length() * edge_c.length() * direction.length() {
        return None;
    }
    let inverse = 1.0 / determinant;
    let offset = origin - a;
    let wb = offset.dot(p) * inverse;
    if !(0.0..=1.0).contains(&wb) {
        return None;
    }
    let q = offset.cross(edge_b);
    let wc = direction.dot(q) * inverse;
    if wc < 0.0 || wb + wc > 1.0 {
        return None;
    }
    let distance = edge_c.dot(q) * inverse;
    (distance > 0.0).then_some((distance, [1.0 - wb - wc, wb, wc]))
}

/// The weights of the triangle's corners at `point`, which lies in its
/// plane; `normal` is the plane's. `None` for a triangle with no area.
fn barycentric(point: Vec3, [a, b, c]: [Vec3; 3], normal: Vec3) -> Option<[f32; 3]> {
    let area = normal.dot(normal);
    if !area.is_normal() {
        return None;
    }
    let wb = normal.dot((point - a).cross(c - a)) / area;
    let wc = normal.dot((b - a).cross(point - a)) / area;
    Some([1.0 - wb - wc, wb, wc])
}

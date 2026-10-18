use std::ops::{Add, Mul, Neg, Sub};

/// A point or a direction in a 3D scene. Scene space uses logical pixels with
/// x to the right, y down and z away from the viewer, so a scene seen from
/// straight ahead lines up with the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    /// Distance to the right.
    pub x: f32,
    /// Distance down.
    pub y: f32,
    /// Distance away from the viewer.
    pub z: f32,
}

impl Vec3 {
    /// The origin.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// A vector from its three coordinates.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// The vector as an array of its coordinates.
    pub const fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// The vector from an array of its coordinates.
    pub const fn from_array([x, y, z]: [f32; 3]) -> Self {
        Self { x, y, z }
    }

    /// The dot product.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// The cross product. In scene space x cross y is z.
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// The vector's length.
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// The vector scaled to length one, or `None` for a vector of no length.
    pub fn normalized(self) -> Option<Self> {
        let length = self.length();
        (length.is_normal()).then(|| self * (1.0 / length))
    }

    /// The point `t` of the way from `self` to `other`.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }

    /// The lower of each coordinate of `self` and `other`.
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    /// The higher of each coordinate of `self` and `other`.
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }
}

impl Neg for Vec3 {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

/// A 4x4 matrix that moves, turns and scales points in 3D. It maps a point
/// `p` to `M * (p, 1)`, so the last column holds the translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    rows: [[f32; 4]; 4],
}

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mat4 {
    /// The matrix that leaves every point where it is.
    pub const IDENTITY: Self = Self::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    /// The matrix with these rows.
    pub const fn from_rows(rows: [[f32; 4]; 4]) -> Self {
        Self { rows }
    }

    /// The matrix's rows.
    pub const fn rows(&self) -> [[f32; 4]; 4] {
        self.rows
    }

    /// The matrix whose columns map the x, y and z axes onto `x`, `y` and
    /// `z` and the origin onto `origin`.
    pub const fn from_axes(x: Vec3, y: Vec3, z: Vec3, origin: Vec3) -> Self {
        Self::from_rows([
            [x.x, y.x, z.x, origin.x],
            [x.y, y.y, z.y, origin.y],
            [x.z, y.z, z.z, origin.z],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// Moves every point by `offset`.
    pub const fn translation(offset: Vec3) -> Self {
        Self::from_axes(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            offset,
        )
    }

    /// Scales each axis by its factor about the origin.
    pub const fn scaling(factors: Vec3) -> Self {
        Self::from_axes(
            Vec3::new(factors.x, 0.0, 0.0),
            Vec3::new(0.0, factors.y, 0.0),
            Vec3::new(0.0, 0.0, factors.z),
            Vec3::ZERO,
        )
    }

    /// Turns points about the x axis by `radians`. A positive angle tips the
    /// y axis, which points down, away from the viewer.
    pub fn rotation_x(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::from_axes(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, cos, sin),
            Vec3::new(0.0, -sin, cos),
            Vec3::ZERO,
        )
    }

    /// Turns points about the y axis by `radians`. A positive angle swings
    /// the x axis away from the viewer.
    pub fn rotation_y(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::from_axes(
            Vec3::new(cos, 0.0, sin),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-sin, 0.0, cos),
            Vec3::ZERO,
        )
    }

    /// Turns points about the z axis by `radians`. A positive angle turns
    /// clockwise on screen.
    pub fn rotation_z(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::from_axes(
            Vec3::new(cos, sin, 0.0),
            Vec3::new(-sin, cos, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::ZERO,
        )
    }

    /// This matrix followed by `next`: `next * self`.
    pub fn then(&self, next: &Self) -> Self {
        let (a, b) = (next.rows, self.rows);
        Self::from_rows(std::array::from_fn(|row| {
            std::array::from_fn(|column| (0..4).map(|k| a[row][k] * b[k][column]).sum())
        }))
    }

    /// Maps `point`, dividing by the resulting w.
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        let [x, y, z, w] = self.transform_homogeneous(point);
        if w == 1.0 || w == 0.0 {
            Vec3::new(x, y, z)
        } else {
            Vec3::new(x / w, y / w, z / w)
        }
    }

    /// Maps `point` and returns all four coordinates of the result, before
    /// any division by w.
    pub fn transform_homogeneous(&self, point: Vec3) -> [f32; 4] {
        self.rows
            .map(|row| row[0] * point.x + row[1] * point.y + row[2] * point.z + row[3])
    }

    /// Maps a direction: the translation does not apply.
    pub fn transform_vector(&self, vector: Vec3) -> Vec3 {
        let [x, y, z, _] = self
            .rows
            .map(|row| row[0] * vector.x + row[1] * vector.y + row[2] * vector.z);
        Vec3::new(x, y, z)
    }

    /// How long the map makes a unit length, averaged over the three axes:
    /// the cube root of how much it scales volume.
    pub fn length_scale(&self) -> f32 {
        let axis = |axis: Vec3| self.transform_vector(axis).length();
        (axis(Vec3::new(1.0, 0.0, 0.0))
            * axis(Vec3::new(0.0, 1.0, 0.0))
            * axis(Vec3::new(0.0, 0.0, 1.0)))
        .cbrt()
    }

    /// The transpose.
    pub fn transpose(&self) -> Self {
        Self::from_rows(std::array::from_fn(|row| {
            std::array::from_fn(|column| self.rows[column][row])
        }))
    }

    /// The inverse, or `None` when the matrix flattens space.
    pub fn inverse(&self) -> Option<Self> {
        let m = self.rows;
        let cofactor = |row: usize, column: usize| {
            let pick = |skip: usize| -> [usize; 3] {
                let mut picked = [0; 3];
                let mut next = 0;
                for index in 0..4 {
                    if index != skip {
                        picked[next] = index;
                        next += 1;
                    }
                }
                picked
            };
            let [r0, r1, r2] = pick(row);
            let [c0, c1, c2] = pick(column);
            let minor = m[r0][c0] * (m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1])
                - m[r0][c1] * (m[r1][c0] * m[r2][c2] - m[r1][c2] * m[r2][c0])
                + m[r0][c2] * (m[r1][c0] * m[r2][c1] - m[r1][c1] * m[r2][c0]);
            if (row + column).is_multiple_of(2) {
                minor
            } else {
                -minor
            }
        };
        let cofactors: [[f32; 4]; 4] =
            std::array::from_fn(|row| std::array::from_fn(|column| cofactor(row, column)));
        let determinant: f32 = (0..4)
            .map(|column| m[0][column] * cofactors[0][column])
            .sum();
        if !determinant.is_normal() {
            return None;
        }
        let scale = 1.0 / determinant;
        Some(Self::from_rows(std::array::from_fn(|row| {
            std::array::from_fn(|column| cofactors[column][row] * scale)
        })))
    }
}

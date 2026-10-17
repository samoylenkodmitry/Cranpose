//! Projective geometry shared by layout coordinates and renderers.

use crate::{Point, Rect, Size};

/// A measured node's local size and mapping into its window's logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowCoordinates {
    /// Size before graphics-layer transforms.
    pub size: Size,
    /// Maps node-local positions through placement and ancestor graphics layers.
    pub local_to_window: ProjectiveTransform,
}

impl WindowCoordinates {
    /// Axis-aligned window bounds of the transformed node.
    pub fn bounds(self) -> Rect {
        self.local_to_window
            .bounds_for_rect(Rect::from_size(self.size))
    }
}

/// The smallest homogeneous `w` at which a point still maps: a point with a
/// smaller `w` lies at or behind the eye of a perspective and lands on no
/// position of the destination plane. A layer's own plane maps at a `w` of
/// one.
pub const NEAR_W: f32 = 1.0 / 64.0;

/// A projective mapping of a two-dimensional coordinate plane.
#[derive(Clone, Copy, Debug)]
pub struct ProjectiveTransform {
    matrix: [[f32; 3]; 3],
}

/// The part of a rectangle in front of a perspective's eye, mapped: a convex
/// polygon of up to five corners, in order around it. Empty when the whole
/// rectangle lies behind the eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrontPolygon {
    corners: [[f32; 2]; 5],
    len: u8,
}

impl FrontPolygon {
    fn push(&mut self, corner: [f32; 2]) {
        self.corners[usize::from(self.len)] = corner;
        self.len += 1;
    }

    /// The corners in order around the polygon.
    pub fn corners(&self) -> &[[f32; 2]] {
        &self.corners[..usize::from(self.len)]
    }

    /// Whether no part of the rectangle lies in front of the eye.
    pub fn is_empty(&self) -> bool {
        self.len < 3
    }

    /// The corners in triangle-strip order: the strip covers the polygon,
    /// which is convex.
    pub fn strip(&self) -> [[f32; 2]; 5] {
        let c = self.corners;
        match self.len {
            5 => [c[0], c[1], c[4], c[2], c[3]],
            4 => [c[0], c[1], c[3], c[2], c[2]],
            _ => c,
        }
    }

    /// Axis-aligned bounds of the polygon; empty at the origin when the
    /// polygon is.
    pub fn bounds(&self) -> Rect {
        if self.is_empty() {
            return Rect::from_size(Size::ZERO);
        }
        let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for corner in self.corners() {
            min = [min[0].min(corner[0]), min[1].min(corner[1])];
            max = [max[0].max(corner[0]), max[1].max(corner[1])];
        }
        Rect {
            x: min[0],
            y: min[1],
            width: (max[0] - min[0]).max(0.0),
            height: (max[1] - min[1]).max(0.0),
        }
    }
}

impl PartialEq for ProjectiveTransform {
    fn eq(&self, other: &Self) -> bool {
        // Entry by entry, as a derived comparison: written out, it compiles
        // to nine comparisons where the derived one loops over each row.
        let [[a0, a1, a2], [a3, a4, a5], [a6, a7, a8]] = self.matrix;
        let [[b0, b1, b2], [b3, b4, b5], [b6, b7, b8]] = other.matrix;
        a0 == b0
            && a1 == b1
            && a2 == b2
            && a3 == b3
            && a4 == b4
            && a5 == b5
            && a6 == b6
            && a7 == b7
            && a8 == b8
    }
}

impl ProjectiveTransform {
    /// Leaves positions unchanged.
    pub const fn identity() -> Self {
        Self {
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Translates positions by the supplied logical-pixel offsets.
    pub fn translation(tx: f32, ty: f32) -> Self {
        Self {
            matrix: [[1.0, 0.0, tx], [0.0, 1.0, ty], [0.0, 0.0, 1.0]],
        }
    }

    /// Uniform scale about the origin (device-scale root transform: render
    /// graphs stay in logical dp; density applies at execution).
    pub fn uniform_scale(scale: f32) -> Self {
        Self {
            matrix: [[scale, 0.0, 0.0], [0.0, scale, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Maps a rectangle to corners ordered top-left, top-right, bottom-left,
    /// bottom-right, with the whole rectangle in front of the eye.
    pub fn from_rect_to_quad(rect: Rect, quad: [[f32; 2]; 4]) -> Self {
        if rect.width.abs() <= f32::EPSILON || rect.height.abs() <= f32::EPSILON {
            return Self::translation(quad[0][0], quad[0][1]);
        }

        if let Some(axis_aligned) = axis_aligned_rect_from_quad(quad) {
            let scale_x = axis_aligned.width / rect.width;
            let scale_y = axis_aligned.height / rect.height;
            return Self {
                matrix: [
                    [scale_x, 0.0, axis_aligned.x - rect.x * scale_x],
                    [0.0, scale_y, axis_aligned.y - rect.y * scale_y],
                    [0.0, 0.0, 1.0],
                ],
            };
        }

        let source = [
            [rect.x, rect.y],
            [rect.x + rect.width, rect.y],
            [rect.x, rect.y + rect.height],
            [rect.x + rect.width, rect.y + rect.height],
        ];
        let Some(coefficients) = solve_homography(source, quad) else {
            return Self::identity();
        };

        let solved = Self {
            matrix: [
                [coefficients[0], coefficients[1], coefficients[2]],
                [coefficients[3], coefficients[4], coefficients[5]],
                [coefficients[6], coefficients[7], 1.0],
            ],
        };
        if solved.w_at(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5) >= 0.0 {
            return solved;
        }
        Self {
            matrix: solved.matrix.map(|row| row.map(|value| -value)),
        }
    }

    /// The transform of a row-major homogeneous matrix, kept as it is: its
    /// bottom row gives each point's `w`, which [`NEAR_W`] cuts.
    pub const fn from_rows(matrix: [[f32; 3]; 3]) -> Self {
        Self { matrix }
    }

    /// The transform a homogeneous matrix describes, scaled so its last entry
    /// is one or minus one wherever that entry is not zero: its other entries
    /// then read as the scale, turn, translation and perspective they are.
    /// The scale is positive, so every point keeps the side of the eye its
    /// homogeneous `w` puts it on.
    pub fn from_homogeneous(matrix: [[f32; 3]; 3]) -> Self {
        let w = matrix[2][2].abs();
        if w == 1.0 || w <= f32::EPSILON {
            return Self { matrix };
        }
        Self {
            matrix: matrix.map(|row| row.map(|value| value / w)),
        }
    }

    /// Returns the composed transform that applies `self` first and `next` second.
    pub fn then(self, next: Self) -> Self {
        Self {
            matrix: multiply_matrices(next.matrix, self.matrix),
        }
    }

    /// Maps destination coordinates back to the source, unless the matrix is singular.
    pub fn inverse(self) -> Option<Self> {
        let m = self.matrix;
        let a = m[0][0];
        let b = m[0][1];
        let c = m[0][2];
        let d = m[1][0];
        let e = m[1][1];
        let f = m[1][2];
        let g = m[2][0];
        let h = m[2][1];
        let i = m[2][2];

        let cofactor00 = e * i - f * h;
        let cofactor01 = -(d * i - f * g);
        let cofactor02 = d * h - e * g;
        let cofactor10 = -(b * i - c * h);
        let cofactor11 = a * i - c * g;
        let cofactor12 = -(a * h - b * g);
        let cofactor20 = b * f - c * e;
        let cofactor21 = -(a * f - c * d);
        let cofactor22 = a * e - b * d;

        let determinant = a * cofactor00 + b * cofactor01 + c * cofactor02;
        if determinant.abs() <= f32::EPSILON {
            return None;
        }
        let inverse_determinant = 1.0 / determinant;

        Some(Self {
            matrix: [
                [
                    cofactor00 * inverse_determinant,
                    cofactor10 * inverse_determinant,
                    cofactor20 * inverse_determinant,
                ],
                [
                    cofactor01 * inverse_determinant,
                    cofactor11 * inverse_determinant,
                    cofactor21 * inverse_determinant,
                ],
                [
                    cofactor02 * inverse_determinant,
                    cofactor12 * inverse_determinant,
                    cofactor22 * inverse_determinant,
                ],
            ],
        })
    }

    /// Returns the row-major homogeneous matrix.
    pub fn matrix(self) -> [[f32; 3]; 3] {
        self.matrix
    }

    /// Maps a position into destination coordinates.
    pub fn map_point(&self, point: Point) -> Point {
        let x = point.x;
        let y = point.y;
        let w = self.matrix[2][0] * x + self.matrix[2][1] * y + self.matrix[2][2];
        let mapped_x = self.matrix[0][0] * x + self.matrix[0][1] * y + self.matrix[0][2];
        let mapped_y = self.matrix[1][0] * x + self.matrix[1][1] * y + self.matrix[1][2];
        // Without perspective `w` is exactly 1, and dividing by it changes no
        // bit. Targets without a float divider pay a call per division.
        if w == 1.0 {
            return Point {
                x: mapped_x,
                y: mapped_y,
            };
        }
        let safe_w = if w.abs() <= f32::EPSILON { 1.0 } else { w };

        Point {
            x: mapped_x / safe_w,
            y: mapped_y / safe_w,
        }
    }

    /// Maps a rectangle's top-left, top-right, bottom-left and bottom-right corners.
    pub fn map_rect(&self, rect: Rect) -> [[f32; 2]; 4] {
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        let corner = |x, y| {
            let point = self.map_point(Point { x, y });
            [point.x, point.y]
        };
        [
            corner(rect.x, rect.y),
            corner(right, rect.y),
            corner(rect.x, bottom),
            corner(right, bottom),
        ]
    }

    fn w_at(&self, x: f32, y: f32) -> f32 {
        self.matrix[2][0] * x + self.matrix[2][1] * y + self.matrix[2][2]
    }

    /// Whether `point` lies in front of the eye of the transform's
    /// perspective, so that it maps to a position: its homogeneous `w` is at
    /// least [`NEAR_W`]. Always true without perspective.
    pub fn in_front(&self, point: Point) -> bool {
        self.w_at(point.x, point.y) >= NEAR_W
    }

    /// Whether all of `rect` lies in front of the eye.
    pub fn rect_in_front(&self, rect: Rect) -> bool {
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        [
            (rect.x, rect.y),
            (right, rect.y),
            (rect.x, bottom),
            (right, bottom),
        ]
        .into_iter()
        .all(|(x, y)| self.w_at(x, y) >= NEAR_W)
    }

    /// Maps `point` through this transform, the [`inverse`](Self::inverse) of
    /// a perspective, back onto the plane the perspective shows; `None` when
    /// the eye's ray through `point` meets the plane behind the eye or nearer
    /// than [`NEAR_W`]. The perspective's own `w` at the mapped point is one
    /// over this transform's `w` at `point`.
    pub fn map_point_in_front(&self, point: Point) -> Option<Point> {
        let w = self.w_at(point.x, point.y);
        (w > 0.0 && w * NEAR_W <= 1.0).then(|| self.map_point(point))
    }

    /// Maps the part of `rect` in front of the eye: the rectangle cut where
    /// its homogeneous `w` falls below [`NEAR_W`], corners in order top-left,
    /// top-right, bottom-right, bottom-left with the cut's corners between.
    pub fn front_polygon(&self, rect: Rect) -> FrontPolygon {
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        let around = [
            [rect.x, rect.y],
            [right, rect.y],
            [right, bottom],
            [rect.x, bottom],
        ];
        let mut polygon = FrontPolygon {
            corners: [[0.0; 2]; 5],
            len: 0,
        };
        let mapped = |[x, y]: [f32; 2]| {
            let point = self.map_point(Point { x, y });
            [point.x, point.y]
        };
        for (index, &from) in around.iter().enumerate() {
            let to = around[(index + 1) % around.len()];
            let (w_from, w_to) = (self.w_at(from[0], from[1]), self.w_at(to[0], to[1]));
            if w_from >= NEAR_W {
                polygon.push(mapped(from));
            }
            if (w_from >= NEAR_W) != (w_to >= NEAR_W) {
                let t = (NEAR_W - w_from) / (w_to - w_from);
                polygon.push(mapped([
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                ]));
            }
        }
        polygon
    }

    /// Returns the axis-aligned bounds of the transformed rectangle's part in
    /// front of the eye; empty when no part is.
    pub fn bounds_for_rect(&self, rect: Rect) -> Rect {
        let [[sx, kx, tx], [ky, sy, ty], [px, py, w]] = self.matrix;
        if px != 0.0 || py != 0.0 || w != 1.0 {
            return self.front_polygon(rect).bounds();
        }
        if kx != 0.0 || ky != 0.0 {
            return quad_bounds(self.map_rect(rect));
        }
        // Scale and translation alone keep a rectangle a rectangle: two
        // corners bound it.
        let x0 = sx * rect.x + tx;
        let x1 = sx * (rect.x + rect.width) + tx;
        let y0 = sy * rect.y + ty;
        let y1 = sy * (rect.y + rect.height) + ty;
        Rect {
            x: x0.min(x1),
            y: y0.min(y1),
            width: (x1 - x0).abs(),
            height: (y1 - y0).abs(),
        }
    }
}

fn axis_aligned_rect_from_quad(quad: [[f32; 2]; 4]) -> Option<Rect> {
    let top_left = quad[0];
    let top_right = quad[1];
    let bottom_left = quad[2];
    let bottom_right = quad[3];
    let x_epsilon = 1e-4;
    let y_epsilon = 1e-4;

    if (top_left[1] - top_right[1]).abs() > y_epsilon
        || (bottom_left[1] - bottom_right[1]).abs() > y_epsilon
        || (top_left[0] - bottom_left[0]).abs() > x_epsilon
        || (top_right[0] - bottom_right[0]).abs() > x_epsilon
    {
        return None;
    }

    Some(Rect {
        x: top_left[0],
        y: top_left[1],
        width: top_right[0] - top_left[0],
        height: bottom_left[1] - top_left[1],
    })
}

impl Default for ProjectiveTransform {
    fn default() -> Self {
        Self::identity()
    }
}

/// Returns the axis-aligned bounds containing all four points.
pub fn quad_bounds(quad: [[f32; 2]; 4]) -> Rect {
    // Unrolled: iterating the array by value copies it and steps an
    // iterator where the compiler optimizes for size.
    let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = quad;
    let min_x = f32::INFINITY.min(x0).min(x1).min(x2).min(x3);
    let min_y = f32::INFINITY.min(y0).min(y1).min(y2).min(y3);
    let max_x = f32::NEG_INFINITY.max(x0).max(x1).max(x2).max(x3);
    let max_y = f32::NEG_INFINITY.max(y0).max(y1).max(y2).max(y3);

    Rect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

fn multiply_matrices(lhs: [[f32; 3]; 3], rhs: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    // Built in place: a zeroed matrix filled by loops costs a fill and a
    // copy where the compiler optimizes for size.
    let cell = |row: usize, col: usize| {
        lhs[row][0] * rhs[0][col] + lhs[row][1] * rhs[1][col] + lhs[row][2] * rhs[2][col]
    };
    [
        [cell(0, 0), cell(0, 1), cell(0, 2)],
        [cell(1, 0), cell(1, 1), cell(1, 2)],
        [cell(2, 0), cell(2, 1), cell(2, 2)],
    ]
}

fn solve_homography(source: [[f32; 2]; 4], target: [[f32; 2]; 4]) -> Option<[f32; 8]> {
    let mut matrix = [[0.0f32; 9]; 8];
    for (index, (src, dst)) in source.into_iter().zip(target).enumerate() {
        let row = index * 2;
        let x = src[0];
        let y = src[1];
        let u = dst[0];
        let v = dst[1];

        matrix[row] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        matrix[row + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }

    for pivot in 0..8 {
        let mut pivot_row = pivot;
        let mut pivot_value = matrix[pivot][pivot].abs();
        let mut candidate = pivot + 1;
        while candidate < 8 {
            let candidate_value = matrix[candidate][pivot].abs();
            if candidate_value > pivot_value {
                pivot_row = candidate;
                pivot_value = candidate_value;
            }
            candidate += 1;
        }

        if pivot_value <= f32::EPSILON {
            return None;
        }

        if pivot_row != pivot {
            matrix.swap(pivot, pivot_row);
        }

        let divisor = matrix[pivot][pivot];
        let mut col = pivot;
        while col < 9 {
            matrix[pivot][col] /= divisor;
            col += 1;
        }

        for row in 0..8 {
            if row == pivot {
                continue;
            }
            let factor = matrix[row][pivot];
            if factor.abs() <= f32::EPSILON {
                continue;
            }
            let mut col = pivot;
            while col < 9 {
                matrix[row][col] -= factor * matrix[pivot][col];
                col += 1;
            }
        }
    }

    let mut solution = [0.0f32; 8];
    for index in 0..8 {
        solution[index] = matrix[index][8];
    }
    Some(solution)
}

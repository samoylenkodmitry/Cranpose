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

/// A projective mapping of a two-dimensional coordinate plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectiveTransform {
    matrix: [[f32; 3]; 3],
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

    /// Maps a rectangle to corners ordered top-left, top-right, bottom-left, bottom-right.
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

        Self {
            matrix: [
                [coefficients[0], coefficients[1], coefficients[2]],
                [coefficients[3], coefficients[4], coefficients[5]],
                [coefficients[6], coefficients[7], 1.0],
            ],
        }
    }

    /// The transform a homogeneous matrix describes, scaled so its last entry
    /// is one wherever that entry is not zero: its other entries then read as
    /// the scale, turn, translation and perspective they are.
    pub fn from_homogeneous(matrix: [[f32; 3]; 3]) -> Self {
        let w = matrix[2][2];
        if w == 1.0 || w.abs() <= f32::EPSILON {
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
    pub fn map_point(self, point: Point) -> Point {
        let x = point.x;
        let y = point.y;
        let w = self.matrix[2][0] * x + self.matrix[2][1] * y + self.matrix[2][2];
        let safe_w = if w.abs() <= f32::EPSILON { 1.0 } else { w };

        Point {
            x: (self.matrix[0][0] * x + self.matrix[0][1] * y + self.matrix[0][2]) / safe_w,
            y: (self.matrix[1][0] * x + self.matrix[1][1] * y + self.matrix[1][2]) / safe_w,
        }
    }

    /// Maps a rectangle's top-left, top-right, bottom-left and bottom-right corners.
    pub fn map_rect(self, rect: Rect) -> [[f32; 2]; 4] {
        [
            self.map_point(Point {
                x: rect.x,
                y: rect.y,
            }),
            self.map_point(Point {
                x: rect.x + rect.width,
                y: rect.y,
            }),
            self.map_point(Point {
                x: rect.x,
                y: rect.y + rect.height,
            }),
            self.map_point(Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            }),
        ]
        .map(|point| [point.x, point.y])
    }

    /// Returns the axis-aligned bounds of the transformed rectangle.
    pub fn bounds_for_rect(self, rect: Rect) -> Rect {
        quad_bounds(self.map_rect(rect))
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
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for [x, y] in quad {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    Rect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

fn multiply_matrices(lhs: [[f32; 3]; 3], rhs: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for row in 0..3 {
        for col in 0..3 {
            out[row][col] =
                lhs[row][0] * rhs[0][col] + lhs[row][1] * rhs[1][col] + lhs[row][2] * rhs[2][col];
        }
    }
    out
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

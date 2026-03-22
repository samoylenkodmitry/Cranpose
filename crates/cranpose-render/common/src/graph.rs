use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_foundation::PointerEvent;
use cranpose_ui::text::AnnotatedString;
use cranpose_ui::{
    GraphicsLayer, Point, Rect, RenderEffect, RoundedCornerShape, TextLayoutOptions, TextStyle,
};
use cranpose_ui_graphics::{BlendMode, ColorFilter, DrawPrimitive};

use crate::raster_cache::LayerRasterCacheHashes;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectiveTransform {
    matrix: [[f32; 3]; 3],
}

impl ProjectiveTransform {
    pub const fn identity() -> Self {
        Self {
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    pub fn translation(tx: f32, ty: f32) -> Self {
        Self {
            matrix: [[1.0, 0.0, tx], [0.0, 1.0, ty], [0.0, 0.0, 1.0]],
        }
    }

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

    /// Returns the composed transform that applies `self` first and `next` second.
    pub fn then(self, next: Self) -> Self {
        Self {
            matrix: multiply_matrices(next.matrix, self.matrix),
        }
    }

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

    pub fn matrix(self) -> [[f32; 3]; 3] {
        self.matrix
    }

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IsolationReasons {
    pub explicit_offscreen: bool,
    pub effect: bool,
    pub backdrop: bool,
    pub group_opacity: bool,
    pub blend_mode: bool,
}

impl IsolationReasons {
    pub fn has_any(self) -> bool {
        self.explicit_offscreen
            || self.effect
            || self.backdrop
            || self.group_opacity
            || self.blend_mode
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CachePolicy {
    #[default]
    None,
    Auto,
}

#[derive(Clone)]
pub struct HitTestNode {
    pub shape: Option<RoundedCornerShape>,
    pub click_actions: Vec<Rc<dyn Fn(Point)>>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub clip: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawPrimitiveNode {
    pub primitive: DrawPrimitive,
    pub clip: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextPrimitiveNode {
    pub node_id: NodeId,
    pub rect: Rect,
    pub text: AnnotatedString,
    pub text_style: TextStyle,
    pub font_size: f32,
    pub layout_options: TextLayoutOptions,
    pub clip: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitivePhase {
    BeforeChildren,
    AfterChildren,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveNode {
    Draw(DrawPrimitiveNode),
    Text(Box<TextPrimitiveNode>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveEntry {
    pub phase: PrimitivePhase,
    pub node: PrimitiveNode,
}

#[derive(Clone)]
pub struct LayerNode {
    pub node_id: Option<NodeId>,
    pub local_bounds: Rect,
    pub transform_to_parent: ProjectiveTransform,
    pub motion_context_animated: bool,
    pub translated_content_context: bool,
    pub graphics_layer: GraphicsLayer,
    pub clip_to_bounds: bool,
    pub shadow_clip: Option<Rect>,
    pub hit_test: Option<HitTestNode>,
    pub has_hit_targets: bool,
    pub isolation: IsolationReasons,
    pub cache_policy: CachePolicy,
    pub cache_hashes: LayerRasterCacheHashes,
    pub cache_hashes_valid: bool,
    pub children: Vec<RenderNode>,
}

impl LayerNode {
    pub fn clip_rect(&self) -> Option<Rect> {
        (self.clip_to_bounds || self.graphics_layer.clip).then_some(self.local_bounds)
    }

    pub fn effect(&self) -> Option<&RenderEffect> {
        self.graphics_layer.render_effect.as_ref()
    }

    pub fn backdrop(&self) -> Option<&RenderEffect> {
        self.graphics_layer.backdrop_effect.as_ref()
    }

    pub fn opacity(&self) -> f32 {
        self.graphics_layer.alpha
    }

    pub fn blend_mode(&self) -> BlendMode {
        self.graphics_layer.blend_mode
    }

    pub fn color_filter(&self) -> Option<ColorFilter> {
        self.graphics_layer.color_filter
    }

    pub fn target_content_hash(&self) -> u64 {
        if self.cache_hashes_valid {
            self.cache_hashes.target_content
        } else {
            crate::graph_hash::layer_raster_cache_hashes(self).target_content
        }
    }

    pub fn effect_hash(&self) -> u64 {
        if self.cache_hashes_valid {
            self.cache_hashes.effect
        } else {
            crate::graph_hash::layer_raster_cache_hashes(self).effect
        }
    }

    pub fn recompute_raster_cache_hashes(&mut self) {
        crate::graph_hash::recompute_layer_raster_cache_hashes(self);
    }
}

#[derive(Clone)]
pub enum RenderNode {
    Primitive(PrimitiveEntry),
    Layer(Box<LayerNode>),
}

#[derive(Clone)]
pub struct RenderGraph {
    pub root: LayerNode,
}

impl RenderGraph {
    pub fn new(mut root: LayerNode) -> Self {
        root.recompute_raster_cache_hashes();
        Self { root }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster_cache::LayerRasterCacheHashes;
    use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

    fn test_layer(local_bounds: Rect, children: Vec<RenderNode>) -> LayerNode {
        LayerNode {
            node_id: None,
            local_bounds,
            transform_to_parent: ProjectiveTransform::identity(),
            motion_context_animated: false,
            translated_content_context: false,
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children,
        }
    }

    #[test]
    fn projective_transform_translation_maps_points() {
        let transform = ProjectiveTransform::translation(7.0, -3.5);
        let mapped = transform.map_point(Point { x: 2.0, y: 4.0 });
        assert!((mapped.x - 9.0).abs() < 1e-6);
        assert!((mapped.y - 0.5).abs() < 1e-6);
    }

    #[test]
    fn projective_transform_then_composes_in_parent_order() {
        let child = ProjectiveTransform::translation(4.0, 2.0);
        let parent = ProjectiveTransform::translation(10.0, -1.0);
        let composed = child.then(parent);
        let mapped = composed.map_point(Point { x: 1.0, y: 1.0 });
        assert!((mapped.x - 15.0).abs() < 1e-6);
        assert!((mapped.y - 2.0).abs() < 1e-6);
    }

    #[test]
    fn homography_maps_rect_corners_to_target_quad() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
        };
        let quad = [[5.0, 7.0], [25.0, 6.0], [7.0, 20.0], [28.0, 21.0]];
        let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);
        let mapped = transform.map_rect(rect);
        for (expected, actual) in quad.into_iter().zip(mapped) {
            assert!((expected[0] - actual[0]).abs() < 1e-4);
            assert!((expected[1] - actual[1]).abs() < 1e-4);
        }
    }

    #[test]
    fn axis_aligned_rect_to_quad_keeps_exact_affine_matrix() {
        let rect = Rect {
            x: 2.0,
            y: 3.0,
            width: 20.0,
            height: 10.0,
        };
        let quad = [[12.0, 9.0], [32.0, 9.0], [12.0, 19.0], [32.0, 19.0]];
        let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);

        assert_eq!(
            transform.matrix(),
            [[1.0, 0.0, 10.0], [0.0, 1.0, 6.0], [0.0, 0.0, 1.0]]
        );
    }

    #[test]
    fn axis_aligned_rect_to_quad_keeps_exact_axis_aligned_scale() {
        let rect = Rect {
            x: 4.0,
            y: 6.0,
            width: 10.0,
            height: 8.0,
        };
        let quad = [[20.0, 18.0], [50.0, 18.0], [20.0, 42.0], [50.0, 42.0]];
        let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);

        assert_eq!(
            transform.matrix(),
            [[3.0, 0.0, 8.0], [0.0, 3.0, 0.0], [0.0, 0.0, 1.0]]
        );
    }

    #[test]
    fn render_graph_new_recomputes_manual_layer_hashes() {
        let primitive = PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 1.0,
                        y: 2.0,
                        width: 8.0,
                        height: 6.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                },
                clip: None,
            }),
        };
        let mut root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Primitive(primitive)],
        );
        root.graphics_layer.render_effect = Some(RenderEffect::blur(3.0));
        let mut expected = root.clone();
        expected.recompute_raster_cache_hashes();

        let graph = RenderGraph::new(root);
        assert_eq!(
            graph.root.target_content_hash(),
            expected.target_content_hash()
        );
        assert_eq!(graph.root.effect_hash(), expected.effect_hash());
    }
}

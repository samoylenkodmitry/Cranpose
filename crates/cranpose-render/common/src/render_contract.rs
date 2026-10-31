use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, DrawScope, DrawScopeDefault, DrawStyle, Path, Point,
    Rect, Size, Stroke, Trapezoid,
};

use crate::{
    graph::{
        DrawPrimitiveNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderGraph, RenderNode,
    },
    image_compare::{
        image_difference_stats, normalize_rgba_region, pixel_difference, sample_pixel,
    },
};

mod draw_groups;
mod draw_text_brush;
mod text_fixtures;

use text_fixtures::{
    assert_clipped_text_frame, assert_multiline_text_frame, assert_translated_plain_text_frames,
    assert_translated_text_decorations_frames, clipped_text_fixture, multiline_text_fixture,
    translated_plain_text_fixture, translated_text_decorations_fixture,
};

const BACKGROUND_COLOR: Color = Color(18.0 / 255.0, 18.0 / 255.0, 24.0 / 255.0, 1.0);
const FOREGROUND_COLOR: Color = Color::WHITE;
const PIXEL_DIFFERENCE_TOLERANCE: u32 = 24;

const TRANSLATED_SUBTREE_BUDGET: NormalizedDifferenceBudget = NormalizedDifferenceBudget {
    max_differing_pixels: 270,
    max_pixel_difference: 360,
};

/// A scene that every backend renders for a shared case, with the frame size it renders at.
#[derive(Clone)]
pub struct RenderFixture {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// The scene to render.
    pub graph: RenderGraph,
    /// Region of the frame, in pixels, to resample to a fixed size before frames are compared, or
    /// `None` when frames are compared at their rendered size.
    pub normalized_rect: Option<Rect>,
}

/// A frame that a backend rendered for a `RenderFixture`.
#[derive(Clone, Debug)]
pub struct RenderedFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// RGBA8 pixels, row by row, four bytes per pixel.
    pub pixels: Vec<u8>,
    /// The fixture's `normalized_rect`, carried over to the frame.
    pub normalized_rect: Option<Rect>,
}

#[derive(Clone, Copy)]
struct NormalizedDifferenceBudget {
    max_differing_pixels: u32,
    max_pixel_difference: u32,
}

/// A rendering case that every backend must draw alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedRenderCase {
    /// A filled rounded rectangle.
    RoundedRect,
    /// A rectangle cut by a primitive clip.
    PrimitiveClip,
    /// A subtree drawn at fractional translations.
    TranslatedSubtree,
    /// Plain text drawn at fractional translations.
    TranslatedPlainText,
    /// Text with decorations drawn at fractional translations.
    TranslatedTextDecorations,
    /// Text wrapped over several lines.
    MultilineText,
    /// Text cut by a clip.
    ClippedText,
    /// A stroked rounded rectangle.
    StrokedRoundRect,
    /// A ring segment drawn as an arc band.
    AnnularSector,
    /// A filled path.
    FilledPath,
    /// Draws a `DrawScope` transform or clip places: scaled, turned, cut to
    /// a rect and to a path, moved, inset, text scaled, and a grouped
    /// overlay over the overlay before it.
    DrawScopeGroups,
    /// Draws in groups of a node whose graphics layer modulates alpha and
    /// filters colour, painted as the node's plain draws are.
    PaintedDrawGroups,
    /// `DrawScope` text filled with a gradient brush across its extent.
    GradientDrawText,
}

/// Every `SharedRenderCase`, for running the whole contract.
pub const ALL_SHARED_RENDER_CASES: [SharedRenderCase; 13] = [
    SharedRenderCase::RoundedRect,
    SharedRenderCase::PrimitiveClip,
    SharedRenderCase::TranslatedSubtree,
    SharedRenderCase::TranslatedPlainText,
    SharedRenderCase::TranslatedTextDecorations,
    SharedRenderCase::MultilineText,
    SharedRenderCase::ClippedText,
    SharedRenderCase::StrokedRoundRect,
    SharedRenderCase::AnnularSector,
    SharedRenderCase::FilledPath,
    SharedRenderCase::DrawScopeGroups,
    SharedRenderCase::PaintedDrawGroups,
    SharedRenderCase::GradientDrawText,
];

impl SharedRenderCase {
    /// Short snake_case name of the case, for test names and reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::RoundedRect => "rounded_rect",
            Self::PrimitiveClip => "primitive_clip",
            Self::TranslatedSubtree => "translated_subtree",
            Self::TranslatedPlainText => "translated_plain_text",
            Self::TranslatedTextDecorations => "translated_text_decorations",
            Self::MultilineText => "multiline_text",
            Self::ClippedText => "clipped_text",
            Self::StrokedRoundRect => "stroked_round_rect",
            Self::AnnularSector => "annular_sector",
            Self::FilledPath => "filled_path",
            Self::DrawScopeGroups => "draw_scope_groups",
            Self::PaintedDrawGroups => "painted_draw_groups",
            Self::GradientDrawText => "gradient_draw_text",
        }
    }

    /// The fixtures to render for this case. A translated case returns two fixtures that differ in
    /// their fractional offsets.
    pub fn fixtures(self) -> Vec<RenderFixture> {
        // Draw scope text records as an app lays it out, in the default
        // font.
        cranpose_ui::AppContext::new().enter(|| {
            cranpose_ui::text::set_text_fonts(
                crate::text_fonts::TextFonts::from_fonts_or_default(&[]).resolver(),
            );
            self.fixtures_in_app()
        })
    }

    fn fixtures_in_app(self) -> Vec<RenderFixture> {
        match self {
            Self::RoundedRect => vec![rounded_rect_fixture()],
            Self::PrimitiveClip => vec![primitive_clip_fixture()],
            Self::TranslatedSubtree => vec![
                translated_subtree_fixture(12.3, 14.7),
                translated_subtree_fixture(32.6, 26.2),
            ],
            Self::TranslatedPlainText => vec![
                translated_plain_text_fixture(14.3, 18.6),
                translated_plain_text_fixture(36.4, 30.1),
            ],
            Self::TranslatedTextDecorations => vec![
                translated_text_decorations_fixture(14.3, 18.6),
                translated_text_decorations_fixture(36.4, 30.1),
            ],
            Self::MultilineText => vec![multiline_text_fixture()],
            Self::ClippedText => vec![clipped_text_fixture()],
            Self::StrokedRoundRect => vec![stroked_round_rect_fixture()],
            Self::AnnularSector => vec![annular_sector_fixture()],
            Self::FilledPath => vec![filled_path_fixture()],
            Self::DrawScopeGroups => vec![draw_groups::draw_groups_fixture()],
            Self::PaintedDrawGroups => vec![draw_groups::painted_groups_fixture()],
            Self::GradientDrawText => vec![draw_text_brush::gradient_text_fixture()],
        }
    }

    /// Checks the frames that were rendered from `fixtures`, in the same order. Panics when the
    /// frame count is wrong or a frame does not match what the case expects.
    pub fn assert_frames(self, frames: &[RenderedFrame]) {
        match self {
            Self::RoundedRect => {
                let [frame] = frames else {
                    panic!("rounded_rect expects exactly one rendered frame");
                };
                assert_rounded_rect_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::PrimitiveClip => {
                let [frame] = frames else {
                    panic!("primitive_clip expects exactly one rendered frame");
                };
                assert_primitive_clip_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::TranslatedSubtree => {
                assert_translated_subtree_frames(frames);
            }
            Self::TranslatedPlainText => {
                assert_translated_plain_text_frames(frames);
            }
            Self::TranslatedTextDecorations => {
                assert_translated_text_decorations_frames(frames);
            }
            Self::MultilineText => {
                let [frame] = frames else {
                    panic!("multiline_text expects exactly one rendered frame");
                };
                assert_multiline_text_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::StrokedRoundRect => {
                let [frame] = frames else {
                    panic!("stroked_round_rect expects exactly one rendered frame");
                };
                assert_stroked_round_rect_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::AnnularSector => {
                let [frame] = frames else {
                    panic!("annular_sector expects exactly one rendered frame");
                };
                assert_annular_sector_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::ClippedText => {
                let [frame] = frames else {
                    panic!("clipped_text expects exactly one rendered frame");
                };
                assert_clipped_text_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::FilledPath => {
                let [frame] = frames else {
                    panic!("filled_path expects exactly one rendered frame");
                };
                assert_filled_path_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::DrawScopeGroups => {
                let [frame] = frames else {
                    panic!("draw_scope_groups expects exactly one rendered frame");
                };
                draw_groups::assert_draw_groups_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::PaintedDrawGroups => {
                let [frame] = frames else {
                    panic!("painted_draw_groups expects exactly one rendered frame");
                };
                draw_groups::assert_painted_groups_frame(&frame.pixels, frame.width, frame.height);
            }
            Self::GradientDrawText => {
                let [frame] = frames else {
                    panic!("gradient_draw_text expects exactly one rendered frame");
                };
                draw_text_brush::assert_gradient_text_frame(
                    &frame.pixels,
                    frame.width,
                    frame.height,
                );
            }
        }
    }
}

fn rounded_rect_fixture() -> RenderFixture {
    build_fixture(
        72,
        72,
        vec![draw_node(
            DrawPrimitive::RoundRect {
                rect: Rect {
                    x: 12.0,
                    y: 12.0,
                    width: 48.0,
                    height: 48.0,
                },
                brush: Brush::solid(FOREGROUND_COLOR),
                radii: CornerRadii::uniform(18.0),
                stroke: None,
            },
            None,
        )],
    )
}

fn primitive_clip_fixture() -> RenderFixture {
    build_fixture(
        52,
        44,
        vec![draw_node(
            DrawPrimitive::Rect {
                rect: Rect {
                    x: 8.0,
                    y: 10.0,
                    width: 28.0,
                    height: 18.0,
                },
                brush: Brush::solid(FOREGROUND_COLOR),
                stroke: None,
            },
            Some(Rect {
                x: 14.0,
                y: 15.0,
                width: 10.0,
                height: 6.0,
            }),
        )],
    )
}

fn translated_subtree_fixture(translation_x: f32, translation_y: f32) -> RenderFixture {
    let subtree_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 48.0,
        height: 36.0,
    };

    build_translated_fixture(
        96,
        84,
        subtree_bounds,
        Point::new(translation_x, translation_y),
        vec![
            draw_node(
                DrawPrimitive::RoundRect {
                    rect: Rect {
                        x: 4.0,
                        y: 4.0,
                        width: 40.0,
                        height: 28.0,
                    },
                    brush: Brush::solid(FOREGROUND_COLOR),
                    radii: CornerRadii::uniform(10.0),
                    stroke: None,
                },
                None,
            ),
            draw_node(
                DrawPrimitive::Rect {
                    rect: Rect {
                        x: 10.0,
                        y: 18.0,
                        width: 18.0,
                        height: 10.0,
                    },
                    brush: Brush::solid(Color(0.2, 0.8, 1.0, 1.0)),
                    stroke: None,
                },
                Some(Rect {
                    x: 12.0,
                    y: 20.0,
                    width: 10.0,
                    height: 4.0,
                }),
            ),
        ],
    )
}

fn build_fixture(width: u32, height: u32, children: Vec<RenderNode>) -> RenderFixture {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };

    RenderFixture {
        width,
        height,
        graph: RenderGraph::new(graph_layer(
            bounds,
            ProjectiveTransform::identity(),
            with_background(bounds, children),
        )),
        normalized_rect: None,
    }
}

fn build_translated_fixture(
    width: u32,
    height: u32,
    subtree_bounds: Rect,
    translation: Point,
    subtree_children: Vec<RenderNode>,
) -> RenderFixture {
    build_translated_fixture_with_context(
        width,
        height,
        subtree_bounds,
        translation,
        false,
        subtree_children,
    )
}

fn build_translated_fixture_with_context(
    width: u32,
    height: u32,
    subtree_bounds: Rect,
    translation: Point,
    translated_content_context: bool,
    subtree_children: Vec<RenderNode>,
) -> RenderFixture {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let subtree = graph_layer(
        subtree_bounds,
        ProjectiveTransform::translation(translation.x, translation.y),
        subtree_children,
    );
    let mut subtree = subtree;
    subtree.translated_content_context = translated_content_context;

    RenderFixture {
        width,
        height,
        graph: RenderGraph::new(graph_layer(
            bounds,
            ProjectiveTransform::identity(),
            with_background(bounds, vec![RenderNode::Layer(Box::new(subtree))]),
        )),
        normalized_rect: Some(Rect {
            x: translation.x,
            y: translation.y,
            width: subtree_bounds.width,
            height: subtree_bounds.height,
        }),
    }
}

fn graph_layer(
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        local_bounds,
        transform_to_parent,
        children,
        ..Default::default()
    }
}

fn with_background(bounds: Rect, mut children: Vec<RenderNode>) -> Vec<RenderNode> {
    children.insert(
        0,
        draw_node(
            DrawPrimitive::Rect {
                rect: bounds,
                brush: Brush::solid(BACKGROUND_COLOR),
                stroke: None,
            },
            None,
        ),
    );
    children
}

fn draw_node(primitive: DrawPrimitive, clip: Option<Rect>) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode { primitive, clip })),
    })
}

fn stroked_round_rect_fixture() -> RenderFixture {
    build_fixture(
        72,
        72,
        vec![draw_node(
            DrawPrimitive::RoundRect {
                rect: Rect {
                    x: 16.0,
                    y: 16.0,
                    width: 40.0,
                    height: 40.0,
                },
                brush: Brush::solid(FOREGROUND_COLOR),
                radii: CornerRadii::uniform(10.0),
                stroke: Some(Stroke::new(6.0)),
            },
            None,
        )],
    )
}

fn annular_sector_fixture() -> RenderFixture {
    build_fixture(
        72,
        72,
        vec![draw_node(
            DrawPrimitive::Arc {
                rect: Rect {
                    x: 36.0,
                    y: 36.0,
                    width: 24.0,
                    height: 24.0,
                },
                brush: Brush::solid(FOREGROUND_COLOR),
                center: Point::new(36.0, 36.0),
                radius: 24.0,
                start_angle: 0.0,
                sweep_angle: std::f32::consts::FRAC_PI_2,
                stroke: None,
                inner_radius: 12.0,
            },
            None,
        )],
    )
}

/// A sparkline a draw scope fills: slices whose sides meet, with steep
/// edges that stand above the columns beside them.
fn filled_path_primitives() -> Vec<DrawPrimitive> {
    let mut path = Path::new();
    path.move_to(Point::new(6.0, 66.0));
    for (index, y) in [40.0, 8.0, 52.0, 20.5, 61.0, 14.25].into_iter().enumerate() {
        path.line_to(Point::new(6.0 + index as f32 * 12.0, y));
    }
    path.line_to(Point::new(66.0, 66.0));
    path.close();
    let mut recording = cranpose_ui_graphics::CommandRecording::default();
    let mut scope = DrawScopeDefault::new(Size::new(72.0, 72.0), &mut recording);
    scope.draw_path(&path, Brush::solid(FOREGROUND_COLOR), DrawStyle::Fill);
    scope.into_primitives()
}

fn filled_path_fixture() -> RenderFixture {
    build_fixture(
        72,
        72,
        filled_path_primitives()
            .into_iter()
            .map(|primitive| draw_node(primitive, None))
            .collect(),
    )
}

/// Each pixel takes the share of the foreground the fill's slices give it.
fn assert_filled_path_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (72, 72));
    let slices: Vec<Trapezoid> = filled_path_primitives()
        .iter()
        .filter_map(|primitive| match primitive {
            DrawPrimitive::Trapezoid { trapezoid, .. } => Some(*trapezoid),
            _ => None,
        })
        .collect();
    assert_eq!(slices.len(), 5, "the fill slices between its points");
    let background = f32::from(sample_pixel(pixels, width, 2, 2)[0]);
    let foreground = FOREGROUND_COLOR.r() * 255.0;
    for y in 0..height {
        for x in 0..width {
            let centre = Point::new(x as f32 + 0.5, y as f32 + 0.5);
            let expected = slices.iter().fold(0.0f32, |value, slice| {
                value + slice.coverage(centre) * (1.0 - value)
            });
            let actual = (f32::from(sample_pixel(pixels, width, x, y)[0]) - background)
                / (foreground - background);
            assert!(
                (actual - expected).abs() <= 0.05,
                "filled path pixel ({x}, {y}) carries {actual:.3} of the foreground, \
                 its slices give it {expected:.3}"
            );
        }
    }
}

fn assert_stroked_round_rect_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (72, 72));
    let background = sample_pixel(pixels, width, 2, 2);

    assert_pixel_matches_background(
        pixels,
        width,
        background,
        36,
        16,
        false,
        "stroked round rect top edge should contain stroke ink",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        16,
        36,
        false,
        "stroked round rect left edge should contain stroke ink",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        36,
        36,
        true,
        "stroked round rect interior must stay empty — a backend that filled \
         the shape instead of stroking it would fail here",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        4,
        36,
        true,
        "outside the stroked round rect should stay background-colored",
    );
}

fn assert_annular_sector_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (72, 72));
    let background = sample_pixel(pixels, width, 2, 2);

    assert_pixel_matches_background(
        pixels,
        width,
        background,
        54,
        38,
        false,
        "annular sector band near 0 degrees should contain fill",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        38,
        54,
        false,
        "annular sector band near 90 degrees should contain fill",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        36,
        36,
        true,
        "the annulus hole must stay empty",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        54,
        30,
        true,
        "past the flat radial edge must stay empty — a rounded cap here would \
         mean the sector is being drawn as a stroked arc",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        66,
        38,
        true,
        "beyond the outer radius must stay empty",
    );
}

fn assert_rounded_rect_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (72, 72));
    let background = sample_pixel(pixels, width, 2, 2);

    assert_pixel_matches_background(
        pixels,
        width,
        background,
        14,
        14,
        true,
        "rounded rect corner should stay background-colored",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        30,
        16,
        false,
        "rounded rect top edge should contain fill",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        36,
        36,
        false,
        "rounded rect center should contain fill",
    );
}

fn assert_primitive_clip_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (52, 44));
    let background = sample_pixel(pixels, width, 2, 2);

    assert_pixel_matches_background(
        pixels,
        width,
        background,
        18,
        18,
        false,
        "pixel inside primitive clip should contain fill",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        10,
        12,
        true,
        "pixel inside source rect but outside clip should stay background-colored",
    );
    assert_pixel_matches_background(
        pixels,
        width,
        background,
        30,
        20,
        true,
        "pixel on the far side of the source rect but outside clip should stay background-colored",
    );
}

fn assert_translated_subtree_frames(frames: &[RenderedFrame]) {
    let [base, moved] = frames else {
        panic!("translated_subtree expects exactly two rendered frames");
    };
    assert_eq!((base.width, base.height), (96, 84));
    assert_eq!((moved.width, moved.height), (96, 84));
    assert_ne!(
        base.pixels, moved.pixels,
        "translated subtree contract should move within the full frame"
    );
    assert_normalized_region_matches(
        base,
        moved,
        TRANSLATED_SUBTREE_BUDGET,
        "translated subtree output should remain invariant under rigid parent translation",
    );
}

fn normalize_frame_region(frame: &RenderedFrame) -> Vec<u8> {
    let rect = normalized_rect(frame);
    let (width, height) = normalized_output_dimensions(frame);
    normalize_rgba_region(
        &frame.pixels,
        frame.width,
        frame.height,
        rect,
        width,
        height,
    )
}

fn assert_normalized_region_matches(
    base: &RenderedFrame,
    moved: &RenderedFrame,
    budget: NormalizedDifferenceBudget,
    message: &str,
) {
    assert_eq!(
        normalized_output_dimensions(base),
        normalized_output_dimensions(moved),
        "normalized comparison requires matching output sizes",
    );
    let (width, height) = normalized_output_dimensions(base);
    let base_normalized = normalize_frame_region(base);
    let moved_normalized = normalize_frame_region(moved);
    let stats = image_difference_stats(
        &base_normalized,
        &moved_normalized,
        width,
        height,
        PIXEL_DIFFERENCE_TOLERANCE,
    );
    if stats.differing_pixels > budget.max_differing_pixels
        || stats.max_difference > budget.max_pixel_difference
    {
        let diff = stats
            .first_difference
            .as_ref()
            .expect("failing normalized comparison should report first difference");
        panic!(
            "{message}; differing_pixels={} max_diff={} first differing normalized pixel at ({}, {}) base={:?} moved={:?} diff={}",
            stats.differing_pixels,
            stats.max_difference,
            diff.x,
            diff.y,
            diff.lhs,
            diff.rhs,
            diff.difference
        );
    }
}

fn normalized_rect(frame: &RenderedFrame) -> Rect {
    frame
        .normalized_rect
        .expect("normalized render frame missing normalized_rect")
}

fn normalized_output_dimensions(frame: &RenderedFrame) -> (u32, u32) {
    let rect = normalized_rect(frame);
    (
        normalized_dimension(rect.width, "width"),
        normalized_dimension(rect.height, "height"),
    )
}

fn normalized_dimension(value: f32, axis: &str) -> u32 {
    let rounded = value.round();
    assert!(
        (value - rounded).abs() <= 0.01,
        "normalized {axis} must stay pixel-sized for stable comparison, got {value}",
    );
    assert!(
        rounded > 0.0,
        "normalized {axis} must be positive, got {value}"
    );
    rounded as u32
}

fn is_background_like(pixel: [u8; 4], background: [u8; 4]) -> bool {
    pixel_difference(pixel, background) <= PIXEL_DIFFERENCE_TOLERANCE
}

fn assert_pixel_matches_background(
    pixels: &[u8],
    width: u32,
    background: [u8; 4],
    x: u32,
    y: u32,
    expect_background: bool,
    message: &str,
) {
    let pixel = sample_pixel(pixels, width, x, y);
    let background_like = is_background_like(pixel, background);
    assert_eq!(
        background_like, expect_background,
        "{message}; pixel at ({x},{y}) was {pixel:?} against background {background:?}"
    );
}

fn count_non_background_pixels(pixels: &[u8], width: u32, height: u32, background: [u8; 4]) -> u32 {
    count_non_background_pixels_in_band(pixels, width, 0, height, background)
}

fn count_non_background_pixels_in_band(
    pixels: &[u8],
    width: u32,
    y_start: u32,
    y_end: u32,
    background: [u8; 4],
) -> u32 {
    let mut count = 0;
    for y in y_start..y_end {
        for x in 0..width {
            if !is_background_like(sample_pixel(pixels, width, x, y), background) {
                count += 1;
            }
        }
    }
    count
}

fn ink_y_range(pixels: &[u8], width: u32, height: u32, background: [u8; 4]) -> Option<(u32, u32)> {
    let mut top = None;
    let mut bottom = 0u32;
    for y in 0..height {
        for x in 0..width {
            if !is_background_like(sample_pixel(pixels, width, x, y), background) {
                top.get_or_insert(y);
                bottom = y + 1;
                break;
            }
        }
    }
    top.map(|top_y| (top_y, bottom))
}

#[cfg(test)]
#[path = "tests/render_contract_tests.rs"]
mod tests;

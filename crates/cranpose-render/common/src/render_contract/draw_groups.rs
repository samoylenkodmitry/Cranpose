//! The shared case for `DrawScope` transforms and clips: one frame of cells,
//! each a draw command whose draws a group places, lowered by the scene
//! builder as an app's draw commands are.

use std::rc::Rc;

use cranpose_ui::{Box, BoxSpec, DrawCommand, LayoutEngine, Modifier, run_test_composition};
use cranpose_ui_graphics::{
    Brush, Color, ColorFilter, CompositingStrategy, DrawScope, DrawScopeDefault, DrawTextStyle,
    DrawTransform, GraphicsLayer, Path, Point, Rect, Size, TextAlign, TextVerticalAlign,
};

use super::{
    FOREGROUND_COLOR, RenderFixture, assert_pixel_matches_background, build_fixture,
    is_background_like,
};
use crate::{
    graph::{PrimitivePhase, RenderGraph, RenderNode},
    image_compare::{pixel_difference, sample_pixel},
    scene_builder::{build_graph_from_applier, draw_command_nodes_for_tests},
    style_shared::DrawPlacement,
};

const CELL: f32 = 60.0;
const CELLS: u32 = 9;
const WIDTH: u32 = CELL as u32 * CELLS;
const HEIGHT: u32 = CELL as u32;
const UNDER_COLOR: Color = Color(0.5, 0.5, 0.5, 1.0);
const TEXT: &str = "H";

fn cell(index: u32) -> Rect {
    Rect {
        x: index as f32 * CELL,
        y: 0.0,
        width: CELL,
        height: CELL,
    }
}

fn centre(rect: Rect) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
}

fn fill(scope: &mut dyn DrawScope, rect: Rect) {
    scope.draw_rect_at(rect, Brush::solid(FOREGROUND_COLOR));
}

fn text(scope: &mut dyn DrawScope, rect: Rect) {
    let style = DrawTextStyle {
        align: TextAlign::Center,
        vertical_align: TextVerticalAlign::Center,
        ..DrawTextStyle::new(40.0)
    };
    scope.draw_text_at(rect, Brush::solid(FOREGROUND_COLOR), TEXT, &style);
}

fn circle(center: Point, radius: f32) -> Path {
    let mut path = Path::new();
    let steps = 64;
    for step in 0..steps {
        let angle = step as f32 / steps as f32 * std::f32::consts::TAU;
        let point = Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        if step == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close();
    path
}

fn grouped_draws(scope: &mut DrawScopeDefault) {
    let scaled = cell(0);
    scope.scale(0.5, 0.5, centre(scaled), &mut |scope| fill(scope, scaled));

    let turned = cell(1);
    let bar = Rect {
        x: turned.x + 4.0,
        y: 26.0,
        width: 52.0,
        height: 8.0,
    };
    scope.rotate(90.0, centre(turned), &mut |scope| fill(scope, bar));

    let clipped = cell(2);
    scope.clip_rect(
        Rect {
            width: CELL * 0.5,
            ..clipped
        },
        &mut |scope| fill(scope, clipped),
    );

    let masked = cell(3);
    let mask = circle(centre(masked), 24.0);
    scope.clip_path(&mask, &mut |scope| fill(scope, masked));

    let moved = cell(4);
    scope.translate(20.0, 0.0, &mut |scope| {
        fill(
            scope,
            Rect {
                x: moved.x,
                y: 20.0,
                width: 20.0,
                height: 20.0,
            },
        );
    });

    let inset = cell(5);
    let size = scope.size();
    let right = size.width - inset.x - inset.width + 10.0;
    scope.inset(inset.x + 10.0, 10.0, right, 10.0, &mut |scope| {
        scope.draw_rect(Brush::solid(FOREGROUND_COLOR));
    });

    text(scope, cell(6));
    let small = cell(7);
    scope.with_transform(
        &[DrawTransform::Scale {
            scale_x: 0.5,
            scale_y: 0.5,
            pivot: centre(small),
        }],
        &mut |scope| text(scope, small),
    );
}

fn overlap() -> Rect {
    let order = cell(8);
    Rect {
        x: order.x + 10.0,
        y: 10.0,
        width: 30.0,
        height: 30.0,
    }
}

fn plain_overlay(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(overlap(), Brush::solid(UNDER_COLOR));
}

fn grouped_overlay(scope: &mut DrawScopeDefault) {
    let over = overlap();
    scope.translate(10.0, 10.0, &mut |scope| fill(scope, over));
}

pub(super) fn draw_groups_fixture() -> RenderFixture {
    let size = Size::new(WIDTH as f32, HEIGHT as f32);
    let behind = [DrawCommand::Behind(Rc::new(grouped_draws))];
    let overlay = [
        DrawCommand::Overlay(Rc::new(plain_overlay)),
        DrawCommand::Overlay(Rc::new(grouped_overlay)),
    ];
    let mut nodes: Vec<RenderNode> = draw_command_nodes_for_tests(
        1,
        &behind,
        DrawPlacement::Behind,
        size,
        PrimitivePhase::BeforeChildren,
    );
    nodes.extend(draw_command_nodes_for_tests(
        1,
        &overlay,
        DrawPlacement::Overlay,
        size,
        PrimitivePhase::AfterChildren,
    ));
    build_fixture(WIDTH, HEIGHT, nodes)
}

/// The rows of the cell at `index` that hold anything but the background.
fn ink_rows(pixels: &[u8], index: u32, background: [u8; 4]) -> u32 {
    let columns = index * CELL as u32..(index + 1) * CELL as u32;
    (0..HEIGHT)
        .filter(|&y| {
            columns
                .clone()
                .any(|x| !is_background_like(sample_pixel(pixels, WIDTH, x, y), background))
        })
        .count() as u32
}

pub(super) fn assert_draw_groups_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (WIDTH, HEIGHT));
    let background = sample_pixel(pixels, width, 2, 2);
    let expect = |x: u32, y: u32, drawn: bool, message: &str| {
        assert_pixel_matches_background(pixels, width, background, x, y, !drawn, message);
        let pixel = sample_pixel(pixels, width, x, y);
        assert!(
            !drawn || pixel[..3].iter().all(|&channel| channel > 200),
            "{message}: pixel at ({x},{y}) is {pixel:?}, not the foreground"
        );
    };

    expect(30, 30, true, "the scaled cell keeps its centre");
    expect(8, 30, false, "the scaled cell leaves its left edge");
    expect(52, 30, false, "the scaled cell leaves its right edge");
    expect(30, 8, false, "the scaled cell leaves its top edge");

    expect(90, 8, true, "the turned bar stands upright");
    expect(68, 30, false, "the turned bar no longer lies across");

    expect(130, 30, true, "the clipped cell keeps its left half");
    expect(170, 30, false, "the clipped cell loses its right half");

    expect(210, 30, true, "the masked cell keeps the circle's centre");
    expect(
        210,
        8,
        true,
        "the masked cell keeps the circle near its edge",
    );
    expect(183, 3, false, "the masked cell loses its corner");
    expect(235, 55, false, "the masked cell loses its far corner");

    expect(250, 30, false, "the moved square leaves where it was drawn");
    expect(270, 30, true, "the moved square lands where it was moved");

    expect(330, 30, true, "the inset scope draws inside its insets");
    expect(305, 30, false, "the inset scope leaves its left inset");
    expect(330, 5, false, "the inset scope leaves its top inset");

    let full = ink_rows(pixels, 6, background);
    let half = ink_rows(pixels, 7, background);
    assert!(full > 10, "the unscaled text draws, {full} rows");
    let ratio = half as f32 / full as f32;
    assert!(
        (0.35..=0.65).contains(&ratio),
        "text scaled by half spans {half} rows against {full}"
    );

    let order = sample_pixel(pixels, width, 8 * CELL as u32 + 35, 35);
    assert!(
        order[0] > 230,
        "the grouped overlay draws over the overlay before it, got {order:?}"
    );
}

const PAINTED_WIDTH: u32 = 120;
const PAINTED_HEIGHT: u32 = 30;

/// Squares of the foreground, plain and in groups, in a node whose
/// graphics layer modulates alpha and filters colour. Every square must take
/// the node's paint as its plain one does.
fn painted_draws(scope: &mut dyn DrawScope) {
    let square = |x: f32| Rect {
        x,
        y: 5.0,
        width: 20.0,
        height: 20.0,
    };
    fill(scope, square(5.0));
    scope.translate(30.0, 0.0, &mut |scope| fill(scope, square(5.0)));
    let mask = circle(Point::new(75.0, 15.0), 9.0);
    scope.clip_path(&mask, &mut |scope| fill(scope, square(65.0)));
    scope.rotate(90.0, Point::new(105.0, 15.0), &mut |scope| {
        fill(scope, square(95.0));
    });
}

pub(super) fn painted_groups_fixture() -> RenderFixture {
    let mut composition = run_test_composition(|| {
        Box(
            Modifier::empty()
                .size(Size::new(PAINTED_WIDTH as f32, PAINTED_HEIGHT as f32))
                .graphics_layer(|| GraphicsLayer {
                    alpha: 0.5,
                    compositing_strategy: CompositingStrategy::ModulateAlpha,
                    color_filter: Some(ColorFilter::Modulate(Color(1.0, 0.2, 0.2, 1.0))),
                    ..GraphicsLayer::default()
                })
                .draw_behind(painted_draws),
            BoxSpec::default(),
            || {},
        );
    });
    let root = composition.root().expect("the composition has a root");
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    applier
        .compute_layout(root, Size::new(PAINTED_WIDTH as f32, PAINTED_HEIGHT as f32))
        .expect("the composition lays out");
    let node = build_graph_from_applier(&applier, root, 1.0).expect("the composition draws");
    applier.clear_runtime_handle();
    let mut fixture = build_fixture(PAINTED_WIDTH, PAINTED_HEIGHT, Vec::new());
    let RenderGraph {
        root: node_root, ..
    } = node;
    fixture
        .graph
        .root
        .children
        .push(RenderNode::Layer(Box::new(node_root)));
    fixture
}

pub(super) fn assert_painted_groups_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (PAINTED_WIDTH, PAINTED_HEIGHT));
    let background = sample_pixel(pixels, width, 2, 2);
    let plain = sample_pixel(pixels, width, 15, 15);
    assert!(
        pixel_difference(plain, background) > 40 && plain[0] < 230,
        "the plain square takes the node's alpha and filter, got {plain:?}"
    );
    for (x, what) in [(45, "moved"), (75, "path-clipped"), (105, "turned")] {
        let grouped = sample_pixel(pixels, width, x, 15);
        assert!(
            pixel_difference(grouped, plain) <= 12,
            "the {what} square paints {grouped:?} where the plain one paints {plain:?}"
        );
    }
}

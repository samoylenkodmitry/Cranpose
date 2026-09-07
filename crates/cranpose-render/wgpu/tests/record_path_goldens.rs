mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use std::path::PathBuf;

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, DrawRunNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderGraph, RenderNode,
    },
};
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, ColorFilter, CompositingStrategy, CornerRadii, DrawPrimitive,
    DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect, ShadowPrimitive, Size, Stroke,
};
use support::{SIZE, record_mixed_scene, record_solid_scene};

const WRITE_ENV: &str = "CRANPOSE_WRITE_GOLDENS";
const MAX_SMALL_DIFF_FRACTION: f64 = 0.005;
const SMALL_DIFF: u8 = 2;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/record_path")
        .join(format!("{name}.png"))
}

fn write_png(path: &PathBuf, width: u32, height: u32, pixels: &[u8]) {
    let file = std::fs::File::create(path).expect("fixture file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(pixels).expect("png data");
}

fn read_png(path: &PathBuf) -> (u32, u32, Vec<u8>) {
    let file = std::fs::File::open(path)
        .unwrap_or_else(|err| panic!("missing golden {}: {err}", path.display()));
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().expect("png info");
    let mut buffer = vec![0; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut buffer).expect("png frame");
    buffer.truncate(info.buffer_size());
    (info.width, info.height, buffer)
}

fn scope(size: u32) -> DrawScopeDefault {
    DrawScopeDefault::new(Size::new(size as f32, size as f32))
}

fn bounds(size: u32) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: size as f32,
        height: size as f32,
    }
}

fn run_node(primitives: Vec<DrawPrimitive>) -> RenderNode {
    RenderNode::DrawRun(DrawRunNode::new(PrimitivePhase::BeforeChildren, primitives))
}

fn draw_node(primitive: DrawPrimitive, clip: Option<Rect>) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode { primitive, clip }),
    })
}

fn root(size: u32, children: Vec<RenderNode>) -> RenderGraph {
    RenderGraph::new(LayerNode {
        local_bounds: bounds(size),
        children,
        ..LayerNode::default()
    })
}

fn layer(
    local_bounds: Rect,
    translation: Point,
    graphics_layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> LayerNode {
    shared_test_support::layer_node(
        local_bounds,
        ProjectiveTransform::translation(translation.x, translation.y),
        graphics_layer,
        children,
    )
}

fn arena_run() -> RenderGraph {
    let mut scope = scope(SIZE);
    record_mixed_scene(&mut scope);
    root(SIZE, vec![run_node(scope.into_primitives())])
}

fn clipped_primitives() -> RenderGraph {
    let mut scope = scope(SIZE);
    record_solid_scene(&mut scope);
    let clip = Some(Rect {
        x: 20.0,
        y: 30.0,
        width: 200.0,
        height: 180.0,
    });
    let children = scope
        .into_primitives()
        .into_iter()
        .map(|primitive| draw_node(primitive, clip))
        .collect();
    root(SIZE, children)
}

fn thin_bars(color: Color) -> Vec<RenderNode> {
    (0..4)
        .map(|index| {
            support::solid_rect(
                Rect {
                    x: 2.0 + index as f32 * 2.0,
                    y: 1.0 + index as f32 * 4.0,
                    width: 32.0 - index as f32 * 2.0,
                    height: if index == 0 { 3.0 } else { 1.0 },
                },
                color,
            )
        })
        .collect()
}

fn translated_thin_shapes() -> RenderGraph {
    let mut leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 18.0,
        },
        Point::new(9.0, 7.0),
        GraphicsLayer::default(),
        thin_bars(Color(0.95, 0.80, 0.84, 1.0)),
    );
    leaf.translated_content_context = true;
    leaf.motion_context_animated = true;
    let mut wrapper = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 40.0,
        },
        Point::new(12.35, 14.65),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(leaf))],
    );
    wrapper.translated_content_context = true;
    wrapper.motion_context_animated = true;
    let mut gradient_leaf = layer(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 20.0,
        },
        Point::new(70.35, 20.65),
        GraphicsLayer::default(),
        vec![support::brush_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 20.0,
            },
            Brush::vertical_gradient(
                vec![Color(0.9, 0.2, 0.2, 1.0), Color(0.1, 0.3, 0.9, 1.0)],
                0.0,
                20.0,
            ),
        )],
    );
    gradient_leaf.translated_content_context = true;
    gradient_leaf.motion_context_animated = true;
    root(
        128,
        vec![
            support::solid_rect(bounds(128), Color::BLACK),
            RenderNode::Layer(Box::new(wrapper)),
            RenderNode::Layer(Box::new(gradient_leaf)),
        ],
    )
}

fn painted_layers() -> RenderGraph {
    let content = |tint: Color| {
        let mut scope = scope(96);
        scope.draw_round_rect_at(
            Rect {
                x: 4.0,
                y: 4.0,
                width: 88.0,
                height: 40.0,
            },
            Brush::solid(tint),
            CornerRadii::uniform(9.0),
        );
        scope.draw_arc(
            Brush::solid(Color(0.2, 0.9, 0.5, 0.8)),
            Point::new(48.0, 70.0),
            18.0,
            0.3,
            4.0,
            Stroke::new(5.0),
        );
        scope.draw_rect_at(
            Rect {
                x: 10.0,
                y: 60.0,
                width: 30.0,
                height: 30.0,
            },
            Brush::linear_gradient(vec![Color(1.0, 0.5, 0.1, 1.0), Color(0.1, 0.5, 1.0, 1.0)]),
        );
        vec![run_node(scope.into_primitives())]
    };
    let modulated = layer(
        bounds(96),
        Point::new(8.0, 8.0),
        GraphicsLayer {
            alpha: 0.6,
            compositing_strategy: CompositingStrategy::ModulateAlpha,
            ..GraphicsLayer::default()
        },
        content(Color(0.8, 0.3, 0.3, 1.0)),
    );
    let tinted = layer(
        bounds(96),
        Point::new(112.0, 8.0),
        GraphicsLayer {
            color_filter: Some(ColorFilter::Tint(Color(0.9, 0.7, 0.2, 0.9))),
            compositing_strategy: CompositingStrategy::ModulateAlpha,
            ..GraphicsLayer::default()
        },
        content(Color(0.3, 0.3, 0.8, 1.0)),
    );
    let matrix = layer(
        bounds(96),
        Point::new(8.0, 120.0),
        GraphicsLayer {
            alpha: 0.85,
            color_filter: Some(ColorFilter::Matrix([
                0.4, 0.4, 0.2, 0.0, 0.05, 0.2, 0.6, 0.2, 0.0, 0.0, 0.1, 0.3, 0.6, 0.0, 0.1, 0.0,
                0.0, 0.0, 1.0, 0.0,
            ])),
            compositing_strategy: CompositingStrategy::ModulateAlpha,
            ..GraphicsLayer::default()
        },
        content(Color(0.3, 0.8, 0.3, 1.0)),
    );
    let isolated = layer(
        bounds(96),
        Point::new(112.0, 120.0),
        GraphicsLayer {
            alpha: 0.5,
            ..GraphicsLayer::default()
        },
        content(Color(0.9, 0.9, 0.2, 1.0)),
    );
    root(
        SIZE,
        vec![
            support::solid_rect(bounds(SIZE), Color(0.05, 0.05, 0.12, 1.0)),
            RenderNode::Layer(Box::new(modulated)),
            RenderNode::Layer(Box::new(tinted)),
            RenderNode::Layer(Box::new(matrix)),
            RenderNode::Layer(Box::new(isolated)),
        ],
    )
}

fn blend_modes() -> RenderGraph {
    let mut scope = scope(SIZE);
    scope.draw_rect_at(
        bounds(SIZE),
        Brush::linear_gradient(vec![Color(0.1, 0.2, 0.4, 1.0), Color(0.6, 0.2, 0.1, 1.0)]),
    );
    for (index, mode) in [
        BlendMode::Plus,
        BlendMode::Screen,
        BlendMode::Multiply,
        BlendMode::DstOut,
        BlendMode::SrcOver,
        BlendMode::Xor,
    ]
    .into_iter()
    .enumerate()
    {
        let x = 16.0 + (index % 3) as f32 * 80.0;
        let y = 20.0 + (index / 3) as f32 * 110.0;
        scope.draw_rect_at_blend(
            Rect {
                x,
                y,
                width: 60.0,
                height: 40.0,
            },
            Brush::solid(Color(0.7, 0.8, 0.3, 0.8)),
            mode,
        );
        scope.draw_annular_sector_blend(
            Brush::solid(Color(0.9, 0.3, 0.6, 0.9)),
            Point::new(x + 30.0, y + 75.0),
            10.0,
            24.0,
            0.4,
            4.5,
            mode,
        );
    }
    root(SIZE, vec![run_node(scope.into_primitives())])
}

fn shadows() -> RenderGraph {
    let card = |rect: Rect| DrawPrimitive::RoundRect {
        rect,
        brush: Brush::solid(Color(0.0, 0.0, 0.0, 0.55)),
        radii: CornerRadii::uniform(12.0),
        stroke: None,
    };
    let drop = DrawPrimitive::Shadow(ShadowPrimitive::Drop {
        shape: Box::new(card(Rect {
            x: 30.0,
            y: 30.0,
            width: 90.0,
            height: 60.0,
        })),
        cutout: None,
        blur_radius: 6.0,
        blend_mode: BlendMode::SrcOver,
    });
    let inner = DrawPrimitive::Shadow(ShadowPrimitive::Inner {
        fill: Box::new(card(Rect {
            x: 140.0,
            y: 40.0,
            width: 90.0,
            height: 60.0,
        })),
        cutout: Box::new(DrawPrimitive::RoundRect {
            rect: Rect {
                x: 146.0,
                y: 46.0,
                width: 78.0,
                height: 48.0,
            },
            brush: Brush::solid(Color::BLACK),
            radii: CornerRadii::uniform(9.0),
            stroke: None,
        }),
        blur_radius: 4.0,
        blend_mode: BlendMode::SrcOver,
        clip_rect: Rect {
            x: 140.0,
            y: 40.0,
            width: 90.0,
            height: 60.0,
        },
    });
    root(
        SIZE,
        vec![
            support::solid_rect(bounds(SIZE), Color(0.92, 0.92, 0.95, 1.0)),
            draw_node(drop, None),
            support::solid_rect(
                Rect {
                    x: 30.0,
                    y: 30.0,
                    width: 90.0,
                    height: 60.0,
                },
                Color::WHITE,
            ),
            draw_node(inner, None),
        ],
    )
}

fn capture(
    renderer: &mut support::LockedRenderer,
    graph: &RenderGraph,
    size: u32,
    scale: f32,
) -> Vec<u8> {
    let mut last = Vec::new();
    for _ in 0..2 {
        renderer.scene_mut().graph = Some(graph.clone());
        let captured = renderer
            .capture_frame_with_scale(size, size, scale)
            .unwrap_or_else(|err| panic!("capture failed: {err:?}"));
        assert_eq!((captured.width, captured.height), (size, size));
        last = captured.pixels;
    }
    last
}

fn check(name: &str, graph: RenderGraph, size: u32, scale: f32) {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping record path golden {name}: headless WGPU init failed: {err}");
            return;
        }
    };
    let pixels = capture(&mut renderer, &graph, size, scale);
    if let Some(directory) = std::env::var_os("CRANPOSE_RECORD_CAPTURE_DIR") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("capture output directory");
        write_png(&directory.join(format!("{name}.png")), size, size, &pixels);
    }
    assert!(
        support::distinct_colors(&pixels) > 8,
        "{name}: the scene must draw something"
    );
    let path = fixture_path(name);
    if std::env::var_os(WRITE_ENV).is_some() {
        write_png(&path, size, size, &pixels);
        eprintln!("{name}: wrote {}", path.display());
        return;
    }
    let (width, height, golden) = read_png(&path);
    assert_eq!((width, height), (size, size), "{name}: golden size");
    let mut small = 0usize;
    let mut large = 0usize;
    let mut worst = 0u8;
    for (a, b) in pixels.iter().zip(&golden) {
        let diff = a.abs_diff(*b);
        worst = worst.max(diff);
        if diff > SMALL_DIFF {
            large += 1;
        } else if diff > 0 {
            small += 1;
        }
    }
    let small_fraction = small as f64 / pixels.len() as f64;
    eprintln!("{name}: small {small} ({small_fraction:.4}) large {large} worst {worst}");
    assert!(
        large == 0 && small_fraction <= MAX_SMALL_DIFF_FRACTION,
        "{name}: {large} bytes differ by more than {SMALL_DIFF} (worst {worst}) and {small} by less \
         ({small_fraction:.4} of the image, bound {MAX_SMALL_DIFF_FRACTION}); the record path must \
         draw what the renderer drew before it"
    );
}

#[test]
fn arena_run_matches_the_golden() {
    check("arena_run", arena_run(), SIZE, 1.0);
}

#[test]
fn clipped_primitives_match_the_golden() {
    check("clipped_primitives", clipped_primitives(), SIZE, 1.0);
}

#[test]
fn translated_thin_shapes_match_the_golden() {
    check("translated_thin_shapes", translated_thin_shapes(), 128, 1.0);
}

#[test]
fn painted_layers_match_the_golden() {
    check("painted_layers", painted_layers(), SIZE, 1.0);
}

#[test]
fn blend_modes_match_the_golden() {
    check("blend_modes", blend_modes(), SIZE, 1.0);
}

#[test]
fn shadows_match_the_golden() {
    check("shadows", shadows(), SIZE, 1.0);
}

#[test]
fn arena_run_at_a_fractional_root_scale_matches_the_golden() {
    check("arena_run_scaled", arena_run(), SIZE, 130.0 / 96.0);
}

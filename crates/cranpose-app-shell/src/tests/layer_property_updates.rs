use super::*;

type LayerCanvasState = (MutableState<f32>, MutableState<Color>, NodeId);

thread_local! {
    static LAYER_CANVAS_STATE: RefCell<Option<LayerCanvasState>> = const { RefCell::new(None) };
    static LAYER_CANVAS_DRAWS: Cell<usize> = const { Cell::new(0) };
}

const FIRST_COLOR: Color = Color(0.9, 0.1, 0.2, 1.0);
const SECOND_COLOR: Color = Color(0.1, 0.7, 0.3, 1.0);

#[composable]
fn LayerPropertyCanvas() {
    let rotation = rememberMutableStateOf(|| 0.0_f32);
    let color = rememberMutableStateOf(|| FIRST_COLOR);
    let layer_rotation = rotation;
    let draw_color = color;
    let node_id = cranpose_ui::Canvas(
        Modifier::empty()
            .size_points(80.0, 40.0)
            .graphics_layer(move || GraphicsLayer {
                rotation_z: layer_rotation.get(),
                ..GraphicsLayer::default()
            }),
        move |scope| {
            LAYER_CANVAS_DRAWS.with(|draws| draws.set(draws.get() + 1));
            scope.draw_rect(Brush::solid(draw_color.get()));
        },
    );
    LAYER_CANVAS_STATE.with(|slot| *slot.borrow_mut() = Some((rotation, color, node_id)));
}

fn canvas_layer_position(
    layer: &cranpose_render_common::graph::LayerNode,
    node_id: NodeId,
) -> Option<Point> {
    if layer.node_id == Some(node_id) {
        return Some(layer.transform_to_parent.map_point(Point::default()));
    }
    layer.children.iter().find_map(|child| match child {
        cranpose_render_common::graph::RenderNode::Layer(child) => {
            canvas_layer_position(child, node_id)
        }
        cranpose_render_common::graph::RenderNode::Primitive(_)
        | cranpose_render_common::graph::RenderNode::DrawRun(_) => None,
    })
}

fn canvas_rect_colors(
    layer: &cranpose_render_common::graph::LayerNode,
    node_id: NodeId,
    colors: &mut Vec<Color>,
) {
    for child in &layer.children {
        match child {
            cranpose_render_common::graph::RenderNode::Layer(child) => {
                canvas_rect_colors(child, node_id, colors);
            }
            cranpose_render_common::graph::RenderNode::DrawRun(run)
                if run
                    .command
                    .is_some_and(|command| command.node_id == node_id) =>
            {
                colors.extend(run.primitives().filter_map(|primitive| match primitive {
                    DrawPrimitive::Rect {
                        brush: Brush::Solid(color),
                        ..
                    } => Some(color),
                    _ => None,
                }));
            }
            cranpose_render_common::graph::RenderNode::Primitive(_)
            | cranpose_render_common::graph::RenderNode::DrawRun(_) => {}
        }
    }
}

fn canvas_output(
    shell: &AppShell<ScopedUpdateCountingRenderer>,
    node_id: NodeId,
) -> (Point, Vec<Color>) {
    let graph = shell.surfaces[0]
        .renderer
        .scene()
        .graph
        .as_ref()
        .expect("the initial frame builds a graph");
    let position =
        canvas_layer_position(&graph.root, node_id).expect("Canvas remains in the retained graph");
    let mut colors = Vec::new();
    canvas_rect_colors(&graph.root, node_id, &mut colors);
    (position, colors)
}

#[test]
fn layer_only_changes_keep_canvas_recording_until_its_own_state_changes() {
    let _guard = test_guard();
    LAYER_CANVAS_DRAWS.with(|draws| draws.set(0));
    LAYER_CANVAS_STATE.with(|slot| *slot.borrow_mut() = None);

    let mut shell = AppShell::new_with_size(
        ScopedUpdateCountingRenderer::new(
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
            Rc::new(RefCell::new(Vec::new())),
        ),
        location_key(file!(), line!(), column!()),
        LayerPropertyCanvas,
        (320, 240),
        (320.0, 240.0),
    );
    shell.update();
    shell.update();

    let (rotation, color, node_id) = LAYER_CANVAS_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("Canvas publishes both independent state values");
    let (initial_position, initial_colors) = canvas_output(&shell, node_id);
    assert_eq!(initial_colors, vec![FIRST_COLOR]);
    let initial_draws = LAYER_CANVAS_DRAWS.with(Cell::get);
    assert!(initial_draws > 0, "the Canvas draw closure ran");

    rotation.set_value(90.0);
    shell.update();
    let (rotated_position, rotated_colors) = canvas_output(&shell, node_id);
    assert_ne!(
        rotated_position, initial_position,
        "rotation changes painted position"
    );
    assert_eq!(rotated_colors, initial_colors);
    assert_eq!(
        LAYER_CANVAS_DRAWS.with(Cell::get),
        initial_draws,
        "a graphics-layer-only update should retain Canvas output"
    );

    color.set_value(SECOND_COLOR);
    shell.update();
    let (_, recolored) = canvas_output(&shell, node_id);
    assert_eq!(recolored, vec![SECOND_COLOR]);
    let recolored_draws = LAYER_CANVAS_DRAWS.with(Cell::get);
    assert!(
        recolored_draws > initial_draws,
        "Canvas state change re-records it"
    );

    rotation.set_value(180.0);
    color.set_value(FIRST_COLOR);
    shell.update();
    let (combined_position, combined_colors) = canvas_output(&shell, node_id);
    assert_ne!(combined_position, rotated_position);
    assert_eq!(combined_colors, vec![FIRST_COLOR]);
    assert!(
        LAYER_CANVAS_DRAWS.with(Cell::get) > recolored_draws,
        "simultaneous layer and Canvas changes still re-record content"
    );
}

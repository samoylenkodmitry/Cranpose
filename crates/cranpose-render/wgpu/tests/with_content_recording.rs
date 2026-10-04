use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_render_common::{
    Renderer,
    graph::{DrawRunNode, PrimitivePhase, RenderGraph, RenderNode},
};
use cranpose_render_wgpu::{CapturedFrame, WgpuRenderer};
use cranpose_ui::{
    Alignment, Canvas, Color, Modifier, Size, composable,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, DrawScope, DrawScopeDefault, Rect};

use crate::support;

const WIDTH: u32 = 48;
const HEIGHT: u32 = 40;
const BEFORE_INITIAL: Color = Color(0.9, 0.15, 0.1, 0.48);
const BEFORE_UPDATED: Color = Color(0.95, 0.65, 0.05, 0.36);
const CHILD_COLOR: Color = Color(0.1, 0.85, 0.3, 0.58);
const AFTER_INITIAL: Color = Color(0.1, 0.2, 0.95, 0.52);
const AFTER_UPDATED: Color = Color(0.8, 0.1, 0.75, 0.44);

#[composable]
fn WithContentPage(
    before: Rc<RefCell<Option<MutableState<Color>>>>,
    after: Rc<RefCell<Option<MutableState<Color>>>>,
) {
    let before_state = rememberMutableStateOf(|| BEFORE_INITIAL);
    let after_state = rememberMutableStateOf(|| AFTER_INITIAL);
    *before.borrow_mut() = Some(before_state);
    *after.borrow_mut() = Some(after_state);

    Box(
        Modifier::empty()
            .size(Size::new(WIDTH as f32, HEIGHT as f32))
            .draw_with_content(move |scope| {
                scope.draw_rect_at(
                    Rect {
                        x: 4.0,
                        y: 4.0,
                        width: 40.0,
                        height: 32.0,
                    },
                    Brush::solid(before_state.get()),
                );
                scope.draw_content();
                scope.draw_rect_at(
                    Rect {
                        x: 16.0,
                        y: 6.0,
                        width: 28.0,
                        height: 28.0,
                    },
                    Brush::solid(after_state.get()),
                );
            }),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        || {
            Canvas(
                Modifier::empty().size(Size::new(WIDTH as f32, HEIGHT as f32)),
                |scope| {
                    scope.draw_rect_at(
                        Rect {
                            x: 10.0,
                            y: 10.0,
                            width: 28.0,
                            height: 20.0,
                        },
                        Brush::solid(CHILD_COLOR),
                    );
                },
            );
        },
    );
}

fn draw_layer(color: Color, rect: Rect) -> RenderNode {
    let mut scope = DrawScopeDefault::new(Size::new(WIDTH as f32, HEIGHT as f32));
    scope.draw_rect_at(rect, Brush::solid(color));
    RenderNode::Layer(Box::new(support::layer_node(
        None,
        WIDTH as f32,
        HEIGHT as f32,
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            scope.into_primitives(),
        ))],
    )))
}

fn explicitly_ordered_graph(before: Color, after: Color) -> RenderGraph {
    RenderGraph::new(support::layer_node(
        None,
        WIDTH as f32,
        HEIGHT as f32,
        vec![
            draw_layer(
                before,
                Rect {
                    x: 4.0,
                    y: 4.0,
                    width: 40.0,
                    height: 32.0,
                },
            ),
            draw_layer(
                CHILD_COLOR,
                Rect {
                    x: 10.0,
                    y: 10.0,
                    width: 28.0,
                    height: 20.0,
                },
            ),
            draw_layer(
                after,
                Rect {
                    x: 16.0,
                    y: 6.0,
                    width: 28.0,
                    height: 28.0,
                },
            ),
        ],
    ))
}

fn settled_capture(shell: &mut AppShell<WgpuRenderer>) -> CapturedFrame {
    support::settle(|| {
        let renderer = shell.renderer();
        let frame = renderer
            .capture_frame(WIDTH, HEIGHT)
            .expect("WGPU frame capture succeeds");
        let stats = renderer
            .last_frame_stats()
            .expect("frame stats are available");
        (stats, frame)
    })
}

fn capture_reference_graph(
    shell: &mut AppShell<WgpuRenderer>,
    graph: RenderGraph,
) -> CapturedFrame {
    let original = shell.renderer().scene_mut().graph.replace(graph);
    let captured = settled_capture(shell);
    shell.renderer().scene_mut().graph = original;
    captured
}

#[test]
fn with_content_matches_ordered_translucent_draws_before_and_after_child_content() {
    let (lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return;
        }
    };
    let _lock = lock;
    let before = Rc::new(RefCell::new(None));
    let after = Rc::new(RefCell::new(None));
    let before_for_app = Rc::clone(&before);
    let after_for_app = Rc::clone(&after);
    let mut shell = AppShell::new(
        renderer,
        location_key(file!(), line!(), column!()),
        move || {
            WithContentPage(before_for_app.clone(), after_for_app.clone());
        },
    );
    shell.set_viewport(WIDTH as f32, HEIGHT as f32);
    shell.set_buffer_size(WIDTH, HEIGHT);
    shell.update();

    let initial = settled_capture(&mut shell);
    let initial_expected = capture_reference_graph(
        &mut shell,
        explicitly_ordered_graph(BEFORE_INITIAL, AFTER_INITIAL),
    );
    support::assert_same_bytes(
        "initial WithContent vs explicit background/child/foreground",
        WIDTH,
        &initial_expected.pixels,
        &initial.pixels,
    );

    let before_state = before
        .borrow()
        .as_ref()
        .copied()
        .expect("before color state was composed");
    let after_state = after
        .borrow()
        .as_ref()
        .copied()
        .expect("after color state was composed");
    shell.debug_enter_app_context(|| {
        before_state.set(BEFORE_UPDATED);
        after_state.set(AFTER_UPDATED);
    });
    shell.update();

    let updated = settled_capture(&mut shell);
    let updated_expected = capture_reference_graph(
        &mut shell,
        explicitly_ordered_graph(BEFORE_UPDATED, AFTER_UPDATED),
    );
    support::assert_same_bytes(
        "updated WithContent vs explicit background/child/foreground",
        WIDTH,
        &updated_expected.pixels,
        &updated.pixels,
    );
    assert_ne!(
        initial.pixels, updated.pixels,
        "dirty colors change rendered pixels"
    );
}

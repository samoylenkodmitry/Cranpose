use super::*;
use cranpose_core::{
    __launched_effect_async_impl as launched_effect_async_impl, location_key, useState,
};
use cranpose_macros::composable;
use cranpose_ui::{
    Box, BoxSpec, Brush, Color, Column, ColumnSpec, HeadlessRenderer, Modifier, Rect, RenderOp,
    Row, RowSpec, Size, Text,
};
use cranpose_ui_graphics::DrawPrimitive;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Default, Clone)]
struct TestHitTarget;

impl HitTestTarget for TestHitTarget {
    fn dispatch(&self, _event: PointerEvent) {}

    fn node_id(&self) -> cranpose_core::NodeId {
        0
    }
}

#[derive(Default)]
struct TestScene;

impl RenderScene for TestScene {
    type HitTarget = TestHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Vec<Self::HitTarget> {
        vec![]
    }

    fn find_target(&self, _node_id: cranpose_core::NodeId) -> Option<Self::HitTarget> {
        None
    }
}

#[derive(Default)]
struct TestRenderer {
    scene: TestScene,
}

impl Renderer for TestRenderer {
    type Scene = TestScene;
    type Error = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        _layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        _applier: &mut cranpose_core::MemoryApplier,
        _root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingRenderer {
    scene: TestScene,
    last_scene: Option<cranpose_ui::RecordedRenderScene>,
}

impl Renderer for RecordingRenderer {
    type Scene = TestScene;
    type Error = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        let renderer = HeadlessRenderer::new();
        self.last_scene = Some(renderer.render(layout_tree));
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        let renderer = HeadlessRenderer::new();
        self.last_scene = Some(renderer.render_from_applier(applier, root));
        Ok(())
    }
}

#[composable]
fn tabbed_progress_content() {
    let progress = useState(|| 0.6f32);
    let active_tab = useState(|| 0i32);

    let progress_effect = progress;
    let active_effect = active_tab;
    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        (),
        move |scope| {
            let progress = progress_effect;
            let active_tab = active_effect;
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                let mut phase: u32 = 0;
                while scope.is_active() {
                    let _ = clock.next_frame().await;
                    if !scope.is_active() {
                        break;
                    }
                    match phase % 3 {
                        0 => {
                            progress.set_value(0.0);
                            active_tab.set_value(1);
                        }
                        1 => {
                            progress.set_value(0.85);
                        }
                        _ => {
                            active_tab.set_value(0);
                        }
                    }
                    phase = phase.wrapping_add(1);
                }
            })
        },
    );

    Column(
        Modifier::empty().padding(8.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Progress {:.2}", progress.value()),
                Modifier::empty().padding(2.0),
            );
            let progress_for_branch = progress;
            let active_for_branch = active_tab;
            Row(
                Modifier::empty()
                    .padding(2.0)
                    .then(Modifier::empty().height(12.0)),
                RowSpec::default(),
                move || {
                    if active_for_branch.value() == 0 && progress_for_branch.value() > 0.0 {
                        let progress_for_bar = progress_for_branch;
                        Row(
                            Modifier::empty()
                                .width(160.0 * progress_for_bar.value())
                                .then(Modifier::empty().height(12.0)),
                            RowSpec::default(),
                            move || {
                                let _ = progress_for_bar.value();
                            },
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn empty_content() {}

#[composable]
fn draw_width_app(width_state: cranpose_core::MutableState<f32>) {
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 40.0,
            })
            .draw_behind({
                let width = width_state.get();
                move |scope| {
                    scope.draw_rect_at(
                        Rect {
                            x: 0.0,
                            y: 0.0,
                            width,
                            height: 10.0,
                        },
                        Brush::solid(Color(0.9, 0.1, 0.1, 1.0)),
                    );
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

struct DeleteSurroundingHandler {
    last_delete: Cell<Option<(usize, usize)>>,
}

impl cranpose_ui::text_field_focus::FocusedTextFieldHandler for DeleteSurroundingHandler {
    fn handle_key(&self, _event: &cranpose_ui::KeyEvent) -> bool {
        false
    }

    fn insert_text(&self, _text: &str) {}

    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
        self.last_delete.set(Some((before_bytes, after_bytes)));
    }

    fn copy_selection(&self) -> Option<String> {
        None
    }

    fn cut_selection(&self) -> Option<String> {
        None
    }

    fn set_composition(&self, _text: &str, _cursor: Option<(usize, usize)>) {}
}

#[test]
fn layout_recovers_after_tab_switching_updates() {
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        tabbed_progress_content()
    });

    for frame in 0..200 {
        shell.update();
        assert!(
            shell.layout_tree.is_some(),
            "layout_tree should remain available after update cycle {frame}"
        );
    }
}

#[test]
fn ime_delete_surrounding_marks_dirty() {
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.update();
    assert!(!shell.needs_redraw());

    let focus_flag = Rc::new(RefCell::new(false));
    let handler = Rc::new(DeleteSurroundingHandler {
        last_delete: Cell::new(None),
    });

    cranpose_ui::text_field_focus::request_focus(focus_flag, handler.clone());
    assert!(shell.on_ime_delete_surrounding(2, 1));
    assert_eq!(handler.last_delete.get(), Some((2, 1)));
    assert!(shell.needs_redraw());
    cranpose_ui::text_field_focus::clear_focus();
}

#[test]
fn draw_repass_updates_render_data_without_layout() {
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);

    let mut shell = AppShell::new(RecordingRenderer::default(), root_key, move || {
        let width_state = useState(|| 24.0f32);
        *state_holder_for_app.borrow_mut() = Some(width_state);
        draw_width_app(width_state);
    });

    shell.update();
    let initial_scene = shell
        .renderer
        .last_scene
        .as_ref()
        .expect("expected initial render scene");
    let initial_width = find_rect_width(initial_scene, Color(0.9, 0.1, 0.1, 1.0))
        .expect("expected initial draw rect");

    let width_state = state_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("width state should be captured");
    width_state.set(120.0);

    shell
        .composition
        .process_invalid_scopes()
        .expect("recompose after width change");
    shell.run_render_phase();

    let updated_scene = shell
        .renderer
        .last_scene
        .as_ref()
        .expect("expected updated render scene");
    let updated_width = find_rect_width(updated_scene, Color(0.9, 0.1, 0.1, 1.0))
        .expect("expected updated draw rect");

    assert_ne!(initial_width, updated_width, "draw width should update");
    assert!(
        (updated_width - 120.0).abs() < 0.1,
        "updated width should reflect latest state"
    );
}

fn find_rect_width(scene: &cranpose_ui::RecordedRenderScene, color: Color) -> Option<f32> {
    for op in scene.operations() {
        if let RenderOp::Primitive {
            primitive: DrawPrimitive::Rect { rect, brush },
            ..
        } = op
        {
            if *brush == Brush::solid(color) {
                return Some(rect.width);
            }
        }
    }
    None
}

use super::*;
use compose_core::{
    __launched_effect_async_impl as launched_effect_async_impl, location_key, useState,
};
use compose_macros::composable;
use compose_ui::{Column, ColumnSpec, Modifier, Row, RowSpec, Text};

#[derive(Default)]
struct TestHitTarget;

impl HitTestTarget for TestHitTarget {
    fn dispatch(&self, _kind: PointerEventKind, _x: f32, _y: f32) {}
}

#[derive(Default)]
struct TestScene;

impl RenderScene for TestScene {
    type HitTarget = TestHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Option<Self::HitTarget> {
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
}

#[composable]
fn tabbed_progress_content() {
    let progress = useState(|| 0.6f32);
    let active_tab = useState(|| 0i32);

    let progress_effect = progress.clone();
    let active_effect = active_tab.clone();
    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        (),
        move |scope| {
            let progress = progress_effect.clone();
            let active_tab = active_effect.clone();
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
            let progress_for_branch = progress.clone();
            let active_for_branch = active_tab.clone();
            Row(
                Modifier::empty()
                    .padding(2.0)
                    .then(Modifier::empty().height(12.0)),
                RowSpec::default(),
                move || {
                    if active_for_branch.value() == 0 && progress_for_branch.value() > 0.0 {
                        let progress_for_bar = progress_for_branch.clone();
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

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, remember, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_common::{RenderScene, Renderer, graph_scene::Scene};
use cranpose_ui::{BoxWithConstraints, Column, ColumnSpec, Modifier, ScrollState, Text, TextStyle};
use cranpose_ui_graphics::Size;

#[derive(Default)]
struct EmptyRenderer(Scene);

impl Renderer for EmptyRenderer {
    type Scene = Scene;
    type Error = std::convert::Infallible;

    fn scene(&self) -> &Self::Scene {
        &self.0
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.0
    }

    fn rebuild_scene(
        &mut self,
        _layout_tree: &cranpose_ui::LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.0.clear();
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        _applier: &mut cranpose_core::MemoryApplier,
        _root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.0.clear();
        Ok(())
    }
}

/// Content as a table in a guide draws it: it remembers a scroll state and
/// a subcomposed child scrolls with it.
#[composable]
fn ScrollingRow(index: usize) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    BoxWithConstraints(Modifier::empty().fill_max_width(), move |_| {
        Column(
            Modifier::empty()
                .fill_max_width()
                .horizontal_scroll(scroll, false),
            ColumnSpec::default(),
            move || {
                Text(
                    format!("Row {index}"),
                    Modifier::empty().size_points(600.0, 40.0),
                    TextStyle::default(),
                );
            },
        );
    });
}

/// A subcomposition that remembers a scroll state and drops the content
/// scrolling with it while it is measured.
#[composable]
fn ToggledScroller(shown: Rc<Cell<Option<MutableState<bool>>>>) {
    let visible = rememberMutableStateOf(|| true);
    shown.set(Some(visible));
    BoxWithConstraints(Modifier::empty().fill_max_size(), move |_| {
        if visible.get() {
            ScrollingRow(0);
        }
    });
}

/// The state a dropped group remembered stays readable until the nodes the
/// group emitted are detached: the scroll modifier node reads it as it
/// detaches. A subcomposition measured outside an update, as an idle prefetch
/// pass or a resize measures one, panicked here.
#[test]
fn a_layout_outside_an_update_drops_a_scrolling_subcomposition() {
    let shown = Rc::new(Cell::new(None));
    let mut shell = AppShell::new_with_size(
        EmptyRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let shown = Rc::clone(&shown);
            move || ToggledScroller(Rc::clone(&shown))
        },
        (320, 200),
        (320.0, 200.0),
    );
    shell.update();
    shown.get().expect("the scroller is composed").set(false);
    // Lays out at once, outside an update.
    shell.set_viewport(300.0, 200.0);
    shell.update();
}

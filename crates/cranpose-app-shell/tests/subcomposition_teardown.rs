use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{
    MutableState, location_key, remember, rememberMovableContentOf, rememberMutableStateOf,
};
use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_liquid::{LiquidContextMenu, LiquidShape, LiquidTheme, LiquidThemeSpec, SchemeMode};
use cranpose_macros::composable;
use cranpose_render_common::{RenderScene, Renderer, graph_scene::Scene};
use cranpose_ui::{
    Box, BoxSpec, BoxWithConstraints, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Modifier,
    ScrollState, SemanticsNode, Text, TextStyle,
};
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

fn collect_labels(node: &SemanticsNode, out: &mut Vec<String>) {
    if let Some(label) = node.accessibility_label() {
        out.push(label.into_owned());
    }
    for child in &node.children {
        collect_labels(child, out);
    }
}

fn visible_labels(shell: &mut AppShell<EmptyRenderer>) -> Vec<String> {
    shell.set_semantics_enabled(true);
    let mut labels = Vec::new();
    if let Some(tree) = shell.semantics_tree() {
        collect_labels(tree.root(), &mut labels);
    }
    labels
}

#[composable]
fn ScrollableTextColumn(label: String, scroll: ScrollState) {
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        move || {
            Text(
                label.clone(),
                Modifier::empty().size_points(280.0, 200.0),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn MovableScrollableRow(index: usize, show_content: Option<MutableState<bool>>) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    let label = match show_content {
        Some(_) => format!("Hidden movable row {index}"),
        None => format!("Movable row {index}"),
    };
    let content = rememberMovableContentOf(move || {
        ScrollableTextColumn(label.clone(), scroll);
    });
    Box(
        Modifier::empty().size_points(300.0, 30.0),
        BoxSpec::default(),
        move || {
            let content = content.clone();
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec::default(),
                move || {
                    if show_content.is_none_or(|state| state.get()) {
                        content.show();
                    }
                },
            );
        },
    );
}

#[composable]
fn ContextMenuScrollableRow(index: usize) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    LiquidContextMenu(
        Modifier::empty().size_points(300.0, 30.0),
        LiquidShape::RoundedRect(12.0),
        "Actions",
        |_| {},
        move || {
            ScrollableTextColumn(format!("Menu row {index}"), scroll);
        },
    );
}

#[composable]
fn IdleLazyList(list_handle: Rc<Cell<Option<LazyListState>>>, row: impl Fn(usize) + 'static) {
    let list = rememberLazyListState();
    list_handle.set(Some(list));
    let row: Rc<dyn Fn(usize)> = Rc::new(row);
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::new(),
        move |scope| {
            let row = Rc::clone(&row);
            scope.items(80, move |index| row(index));
        },
    );
}

fn plain_row(index: usize) {
    Text(
        format!("Plain row {index}"),
        Modifier::empty().size_points(300.0, 30.0),
        TextStyle::default(),
    );
}

fn idle_lazy_shell(content: impl FnMut() + 'static) -> AppShell<EmptyRenderer> {
    cranpose_ui::set_idle_prefetch_passes(true);
    let mut shell = AppShell::new_with_size(
        EmptyRenderer::default(),
        location_key(file!(), line!(), column!()),
        content,
        (320, 480),
        (320.0, 480.0),
    );
    shell.update();
    while shell.run_idle_prefetch(None, |_| false) {}
    shell
}

fn scroll_forward_to_rebind(shell: &mut AppShell<EmptyRenderer>, list: LazyListState) {
    for _ in 0..20 {
        let _consumed: f32 = list.dispatch_scroll_delta(-35.0);
        shell.update();
        shell.run_idle_prefetch(None, |_| false);
    }
}

fn expire_pooled_slots(shell: &mut AppShell<EmptyRenderer>, list: LazyListState) {
    // Step far down for more than PASSES_KEPT passes so the reuse pool expires
    // and drops pooled slot hosts.
    for step in 0..20 {
        list.scroll_to_item(40 + step, 0.0);
        shell.update();
        while shell.run_idle_prefetch(None, |_| false) {}
    }
}

fn return_to_top_and_assert_label(
    shell: &mut AppShell<EmptyRenderer>,
    list: LazyListState,
    expected: &str,
) {
    list.scroll_to_item(0, 0.0);
    shell.update();
    while shell.run_idle_prefetch(None, |_| false) {}
    assert!(
        visible_labels(shell).iter().any(|label| label == expected),
        "expected {expected:?} after returning to the top"
    );
}

#[test]
fn movable_content_in_a_lazy_item_survives_disposal_and_return_under_idle_prefetch() {
    let list_handle = Rc::new(Cell::new(None));
    let mut shell = idle_lazy_shell({
        let list_handle = Rc::clone(&list_handle);
        move || {
            IdleLazyList(Rc::clone(&list_handle), |index| {
                if index % 4 == 0 {
                    MovableScrollableRow(index, None);
                } else {
                    plain_row(index);
                }
            });
        }
    });
    assert!(
        visible_labels(&mut shell)
            .iter()
            .any(|label| label == "Movable row 0")
    );
    let list = list_handle.get().expect("the lazy list is composed");
    scroll_forward_to_rebind(&mut shell, list);
    expire_pooled_slots(&mut shell, list);
    return_to_top_and_assert_label(&mut shell, list, "Movable row 0");
}

#[test]
fn liquid_context_menu_in_a_lazy_item_survives_disposal_and_return_under_idle_prefetch() {
    let list_handle = Rc::new(Cell::new(None));
    let mut shell = idle_lazy_shell({
        let list_handle = Rc::clone(&list_handle);
        move || {
            let list_handle = Rc::clone(&list_handle);
            LiquidTheme(LiquidThemeSpec::system(SchemeMode::Light), move || {
                IdleLazyList(Rc::clone(&list_handle), |index| {
                    if index % 4 == 0 {
                        ContextMenuScrollableRow(index);
                    } else {
                        plain_row(index);
                    }
                });
            });
        }
    });
    let list = list_handle.get().expect("the lazy list is composed");
    scroll_forward_to_rebind(&mut shell, list);
    expire_pooled_slots(&mut shell, list);
    return_to_top_and_assert_label(&mut shell, list, "Menu row 0");
}

#[test]
fn hidden_movable_content_in_an_evicted_lazy_item_unmounts_before_its_scroll_state_drops() {
    let list_handle = Rc::new(Cell::new(None));
    let show_handle = Rc::new(Cell::new(None));
    let mut shell = idle_lazy_shell({
        let list_handle = Rc::clone(&list_handle);
        let show_handle = Rc::clone(&show_handle);
        move || {
            let show = rememberMutableStateOf(|| true);
            show_handle.set(Some(show));
            IdleLazyList(Rc::clone(&list_handle), move |index| {
                if index == 0 {
                    MovableScrollableRow(index, Some(show));
                } else {
                    plain_row(index);
                }
            });
        }
    });
    // Detach the movable into the row's own retention map while the row stays
    // active, then jump away so the row's SlotsHost is evicted directly.
    let show = show_handle.get().expect("show state");
    show.set(false);
    shell.update();
    let list = list_handle.get().expect("the lazy list is composed");
    expire_pooled_slots(&mut shell, list);
    show.set(true);
    return_to_top_and_assert_label(&mut shell, list, "Hidden movable row 0");
}

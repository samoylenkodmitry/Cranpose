use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_macros::composable;
use cranpose_render_common::{RenderScene, Renderer, graph_scene::Scene};
use cranpose_ui::{
    Box, BoxSpec, LazyColumn, LazyColumnSpec, Modifier, SemanticsNode, Text, TextStyle,
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

#[derive(Clone, Default)]
struct Compositions(Rc<RefCell<HashMap<usize, usize>>>);

impl PartialEq for Compositions {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(PartialEq)]
struct Handles {
    list: LazyListState,
    frame: MutableState<u32>,
}

#[composable]
fn WidthAnimatedList(handles: Rc<RefCell<Option<Handles>>>, compositions: Compositions) {
    let list = rememberLazyListState();
    let frame = rememberMutableStateOf(|| 0u32);
    *handles.borrow_mut() = Some(Handles { list, frame });
    let width = 300.0 + (frame.get() % 3) as f32 * 10.0;
    Box(
        Modifier::empty().size_points(width, 120.0),
        BoxSpec::default(),
        move || Rows(list, compositions.clone()),
    );
}

#[composable]
fn Rows(list: LazyListState, compositions: Compositions) {
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::new(),
        move |scope| {
            scope.items(100, move |index| {
                let entered = compositions.clone();
                cranpose_core::DisposableEffect((), move |scope| {
                    *entered.0.borrow_mut().entry(index).or_insert(0) += 1;
                    scope.on_dispose(|| {})
                });
                Text(
                    format!("Row {index}"),
                    Modifier::empty().size_points(280.0, 40.0),
                    TextStyle::default(),
                );
            });
        },
    );
}

fn labels(node: &SemanticsNode, out: &mut Vec<String>) {
    if let Some(label) = node.accessibility_label() {
        out.push(label.into_owned());
    }
    for child in &node.children {
        labels(child, out);
    }
}

#[test]
fn items_beyond_a_scrolling_viewport_compose_once_and_stay_unplaced() {
    let handles = Rc::new(RefCell::new(None));
    let compositions = Compositions::default();
    let mut shell = AppShell::new_with_size(
        EmptyRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let handles = Rc::clone(&handles);
            let compositions = compositions.clone();
            move || WidthAnimatedList(Rc::clone(&handles), compositions.clone())
        },
        (320, 120),
        (320.0, 120.0),
    );
    shell.update();
    let (list, frame) = {
        let handles = handles.borrow();
        let handles = handles.as_ref().expect("the list is composed");
        (handles.list, handles.frame)
    };

    for index in 1..=60u32 {
        frame.set(index);
        let _ = list.dispatch_scroll_delta(-7.0);
        shell.update();
        let passes = Cell::new(0);
        shell.run_idle_prefetch(None, |_| {
            passes.set(passes.get() + 1);
            passes.get() > 2
        });
    }

    let recomposed: Vec<_> = compositions
        .0
        .borrow()
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(index, count)| (*index, *count))
        .collect();
    assert!(
        recomposed.is_empty(),
        "an item composed ahead of the scroll stays composed while new constraints \
         arrive every frame: (index, compositions) {recomposed:?}"
    );

    assert_only_visible_rows_are_placed(&mut shell, list, &compositions);

    for index in 61..=80u32 {
        frame.set(index);
        let _ = list.dispatch_scroll_delta(7.0);
        shell.update();
    }
    assert_only_visible_rows_are_placed(&mut shell, list, &compositions);
}

fn assert_only_visible_rows_are_placed(
    shell: &mut AppShell<EmptyRenderer>,
    list: LazyListState,
    compositions: &Compositions,
) {
    let visible: Vec<_> = list
        .layout_info()
        .visible_items_info
        .iter()
        .map(|item| item.index)
        .collect();
    let first_beyond = visible.last().expect("visible rows") + 1;
    assert!(
        compositions.0.borrow().contains_key(&first_beyond),
        "the row after the viewport is composed ahead of the scroll"
    );
    shell.set_semantics_enabled(true);
    let mut shown = Vec::new();
    labels(
        shell.semantics_tree().expect("semantics").root(),
        &mut shown,
    );
    let rows: Vec<_> = shown
        .into_iter()
        .filter(|label| label.starts_with("Row "))
        .collect();
    let expected: Vec<_> = visible.iter().map(|index| format!("Row {index}")).collect();
    assert_eq!(rows, expected, "only the rows in the viewport are placed");
}

fn composed_rows(compositions: &Compositions) -> Vec<usize> {
    let mut rows: Vec<_> = compositions.0.borrow().keys().copied().collect();
    rows.sort_unstable();
    rows
}

/// A shell showing [`Rows`] in a viewport `height` high, which hands its
/// list's state to `list`.
fn list_shell(
    compositions: &Compositions,
    height: u32,
    list: &Rc<Cell<Option<LazyListState>>>,
) -> AppShell<EmptyRenderer> {
    let compositions = compositions.clone();
    let list = Rc::clone(list);
    AppShell::new_with_size(
        EmptyRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let state = rememberLazyListState();
            list.set(Some(state));
            Rows(state, compositions.clone());
        },
        (320, height),
        (320.0, height as f32),
    )
}

fn resting_list(compositions: &Compositions) -> AppShell<EmptyRenderer> {
    list_shell(compositions, 120, &Rc::default())
}

#[test]
fn a_list_at_rest_leaves_the_items_beyond_its_viewport_to_idle_passes() {
    cranpose_ui::set_idle_prefetch_passes(true);
    let compositions = Compositions::default();
    let mut shell = resting_list(&compositions);
    shell.update();
    assert_eq!(
        composed_rows(&compositions),
        [0, 1, 2],
        "the first frame composes the rows in the viewport"
    );
    assert!(shell.has_idle_prefetch_work());

    while shell.run_idle_prefetch(None, |_| false) {}
    assert_eq!(
        composed_rows(&compositions),
        [0, 1, 2, 3, 4],
        "idle passes compose the two rows after the viewport"
    );
    assert!(!shell.has_idle_prefetch_work());
}

#[test]
fn a_list_at_rest_composes_the_items_beyond_its_viewport_without_idle_passes() {
    let compositions = Compositions::default();
    let mut shell = resting_list(&compositions);
    shell.update();
    assert_eq!(composed_rows(&compositions), [0, 1, 2, 3, 4]);
    assert!(!shell.has_idle_prefetch_work());
}

#[test]
fn a_slowly_scrolling_list_leaves_the_rows_its_scroll_does_not_reach_to_idle_passes() {
    cranpose_ui::set_idle_prefetch_passes(true);
    let compositions = Compositions::default();
    let list = Rc::default();
    // Rows 40 high in a viewport 100 high: row 3 starts 20 past its edge.
    let mut shell = list_shell(&compositions, 100, &list);
    shell.update();
    let list = list.get().expect("the list is composed");
    for _ in 0..3 {
        let _ = list.dispatch_scroll_delta(-3.0);
        shell.update();
    }
    assert_eq!(
        composed_rows(&compositions),
        [0, 1, 2],
        "a scroll of 3 a frame does not reach row 3 within two frames"
    );

    while shell.run_idle_prefetch(None, |_| false) {}
    let prefetched = composed_rows(&compositions);
    assert!(
        prefetched.starts_with(&[0, 1, 2, 3, 4]),
        "idle passes compose the rows ahead: {prefetched:?}"
    );

    for _ in 0..4 {
        let _ = list.dispatch_scroll_delta(-3.0);
        shell.update();
    }
    assert_eq!(
        composed_rows(&compositions),
        prefetched,
        "the frames find the rows ahead composed"
    );
}

#[test]
fn a_fast_scroll_composes_the_rows_it_reaches_in_its_frame() {
    cranpose_ui::set_idle_prefetch_passes(true);
    let compositions = Compositions::default();
    let list = Rc::default();
    let mut shell = list_shell(&compositions, 100, &list);
    shell.update();
    let list = list.get().expect("the list is composed");
    let _ = list.dispatch_scroll_delta(-30.0);
    shell.update();
    // The viewport shows rows 0 to 3 now. Row 4 starts 30 past its edge,
    // within the 60 two frames of the scroll cover; row 5, 70 past it, waits
    // for a pass.
    assert_eq!(composed_rows(&compositions), [0, 1, 2, 3, 4]);
    assert!(shell.has_idle_prefetch_work());
}

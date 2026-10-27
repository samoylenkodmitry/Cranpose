// A lazy list composed alone in a viewport, as the lazy list tests drive it:
// scrolled, laid out, and read back as the texts it draws.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    AppContext, Column, ColumnSpec, HeadlessRenderer, LayoutEngine, LazyColumn, LazyColumnSpec,
    MemoryHeadroom, Modifier, RenderOp, Size, Text, TextStyle, composable, set_memory_headroom,
};

pub(crate) struct LazyScreen {
    pub(crate) context: Rc<AppContext>,
    composition: Composition<MemoryApplier>,
    pub(crate) list: LazyListState,
    viewport: Size,
}

impl LazyScreen {
    // Composes what `content` makes of a remembered list state, and lays its
    // first frame out.
    pub(crate) fn new(viewport: Size, mut content: impl FnMut(LazyListState) + 'static) -> Self {
        let context = AppContext::new();
        let mut composition = Composition::new(MemoryApplier::new());
        let captured = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&captured);
        context
            .enter(|| {
                composition.render(location_key(file!(), line!(), column!()), move || {
                    let list = rememberLazyListState();
                    *capture.borrow_mut() = Some(list);
                    content(list);
                })
            })
            .expect("initial composition");
        let list = captured
            .borrow()
            .as_ref()
            .copied()
            .expect("lazy list state");
        let mut screen = Self {
            context,
            composition,
            list,
            viewport,
        };
        screen.texts();
        screen
    }

    pub(crate) fn set_headroom(&self, headroom: MemoryHeadroom) {
        self.context.enter(|| set_memory_headroom(headroom));
    }

    pub(crate) fn scroll_to(&mut self, index: usize, offset: f32) {
        self.context
            .enter(|| self.list.scroll_to_item(index, offset));
        self.settle();
    }

    pub(crate) fn scroll_by(&mut self, delta: f32) {
        self.context
            .enter(|| self.list.dispatch_scroll_delta(delta));
        self.settle();
    }

    pub(crate) fn settle(&mut self) {
        while self
            .context
            .enter(|| self.composition.process_invalid_scopes())
            .expect("recomposition")
        {}
    }

    // Each text the screen draws, with its top.
    pub(crate) fn texts(&mut self) -> Vec<(String, f32)> {
        let (composition, viewport) = (&mut self.composition, self.viewport);
        self.context.enter(|| {
            let root = composition.root().expect("lazy list root");
            let runtime = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(runtime);
            let layout = applier
                .compute_layout(root, viewport)
                .expect("lazy list layout");
            applier.clear_runtime_handle();
            HeadlessRenderer::new()
                .render(&layout)
                .operations()
                .iter()
                .filter_map(|operation| match operation {
                    RenderOp::Text { rect, value, .. } => Some((value.clone(), rect.y)),
                    _ => None,
                })
                .collect()
        })
    }

    // The first visible item and how far it scrolled out.
    pub(crate) fn position(&self) -> (usize, f32) {
        self.context.enter(|| {
            (
                self.list.first_visible_item_index(),
                self.list.first_visible_item_scroll_offset(),
            )
        })
    }

    // The nodes the composition keeps, the rows' in the reuse pool included.
    pub(crate) fn nodes(&mut self) -> usize {
        self.composition.applier_mut().len()
    }
}

// The viewport of the list tests and the rows they scroll.
pub(crate) const VIEWPORT: Size = Size::new(320.0, 360.0);
pub(crate) const ROW_HEIGHT: f32 = 24.0;
// Each of `TitledRows`' rows: the item's host, its column and two texts.
pub(crate) const NODES_PER_ROW: usize = 4;

// Fifty rows of `list`, each a title over a value.
#[composable]
pub(crate) fn TitledRows(list: LazyListState) {
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(50), move |index| {
                Column(
                    Modifier::empty().height(ROW_HEIGHT),
                    ColumnSpec::default(),
                    move || {
                        Text(
                            format!("Row {index}"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                        Text(
                            format!("{index} kWh"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            });
        },
    );
}

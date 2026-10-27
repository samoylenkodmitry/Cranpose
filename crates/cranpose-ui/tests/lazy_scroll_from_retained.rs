use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    AppContext, HeadlessRenderer, LayoutEngine, LazyColumn, LazyColumnSpec, Modifier, RenderOp,
    Size, Text, TextStyle, composable, retained_measure_count,
};

const VIEWPORT: Size = Size::new(320.0, 360.0);
const ROW_HEIGHT: f32 = 24.0;

type LiveRows = Rc<RefCell<BTreeSet<usize>>>;

// Fifty rows; row 3 shows `version` and grows with it. A row is in `live`
// while its effects run.
#[composable]
fn Rows(version: MutableState<u32>, live: LiveRows, captured: Rc<RefCell<Option<LazyListState>>>) {
    let list = rememberLazyListState();
    *captured.borrow_mut() = Some(list);
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(50).key(|index| index as u64), move |index| {
                let (text, height) = if index == 3 {
                    let version = version.value();
                    (
                        format!("Row 3 v{version}"),
                        ROW_HEIGHT + 16.0 * version as f32,
                    )
                } else {
                    (format!("Row {index}"), ROW_HEIGHT)
                };
                Text(text, Modifier::empty().height(height), TextStyle::default());
                let live = Rc::clone(&live);
                cranpose_core::DisposableEffect(index, move |scope| {
                    live.borrow_mut().insert(index);
                    scope.on_dispose(move || {
                        live.borrow_mut().remove(&index);
                    })
                });
            });
        },
    );
}

struct Screen {
    context: Rc<AppContext>,
    composition: Composition<MemoryApplier>,
    list: LazyListState,
    version: MutableState<u32>,
    live: LiveRows,
}

impl Screen {
    fn new() -> Self {
        let context = AppContext::new();
        let mut composition = Composition::new(MemoryApplier::new());
        let version = context.enter(|| MutableState::with_runtime(0, composition.runtime_handle()));
        let captured = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&captured);
        let live = LiveRows::default();
        let rows_live = Rc::clone(&live);
        context
            .enter(|| {
                composition.render(location_key(file!(), line!(), column!()), move || {
                    Rows(version, Rc::clone(&rows_live), Rc::clone(&capture));
                })
            })
            .expect("initial composition");
        let list = captured
            .borrow()
            .as_ref()
            .copied()
            .expect("lazy list state");
        Self {
            context,
            composition,
            list,
            version,
            live,
        }
    }

    // A screen whose first layout shows the list at `index`, `offset`.
    fn at(index: usize, offset: f32) -> Self {
        let mut screen = Self::new();
        screen.scroll_to(index, offset);
        screen
    }

    fn scroll_to(&mut self, index: usize, offset: f32) {
        self.context
            .enter(|| self.list.scroll_to_item(index, offset));
        self.settle();
    }

    fn scroll_by(&mut self, delta: f32) {
        self.context
            .enter(|| self.list.dispatch_scroll_delta(delta));
        self.settle();
    }

    fn settle(&mut self) {
        while self
            .context
            .enter(|| self.composition.process_invalid_scopes())
            .expect("recomposition")
        {}
    }

    // Each text the screen draws, with its top.
    fn texts(&mut self) -> Vec<(String, f32)> {
        let composition = &mut self.composition;
        self.context.enter(|| {
            let root = composition.root().expect("lazy list root");
            let runtime = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(runtime);
            let layout = applier
                .compute_layout(root, VIEWPORT)
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

    fn position(&self) -> (usize, f32) {
        self.context.enter(|| {
            (
                self.list.first_visible_item_index(),
                self.list.first_visible_item_scroll_offset(),
            )
        })
    }
}

#[test]
fn a_list_scrolled_a_little_at_a_time_draws_each_frame_as_a_list_shown_there_at_once() {
    let mut screen = Screen::new();
    screen.texts();
    let retained_before = retained_measure_count();
    let mut frames = 0u64;
    for step in 0..40u16 {
        let scroll = 50.0 + f32::from(step) * 2.5;
        let index = (scroll / ROW_HEIGHT) as usize;
        let offset = scroll % ROW_HEIGHT;
        screen.scroll_to(index, offset);
        let texts = screen.texts();
        let mut fresh = Screen::at(index, offset);
        assert_eq!(texts, fresh.texts(), "the frame at {scroll} px");
        assert_eq!(
            screen.position(),
            fresh.position(),
            "the position at {scroll} px"
        );
        frames += 1;
    }
    for delta in [-1.5_f32, -3.25, 2.0, -0.75, 5.5] {
        screen.scroll_by(delta);
        let texts = screen.texts();
        let (index, offset) = screen.position();
        assert_eq!(
            texts,
            Screen::at(index, offset).texts(),
            "the frame after {delta} px"
        );
        frames += 1;
    }
    let retained = retained_measure_count() - retained_before;
    assert!(
        retained * 2 > frames,
        "liveness: most frames keep the rows of the frame before, {retained} of {frames} measured from them"
    );
}

#[test]
fn a_row_that_changes_in_the_frame_the_list_scrolls_shows_its_new_text_and_size() {
    let mut screen = Screen::new();
    screen.scroll_to(1, 2.0);
    screen.texts();
    screen.scroll_to(1, 4.0);
    screen.texts();

    screen.context.enter(|| screen.version.set(1));
    screen.scroll_to(1, 6.0);
    let texts = screen.texts();

    let top = |row: &str| {
        texts.iter().find(|(value, _)| value == row).map_or_else(
            || panic!("{row} is on the screen, got {texts:?}"),
            |(_, top)| *top,
        )
    };
    assert_eq!(top("Row 3 v1"), 2.0 * ROW_HEIGHT - 6.0);
    assert_eq!(top("Row 4"), top("Row 3 v1") + ROW_HEIGHT + 16.0);
}

#[test]
fn a_row_that_leaves_the_kept_rows_ends_its_effects() {
    let mut screen = Screen::new();
    screen.texts();
    // The 360 px viewport shows fifteen rows at a row's edge and sixteen
    // between, and the list keeps two more composed after them.
    for offset in [5.0, 0.0, 7.5, 0.0, 12.0, 0.0] {
        screen.scroll_to(0, offset);
        screen.texts();
        let last = if offset > 0.0 { 17 } else { 16 };
        assert_eq!(
            *screen.live.borrow(),
            (0..=last).collect::<BTreeSet<_>>(),
            "the rows composed at {offset} px"
        );
    }
}

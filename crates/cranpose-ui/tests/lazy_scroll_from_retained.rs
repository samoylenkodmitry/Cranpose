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

// Fifty rows of `list`; row 3 grows by 16 px while `grown`. A row is in
// `live` while its effects run.
#[composable]
fn Rows(grown: MutableState<bool>, live: LiveRows, list: LazyListState) {
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(50), move |index| {
                let (text, height) = match index {
                    3 if grown.value() => ("Row 3 grown".to_owned(), ROW_HEIGHT + 16.0),
                    _ => (format!("Row {index}"), ROW_HEIGHT),
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
    grown: MutableState<bool>,
    live: LiveRows,
}

impl Screen {
    fn new() -> Self {
        let context = AppContext::new();
        let mut composition = Composition::new(MemoryApplier::new());
        let grown =
            context.enter(|| MutableState::with_runtime(false, composition.runtime_handle()));
        let captured = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&captured);
        let live = LiveRows::default();
        let rows_live = Rc::clone(&live);
        context
            .enter(|| {
                composition.render(location_key(file!(), line!(), column!()), move || {
                    let list = rememberLazyListState();
                    *capture.borrow_mut() = Some(list);
                    Rows(grown, Rc::clone(&rows_live), list);
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
            grown,
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
    // From 50 px on, 2.5 px a frame, counted in half pixels.
    for step in 0..40u16 {
        let half_pixels = 100 + 5 * step;
        let scroll = f32::from(half_pixels) / 2.0;
        let index = usize::from(half_pixels / 48);
        let offset = f32::from(half_pixels % 48) / 2.0;
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

    screen.context.enter(|| screen.grown.set(true));
    screen.scroll_to(1, 6.0);
    let texts = screen.texts();

    let top = |row: &str| {
        texts.iter().find(|(value, _)| value == row).map_or_else(
            || panic!("{row} is on the screen, got {texts:?}"),
            |(_, top)| *top,
        )
    };
    assert_eq!(top("Row 3 grown"), 2.0 * ROW_HEIGHT - 6.0);
    assert_eq!(top("Row 4"), top("Row 3 grown") + ROW_HEIGHT + 16.0);
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

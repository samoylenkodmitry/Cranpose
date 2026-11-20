// A scoped scene update gives a text field and a reporter of its window
// rect, the sink a scroll viewport reports through, where they lie in the
// window: when a box above them that reports nothing moves or takes a new
// size, when they enter under a box the update keeps, when a reporter leaves
// and comes back, and when a row that reports enters a lazy list. Each
// update is the scoped one; a fresh scene then publishes the same. A window
// that takes a new width resizes the holder of the sinks and composes
// nothing, so only the records above the sinks change.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, NodeId, mutableStateOf};
use cranpose_render_common::graph_scene::Scene;
use cranpose_ui::{
    Alignment, BasicTextField, Box, BoxSpec, Column, ColumnSpec, GraphicsLayer, LayoutNode,
    LazyColumn, LazyColumnSpec, LazyItems, LazyListScope, LazyListState, Modifier, Point, Rect,
    Size, TestComposition, Text, TextFieldState, TextStyle, rememberLazyListState,
    run_test_composition,
};

use crate::scene_probe::{fresh_graph, initial_graph, update_scene, with_applier};

const VIEWPORT: Size = Size::new(320.0, 200.0);
// The box that holds the sinks: as wide as the window, this tall, its
// children in its center.
const HOLDER_HEIGHT: f32 = 120.0;
const REPORTER: Size = Size::new(40.0, 40.0);
const FIELD: Size = Size::new(100.0, 30.0);
// The box in the holder's center that the entering sinks enter, its own
// children in its center.
const ENTRY: Size = Size::new(120.0, 100.0);
const ROW_HEIGHT: f32 = 40.0;

fn unreported() -> Rect {
    Rect::from_size(Size::default())
}

// Where the text field `field` lies in the window, as it reported it.
fn field_rect(composition: &mut TestComposition, field: NodeId) -> Rect {
    let window = with_applier(composition, |applier| {
        applier
            .with_node::<LayoutNode, _>(field, |node| {
                node.modifier_slices_snapshot()
                    .text_window()
                    .map(|cell| cell.get())
            })
            .ok()
            .flatten()
    })
    .expect("a text field reports its window coordinates");
    let origin = window.local_to_window.map_point(Point::default());
    Rect {
        x: origin.x,
        y: origin.y,
        width: window.size.width,
        height: window.size.height,
    }
}

// The states a test changes.
#[derive(Clone, Copy)]
struct Knobs {
    spacer: MutableState<f32>,
    keeping: MutableState<bool>,
    label: MutableState<&'static str>,
    entered: MutableState<bool>,
    reporting: MutableState<bool>,
    badge: MutableState<&'static str>,
    badge_reporting: MutableState<bool>,
}

// The cells the reporters publish into, and the text field.
#[derive(Clone, Default)]
struct Sinks {
    kept: Rc<Cell<Rect>>,
    label: Rc<Cell<Rect>>,
    entering: Rc<Cell<Rect>>,
    toggled: Rc<Cell<Rect>>,
    badge: Rc<Cell<Rect>>,
    field: Rc<Cell<Option<NodeId>>>,
}

// A spacer above a holder box that reports nothing. The holder holds a
// plain box, a reporter and a reporting label that `keeping` keeps, a box
// whose reporter `reporting` turns on and off, a badge, a label in a layer
// of its own, whose reporter `badge_reporting` turns on and off, and an
// entry box that holds no label: `entered` brings a reporter and a text
// field into it.
fn holder(knobs: Rc<Cell<Option<Knobs>>>, sinks: Sinks) -> TestComposition {
    run_test_composition(move || {
        let knobs_slot = Rc::clone(&knobs);
        let sinks = sinks.clone();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let state = cranpose_core::remember(|| Knobs {
                spacer: mutableStateOf(20.0_f32),
                keeping: mutableStateOf(true),
                label: mutableStateOf("label"),
                entered: mutableStateOf(true),
                reporting: mutableStateOf(true),
                badge: mutableStateOf("badge"),
                badge_reporting: mutableStateOf(false),
            })
            .with(|knobs| *knobs);
            knobs_slot.set(Some(state));
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(state.spacer.get()),
                BoxSpec::default(),
                || {},
            );
            let sinks = sinks.clone();
            Box(
                Modifier::empty().fill_max_width().height(HOLDER_HEIGHT),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || holder_content(state, &sinks),
            );
        });
    })
}

fn holder_content(state: Knobs, sinks: &Sinks) {
    Box(
        Modifier::empty().size(Size::new(10.0, 10.0)),
        BoxSpec::default(),
        || {},
    );
    if state.keeping.get() {
        Box(
            Modifier::empty()
                .size(REPORTER)
                .report_window_rect(Rc::clone(&sinks.kept)),
            BoxSpec::default(),
            || {},
        );
        Text(
            state.label.get(),
            Modifier::empty().report_window_rect(Rc::clone(&sinks.label)),
            TextStyle::default(),
        );
    }
    let toggled = Modifier::empty().size(REPORTER);
    let toggled = reporting(toggled, state.reporting.get().then_some(&sinks.toggled));
    Box(toggled, BoxSpec::default(), || {
        Box(
            Modifier::empty().size(Size::new(8.0, 8.0)),
            BoxSpec::default(),
            || {},
        );
    });
    let badge = Modifier::empty().graphics_layer(|| GraphicsLayer {
        alpha: 0.5,
        ..GraphicsLayer::default()
    });
    let badge = reporting(badge, state.badge_reporting.get().then_some(&sinks.badge));
    Text(state.badge.get(), badge, TextStyle::default());
    let sinks = sinks.clone();
    Box(
        Modifier::empty().size(ENTRY),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || entry_content(state.entered, &sinks),
    );
}

// A plain box the entry keeps, so the entry stays a layer of its own, and
// the reporter and the text field while `entered` holds.
fn entry_content(entered: MutableState<bool>, sinks: &Sinks) {
    Box(
        Modifier::empty().size(Size::new(8.0, 8.0)),
        BoxSpec::default(),
        || {},
    );
    if entered.get() {
        Box(
            Modifier::empty()
                .size(REPORTER)
                .report_window_rect(Rc::clone(&sinks.entering)),
            BoxSpec::default(),
            || {},
        );
        let field = cranpose_core::remember(|| TextFieldState::new("field")).with(|field| *field);
        sinks.field.set(Some(BasicTextField(
            field,
            Modifier::empty().size(FIELD),
            TextStyle::default(),
        )));
    }
}

// `modifier`, which reports its window rect into `sink` when there is one.
fn reporting(modifier: Modifier, sink: Option<&Rc<Cell<Rect>>>) -> Modifier {
    match sink {
        Some(sink) => modifier.report_window_rect(Rc::clone(sink)),
        None => modifier,
    }
}

struct Holder {
    composition: TestComposition,
    root: NodeId,
    scene: Scene,
    knobs: Knobs,
    sinks: Sinks,
    // The window, which the holder fills the width of.
    viewport: Size,
}

impl Holder {
    fn new() -> Self {
        let knobs = Rc::new(Cell::new(None));
        let sinks = Sinks::default();
        let mut composition = holder(Rc::clone(&knobs), sinks.clone());
        let root = composition.root().expect("root");
        let scene = initial_graph(&mut composition, root, VIEWPORT);
        let knobs = knobs.get().expect("the holder composed");
        Self {
            composition,
            root,
            scene,
            knobs,
            sinks,
            viewport: VIEWPORT,
        }
    }

    fn update(&mut self) {
        update_scene(
            &mut self.composition,
            self.root,
            self.viewport,
            &mut self.scene,
        );
    }

    // The window takes `width`; the holder takes it too.
    fn widen(&mut self, width: f32) {
        self.viewport.width = width;
    }

    // Where a child of `size` lies now, centered in the holder on whole
    // pixels.
    fn centered(&self, size: Size) -> Rect {
        Rect {
            x: ((self.viewport.width - size.width) / 2.0).round(),
            y: self.knobs.spacer.get() + ((HOLDER_HEIGHT - size.height) / 2.0).round(),
            width: size.width,
            height: size.height,
        }
    }

    fn field_rect(&mut self) -> Rect {
        let field = self.sinks.field.get().expect("the field composed");
        field_rect(&mut self.composition, field)
    }

    // Each sink in `cells` and the field when `field` holds where it lies
    // now, as a fresh scene publishes it.
    fn assert_where_they_lie(
        &mut self,
        cells: &[(&str, &Rc<Cell<Rect>>)],
        field: bool,
        step: &str,
    ) {
        let reporter = self.centered(REPORTER);
        for (name, cell) in cells {
            assert_eq!(
                cell.get(),
                reporter,
                "{step}: the {name} reporter publishes where it lies"
            );
        }
        if field {
            assert_eq!(
                self.field_rect(),
                self.centered(FIELD),
                "{step}: the field reports where it lies"
            );
        }
        let label = self.sinks.label.get();
        let keeping = self.knobs.keeping.get();
        if keeping {
            assert_eq!(
                label,
                self.centered(Size::new(label.width, label.height)),
                "{step}: the label publishes where it lies"
            );
        }
        let scoped: Vec<Rect> = cells.iter().map(|(_, cell)| cell.get()).collect();
        let _fresh = fresh_graph(&mut self.composition, self.root);
        let fresh: Vec<Rect> = cells.iter().map(|(_, cell)| cell.get()).collect();
        assert_eq!(scoped, fresh, "{step}: a fresh scene publishes the same");
        if keeping {
            assert_eq!(label, self.sinks.label.get(), "{step}: and the same label");
        }
    }
}

#[test]
fn sinks_follow_a_box_above_them_that_moves_and_takes_a_new_size() {
    let mut holder = Holder::new();
    let changes: [(&str, fn(&mut Holder)); 4] = [
        ("move", |holder| holder.knobs.spacer.set(50.0)),
        ("resize", |holder| holder.widen(260.0)),
        ("move and resize", |holder| {
            holder.knobs.spacer.set(10.0);
            holder.widen(230.0);
        }),
        ("label text", |holder| {
            holder.knobs.label.set("a longer label")
        }),
    ];
    for (step, change) in changes {
        let label = holder.sinks.label.get();
        change(&mut holder);
        holder.update();
        assert_ne!(holder.sinks.label.get(), label, "{step}: the label moves");
        let sinks = holder.sinks.clone();
        holder.assert_where_they_lie(
            &[
                ("kept", &sinks.kept),
                ("entered", &sinks.entering),
                ("toggled", &sinks.toggled),
            ],
            true,
            step,
        );
    }
}

#[test]
fn sinks_that_enter_under_a_kept_box_publish_where_they_lie() {
    let mut holder = Holder::new();
    holder.knobs.keeping.set(false);
    holder.knobs.entered.set(false);
    holder.knobs.reporting.set(false);
    holder.update();
    holder.sinks.entering.set(unreported());
    holder.sinks.toggled.set(unreported());
    holder.knobs.spacer.set(40.0);
    holder.update();

    holder.knobs.entered.set(true);
    holder.update();
    let sinks = holder.sinks.clone();
    holder.assert_where_they_lie(&[("entering", &sinks.entering)], true, "enter");

    holder.knobs.reporting.set(true);
    holder.update();
    holder.assert_where_they_lie(&[("taken", &sinks.toggled)], false, "take a reporter");
}

#[test]
fn a_kept_badge_that_takes_new_text_and_a_reporter_publishes_where_it_lies() {
    let mut holder = Holder::new();
    holder.knobs.keeping.set(false);
    holder.knobs.entered.set(false);
    holder.knobs.reporting.set(false);
    holder.update();

    holder.widen(260.0);
    holder.knobs.badge.set("a wider badge");
    holder.knobs.badge_reporting.set(true);
    holder.update();
    let badge = holder.sinks.badge.get();
    assert_ne!(badge, unreported(), "the badge publishes its rect");
    assert_eq!(
        badge,
        holder.centered(Size::new(badge.width, badge.height)),
        "the badge publishes where it lies"
    );
    let _fresh = fresh_graph(&mut holder.composition, holder.root);
    assert_eq!(
        holder.sinks.badge.get(),
        badge,
        "a fresh scene publishes the same"
    );
}

#[test]
fn a_reporter_that_leaves_and_comes_back_publishes_where_it_now_lies() {
    let mut holder = Holder::new();
    let first = holder.centered(REPORTER);
    assert_eq!(holder.sinks.toggled.get(), first);

    holder.knobs.reporting.set(false);
    holder.knobs.entered.set(false);
    holder.update();
    holder.knobs.spacer.set(60.0);
    holder.widen(280.0);
    holder.update();
    assert_eq!(
        holder.sinks.toggled.get(),
        first,
        "a box without its reporter publishes nothing"
    );

    holder.knobs.reporting.set(true);
    holder.knobs.entered.set(true);
    holder.update();
    let sinks = holder.sinks.clone();
    holder.assert_where_they_lie(
        &[("returned", &sinks.toggled), ("entered", &sinks.entering)],
        true,
        "come back",
    );
}

// The rows that enter lie below the rows the list composed first.
const ROWS: usize = 30;
const REPORTING_ROW: usize = 20;
const FIELD_ROW: usize = 21;

#[test]
fn a_row_that_reports_enters_a_lazy_list_where_it_lies() {
    let list = Rc::new(Cell::new(None::<LazyListState>));
    let row_rect = Rc::new(Cell::new(unreported()));
    let field = Rc::new(Cell::new(None::<NodeId>));
    let (content_list, content_rect, content_field) =
        (Rc::clone(&list), Rc::clone(&row_rect), Rc::clone(&field));
    let mut composition = run_test_composition(move || {
        let state = rememberLazyListState();
        content_list.set(Some(state));
        let rect = Rc::clone(&content_rect);
        let field = Rc::clone(&content_field);
        LazyColumn(
            Modifier::empty().fill_max_size(),
            state,
            LazyColumnSpec::default(),
            move |scope| {
                let rect = Rc::clone(&rect);
                let field = Rc::clone(&field);
                scope.items(LazyItems::new(ROWS), move |index| {
                    let row = Modifier::empty().fill_max_width().height(ROW_HEIGHT);
                    match index {
                        REPORTING_ROW => {
                            Box(
                                row.report_window_rect(Rc::clone(&rect)),
                                BoxSpec::default(),
                                || {},
                            );
                        }
                        FIELD_ROW => {
                            let state = cranpose_core::remember(|| TextFieldState::new("row"))
                                .with(|state| *state);
                            field.set(Some(BasicTextField(state, row, TextStyle::default())));
                        }
                        _ => {
                            Box(row, BoxSpec::default(), || {});
                        }
                    }
                });
            },
        );
    });
    let root = composition.root().expect("root");
    let mut scene = initial_graph(&mut composition, root, VIEWPORT);
    let list = list.get().expect("the list composed");
    assert_eq!(row_rect.get(), unreported(), "the row lies below the list");
    assert_eq!(field.get(), None, "the field row lies below the list");

    let scrolled = 700.0;
    assert_eq!(list.dispatch_scroll_delta(-scrolled).abs(), scrolled);
    update_scene(&mut composition, root, VIEWPORT, &mut scene);
    let row_at = |row: usize| Rect {
        x: 0.0,
        y: row as f32 * ROW_HEIGHT - scrolled,
        width: VIEWPORT.width,
        height: ROW_HEIGHT,
    };
    assert_eq!(
        row_rect.get(),
        row_at(REPORTING_ROW),
        "the row that entered publishes where it lies"
    );
    let field = field.get().expect("the field row entered");
    assert_eq!(
        field_rect(&mut composition, field),
        row_at(FIELD_ROW),
        "the field row that entered reports where it lies"
    );
    let _fresh = fresh_graph(&mut composition, root);
    assert_eq!(
        row_rect.get(),
        row_at(REPORTING_ROW),
        "a fresh scene publishes the same"
    );
}

//! A composable that receives content skips its body when its other
//! parameters are unchanged, as Compose skips a call whose composable lambda
//! is the same instance, and the new content still shows: the scope that ran
//! the old content runs the new one in place.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};
use cranpose_foundation::lazy::{LazyListScope, rememberLazyListState};

use crate::{
    composable,
    modifier::{Modifier, Size},
    test_support::rendered_texts,
    text::TextStyle,
    widgets::{
        Box, BoxSpec, BoxWithConstraints, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row,
        RowSpec, Text,
    },
};

const VIEWPORT: Size = Size {
    width: 320.0,
    height: 480.0,
};

thread_local! {
    static CARD_BODIES: Cell<u32> = const { Cell::new(0) };
}

fn card_bodies() -> u32 {
    CARD_BODIES.with(Cell::get)
}

fn label(text: String) {
    Text(text, Modifier::empty(), TextStyle::default());
}

/// A titled column around caller content; counts its body's runs.
#[composable]
fn Card(title: &'static str, content: impl FnMut() + 'static) {
    CARD_BODIES.with(|runs| runs.set(runs.get() + 1));
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        label(title.to_string());
        content();
    });
}

/// Runs its content in its own body, not under a layout of its own.
#[composable]
fn Inline(content: impl FnMut() + 'static) {
    content();
}

/// Lays its content out once the constraints are known.
#[composable]
fn Measured(content: impl FnMut() + 'static) {
    BoxWithConstraints(Modifier::empty(), move |_| content());
}

/// What a test's screen composes from the state's value.
#[derive(Clone, Copy, PartialEq)]
enum Scene {
    Totals,
    Titled,
    Nested,
    Toggle,
    Inline,
    Measured,
    Lazy,
}

#[composable]
fn Screen(scene: Scene, state: MutableState<u32>) {
    let value = state.value();
    match scene {
        Scene::Totals => Card("Totals", move || label(format!("count {value}"))),
        Scene::Titled => {
            let title = if value == 0 { "Narrow" } else { "Wide" };
            Card(title, || label("body".to_string()));
        }
        Scene::Nested => Card("Nested", move || {
            Row(Modifier::empty(), RowSpec::default(), move || {
                Box(Modifier::empty(), BoxSpec::default(), move || {
                    label(format!("deep {value}"));
                });
            });
        }),
        Scene::Toggle => Card("Toggle", move || {
            if value == 1 {
                label("extra".to_string());
            }
            label("always".to_string());
        }),
        Scene::Inline => {
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Inline(move || label(format!("inline {value}")));
            });
        }
        Scene::Measured => Measured(move || label(format!("measured {value}"))),
        Scene::Lazy => LazyCards(value),
    }
}

/// Three lazy rows, each a card over content showing `value`.
#[composable]
fn LazyCards(value: u32) {
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty(),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            for row in 0..3_u64 {
                scope.item_keyed(Some(row), None, move || {
                    Card("Row", move || label(format!("row {row} at {value}")));
                });
            }
        },
    );
}

struct Harness {
    composition: Composition<MemoryApplier>,
    state: MutableState<u32>,
}

impl Harness {
    /// Composes `scene` over a state that starts at 0.
    fn new(scene: Scene) -> Self {
        Self::compose(move |state| Screen(scene, state))
    }

    fn compose(content: impl Fn(MutableState<u32>) + 'static) -> Self {
        CARD_BODIES.with(|runs| runs.set(0));
        let mut composition = Composition::new(MemoryApplier::new());
        let state = MutableState::with_runtime(0, composition.runtime_handle());
        let content = Rc::new(content);
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                let content = Rc::clone(&content);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    content(state)
                });
            })
            .expect("initial composition");
        Self { composition, state }
    }

    fn set(&mut self, value: u32) {
        self.state.set(value);
        self.composition
            .process_invalid_scopes()
            .expect("recomposition");
    }

    fn texts(&mut self) -> Vec<String> {
        let root = self.composition.root().expect("root node");
        rendered_texts(&mut self.composition, root, VIEWPORT)
    }
}

#[test]
fn a_card_with_an_unchanged_title_shows_new_content_without_running_again() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut harness = Harness::new(Scene::Totals);
    assert_eq!(harness.texts(), ["Totals", "count 0"]);

    harness.set(1);

    assert_eq!(harness.texts(), ["Totals", "count 1"]);
    assert_eq!(card_bodies(), 1, "the card's own parameters did not change");
}

#[test]
fn a_card_whose_title_changes_runs_again() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut harness = Harness::new(Scene::Titled);
    assert_eq!(harness.texts(), ["Narrow", "body"]);

    harness.set(1);

    assert_eq!(harness.texts(), ["Wide", "body"]);
    assert_eq!(card_bodies(), 2);
}

#[test]
fn content_nested_in_containers_of_a_skipped_card_shows_the_new_value() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut harness = Harness::new(Scene::Nested);
    assert_eq!(harness.texts(), ["Nested", "deep 0"]);

    harness.set(7);

    assert_eq!(harness.texts(), ["Nested", "deep 7"]);
    assert_eq!(card_bodies(), 1);
}

#[test]
fn content_that_appears_and_leaves_inside_a_skipped_card_follows_the_state() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut harness = Harness::new(Scene::Toggle);
    assert_eq!(harness.texts(), ["Toggle", "always"]);

    harness.set(1);
    assert_eq!(harness.texts(), ["Toggle", "extra", "always"]);

    harness.set(0);
    assert_eq!(harness.texts(), ["Toggle", "always"]);
    assert_eq!(card_bodies(), 1);
}

#[test]
fn content_run_in_the_receiving_body_shows_the_new_value() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut harness = Harness::new(Scene::Inline);
    assert_eq!(harness.texts(), ["inline 0"]);

    harness.set(3);

    assert_eq!(harness.texts(), ["inline 3"]);
}

#[test]
fn content_measured_in_a_subcomposition_shows_the_new_value() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut harness = Harness::new(Scene::Measured);
    assert_eq!(harness.texts(), ["measured 0"]);

    harness.set(5);

    assert_eq!(harness.texts(), ["measured 5"]);
}

#[test]
fn lazy_rows_whose_cards_skip_show_the_new_value() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut harness = Harness::new(Scene::Lazy);
    assert_eq!(
        harness.texts(),
        [
            "Row",
            "row 0 at 0",
            "Row",
            "row 1 at 0",
            "Row",
            "row 2 at 0"
        ]
    );

    harness.set(2);

    assert_eq!(
        harness.texts(),
        [
            "Row",
            "row 0 at 2",
            "Row",
            "row 1 at 2",
            "Row",
            "row 2 at 2"
        ]
    );
}

/// Keeps the click handler a call received when its body ran.
#[derive(Clone, Default)]
struct ClickSink(Rc<RefCell<Option<Rc<dyn Fn()>>>>);

impl PartialEq for ClickSink {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl ClickSink {
    fn click(&self) {
        let handler = self.0.borrow().clone().expect("a handler was kept");
        handler();
    }
}

#[composable]
fn Clicker(sink: ClickSink, on_click: impl FnMut() + 'static) {
    CARD_BODIES.with(|runs| runs.set(runs.get() + 1));
    *sink.0.borrow_mut() = Some(Rc::new(on_click));
}

#[composable]
fn ClickScreen(sink: ClickSink, clicks: Clicks, state: MutableState<u32>) {
    let value = state.value();
    Clicker(sink, move || clicks.0.borrow_mut().push(value));
}

#[derive(Clone, Default)]
struct Clicks(Rc<RefCell<Vec<u32>>>);

impl PartialEq for Clicks {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[test]
fn a_skipped_call_runs_the_newest_click_handler() {
    let _app_context = crate::render_state::app_context_test_scope();
    let sink = ClickSink::default();
    let clicks = Clicks::default();
    let mut harness = Harness::compose({
        let sink = sink.clone();
        let clicks = clicks.clone();
        move |state| ClickScreen(sink.clone(), clicks.clone(), state)
    });

    sink.click();
    harness.set(4);
    sink.click();

    assert_eq!(*clicks.0.borrow(), [0, 4]);
    assert_eq!(card_bodies(), 1, "the handler alone changed");
}

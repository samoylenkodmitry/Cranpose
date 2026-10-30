use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose_core::{
    Composition, DefaultScheduler, DisposableEffect, MemoryApplier, MutableState, Node, NodeId,
    PausableContent, Runtime, SideEffect, movable, pop_parent, push_parent, remember,
    scheduler_ref, with_current_composer, with_pausable_content,
};

use super::ordered_parent::OrderedParent;

struct LabelNode {
    label: &'static str,
}

impl Node for LabelNode {}

type Log = Rc<RefCell<Vec<String>>>;

const FRAME_NANOS: u64 = 16_666_667;

#[derive(Clone)]
struct Fixture {
    show: MutableState<bool>,
    // Whether `MovingScreen` shows its content before its leaf.
    first: MutableState<bool>,
    key: MutableState<u32>,
    log: Log,
    // The keys `Keyed` composed with.
    composed: Rc<RefCell<Vec<u32>>>,
    parent: Rc<RefCell<Option<NodeId>>>,
    complete: Rc<RefCell<Option<PausableContent>>>,
}

impl PartialEq for Fixture {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.log, &other.log)
    }
}

#[cranpose_macros::composable]
fn Screen(fixture: Fixture) {
    let content = remember(|| PausableContent::new(|| {})).with(PausableContent::clone);
    *fixture.complete.borrow_mut() = Some(content.clone());
    let parent = with_current_composer(|composer| composer.emit_node(OrderedParent::default));
    *fixture.parent.borrow_mut() = Some(parent);
    push_parent(parent);
    Leaf("before");
    if fixture.show.get() {
        with_pausable_content(&content, move || {
            Row("a", fixture.clone());
            Row("b", fixture.clone());
            Row("c", fixture);
        });
    }
    Leaf("after");
    pop_parent();
}

// Shows the rows of `Screen` as movable content in one of two groups, before
// or after a leaf.
#[cranpose_macros::composable]
fn MovingScreen(fixture: Fixture) {
    let content = remember(|| PausableContent::new(|| {})).with(PausableContent::clone);
    *fixture.complete.borrow_mut() = Some(content.clone());
    let parent = with_current_composer(|composer| composer.emit_node(OrderedParent::default));
    *fixture.parent.borrow_mut() = Some(parent);
    push_parent(parent);
    let first = fixture.first.get();
    Side(first, content.clone(), fixture.clone());
    Leaf("x");
    Side(!first, content, fixture);
    pop_parent();
}

#[cranpose_macros::composable]
fn Side(shows: bool, content: PausableContent, fixture: Fixture) {
    if shows && fixture.show.get() {
        movable("rows", || Rows(content, fixture));
    }
}

// Skips when moved: the move alone reaches the scopes of its waiting bodies.
#[cranpose_macros::composable]
fn Rows(content: PausableContent, fixture: Fixture) {
    with_pausable_content(&content, move || {
        Row("a", fixture.clone());
        Row("b", fixture.clone());
        Row("c", fixture);
    });
}

#[cranpose_macros::composable]
fn Row(label: &'static str, fixture: Fixture) {
    Leaf(label);
    let log = Rc::clone(&fixture.log);
    SideEffect(move || log.borrow_mut().push(format!("effect {label}")));
    if label == "b" {
        Keyed(fixture);
    }
}

#[cranpose_macros::composable]
fn Keyed(fixture: Fixture) {
    let key = fixture.key.get();
    fixture.composed.borrow_mut().push(key);
    let log = Rc::clone(&fixture.log);
    DisposableEffect(key, move |scope| {
        log.borrow_mut().push(format!("start {key}"));
        let log = Rc::clone(&log);
        scope.on_dispose(move || log.borrow_mut().push(format!("stop {key}")))
    });
}

#[cranpose_macros::composable]
fn Leaf(label: &'static str) {
    with_current_composer(|composer| {
        composer.emit_node(|| LabelNode { label });
    });
}

struct Harness {
    composition: Composition<MemoryApplier>,
    fixture: Fixture,
    frame: u64,
}

impl Harness {
    fn new(budget: Option<Duration>) -> Self {
        Self::rendering(budget, Screen)
    }

    fn rendering(budget: Option<Duration>, screen: fn(Fixture)) -> Self {
        Self::rendering_in(Composition::new(MemoryApplier::new()), budget, screen)
    }

    fn rendering_in(
        mut composition: Composition<MemoryApplier>,
        budget: Option<Duration>,
        screen: fn(Fixture),
    ) -> Self {
        if let Some(budget) = budget {
            composition.set_pausable_budget(Some(budget));
        }
        let runtime = composition.runtime_handle();
        let fixture = Fixture {
            show: MutableState::with_runtime(false, runtime.clone()),
            first: MutableState::with_runtime(true, runtime.clone()),
            key: MutableState::with_runtime(1, runtime),
            log: Rc::default(),
            composed: Rc::default(),
            parent: Rc::default(),
            complete: Rc::default(),
        };
        let root_key = cranpose_core::location_key(file!(), line!(), column!());
        let root = fixture.clone();
        composition
            .render(root_key, move || screen(root.clone()))
            .expect("initial composition succeeds");
        Self {
            composition,
            fixture,
            frame: 0,
        }
    }

    fn recompose(&mut self) {
        self.composition
            .process_invalid_scopes()
            .expect("recomposition succeeds");
    }

    fn next_frame(&mut self) {
        self.frame += 1;
        self.composition
            .runtime()
            .drain_frame_callbacks(self.frame * FRAME_NANOS);
        assert!(
            self.composition.should_recompose(),
            "waiting content asks for composition on the next frame"
        );
        self.recompose();
    }

    fn labels(&mut self) -> Vec<&'static str> {
        let parent = self
            .fixture
            .parent
            .borrow()
            .expect("screen emitted its parent");
        let mut applier = self.composition.applier_mut();
        let children = applier
            .with_node::<OrderedParent, _>(parent, |parent| parent.children.clone())
            .expect("parent exists");
        children
            .into_iter()
            .map(|child| {
                applier
                    .with_node::<LabelNode, _>(child, |node| node.label)
                    .expect("child is a label")
            })
            .collect()
    }

    fn log(&self) -> Vec<String> {
        self.fixture.log.borrow().clone()
    }

    fn is_complete(&self) -> bool {
        self.fixture
            .complete
            .borrow()
            .as_ref()
            .is_some_and(PausableContent::is_complete)
    }
}

#[test]
fn content_within_budget_completes_in_its_pass() {
    let mut harness = Harness::new(None);
    harness.fixture.show.set(true);
    harness.recompose();

    assert!(harness.is_complete());
    assert_eq!(harness.labels(), ["before", "a", "b", "c", "after"]);
    assert_eq!(
        harness.log(),
        ["effect a", "effect b", "start 1", "effect c"]
    );
}

#[test]
fn content_past_budget_composes_one_body_a_frame_in_place() {
    let mut harness = Harness::new(Some(Duration::ZERO));
    harness.fixture.show.set(true);
    harness.recompose();
    let mut shown = vec![harness.labels()];
    let mut frames = 0;
    while !harness.is_complete() {
        assert!(
            harness.log().is_empty(),
            "effects wait until the content completes"
        );
        // Rows a, b and c, the leaf of each and `Keyed`: one body a frame.
        assert!(frames < 7, "every frame runs a waiting body");
        harness.next_frame();
        frames += 1;
        if shown.last() != Some(&harness.labels()) {
            shown.push(harness.labels());
        }
    }

    assert_eq!(frames, 7);
    assert_eq!(
        shown,
        [
            vec!["before", "after"],
            vec!["before", "a", "after"],
            vec!["before", "a", "b", "after"],
            vec!["before", "a", "b", "c", "after"],
        ]
    );
    assert_eq!(
        harness.log(),
        ["effect a", "effect b", "start 1", "effect c"]
    );
    assert!(
        !harness.composition.should_render(),
        "complete content asks for no more frames"
    );
}

#[test]
fn invalidation_of_waiting_content_runs_after_its_effects() {
    let mut harness = Harness::new(Some(Duration::ZERO));
    harness.fixture.show.set(true);
    harness.recompose();
    while !harness.fixture.composed.borrow().contains(&1) {
        harness.next_frame();
    }
    assert!(!harness.is_complete());
    harness.fixture.key.set(2);
    harness.recompose();
    assert_eq!(
        *harness.fixture.composed.borrow(),
        [1],
        "a scope of waiting content recomposes once the content completes"
    );
    while !harness.is_complete() {
        harness.next_frame();
    }

    assert_eq!(*harness.fixture.composed.borrow(), [1, 2]);
    assert_eq!(
        harness.log(),
        [
            "effect a", "effect b", "start 1", "effect c", "stop 1", "start 2"
        ]
    );
}

#[test]
fn content_removed_before_it_completes_runs_no_effect() {
    let mut harness = Harness::new(Some(Duration::ZERO));
    harness.fixture.show.set(true);
    harness.recompose();
    while harness.labels() != ["before", "a", "after"] {
        harness.next_frame();
    }

    harness.fixture.show.set(false);
    harness.recompose();
    for _ in 0..4 {
        harness.frame += 1;
        harness
            .composition
            .runtime()
            .drain_frame_callbacks(harness.frame * FRAME_NANOS);
        harness.recompose();
    }

    assert_eq!(harness.labels(), ["before", "after"]);
    assert!(harness.log().is_empty());
}

#[test]
fn waiting_content_that_moves_keeps_waiting() {
    let mut harness = Harness::rendering(Some(Duration::ZERO), MovingScreen);
    harness.fixture.show.set(true);
    harness.recompose();
    while harness.labels() != ["a", "x"] {
        harness.next_frame();
    }
    harness.fixture.first.set(false);
    harness.recompose();
    assert!(
        harness.log().is_empty(),
        "bodies of moved content wait and its effects wait until it completes"
    );
    while !harness.is_complete() {
        harness.next_frame();
    }

    assert_eq!(harness.labels(), ["x", "a", "b", "c"]);
    assert_eq!(
        harness.log(),
        ["effect a", "effect b", "start 1", "effect c"]
    );
}

#[test]
fn a_composition_that_leaves_while_its_frame_is_due_leaves_its_runtime_idle() {
    let runtime = Runtime::new(scheduler_ref(DefaultScheduler));
    let composition = Composition::with_runtime(MemoryApplier::new(), runtime.clone());
    let mut harness = Harness::rendering_in(composition, Some(Duration::ZERO), Screen);
    harness.fixture.show.set(true);
    harness.recompose();
    runtime.handle().drain_frame_callbacks(FRAME_NANOS);
    assert!(
        runtime.has_updates(),
        "a frame in which waiting bodies run asks for a pass"
    );

    drop(harness);
    runtime.handle().drain_frame_callbacks(2 * FRAME_NANOS);

    assert!(!runtime.has_updates());
    assert!(!runtime.has_frame_callbacks());
}

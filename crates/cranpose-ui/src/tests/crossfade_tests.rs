use std::{cell::RefCell, rc::Rc};

use cranpose_animation::{Easing, tween};
use cranpose_core::{DisposableEffectResult, MutableState, NodeId};
use cranpose_macros::composable;

use crate::{
    Crossfade, HeadlessRenderer, LayoutEngine, Modifier, RenderOp, Size, TestComposition, Text,
    TextStyle, run_test_composition,
};

const FRAME_NANOS: u64 = 16_666_667;
const CROSSFADE_MILLIS: u64 = 160;

fn drain(composition: &mut TestComposition) {
    while composition
        .process_invalid_scopes()
        .expect("process invalid scopes")
    {}
}

fn pump_frames(composition: &mut TestComposition, frame_time: &mut u64, frames: usize) {
    let runtime = composition.runtime_handle();
    for _ in 0..frames {
        *frame_time += FRAME_NANOS;
        composition.with_app_context(|| {
            runtime.drain_frame_callbacks(*frame_time);
            runtime.drain_ui();
        });
        drain(composition);
    }
}

fn settle(composition: &mut TestComposition, frame_time: &mut u64) {
    for _ in 0..120 {
        if !composition.should_render() {
            break;
        }
        pump_frames(composition, frame_time, 1);
    }
}

#[composable]
#[allow(non_snake_case)]
fn CrossfadeHost(target: MutableState<u32>, alive: Rc<RefCell<Vec<u32>>>) {
    let current = target.value();
    let alive_for_content = Rc::clone(&alive);
    Crossfade(
        current,
        tween(CROSSFADE_MILLIS, Easing::LinearEasing),
        move |value: u32| {
            let alive = Rc::clone(&alive_for_content);
            cranpose_core::DisposableEffect(value, move |_scope| {
                alive.borrow_mut().push(value);
                let alive = Rc::clone(&alive);
                DisposableEffectResult::new(move || {
                    alive.borrow_mut().retain(|item| *item != value);
                })
            });
        },
    );
}

fn crossfade_composition(
    initial: u32,
) -> (TestComposition, MutableState<u32>, Rc<RefCell<Vec<u32>>>) {
    let alive = Rc::new(RefCell::new(Vec::<u32>::new()));
    let target_slot = Rc::new(RefCell::new(None::<MutableState<u32>>));

    let composition = {
        let alive = Rc::clone(&alive);
        let target_slot = Rc::clone(&target_slot);
        run_test_composition(move || {
            let target = cranpose_core::rememberMutableStateOf(move || initial);
            target_slot.borrow_mut().replace(target);
            CrossfadeHost(target, Rc::clone(&alive));
        })
    };

    let target = target_slot
        .borrow()
        .as_ref()
        .copied()
        .expect("target state captured");
    (composition, target, alive)
}

#[test]
fn crossfade_shows_initial_content_immediately() {
    let (mut composition, _target, alive) = crossfade_composition(1);
    let mut frame_time = 0u64;

    assert_eq!(
        alive.borrow().as_slice(),
        &[1],
        "initial content should be composed right away"
    );

    settle(&mut composition, &mut frame_time);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1],
        "initial content should stay composed with no transition running"
    );
    assert!(!composition.should_render());
}

#[test]
fn crossfade_keeps_both_contents_during_transition_and_removes_old_after() {
    let (mut composition, target, alive) = crossfade_composition(1);
    let mut frame_time = 0u64;
    settle(&mut composition, &mut frame_time);

    composition.with_app_context(|| target.set_value(2));
    drain(&mut composition);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1, 2],
        "both contents should be composed as soon as the transition starts"
    );

    pump_frames(&mut composition, &mut frame_time, 3);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1, 2],
        "both contents should stay composed mid-transition"
    );

    settle(&mut composition, &mut frame_time);
    assert_eq!(
        alive.borrow().as_slice(),
        &[2],
        "old content should be removed after its fade-out completes"
    );
    assert!(
        !composition.should_render(),
        "crossfade should settle with no pending animation frames"
    );
}

#[test]
fn crossfade_retargeting_mid_transition_restores_previous_content() {
    let (mut composition, target, alive) = crossfade_composition(1);
    let mut frame_time = 0u64;
    settle(&mut composition, &mut frame_time);

    composition.with_app_context(|| target.set_value(2));
    drain(&mut composition);
    pump_frames(&mut composition, &mut frame_time, 2);
    assert_eq!(alive.borrow().as_slice(), &[1, 2]);

    composition.with_app_context(|| target.set_value(1));
    drain(&mut composition);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1, 2],
        "both contents remain composed after retargeting mid-transition"
    );

    settle(&mut composition, &mut frame_time);
    assert_eq!(
        alive.borrow().as_slice(),
        &[1],
        "interrupted content should be removed once its fade-out completes"
    );
}

/// Everything a pane needs, bundled into one `PartialEq` argument — the shape
/// that lets `#[composable]` skip a call whose arguments did not change.
#[derive(Clone, Copy, PartialEq)]
struct Shell {
    rows: MutableState<usize>,
    revision: usize,
}

#[composable]
#[allow(non_snake_case)]
fn ShellRows(rows: MutableState<usize>) {
    let count = rows.get();
    for index in 0..count {
        cranpose_core::with_key(&index, || {
            Text(
                format!("Row {index}"),
                Modifier::empty().fill_max_width().height(24.0),
                TextStyle::default(),
            );
        });
    }
}

#[composable]
#[allow(non_snake_case)]
fn ShellFooter(revision: usize) {
    Text(
        format!("Footer {revision}"),
        Modifier::empty().fill_max_width().height(24.0),
        TextStyle::default(),
    );
}

/// A whole crossfade entry in one skippable composable: one half reads shared
/// state and one half only ever sees what it was passed.
#[composable]
#[allow(non_snake_case)]
fn ShellPane(shell: Shell) {
    ShellRows(shell.rows);
    ShellFooter(shell.revision);
}

#[composable]
#[allow(non_snake_case)]
fn ShellHost(rows: MutableState<usize>) {
    let shell = Shell {
        rows,
        revision: rows.get(),
    };
    Crossfade(
        0u32,
        tween(CROSSFADE_MILLIS, Easing::LinearEasing),
        move |_| ShellPane(shell),
    );
}

fn rendered_texts(composition: &mut TestComposition, root: NodeId) -> Vec<String> {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 320.0,
                height: 480.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    let scene = HeadlessRenderer::new().render(&layout);
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Text { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect()
}

/// A crossfade entry shows what its caller last handed it.
///
/// The content closure is captured fresh on every `Crossfade` call and stashed
/// in the remembered handle, so it never reaches `CrossfadeContents` as an
/// argument. When the entry's content is a composable that could skip, only
/// the half that reads shared state stays current: the half that reads its own
/// arguments keeps painting the values from the composition before.
#[test]
fn a_crossfade_entry_shows_what_its_caller_last_supplied() {
    let rows_slot = Rc::new(RefCell::new(None::<MutableState<usize>>));
    let mut composition = {
        let rows_slot = Rc::clone(&rows_slot);
        run_test_composition(move || {
            let rows = cranpose_core::rememberMutableStateOf(|| 2usize);
            rows_slot.borrow_mut().replace(rows);
            ShellHost(rows);
        })
    };
    let rows = rows_slot.borrow().expect("rows state captured");
    let root = composition.root().expect("root");

    assert_eq!(
        rendered_texts(&mut composition, root),
        vec![
            "Row 0".to_string(),
            "Row 1".to_string(),
            "Footer 2".to_string()
        ],
        "the entry should start with both rows and a footer that agrees"
    );

    rows.set(1);
    drain(&mut composition);

    assert_eq!(
        rendered_texts(&mut composition, root),
        vec!["Row 0".to_string(), "Footer 1".to_string()],
        "the footer reads the argument the caller recomposed with, so an entry \
         that kept the old closure leaves it stale while the rows move on"
    );
}

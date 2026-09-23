use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    LaunchedEffect, LaunchedEffectAsync, MemoryApplier, MutableState, NodeId, SideEffect, SlotId,
    location_key, mutableStateOf, remember, rememberCoroutineScope,
};

use crate::{
    BoxWithConstraints, Composition, LayoutEngine, Modifier, Placement, Size, SubcomposeLayout,
    SubcomposeLayoutScope, SubcomposeMeasureScope, Text, TextStyle,
};

const VIEWPORT: Size = Size {
    width: 200.0,
    height: 200.0,
};
const SETTLE_LIMIT: usize = 16;

#[derive(Clone, Copy)]
enum ProbeReader {
    MeasurePolicy,
    LaunchedEffect,
    LaunchedEffectAsync,
    CoroutineScope,
    SideEffect,
}

#[derive(Clone, Copy)]
enum ProbeSite {
    Slot,
    NestedSubcomposeLayout,
}

struct ProbeOutcome {
    probe_reads: usize,
    settled_invalidated: bool,
    write_invalidated: bool,
}

fn compute_layout(composition: &mut Composition<MemoryApplier>, root: NodeId) {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, VIEWPORT).expect("layout");
    applier.clear_runtime_handle();
}

fn settle(composition: &mut Composition<MemoryApplier>) {
    for _ in 0..SETTLE_LIMIT {
        composition.runtime_handle().drain_ui();
        if !composition
            .process_invalid_scopes()
            .expect("process invalid scopes")
        {
            return;
        }
    }
    panic!("composition did not settle within {SETTLE_LIMIT} passes");
}

fn read_probe(probe: MutableState<i32>, reads: &Cell<usize>) {
    let _ = probe.value();
    reads.set(reads.get() + 1);
}

fn launch_probe_reader(reader: ProbeReader, probe: MutableState<i32>, reads: Rc<Cell<usize>>) {
    match reader {
        ProbeReader::MeasurePolicy => {}
        ProbeReader::LaunchedEffect => {
            LaunchedEffect((), move |_| read_probe(probe, &reads));
        }
        ProbeReader::LaunchedEffectAsync => {
            LaunchedEffectAsync((), move |_| {
                Box::pin(async move { read_probe(probe, &reads) })
            });
        }
        ProbeReader::CoroutineScope => {
            let coroutines = rememberCoroutineScope();
            remember(move || coroutines.launch(async move { read_probe(probe, &reads) }));
        }
        ProbeReader::SideEffect => {
            SideEffect(move || read_probe(probe, &reads));
        }
    }
}

fn probe_slot(
    site: ProbeSite,
    reader: ProbeReader,
    probe: MutableState<i32>,
    reads: Rc<Cell<usize>>,
) {
    match site {
        ProbeSite::Slot => {
            Text("probe".to_string(), Modifier::empty(), TextStyle::default());
            launch_probe_reader(reader, probe, reads);
        }
        ProbeSite::NestedSubcomposeLayout => {
            BoxWithConstraints(Modifier::empty(), move |_| {
                launch_probe_reader(reader, probe, Rc::clone(&reads));
            });
        }
    }
}

fn invalidation_after_probe_write(site: ProbeSite, reader: ProbeReader) -> ProbeOutcome {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let probe = mutableStateOf(0);
    let probe_reads = Rc::new(Cell::new(0));

    let reads_for_content = Rc::clone(&probe_reads);
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            let reads = Rc::clone(&reads_for_content);
            SubcomposeLayout(Modifier::empty(), move |scope, constraints| {
                if matches!(reader, ProbeReader::MeasurePolicy) {
                    let _ = probe.value();
                }
                let reads = Rc::clone(&reads);
                let children = scope.subcompose(SlotId::new(0), (), move || {
                    probe_slot(site, reader, probe, Rc::clone(&reads));
                });
                let mut placements = Vec::with_capacity(children.len());
                for child in children {
                    let placeable = scope.measure(child, constraints);
                    placeable.place(0.0, 0.0);
                    placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
                }
                scope.layout(constraints.max_width, constraints.max_height, placements)
            });
        })
        .expect("initial render");
    let root = composition.root().expect("subcompose root");

    compute_layout(&mut composition, root);
    settle(&mut composition);
    let runtime = composition.runtime_handle();
    let settled_invalidated = runtime.has_invalid_scopes();

    probe.set(1);
    runtime.drain_ui();

    ProbeOutcome {
        probe_reads: probe_reads.get(),
        settled_invalidated,
        write_invalidated: runtime.has_invalid_scopes(),
    }
}

fn assert_probe_read_leaves_layout_clean(site: ProbeSite, reader: ProbeReader) {
    let outcome = invalidation_after_probe_write(site, reader);
    assert_eq!(outcome.probe_reads, 1, "the probe must read the state once");
    assert!(!outcome.settled_invalidated, "the composition must settle");
    assert!(
        !outcome.write_invalidated,
        "work queued by a subcomposition must not subscribe the layout that measured it"
    );
}

#[test]
fn a_measure_policy_that_reads_state_is_invalidated_when_it_changes() {
    let outcome = invalidation_after_probe_write(ProbeSite::Slot, ProbeReader::MeasurePolicy);
    assert!(!outcome.settled_invalidated, "the composition must settle");
    assert!(outcome.write_invalidated);
}

#[test]
fn an_effect_launched_by_a_nested_subcomposition_does_not_subscribe_the_measuring_layout() {
    assert_probe_read_leaves_layout_clean(
        ProbeSite::NestedSubcomposeLayout,
        ProbeReader::LaunchedEffect,
    );
}

#[test]
fn an_async_effect_launched_by_a_nested_subcomposition_does_not_subscribe_the_measuring_layout() {
    assert_probe_read_leaves_layout_clean(
        ProbeSite::NestedSubcomposeLayout,
        ProbeReader::LaunchedEffectAsync,
    );
}

#[test]
fn a_coroutine_launched_while_subcomposing_does_not_subscribe_the_measuring_layout() {
    assert_probe_read_leaves_layout_clean(ProbeSite::Slot, ProbeReader::CoroutineScope);
}

#[test]
fn a_side_effect_of_a_nested_subcomposition_does_not_subscribe_the_measuring_layout() {
    assert_probe_read_leaves_layout_clean(
        ProbeSite::NestedSubcomposeLayout,
        ProbeReader::SideEffect,
    );
}

//! The device pixel grid is a static composition local, as Compose's
//! `LocalDensity` is: a provider given a new grid recomposes everything it
//! provides to, content composed at measure time included, and nothing
//! outside it.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, MutableState, NodeId, location_key};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    AppContext, Box, BoxSpec, BoxWithConstraints, Column, ColumnSpec, Density, LayoutBox,
    LayoutEngine, LayoutNode, LazyColumn, LazyColumnSpec, Modifier, Size, composable, density,
    density::ProvideDensity,
};

type Seen = Rc<RefCell<Vec<(&'static str, f32)>>>;

#[composable]
fn Probe(label: &'static str, seen: Seen) {
    seen.borrow_mut().push((label, density().density()));
}

/// Takes nothing that changes: a call to it skips unless something forces
/// its body.
#[composable]
fn Middle(seen: Seen) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Probe("direct", Rc::clone(&seen));
        let subcomposed = Rc::clone(&seen);
        BoxWithConstraints(Modifier::empty(), move |_| {
            Probe("subcomposed", Rc::clone(&subcomposed));
            let nested = Rc::clone(&subcomposed);
            BoxWithConstraints(Modifier::empty(), move |_| {
                Probe("nested", Rc::clone(&nested));
            });
        });
        let list_state = rememberLazyListState();
        let lazy = Rc::clone(&seen);
        LazyColumn(
            Modifier::empty().height(100.0),
            list_state,
            LazyColumnSpec::default(),
            move |scope| {
                let lazy = Rc::clone(&lazy);
                scope.items(LazyItems::new(2), move |_| {
                    Probe("lazy", Rc::clone(&lazy));
                });
            },
        );
    });
}

/// Composes its probe only once `show` turns on, after the slot holding it
/// first composed.
#[composable]
fn Toggled(label: &'static str, show: MutableState<bool>, seen: Seen) {
    if show.value() {
        Probe(label, seen);
    }
}

#[composable]
fn ToggledScreen(grid: MutableState<f32>, show: MutableState<bool>, seen: Seen) {
    ProvideDensity(Density::new(grid.value(), 1.0), move || {
        let seen = Rc::clone(&seen);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let boxed = Rc::clone(&seen);
            BoxWithConstraints(Modifier::empty(), move |_| {
                Toggled("boxed", show, Rc::clone(&boxed));
            });
            let list_state = rememberLazyListState();
            let listed = Rc::clone(&seen);
            LazyColumn(
                Modifier::empty().height(100.0),
                list_state,
                LazyColumnSpec::default(),
                move |scope| {
                    let listed = Rc::clone(&listed);
                    scope.items(LazyItems::new(1), move |_| {
                        Toggled("listed", show, Rc::clone(&listed));
                    });
                },
            );
        });
    });
}

/// A row whose call never changes: a composition a list recycles for
/// another item skips it unless its body is forced.
#[composable]
fn Row() {
    Box(Modifier::empty().height(20.0), BoxSpec::default(), || {});
}

#[composable]
fn RecycledRows(grid: MutableState<f32>, list: Rc<RefCell<Option<LazyListState>>>) {
    ProvideDensity(Density::new(grid.value(), 1.0), move || {
        let list_state = rememberLazyListState();
        *list.borrow_mut() = Some(list_state);
        LazyColumn(
            Modifier::empty().height(100.0),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(LazyItems::new(60), |_| Row());
            },
        );
    });
}

#[composable]
fn Screen(grid: MutableState<f32>, tick: MutableState<u32>, seen: Seen) {
    let _ = tick.value();
    Probe("outside", Rc::clone(&seen));
    ProvideDensity(Density::new(grid.value(), 1.0), move || {
        Middle(Rc::clone(&seen));
    });
}

/// Recomposes, lays out and returns the grid of every laid-out node.
fn frame(context: &Rc<AppContext>, composition: &mut Composition<MemoryApplier>) -> Vec<f32> {
    fn collect(node: &LayoutBox, out: &mut Vec<NodeId>) {
        out.push(node.node_id);
        for child in &node.children {
            collect(child, out);
        }
    }
    context.enter(|| {
        while composition.process_invalid_scopes().expect("recomposition") {}
        let root = composition.root().expect("root");
        let runtime = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(runtime);
        let layout = applier
            .compute_layout(root, Size::new(320.0, 360.0))
            .expect("layout");
        let mut ids = Vec::new();
        collect(layout.root(), &mut ids);
        let grids = ids
            .into_iter()
            .filter_map(|id| {
                applier
                    .with_node::<LayoutNode, _>(id, |node| node.density().density())
                    .ok()
            })
            .collect();
        applier.clear_runtime_handle();
        grids
    })
}

/// The reads since the last call, each label once: a debug build composes
/// a measure-time slot it keeps a second time to check it.
fn reads(seen: &Seen) -> Vec<(&'static str, f32)> {
    let mut reads = std::mem::take(&mut *seen.borrow_mut());
    reads.sort_by(|a, b| a.0.cmp(b.0).then(a.1.total_cmp(&b.1)));
    reads.dedup();
    reads
}

#[test]
fn a_new_grid_recomposes_everything_its_provider_holds_and_nothing_else() {
    let context = AppContext::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let (grid, tick) = context.enter(|| {
        let runtime = composition.runtime_handle();
        (
            MutableState::with_runtime(2.0_f32, runtime.clone()),
            MutableState::with_runtime(0_u32, runtime),
        )
    });
    let seen: Seen = Rc::default();
    {
        let seen = Rc::clone(&seen);
        context
            .enter(|| {
                composition.render(location_key(file!(), line!(), column!()), move || {
                    Screen(grid, tick, Rc::clone(&seen));
                })
            })
            .expect("initial composition");
    }
    let _ = frame(&context, &mut composition);
    assert_eq!(
        reads(&seen),
        [
            ("direct", 2.0),
            ("lazy", 2.0),
            ("nested", 2.0),
            ("outside", 1.0),
            ("subcomposed", 2.0),
        ]
    );

    context.enter(|| grid.set(3.0));
    let _ = frame(&context, &mut composition);
    assert_eq!(
        reads(&seen),
        [
            ("direct", 3.0),
            ("lazy", 3.0),
            ("nested", 3.0),
            ("subcomposed", 3.0),
        ],
        "everything under the provider reads the new grid, and the reader \
         outside it, whose call did not change, skips"
    );

    // The provider runs again with the grid it had: what it holds skips.
    context.enter(|| tick.set(1));
    let _ = frame(&context, &mut composition);
    assert_eq!(reads(&seen), [], "an unchanged grid recomposes nothing");
}

#[test]
fn a_new_grid_reaches_content_a_slot_gained_after_it_first_composed() {
    let context = AppContext::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let (grid, show) = context.enter(|| {
        let runtime = composition.runtime_handle();
        (
            MutableState::with_runtime(2.0_f32, runtime.clone()),
            MutableState::with_runtime(false, runtime),
        )
    });
    let seen: Seen = Rc::default();
    {
        let seen = Rc::clone(&seen);
        context
            .enter(|| {
                composition.render(location_key(file!(), line!(), column!()), move || {
                    ToggledScreen(grid, show, Rc::clone(&seen));
                })
            })
            .expect("initial composition");
    }
    let _ = frame(&context, &mut composition);
    assert_eq!(reads(&seen), []);

    context.enter(|| show.set(true));
    let _ = frame(&context, &mut composition);
    assert_eq!(reads(&seen), [("boxed", 2.0), ("listed", 2.0)]);

    context.enter(|| grid.set(3.0));
    let _ = frame(&context, &mut composition);
    assert_eq!(reads(&seen), [("boxed", 3.0), ("listed", 3.0)]);
}

#[test]
fn a_row_a_list_recycles_after_a_new_grid_lands_on_that_grid() {
    let context = AppContext::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let grid = context.enter(|| MutableState::with_runtime(2.0_f32, composition.runtime_handle()));
    let list: Rc<RefCell<Option<LazyListState>>> = Rc::default();
    {
        let list = Rc::clone(&list);
        context
            .enter(|| {
                composition.render(location_key(file!(), line!(), column!()), move || {
                    RecycledRows(grid, Rc::clone(&list));
                })
            })
            .expect("initial composition");
    }
    assert!(
        frame(&context, &mut composition)
            .iter()
            .all(|grid| *grid == 2.0)
    );
    let list_state = list.borrow().as_ref().copied().expect("list state");
    // Rows leave the viewport, and their compositions wait for reuse.
    context.enter(|| list_state.scroll_to_item(20, 0.0));
    let _ = frame(&context, &mut composition);

    // The new rows take compositions made on the old grid.
    context.enter(|| {
        grid.set(3.0);
        list_state.scroll_to_item(40, 0.0);
    });
    let grids = frame(&context, &mut composition);
    assert!(
        grids.iter().all(|grid| *grid == 3.0),
        "every laid-out node is on the new grid: {grids:?}"
    );
}

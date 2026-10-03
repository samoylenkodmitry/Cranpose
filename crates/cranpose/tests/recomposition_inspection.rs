use std::{cell::Cell, rc::Rc};

use cranpose::{Box, BoxSpec, Modifier, composable, inspection};
use cranpose_core::{
    MutableState, rememberMutableStateOf, source_trace::set_recomposition_tracking,
};
use cranpose_ui::{LayoutEngine, Size, run_test_composition};

struct Tracking;
impl Drop for Tracking {
    fn drop(&mut self) {
        set_recomposition_tracking(false);
    }
}

#[composable]
fn TrackedScreen(value: MutableState<u32>) {
    let _ = value.get();
    Box(
        Modifier::empty().size(Size::new(80.0, 40.0)),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn recomposition_inspection_serializes_live_counts_from_a_retained_layout() {
    set_recomposition_tracking(true);
    let _tracking = Tracking;
    let state = Rc::new(Cell::new(None));
    let output = Rc::clone(&state);
    let mut composition = run_test_composition(move || {
        let value = rememberMutableStateOf(|| 0u32);
        output.set(Some(value));
        TrackedScreen(value);
    });
    let root = composition.root().expect("root");
    let runtime = composition.runtime_handle();
    let layout = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(runtime);
        let layout = applier
            .compute_layout(root, Size::new(320.0, 240.0))
            .expect("layout");
        applier.clear_runtime_handle();
        layout
    };
    let count = |request| {
        let json = serde_json::to_value(inspection::snapshot(Some(&layout), request, 100))
            .expect("snapshot encodes");
        json["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .flat_map(|node| node["sources"].as_array().expect("sources"))
            .find(|source| source["name"] == "TrackedScreen")
            .expect("composable source")["recompositions"]
            .as_u64()
    };
    assert_eq!(count(1), cfg!(debug_assertions).then_some(0));
    state.get().expect("state").set(1);
    composition.process_invalid_scopes().expect("recompose");
    assert_eq!(count(2), cfg!(debug_assertions).then_some(1));
    assert_eq!(count(3), cfg!(debug_assertions).then_some(1));
}

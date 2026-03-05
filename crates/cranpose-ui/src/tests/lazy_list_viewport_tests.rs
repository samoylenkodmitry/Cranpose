use super::*;
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use cranpose_ui_graphics::Size as ViewportSize;
use std::cell::RefCell;

thread_local! {
    static LAST_LAZY_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
}

#[test]
fn lazy_column_unbounded_height_matches_effective_viewport() {
    let mut composition = run_test_composition(|| {
        let list_state = remember_lazy_list_state();
        LAST_LAZY_STATE.with(|cell| {
            *cell.borrow_mut() = Some(list_state);
        });

        LazyColumn(
            Modifier::empty(),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(
                    100,
                    None::<fn(usize) -> u64>,
                    None::<fn(usize) -> u64>,
                    |_| {
                        Spacer(Size {
                            width: 0.0,
                            height: 100.0,
                        });
                    },
                );
            },
        );
    });

    let root = composition.root().expect("lazy column root");
    let handle = composition.runtime_handle();
    let measurements = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let result = measure_layout(
            &mut applier,
            root,
            ViewportSize {
                width: 320.0,
                height: f32::INFINITY,
            },
        )
        .expect("layout measurement");
        applier.clear_runtime_handle();
        result
    };

    let list_state = LAST_LAZY_STATE.with(|cell| (*cell.borrow()).expect("state captured"));
    let expected_height = list_state.layout_info().viewport_size;
    let actual_height = measurements.root_size().height;

    assert!(actual_height.is_finite());
    assert!(
        (actual_height - expected_height).abs() < 0.01,
        "expected lazy column height to match effective viewport {expected_height}, got {actual_height}"
    );

    LAST_LAZY_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

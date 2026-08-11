use super::*;
use cranpose_core::NodeId;
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope, LazyListState};
use cranpose_ui_graphics::Size as ViewportSize;
use std::cell::RefCell;
use std::rc::Rc;

const ROWS: usize = 40;
const ROW_HEIGHT: f32 = 48.0;
const VIEWPORT: ViewportSize = ViewportSize {
    width: 320.0,
    height: 200.0,
};

fn measure(composition: &mut TestComposition, root: NodeId) {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    measure_layout(&mut applier, root, VIEWPORT).expect("layout measurement");
    applier.clear_runtime_handle();
}

#[test]
fn a_recycled_lazy_row_stops_its_parked_effect() {
    let held: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let list_state = remember_lazy_list_state();
        *keeper.borrow_mut() = Some(list_state.clone());
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(
                    ROWS,
                    Some(|index: usize| index as u64),
                    None::<fn(usize) -> u64>,
                    |index| {
                        if index == 0 {
                            cranpose_core::LaunchedEffectAsync!(0u32, move |_scope| {
                                Box::pin(async move {
                                    std::future::pending::<()>().await;
                                })
                            });
                        }
                        Text(
                            format!("row {index}"),
                            Modifier::empty().height(ROW_HEIGHT),
                            TextStyle::default(),
                        );
                    },
                );
            },
        );
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    assert_eq!(
        composition.runtime_handle().debug_stats().tasks_len,
        1,
        "the first row parks one effect while it is on the screen"
    );

    let list_state = held.borrow().clone().expect("list state");
    list_state.scroll_to_item(ROWS - 4, 0.0);
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();

    assert_eq!(
        composition.runtime_handle().debug_stats().tasks_len,
        0,
        "the first row is off the screen, so its effect must be cancelled"
    );
}

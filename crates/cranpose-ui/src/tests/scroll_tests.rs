use crate::{collect_modifier_slices, Modifier, ScrollState};
use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{BasicModifierNodeContext, ModifierNodeChain};
use std::sync::Arc;

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

#[test]
fn vertical_scroll_clips_to_bounds_by_default() {
    with_test_runtime(|| {
        let scroll_state = ScrollState::new(0.0);
        let modifier = Modifier::empty().vertical_scroll(scroll_state, false);
        let elements = modifier.elements();

        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();
        chain.update_from_slice(&elements, &mut context);

        let slices = collect_modifier_slices(&chain);
        assert!(slices.clip_to_bounds());
    });
}

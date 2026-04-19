use cranpose_core::{with_current_composer, ValueSlotHandle};
use cranpose_macros::composable;

#[composable]
fn ReturnSlotHandle() -> ValueSlotHandle<'static> {
    with_current_composer(|composer| composer.use_value_slot(|| 1_i32))
}

fn main() {}

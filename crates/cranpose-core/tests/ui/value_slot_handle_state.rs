use cranpose_core::{mutableStateOfNeverEqual, with_current_composer};
use cranpose_macros::composable;

#[composable]
fn StoreSlotHandleInState() {
    with_current_composer(|composer| {
        let handle = composer.use_value_slot(|| 1_i32);
        let _state = mutableStateOfNeverEqual(handle);
    });
}

fn main() {}

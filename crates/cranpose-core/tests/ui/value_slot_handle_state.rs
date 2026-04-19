use cranpose_core::{mutableStateOf, with_current_composer};
use cranpose_macros::composable;

#[composable]
fn StoreSlotHandleInState() {
    with_current_composer(|composer| {
        let handle = composer.use_value_slot(|| 1_i32);
        let _state = mutableStateOf(handle);
    });
}

fn main() {}

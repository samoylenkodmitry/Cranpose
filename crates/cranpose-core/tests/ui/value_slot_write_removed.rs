use cranpose_core::with_current_composer;
use cranpose_macros::composable;

#[composable]
fn WriteSlotValue() {
    with_current_composer(|composer| {
        let handle = composer.use_value_slot(|| 1_i32);
        composer.write_slot_value(handle, 2_i32);
    });
}

fn main() {}

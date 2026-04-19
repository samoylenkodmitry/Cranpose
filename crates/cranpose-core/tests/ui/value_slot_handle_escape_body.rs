use cranpose_core::with_current_composer;
use cranpose_macros::composable;

#[composable]
fn LeakHandleIntoOuterBinding() {
    let mut leaked = None;
    with_current_composer(|composer| {
        leaked = Some(composer.use_value_slot(|| 1_i32));
    });
    let _ = leaked;
}

fn main() {}

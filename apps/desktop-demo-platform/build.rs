use cranpose_capabilities::{declare, Use};

fn main() {
    declare(&[Use::haptics(), Use::overlay()]).emit();
}

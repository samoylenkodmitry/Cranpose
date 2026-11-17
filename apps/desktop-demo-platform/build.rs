use cranpose_capabilities::{declare, Use};

fn main() {
    declare(&[
        Use::haptics(),
        Use::overlay(),
        Use::network(),
        Use::device_auth("Unlocks the demo with your face."),
    ])
    .emit();
}

#![allow(
    clippy::disallowed_methods,
    clippy::print_stdout,
    reason = "a build script reads cargo's environment and prints cargo's directives"
)]

use cranpose_capabilities::{Use, declare};

// The tracking that the demo asks for. Hand tracking needs no prompt on a Meta Quest, so the
// demo always asks for it. `XR_DEMO_TRACKING=all` adds the body, the face and the eyes, which a
// headset asks the user to allow.
fn main() {
    println!("cargo::rerun-if-env-changed=XR_DEMO_TRACKING");
    if std::env::var("XR_DEMO_TRACKING").is_ok_and(|value| value == "all") {
        declare(&[
            Use::hand_tracking(),
            Use::body_tracking(),
            Use::face_tracking(),
            Use::eye_tracking(),
        ])
        .emit();
    } else {
        declare(&[Use::hand_tracking()]).emit();
    }
}

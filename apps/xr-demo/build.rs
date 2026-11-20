#![allow(
    clippy::disallowed_methods,
    clippy::print_stdout,
    reason = "a build script reads cargo's environment and prints cargo's directives"
)]

use cranpose_capabilities::{Use, declare};

// The tracking that the demo asks for. Hand tracking needs no prompt on a Meta Quest, so the demo
// asks for it unless `XR_DEMO_TRACKING` says otherwise: `none` asks for nothing, which shows what a
// runtime lists when the app enables no extension, and `all` adds the body, the face and the eyes,
// which a headset asks the user to allow.
//
// `XR_DEMO_VOLUME=through` builds the depth test: a panel and a torus that pierces it, a meter ahead
// of the head, in place of the demo's content. A compositor that tests depth shows the ring's near
// half over the panel and hides its far half. One that does not shows the panel over all of it.
// `XR_DEMO_VOLUME=alone` builds the same scene without the panel, as the reference for
// `scripts/dev/xr_depth_check.py`.
fn main() {
    println!("cargo::rerun-if-env-changed=XR_DEMO_VOLUME");
    if let Ok(place) = std::env::var("XR_DEMO_VOLUME") {
        println!("cargo::rustc-env=XR_DEMO_VOLUME={place}");
    }
    // `XR_DEMO_SCRIPTED_HAND=right` (or `left`) builds a hand that the backend plays in place of the
    // cameras': it pinches the counter's button in a cycle. The Android XR emulator's hand
    // simulation does not reach the runtime, so the build shows the hand pointer there.
    println!("cargo::rerun-if-env-changed=XR_DEMO_SCRIPTED_HAND");
    if let Ok(hand) = std::env::var("XR_DEMO_SCRIPTED_HAND") {
        println!("cargo::rustc-env=XR_DEMO_SCRIPTED_HAND={hand}");
    }
    // `XR_DEMO_ROOM` (`with`, `only` or `script`) shows the room panel, and the build asks for the
    // anchors and the planes that it needs.
    println!("cargo::rerun-if-env-changed=XR_DEMO_ROOM");
    if let Ok(room) = std::env::var("XR_DEMO_ROOM") {
        println!("cargo::rustc-env=XR_DEMO_ROOM={room}");
        declare(&[Use::hand_tracking(), Use::anchors(), Use::plane_tracking()]).emit();
        return;
    }
    println!("cargo::rerun-if-env-changed=XR_DEMO_TRACKING");
    match std::env::var("XR_DEMO_TRACKING").as_deref() {
        Ok("none") => declare(&[]).emit(),
        Ok("all") => declare(&[
            Use::hand_tracking(),
            Use::body_tracking(),
            Use::face_tracking(),
            Use::eye_tracking(),
        ])
        .emit(),
        _ => declare(&[Use::hand_tracking()]).emit(),
    }
}

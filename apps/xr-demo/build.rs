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
// `XR_DEMO_VOLUME=through` puts the demo's torus in the plane of the counter panel, so that a
// part of it lies in front of the panel and a part behind. A compositor that tests depth shows the
// front part over the panel and hides the rest, and one that does not shows the panel over all of it.
fn main() {
    println!("cargo::rerun-if-env-changed=XR_DEMO_VOLUME");
    if let Ok(place) = std::env::var("XR_DEMO_VOLUME") {
        println!("cargo::rustc-env=XR_DEMO_VOLUME={place}");
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

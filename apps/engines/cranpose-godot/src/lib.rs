#![expect(
    clippy::redundant_field_names,
    reason = "godot-rust's `#[class(init)]` derive writes `base: base`"
)]

mod hud_demo;
mod surface;
mod window_demo;
mod world;

cranpose_gpu_interop::godot_extension!(CranposeExtension);

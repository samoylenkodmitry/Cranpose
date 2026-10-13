#![expect(
    clippy::redundant_field_names,
    reason = "godot-rust's `#[class(init)]` derive writes `base: base`"
)]

mod hud_demo;
mod surface;
mod window_demo;
mod world;

use godot::prelude::*;

struct CranposeExtension;

#[gdextension]
unsafe impl ExtensionLibrary for CranposeExtension {}

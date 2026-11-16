// The XR tab: a small 3D scene of live composables, seen flat, as two eyes side by side, or in
// a headset through the browser's WebXR session.

mod eye_view;
mod page;
mod scene;
mod tab;

pub(crate) use page::presenting;
pub use page::{set_immersive_entry, set_presenting, set_views};
pub(crate) use tab::{ImmersiveView, XrTab};

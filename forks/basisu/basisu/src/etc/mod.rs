//! ETC1 and ETC2-EAC block encoding, shared by the UASTC-to-ETC and
//! ETC1S-to-ETC transcode paths. `tables` holds the intensity and selector
//! constants; `block` the block bit layout; `uastc_to_etc1` and `uastc_to_etc2`
//! the per-block encoders built on top of them.

pub mod block;
pub mod tables;
pub mod uastc_to_etc1;
pub mod uastc_to_etc2;

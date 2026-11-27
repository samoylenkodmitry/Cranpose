//! Real-time re-encoders used by the raw-ASTC/HDR-6x6 (and later XUASTC)
//! decode paths: decoded pixels are re-encoded into compressed target blocks.
//! `etc1f` packs ETC1 color (and ETC2's color half), `bc7f` packs BC7, and
//! `bc6h_enc` packs unsigned BC6H from half-float texels.

pub mod bc6h_enc;
pub mod bc7f;
pub mod etc1f;
pub mod etc1f_tables;

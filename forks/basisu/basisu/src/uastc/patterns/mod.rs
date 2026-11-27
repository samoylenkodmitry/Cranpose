//! UASTC and BC7 shared partition pattern and anchor tables, stored as flat
//! `u8` arrays. The anchor tables are declared `[N][3]`, zero-filling any row
//! whose pattern lists fewer than three anchors so every row has a fixed width
//! and the tables keep a stable byte layout.

mod bc7_3_astc2_patterns2;
mod bc7_3_astc2_patterns2_anchors;
mod pattern2_anchors;
mod pattern3_anchors;
mod patterns2;
mod patterns3;

pub use bc7_3_astc2_patterns2::BC7_3_ASTC2_PATTERNS2;
pub use bc7_3_astc2_patterns2_anchors::BC7_3_ASTC2_PATTERNS2_ANCHORS;
pub use pattern2_anchors::ASTC_BC7_PATTERN2_ANCHORS;
pub use pattern3_anchors::ASTC_BC7_PATTERN3_ANCHORS;
pub use patterns2::ASTC_BC7_PATTERNS2;
pub use patterns3::ASTC_BC7_PATTERNS3;

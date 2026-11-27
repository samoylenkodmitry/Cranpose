//! Lookup tables used by the decoders. Two kinds live here: static constant
//! tables holding precomputed transcode data, and tables built in code once at
//! first use.

// The shared entry type the single-color match tables are built from.
pub mod solution;

// Tables built once at first use.
pub mod bc1;

// Static constant tables holding precomputed transcode data.
pub mod astc;
pub mod astc_0_255;
pub mod atc_55;
pub mod atc_56;
pub mod bc7_m5_alpha;
pub mod bc7_m5_color;
pub mod bc7_m5_equals_1;
pub mod dxt1_5;
pub mod dxt1_6;
pub mod dxt5a;
pub mod etc2_eac_a8;
pub mod etc2_eac_r11;
pub mod pvrtc2_45;

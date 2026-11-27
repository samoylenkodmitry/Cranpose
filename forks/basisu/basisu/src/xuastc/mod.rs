//! The XUASTC LDR intermediate codec (`astc_ldr_t`, KTX2 DFD model 169,
//! `.basis` tex_formats 5..=18): an arithmetic- or zstd-coded stream of
//! logical ASTC LDR blocks (any of the 14 block sizes), decompressed block
//! by block and then transcoded through the raw-ASTC LDR target machinery
//! plus dedicated BC7 fast paths.

pub mod arith;
pub mod bc7_fast;
pub mod dct;
pub mod decode;
pub mod endpoints;
pub mod idct;
pub mod idct_tables;
pub mod modes;
pub mod tables;
pub mod transcode;

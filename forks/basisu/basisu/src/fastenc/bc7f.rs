//! Real-time BC7 encoder for decoded RGBA pixels, used by the raw-ASTC BC7
//! target. Every `f32` operation keeps a fixed evaluation order and explicit
//! operands (no `mul_add`), so fused-multiply-add contraction cannot change a
//! result and the packed bytes come out the same on every platform and build.
//! Only scalar paths exist here; there are no SIMD variants to keep in sync.
// Index-based loops and long argument lists are kept where a direct one-to-one
// mapping to the bit layout reads more clearly than an iterator rewrite.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

use crate::once::OnceBox;
use crate::uastc::bc7_tables::{
    G_BC7_ANCHOR_SECOND, G_BC7_ANCHOR_THIRD_1, G_BC7_ANCHOR_THIRD_2, G_BC7_PARTITION2,
    G_BC7_PARTITION3,
};
use alloc::boxed::Box;

/// One decoded texel, RGBA byte order.
pub type Rgba = [u8; 4];

/// The low-level BC7 encoder configuration bits.
pub mod flags {
    pub const USE_2_SUBSETS_RGB: u32 = 1;
    pub const USE_2_SUBSETS_RGBA: u32 = 2;
    pub const USE_3_SUBSETS_RGB: u32 = 4;
    pub const USE_DUAL_PLANE_RGB: u32 = 8;
    pub const USE_DUAL_PLANE_RGBA: u32 = 16;
    pub const PBIT_OPT: u32 = 32;
    pub const PBIT_OPT_MODE6: u32 = 64;
    pub const USE_TRIVIAL_MODE6: u32 = 128;
    pub const PARTIALLY_ANALYTICAL_RGB: u32 = 256;
    pub const PARTIALLY_ANALYTICAL_RGBA: u32 = 512;

    /// The default flag set, used when HIGH_QUALITY is off.
    pub const DEFAULT: u32 = USE_2_SUBSETS_RGB
        | USE_2_SUBSETS_RGBA
        | USE_3_SUBSETS_RGB
        | USE_DUAL_PLANE_RGB
        | USE_DUAL_PLANE_RGBA
        | PBIT_OPT
        | PBIT_OPT_MODE6
        | USE_TRIVIAL_MODE6;
    /// The flag set HIGH_QUALITY selects: the default plus the
    /// partially-analytical passes.
    pub const DEFAULT_PARTIALLY_ANALYTICAL: u32 =
        DEFAULT | PARTIALLY_ANALYTICAL_RGB | PARTIALLY_ANALYTICAL_RGBA;
}

/// The BC7 interpolation weight ramps for 2-, 3-, and 4-bit indices.
pub(crate) const BC7_WEIGHTS2: [u32; 4] = [0, 21, 43, 64];
pub(crate) const BC7_WEIGHTS3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
pub(crate) const BC7_WEIGHTS4: [u32; 16] =
    [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

const MAX_PATTERNS2_TO_CHECK: usize = 64;
const MAX_PATTERNS3_TO_CHECK: usize = 64;
const UNIQUE_PBIT_DISCOUNT: f32 = 0.85;
const SHARED_PBIT_DISCOUNT: f32 = 0.95;

/// Round-half-up for a non-negative float, via truncation of `x + 0.5`.
#[inline]
fn fast_roundf_pos_int(x: f32) -> i32 {
    (x + 0.5) as i32
}

/// Round-half-away-from-zero via truncation.
#[inline]
fn fast_roundf_int(x: f32) -> i32 {
    if x >= 0.0 {
        (x + 0.5) as i32
    } else {
        (x - 0.5) as i32
    }
}

/// Expand a 7-bit endpoint code to 8 bits, replicating the high bit into the
/// new low bit.
#[inline]
fn from_7(v: u32) -> u32 {
    (v << 1) | (v >> 6)
}

/// Append pbit `p` to a 7-bit code, forming the full 8-bit endpoint (7 code
/// bits plus the pbit already fill a byte, so no bit replication is needed).
#[inline]
fn from_7_p(v: u32, p: u32) -> u32 {
    (v << 1) | p
}

/// Quantize an 8-bit component to 7 bits, honoring `pbit`.
#[inline]
fn to_7_int_p(c8: i32, pbit: i32) -> i32 {
    let e = ((c8 as u32) + ((pbit as u32) ^ 1)) >> 1;
    127.min(e) as i32
}

/// Quantize an 8-bit component to 7 bits, rounded, with no pbit.
#[inline]
fn to_7_int(c8: i32) -> i32 {
    (c8 * 127 + 127) / 255
}

/// Quantize a non-negative float to 7 bits plus `pbit`, rounding the float
/// first.
#[inline]
fn to_7_f(c: f32, pbit: i32) -> i32 {
    to_7_int_p(fast_roundf_pos_int(c), pbit)
}

/// Quantize a possibly out-of-range float to 7 bits plus `pbit`, clamping to
/// 0..=255 before quantizing.
#[inline]
fn to_7_clamp(c: f32, pbit: i32) -> i32 {
    to_7_int_p(fast_roundf_int(c).clamp(0, 255), pbit)
}

/// Quantize an 8-bit component to 5 bits, rounded.
#[inline]
fn to_5_int(c8: i32) -> i32 {
    (c8 * 31 + 127) / 255
}

/// Quantize a float to 5 bits, clamped to 0..=31, with no pbit.
#[inline]
fn to_5_clamp_np(c: f32) -> i32 {
    fast_roundf_int(c * (31.0 / 255.0)).clamp(0, 31)
}

/// Quantize an 8-bit component to 6 bits, rounded.
#[inline]
fn to_6_int(c8: i32) -> i32 {
    (c8 * 63 + 127) / 255
}

/// Quantize to 6 bits plus `pbit`, nudging the 7-bit code to the requested
/// parity in the direction of lower error.
#[inline]
fn to_6_int_p(c8: i32, pbit: i32) -> i32 {
    let mut q7 = (c8 * 127 + 127) / 255;
    if (q7 & 1) != pbit {
        let lhs = c8 * 127;
        let rhs = 255 * q7;
        if lhs >= rhs {
            q7 = if q7 < 127 { q7 + 1 } else { q7 - 1 };
        } else {
            q7 = if q7 > 0 { q7 - 1 } else { q7 + 1 };
        }
    }
    q7 >> 1
}

/// Quantize a float to 6 bits plus `pbit`, clamping to 0..=255 first.
#[inline]
fn to_6_clamp(c: f32, pbit: i32) -> i32 {
    to_6_int_p(fast_roundf_int(c).clamp(0, 255), pbit)
}

/// Append pbit `p` to a 6-bit code, then expand the resulting 7-bit value to
/// 8 bits by replication.
#[inline]
fn from_6_p(v: u32, p: u32) -> u32 {
    let v = (v << 1) | p;
    (v << 1) | (v >> 6)
}

/// Append pbit `p` to a 4-bit code, then expand the resulting 5-bit value to
/// 8 bits by replication.
#[inline]
fn from_4_p(v: u32, p: u32) -> u32 {
    let v = (v << 1) | p;
    (v << 3) | (v >> 2)
}

/// Expand a 5-bit endpoint code to 8 bits by replication.
#[inline]
fn from_5(v: u32) -> u32 {
    (v << 3) | (v >> 2)
}

/// Append pbit `p` to a 5-bit code, then expand the resulting 6-bit value to
/// 8 bits by replication.
#[inline]
fn from_5_p(v: u32, p: u32) -> u32 {
    let v = (v << 1) | p;
    (v << 2) | (v >> 4)
}

/// Expand a 6-bit endpoint code to 8 bits by replication.
#[inline]
fn from_6(v: u32) -> u32 {
    (v << 2) | (v >> 4)
}

/// Quantize to 5 bits plus `pbit` (via the 6-bit code's parity).
#[inline]
fn to_5_int_p(c8: i32, pbit: i32) -> i32 {
    let mut q6 = (c8 * 63 + 127) / 255;
    if (q6 & 1) != pbit {
        let lhs = c8 * 63;
        let rhs = 255 * q6;
        if lhs >= rhs {
            q6 = if q6 < 63 { q6 + 1 } else { q6 - 1 };
        } else {
            q6 = if q6 > 0 { q6 - 1 } else { q6 + 1 };
        }
    }
    q6 >> 1
}

/// Quantize a float to 5 bits plus `pbit`, clamping to 0..=255 first.
#[inline]
fn to_5_clamp(c: f32, pbit: i32) -> i32 {
    to_5_int_p(fast_roundf_int(c).clamp(0, 255), pbit)
}

/// The least-squares weight tables and partition bitmasks, built once on first
/// use. Each ls-tab entry is the 4-tuple `(w*w, (1-w)*w, (1-w)*(1-w), w)` for
/// the normalized weight `w`.
pub(crate) struct Tables {
    /// Least-squares weight tab for the 2-bit index ramp (4 entries).
    pub ls2: [[f32; 4]; 4],
    /// Least-squares weight tab for the 3-bit index ramp (8 entries).
    pub ls3: [[f32; 4]; 8],
    /// Least-squares weight tab for the 4-bit index ramp (16 entries).
    pub ls4: [[f32; 4]; 16],
    /// Bit i set = texel i is in subset 1 (2-subset partitions).
    pub part2_bitmasks: [u16; 64],
    /// Low 16 bits: subset-0 texels; high 16: subset-1 (3-subset partitions).
    pub part3_bitmasks: [u32; 64],
}

/// The [`Tables`], built on first call and shared for the process lifetime.
pub(crate) fn tables() -> &'static Tables {
    static TABLES: OnceBox<Tables> = OnceBox::new();
    TABLES.get_or_init(|| {
        let mut t = Box::new(Tables {
            ls2: [[0.0; 4]; 4],
            ls3: [[0.0; 4]; 8],
            ls4: [[0.0; 4]; 16],
            part2_bitmasks: [0; 64],
            part3_bitmasks: [0; 64],
        });
        for i in 0..4 {
            let w = BC7_WEIGHTS2[i] as f32 * (1.0 / 64.0);
            t.ls2[i] = [w * w, (1.0 - w) * w, (1.0 - w) * (1.0 - w), w];
        }
        for i in 0..8 {
            let w = BC7_WEIGHTS3[i] as f32 * (1.0 / 64.0);
            t.ls3[i] = [w * w, (1.0 - w) * w, (1.0 - w) * (1.0 - w), w];
        }
        for i in 0..16 {
            let w = BC7_WEIGHTS4[i] as f32 * (1.0 / 64.0);
            t.ls4[i] = [w * w, (1.0 - w) * w, (1.0 - w) * (1.0 - w), w];
        }
        for i in 0..64 {
            let mut y = 0u16;
            for x in 0..16 {
                y |= (G_BC7_PARTITION2[i * 16 + x] as u16) << x;
            }
            t.part2_bitmasks[i] = y;
        }
        for i in 0..64 {
            for j in 0..16 {
                let s = G_BC7_PARTITION3[i * 16 + j];
                if s == 0 {
                    t.part3_bitmasks[i] |= 1 << j;
                } else if s == 1 {
                    t.part3_bitmasks[i] |= 0x10000 << j;
                }
            }
        }
        t
    })
}

/// `encode_mode0_rgb_block`: 3 subsets, 4-bit partition id, 4-bit endpoints,
/// 6 unique pbits, 3-bit weights.
fn encode_mode0_rgb_block(
    block: &mut [u8; 16],
    part_id: u32,
    lr: &mut [u32; 3],
    lg: &mut [u32; 3],
    lb: &mut [u32; 3],
    hr: &mut [u32; 3],
    hg: &mut [u32; 3],
    hb: &mut [u32; 3],
    p: &mut [u32; 6],
    weights: &[u8; 16],
) {
    let part_map = &G_BC7_PARTITION3[(part_id as usize) * 16..(part_id as usize) * 16 + 16];
    let anchor_index0 = G_BC7_ANCHOR_THIRD_1[part_id as usize] as usize;
    let anchor_index1 = G_BC7_ANCHOR_THIRD_2[part_id as usize] as usize;

    let mut weight_inv = [0u8; 3];
    if weights[0] & 4 != 0 {
        core::mem::swap(&mut lr[0], &mut hr[0]);
        core::mem::swap(&mut lg[0], &mut hg[0]);
        core::mem::swap(&mut lb[0], &mut hb[0]);
        p.swap(0, 1);
        weight_inv[0] = 7;
    }
    if weights[anchor_index0] & 4 != 0 {
        core::mem::swap(&mut lr[1], &mut hr[1]);
        core::mem::swap(&mut lg[1], &mut hg[1]);
        core::mem::swap(&mut lb[1], &mut hb[1]);
        p.swap(2, 3);
        weight_inv[1] = 7;
    }
    if weights[anchor_index1] & 4 != 0 {
        core::mem::swap(&mut lr[2], &mut hr[2]);
        core::mem::swap(&mut lg[2], &mut hg[2]);
        core::mem::swap(&mut lb[2], &mut hb[2]);
        p.swap(4, 5);
        weight_inv[2] = 7;
    }

    let low: u64 = 1
        | ((part_id as u64) << 1)
        | ((lr[0] as u64) << 5)
        | ((hr[0] as u64) << 9)
        | ((lr[1] as u64) << 13)
        | ((hr[1] as u64) << 17)
        | ((lr[2] as u64) << 21)
        | ((hr[2] as u64) << 25)
        | ((lg[0] as u64) << 29)
        | ((hg[0] as u64) << 33)
        | ((lg[1] as u64) << 37)
        | ((hg[1] as u64) << 41)
        | ((lg[2] as u64) << 45)
        | ((hg[2] as u64) << 49)
        | ((lb[0] as u64) << 53)
        | ((hb[0] as u64) << 57)
        | ((lb[1] as u64) << 61);
    block[0..8].copy_from_slice(&low.to_le_bytes());

    let mut high: u64 = ((lb[1] >> 3) as u64)
        | ((hb[1] as u64) << 1)
        | ((lb[2] as u64) << 5)
        | ((hb[2] as u64) << 9)
        | ((p[0] as u64) << 13)
        | ((p[1] as u64) << 14)
        | ((p[2] as u64) << 15)
        | ((p[3] as u64) << 16)
        | ((p[4] as u64) << 17)
        | ((p[5] as u64) << 18);

    let mut ofs = 19u32;
    for i in 0..16usize {
        let subset_index = part_map[i] as usize;
        let w = (weights[i] ^ weight_inv[subset_index]) as u64;
        high |= w << ofs;
        ofs += 3 - u32::from(i == 0 || i == anchor_index0 || i == anchor_index1);
    }
    debug_assert_eq!(ofs, 64);
    block[8..16].copy_from_slice(&high.to_le_bytes());
}

/// `encode_mode1_rgb_block`: 2 subsets, 6-bit partition id, 6-bit endpoints,
/// 2 shared pbits, 3-bit weights.
pub(crate) fn encode_mode1_rgb_block(
    block: &mut [u8; 16],
    part_id: u32,
    lr: &mut [u32; 2],
    lg: &mut [u32; 2],
    lb: &mut [u32; 2],
    hr: &mut [u32; 2],
    hg: &mut [u32; 2],
    hb: &mut [u32; 2],
    p0: u32,
    p1: u32,
    weights: &[u8; 16],
) {
    let part_map = &G_BC7_PARTITION2[(part_id as usize) * 16..(part_id as usize) * 16 + 16];
    let anchor_index = G_BC7_ANCHOR_SECOND[part_id as usize] as usize;

    let mut weight_inv = [0u8; 2];
    if weights[0] & 4 != 0 {
        core::mem::swap(&mut lr[0], &mut hr[0]);
        core::mem::swap(&mut lg[0], &mut hg[0]);
        core::mem::swap(&mut lb[0], &mut hb[0]);
        weight_inv[0] = 7;
    }
    if weights[anchor_index] & 4 != 0 {
        core::mem::swap(&mut lr[1], &mut hr[1]);
        core::mem::swap(&mut lg[1], &mut hg[1]);
        core::mem::swap(&mut lb[1], &mut hb[1]);
        weight_inv[1] = 7;
    }

    block[0] = (0b10 | (part_id << 2)) as u8;

    let x: u64 = (lr[0] as u64)
        | ((hr[0] as u64) << 6)
        | ((lr[1] as u64) << 12)
        | ((hr[1] as u64) << 18)
        | ((lg[0] as u64) << 24)
        | ((hg[0] as u64) << 30)
        | ((lg[1] as u64) << 36)
        | ((hg[1] as u64) << 42)
        | ((lb[0] as u64) << 48)
        | ((hb[0] as u64) << 54)
        | ((lb[1] as u64) << 60);
    block[1..9].copy_from_slice(&x.to_le_bytes());
    block[9] = ((lb[1] >> 4) | (hb[1] << 2)) as u8;

    let mut y: u64 = (p0 as u64) | ((p1 as u64) << 1);
    let mut ofs = 2u32;
    for i in 0..16usize {
        let subset_index = part_map[i] as usize;
        let w = (weights[i] ^ weight_inv[subset_index]) as u64;
        y |= w << ofs;
        ofs += 3 - u32::from(i == 0 || i == anchor_index);
    }
    debug_assert_eq!(ofs, 48);
    block[10..16].copy_from_slice(&y.to_le_bytes()[..6]);
}

/// `encode_mode2_rgb_block`: 3 subsets, 6-bit partition id, 5-bit endpoints,
/// no pbits, 2-bit weights.
fn encode_mode2_rgb_block(
    block: &mut [u8; 16],
    part_id: u32,
    lr: &mut [u32; 3],
    lg: &mut [u32; 3],
    lb: &mut [u32; 3],
    hr: &mut [u32; 3],
    hg: &mut [u32; 3],
    hb: &mut [u32; 3],
    weights: &[u8; 16],
) {
    let part_map = &G_BC7_PARTITION3[(part_id as usize) * 16..(part_id as usize) * 16 + 16];

    let mut weight_inv = [0u8; 3];
    if weights[0] & 2 != 0 {
        core::mem::swap(&mut lr[0], &mut hr[0]);
        core::mem::swap(&mut lg[0], &mut hg[0]);
        core::mem::swap(&mut lb[0], &mut hb[0]);
        weight_inv[0] = 3;
    }
    let anchor_index0 = G_BC7_ANCHOR_THIRD_1[part_id as usize] as usize;
    if weights[anchor_index0] & 2 != 0 {
        core::mem::swap(&mut lr[1], &mut hr[1]);
        core::mem::swap(&mut lg[1], &mut hg[1]);
        core::mem::swap(&mut lb[1], &mut hb[1]);
        weight_inv[1] = 3;
    }
    let anchor_index1 = G_BC7_ANCHOR_THIRD_2[part_id as usize] as usize;
    if weights[anchor_index1] & 2 != 0 {
        core::mem::swap(&mut lr[2], &mut hr[2]);
        core::mem::swap(&mut lg[2], &mut hg[2]);
        core::mem::swap(&mut lb[2], &mut hb[2]);
        weight_inv[2] = 3;
    }

    let v: u64 = 0b100
        | ((part_id as u64) << 3)
        | ((lr[0] as u64) << 9)
        | ((hr[0] as u64) << 14)
        | ((lr[1] as u64) << 19)
        | ((hr[1] as u64) << 24)
        | ((lr[2] as u64) << 29)
        | ((hr[2] as u64) << 34)
        | ((lg[0] as u64) << 39)
        | ((hg[0] as u64) << 44)
        | ((lg[1] as u64) << 49)
        | ((hg[1] as u64) << 54)
        | ((lg[2] as u64) << 59);
    block[0..8].copy_from_slice(&v.to_le_bytes());

    let mut v1: u64 = (hg[2] as u64)
        | ((lb[0] as u64) << 5)
        | ((hb[0] as u64) << 10)
        | ((lb[1] as u64) << 15)
        | ((hb[1] as u64) << 20)
        | ((lb[2] as u64) << 25)
        | ((hb[2] as u64) << 30);
    block[8..12].copy_from_slice(&(v1 as u32).to_le_bytes());
    v1 >>= 32;

    // 3 bits of hb[2] left over in v1.
    let mut ofs = 3u32;
    for i in 0..16usize {
        let subset_index = part_map[i] as usize;
        let w = (weights[i] ^ weight_inv[subset_index]) as u64;
        v1 |= w << ofs;
        ofs += 2 - u32::from(i == 0 || i == anchor_index0 || i == anchor_index1);
    }
    debug_assert_eq!(ofs, 32);
    block[12..16].copy_from_slice(&(v1 as u32).to_le_bytes());
}

/// `encode_mode3_rgb_block`: 2 subsets, 6-bit partition id, 7-bit endpoints,
/// 4 unique pbits, 2-bit weights.
fn encode_mode3_rgb_block(
    block: &mut [u8; 16],
    part_id: u32,
    lr: &mut [u32; 2],
    lg: &mut [u32; 2],
    lb: &mut [u32; 2],
    hr: &mut [u32; 2],
    hg: &mut [u32; 2],
    hb: &mut [u32; 2],
    p: &mut [u32; 4],
    weights: &[u8; 16],
) {
    let part_map = &G_BC7_PARTITION2[(part_id as usize) * 16..(part_id as usize) * 16 + 16];
    let anchor_index = G_BC7_ANCHOR_SECOND[part_id as usize] as usize;

    let mut weight_inv = [0u8; 2];
    if weights[0] & 2 != 0 {
        core::mem::swap(&mut lr[0], &mut hr[0]);
        core::mem::swap(&mut lg[0], &mut hg[0]);
        core::mem::swap(&mut lb[0], &mut hb[0]);
        p.swap(0, 1);
        weight_inv[0] = 3;
    }
    if weights[anchor_index] & 2 != 0 {
        core::mem::swap(&mut lr[1], &mut hr[1]);
        core::mem::swap(&mut lg[1], &mut hg[1]);
        core::mem::swap(&mut lb[1], &mut hb[1]);
        p.swap(2, 3);
        weight_inv[1] = 3;
    }

    let x: u64 = 0b1000
        | ((part_id as u64) << 4)
        | ((lr[0] as u64) << 10)
        | ((hr[0] as u64) << 17)
        | ((lr[1] as u64) << 24)
        | ((hr[1] as u64) << 31)
        | ((lg[0] as u64) << 38)
        | ((hg[0] as u64) << 45)
        | ((lg[1] as u64) << 52)
        | ((hg[1] as u64) << 59);
    block[0..8].copy_from_slice(&x.to_le_bytes());

    // 2 bits of hg[1] remain; then blue, then the four pbits (34 bits total).
    let mut y: u64 = ((hg[1] >> 5) as u64)
        | ((lb[0] as u64) << 2)
        | ((hb[0] as u64) << 9)
        | ((lb[1] as u64) << 16)
        | ((hb[1] as u64) << 23)
        | ((p[0] as u64) << 30)
        | ((p[1] as u64) << 31)
        | ((p[2] as u64) << 32)
        | ((p[3] as u64) << 33);

    let mut ofs = 34u32;
    for i in 0..16usize {
        let subset_index = part_map[i] as usize;
        let w = (weights[i] ^ weight_inv[subset_index]) as u64;
        y |= w << ofs;
        ofs += 2 - u32::from(i == 0 || i == anchor_index);
    }
    debug_assert_eq!(ofs, 64);
    block[8..16].copy_from_slice(&y.to_le_bytes());
}

/// `encode_mode4_rgba_block`: single subset, 5-bit RGB + 6-bit A endpoints,
/// no pbits, one 3-bit and one 2-bit index plane with a rotation and an
/// index-swap flag.
fn encode_mode4_rgba_block(
    block: &mut [u8; 16],
    mut lr: u32,
    mut lg: u32,
    mut lb: u32,
    mut la: u32,
    mut hr: u32,
    mut hg: u32,
    mut hb: u32,
    mut ha: u32,
    weights0: &[u8; 16],
    weights1: &[u8; 16],
    rot_index: u32,
    index_flag: u32,
) {
    let mut weights_inv = [0u8; 2];
    let w2bit: &[u8; 16] = if index_flag != 0 { weights1 } else { weights0 };
    let w3bit: &[u8; 16] = if index_flag != 0 { weights0 } else { weights1 };

    if w3bit[0] & 4 != 0 {
        weights_inv[0] = 7;
        if index_flag != 0 {
            core::mem::swap(&mut lr, &mut hr);
            core::mem::swap(&mut lg, &mut hg);
            core::mem::swap(&mut lb, &mut hb);
        } else {
            core::mem::swap(&mut la, &mut ha);
        }
    }
    if w2bit[0] & 2 != 0 {
        weights_inv[1] = 3;
        if index_flag != 0 {
            core::mem::swap(&mut la, &mut ha);
        } else {
            core::mem::swap(&mut lr, &mut hr);
            core::mem::swap(&mut lg, &mut hg);
            core::mem::swap(&mut lb, &mut hb);
        }
    }

    block[0] = (0b10000 | (rot_index << 5) | (index_flag << 7)) as u8;

    let mut x: u64 = (lr as u64)
        | ((hr as u64) << 5)
        | ((lg as u64) << 10)
        | ((hg as u64) << 15)
        | ((lb as u64) << 20)
        | ((hb as u64) << 25)
        | ((la as u64) << 30)
        | ((ha as u64) << 36);
    block[1..6].copy_from_slice(&x.to_le_bytes()[..5]);

    // 2 leftover endpoint bits, then the 2-bit (alpha-plane) indices.
    x >>= 40;
    let mut ofs0 = 2u32;
    for i in 0..16usize {
        let w = (w2bit[i] ^ weights_inv[1]) as u64;
        x |= w << ofs0;
        ofs0 += 2 - u32::from(i == 0);
    }
    block[6..10].copy_from_slice(&(x as u32).to_le_bytes());
    x >>= 32;

    // 1 leftover bit, then the 3-bit (color-plane) indices.
    let mut ofs1 = 1u32;
    for i in 0..16usize {
        let w = (w3bit[i] ^ weights_inv[0]) as u64;
        x |= w << ofs1;
        ofs1 += 3 - u32::from(i == 0);
    }
    debug_assert_eq!(ofs1, 48);
    block[10..16].copy_from_slice(&x.to_le_bytes()[..6]);
}

/// `pack_mode5_solid`: losslessly encode a solid RGBA color as mode 5 via the
/// per-component optimal 7-bit endpoint table.
pub(crate) fn pack_mode5_solid(block: &mut [u8; 16], c: Rgba) {
    let opt = crate::uastc::bc7::mode5_optimal();
    block[0] = 0b0010_0000;

    let (lr, hr) = (
        opt[c[0] as usize].m_lo as u64,
        opt[c[0] as usize].m_hi as u64,
    );
    let (lg, hg) = (
        opt[c[1] as usize].m_lo as u64,
        opt[c[1] as usize].m_hi as u64,
    );
    let (lb, hb) = (
        opt[c[2] as usize].m_lo as u64,
        opt[c[2] as usize].m_hi as u64,
    );
    let a = c[3] as u64;

    let x: u64 =
        lr | (hr << 7) | (lg << 14) | (hg << 21) | (lb << 28) | (hb << 35) | (a << 42) | (a << 50);
    block[1..8].copy_from_slice(&x.to_le_bytes()[..7]);
    let x = x >> 56;

    // Uniform mid weights for both planes; the two leftover endpoint bits
    // fold into the first tail byte.
    const TAIL: [u8; 8] = [0xAC, 0xAA, 0xAA, 0xAA, 0, 0, 0, 0];
    block[8..16].copy_from_slice(&TAIL);
    block[8] |= x as u8;
}

/// `encode_mode5_rgba_block`: single subset, 7-bit RGB + 8-bit A endpoints,
/// two 2-bit index planes, rotation.
fn encode_mode5_rgba_block(
    block: &mut [u8; 16],
    mut lr: u32,
    mut lg: u32,
    mut lb: u32,
    mut la: u32,
    mut hr: u32,
    mut hg: u32,
    mut hb: u32,
    mut ha: u32,
    color_weights: &[u8; 16],
    alpha_weights: &[u8; 16],
    rot_index: u32,
) {
    let mut color_inv = 0u8;
    let mut alpha_inv = 0u8;
    if color_weights[0] & 2 != 0 {
        core::mem::swap(&mut lr, &mut hr);
        core::mem::swap(&mut lg, &mut hg);
        core::mem::swap(&mut lb, &mut hb);
        color_inv = 3;
    }
    if alpha_weights[0] & 2 != 0 {
        core::mem::swap(&mut la, &mut ha);
        alpha_inv = 3;
    }

    let low: u64 = (1 << 5)
        | ((rot_index as u64) << 6)
        | ((lr as u64) << 8)
        | ((hr as u64) << 15)
        | ((lg as u64) << 22)
        | ((hg as u64) << 29)
        | ((lb as u64) << 36)
        | ((hb as u64) << 43)
        | ((la as u64) << 50)
        | ((ha as u64) << 58);
    block[0..8].copy_from_slice(&low.to_le_bytes());

    let mut high: u64 = ((ha >> 6) & 3) as u64;
    let mut ofs = 2u32;
    for i in 0..16usize {
        let w = (color_weights[i] ^ color_inv) as u64;
        high |= w << ofs;
        ofs += 2 - u32::from(i == 0);
    }
    debug_assert_eq!(ofs, 33);
    for i in 0..16usize {
        let w = (alpha_weights[i] ^ alpha_inv) as u64;
        high |= w << ofs;
        ofs += 2 - u32::from(i == 0);
    }
    debug_assert_eq!(ofs, 64);
    block[8..16].copy_from_slice(&high.to_le_bytes());
}

/// `encode_mode6_rgba_block`: single subset, 7-bit RGBA endpoints, 2 unique
/// pbits, 4-bit weights.
pub(crate) fn encode_mode6_rgba_block(
    block: &mut [u8; 16],
    mut lr: u32,
    mut lg: u32,
    mut lb: u32,
    mut la: u32,
    mut p0: u32,
    mut hr: u32,
    mut hg: u32,
    mut hb: u32,
    mut ha: u32,
    mut p1: u32,
    weights: &[u8; 16],
) {
    let mut weight_inv = 0u8;
    if weights[0] & 8 != 0 {
        core::mem::swap(&mut lr, &mut hr);
        core::mem::swap(&mut lg, &mut hg);
        core::mem::swap(&mut lb, &mut hb);
        core::mem::swap(&mut la, &mut ha);
        core::mem::swap(&mut p0, &mut p1);
        weight_inv = 15;
    }

    let x: u64 = 0b100_0000
        | ((lr as u64) << 7)
        | ((hr as u64) << 14)
        | ((lg as u64) << 21)
        | ((hg as u64) << 28)
        | ((lb as u64) << 35)
        | ((hb as u64) << 42)
        | ((la as u64) << 49)
        | ((ha as u64) << 56);
    block[0..7].copy_from_slice(&x.to_le_bytes()[..7]);
    let x = x >> 56;
    block[7] = (x | ((p0 as u64) << 7)) as u8;

    let mut y: u64 = p1 as u64;
    let mut ofs = 1u32;
    for i in 0..16usize {
        let w = (weights[i] ^ weight_inv) as u64;
        y |= w << ofs;
        ofs += 3 + u32::from(i > 0);
    }
    debug_assert_eq!(ofs, 64);
    block[8..16].copy_from_slice(&y.to_le_bytes());
}

/// `encode_mode7_rgba_block`: 2 subsets, 6-bit partition id, 5-bit RGBA
/// endpoints, 4 unique pbits, 2-bit weights.
fn encode_mode7_rgba_block(
    block: &mut [u8; 16],
    part_id: u32,
    lr: &mut [u32; 2],
    lg: &mut [u32; 2],
    lb: &mut [u32; 2],
    la: &mut [u32; 2],
    hr: &mut [u32; 2],
    hg: &mut [u32; 2],
    hb: &mut [u32; 2],
    ha: &mut [u32; 2],
    p: &mut [u32; 4],
    weights: &[u8; 16],
) {
    let part_map = &G_BC7_PARTITION2[(part_id as usize) * 16..(part_id as usize) * 16 + 16];
    let anchor_index = G_BC7_ANCHOR_SECOND[part_id as usize] as usize;

    let mut weight_inv = [0u8; 2];
    if weights[0] & 2 != 0 {
        core::mem::swap(&mut lr[0], &mut hr[0]);
        core::mem::swap(&mut lg[0], &mut hg[0]);
        core::mem::swap(&mut lb[0], &mut hb[0]);
        core::mem::swap(&mut la[0], &mut ha[0]);
        p.swap(0, 1);
        weight_inv[0] = 3;
    }
    if weights[anchor_index] & 2 != 0 {
        core::mem::swap(&mut lr[1], &mut hr[1]);
        core::mem::swap(&mut lg[1], &mut hg[1]);
        core::mem::swap(&mut lb[1], &mut hb[1]);
        core::mem::swap(&mut la[1], &mut ha[1]);
        p.swap(2, 3);
        weight_inv[1] = 3;
    }

    let x: u64 = 0x80
        | ((part_id as u64) << 8)
        | ((lr[0] as u64) << 14)
        | ((hr[0] as u64) << 19)
        | ((lr[1] as u64) << 24)
        | ((hr[1] as u64) << 29)
        | ((lg[0] as u64) << 34)
        | ((hg[0] as u64) << 39)
        | ((lg[1] as u64) << 44)
        | ((hg[1] as u64) << 49)
        | ((lb[0] as u64) << 54)
        | ((hb[0] as u64) << 59);
    block[0..8].copy_from_slice(&x.to_le_bytes());

    let mut y: u64 = (lb[1] as u64)
        | ((hb[1] as u64) << 5)
        | ((la[0] as u64) << 10)
        | ((ha[0] as u64) << 15)
        | ((la[1] as u64) << 20)
        | ((ha[1] as u64) << 25)
        | ((p[0] as u64) << 30)
        | ((p[1] as u64) << 31)
        | ((p[2] as u64) << 32)
        | ((p[3] as u64) << 33);

    let mut ofs = 34u32;
    for i in 0..16usize {
        let subset_index = part_map[i] as usize;
        let w = (weights[i] ^ weight_inv[subset_index]) as u64;
        y |= w << ofs;
        ofs += 2 - u32::from(i == 0 || i == anchor_index);
    }
    debug_assert_eq!(ofs, 64);
    block[8..16].copy_from_slice(&y.to_le_bytes());
}

/// A 4-component float vector (RGBA, or the packed least-squares terms).
type Vec4F = [f32; 4];

/// Solve the normal equations for the least-squares optimal low/high endpoint
/// of a single component (`comp_index`) given the fixed per-texel `weights`.
/// `t_r` is that component's pixel sum. False when the 2x2 system is singular.
fn compute_least_squares_endpoints_1d(
    n: usize,
    weights: &[u8],
    selector_weights: &[Vec4F],
    xl: &mut f32,
    xh: &mut f32,
    colors: &[Rgba],
    comp_index: usize,
    t_r: f32,
) -> bool {
    let mut z00 = 0.0f32;
    let mut z10 = 0.0f32;
    let mut z11 = 0.0f32;
    let mut q00_r = 0.0f32;
    for i in 0..n {
        let sel = weights[i] as usize;
        z00 += selector_weights[sel][0];
        z10 += selector_weights[sel][1];
        z11 += selector_weights[sel][2];
        let w = selector_weights[sel][3];
        q00_r += w * colors[i][comp_index] as f32;
    }
    let q10_r = t_r - q00_r;
    let z01 = z10;
    let mut det = z00 * z11 - z01 * z10;
    if crate::mathf::fabsf(det) < 1e-8 {
        return false;
    }
    det = 1.0 / det;
    let iz00 = z11 * det;
    let iz01 = -z01 * det;
    let iz10 = -z10 * det;
    let iz11 = z00 * det;
    *xh = (iz00 * q00_r + iz01 * q10_r).clamp(0.0, 255.0);
    *xl = (iz10 * q00_r + iz11 * q10_r).clamp(0.0, 255.0);
    true
}

/// The RGB (three-component) form of [`compute_least_squares_endpoints_1d`]:
/// one 2x2 system, shared across R, G, and B.
fn compute_least_squares_endpoints_3d(
    n: usize,
    weights: &[u8],
    selector_weights: &[Vec4F],
    xl: &mut Vec4F,
    xh: &mut Vec4F,
    colors: &[Rgba],
    t_r: f32,
    t_g: f32,
    t_b: f32,
) -> bool {
    let mut z00 = 0.0f32;
    let mut z10 = 0.0f32;
    let mut z11 = 0.0f32;
    let mut q00_r = 0.0f32;
    let mut q00_g = 0.0f32;
    let mut q00_b = 0.0f32;
    for i in 0..n {
        let sel = weights[i] as usize;
        z00 += selector_weights[sel][0];
        z10 += selector_weights[sel][1];
        z11 += selector_weights[sel][2];
        let w = selector_weights[sel][3];
        q00_r += w * colors[i][0] as f32;
        q00_g += w * colors[i][1] as f32;
        q00_b += w * colors[i][2] as f32;
    }
    let q10_r = t_r - q00_r;
    let q10_g = t_g - q00_g;
    let q10_b = t_b - q00_b;
    let z01 = z10;
    let mut det = z00 * z11 - z01 * z10;
    if crate::mathf::fabsf(det) < 1e-8 {
        return false;
    }
    det = 1.0 / det;
    let iz00 = z11 * det;
    let iz01 = -z01 * det;
    let iz10 = -z10 * det;
    let iz11 = z00 * det;
    xh[0] = (iz00 * q00_r + iz01 * q10_r).clamp(0.0, 255.0);
    xl[0] = (iz10 * q00_r + iz11 * q10_r).clamp(0.0, 255.0);
    xh[1] = (iz00 * q00_g + iz01 * q10_g).clamp(0.0, 255.0);
    xl[1] = (iz10 * q00_g + iz11 * q10_g).clamp(0.0, 255.0);
    xh[2] = (iz00 * q00_b + iz01 * q10_b).clamp(0.0, 255.0);
    xl[2] = (iz10 * q00_b + iz11 * q10_b).clamp(0.0, 255.0);
    xh[3] = 0.0;
    xl[3] = 0.0;
    true
}

/// The RGBA (four-component) form of [`compute_least_squares_endpoints_1d`].
fn compute_least_squares_endpoints_4d(
    n: usize,
    weights: &[u8],
    selector_weights: &[Vec4F],
    xl: &mut Vec4F,
    xh: &mut Vec4F,
    colors: &[Rgba],
    t_r: f32,
    t_g: f32,
    t_b: f32,
    t_a: f32,
) -> bool {
    let mut z00 = 0.0f32;
    let mut z10 = 0.0f32;
    let mut z11 = 0.0f32;
    let mut q00_r = 0.0f32;
    let mut q00_g = 0.0f32;
    let mut q00_b = 0.0f32;
    let mut q00_a = 0.0f32;
    for i in 0..n {
        let sel = weights[i] as usize;
        z00 += selector_weights[sel][0];
        z10 += selector_weights[sel][1];
        z11 += selector_weights[sel][2];
        let w = selector_weights[sel][3];
        q00_r += w * colors[i][0] as f32;
        q00_g += w * colors[i][1] as f32;
        q00_b += w * colors[i][2] as f32;
        q00_a += w * colors[i][3] as f32;
    }
    let q10_r = t_r - q00_r;
    let q10_g = t_g - q00_g;
    let q10_b = t_b - q00_b;
    let q10_a = t_a - q00_a;
    let z01 = z10;
    let mut det = z00 * z11 - z01 * z10;
    if crate::mathf::fabsf(det) < 1e-8 {
        return false;
    }
    det = 1.0 / det;
    let iz00 = z11 * det;
    let iz01 = -z01 * det;
    let iz10 = -z10 * det;
    let iz11 = z00 * det;
    xh[0] = (iz00 * q00_r + iz01 * q10_r).clamp(0.0, 255.0);
    xl[0] = (iz10 * q00_r + iz11 * q10_r).clamp(0.0, 255.0);
    xh[1] = (iz00 * q00_g + iz01 * q10_g).clamp(0.0, 255.0);
    xl[1] = (iz10 * q00_g + iz11 * q10_g).clamp(0.0, 255.0);
    xh[2] = (iz00 * q00_b + iz01 * q10_b).clamp(0.0, 255.0);
    xl[2] = (iz10 * q00_b + iz11 * q10_b).clamp(0.0, 255.0);
    xh[3] = (iz00 * q00_a + iz01 * q10_a).clamp(0.0, 255.0);
    xl[3] = (iz10 * q00_a + iz11 * q10_a).clamp(0.0, 255.0);
    true
}

/// `bc7_sse` (single channel): squared error of one component at weight `w`.
#[inline]
fn bc7_sse_1(pr: i32, lr: i32, dr: i32, w: i32) -> u32 {
    let re = pr - (lr + ((dr * w + 32) >> 6));
    (re * re) as u32
}

/// `bc7_sse` summed over the three RGB components at weight `w`.
#[inline]
fn bc7_sse_3(
    pr: i32,
    pg: i32,
    pb: i32,
    lr: i32,
    lg: i32,
    lb: i32,
    dr: i32,
    dg: i32,
    db: i32,
    w: i32,
) -> u32 {
    let re = pr - (lr + ((dr * w + 32) >> 6));
    let ge = pg - (lg + ((dg * w + 32) >> 6));
    let be = pb - (lb + ((db * w + 32) >> 6));
    (re * re + ge * ge + be * be) as u32
}

/// `bc7_sse` summed over all four RGBA components at weight `w`.
#[inline]
fn bc7_sse_4(
    pr: i32,
    pg: i32,
    pb: i32,
    pa: i32,
    lr: i32,
    lg: i32,
    lb: i32,
    la: i32,
    dr: i32,
    dg: i32,
    db: i32,
    da: i32,
    w: i32,
) -> u32 {
    let re = pr - (lr + ((dr * w + 32) >> 6));
    let ge = pg - (lg + ((dg * w + 32) >> 6));
    let be = pb - (lb + ((db * w + 32) >> 6));
    let ae = pa - (la + ((da * w + 32) >> 6));
    (re * re + ge * ge + be * be + ae * ae) as u32
}

/// `eval_weights_mode6_rgb`: project each pixel onto the RGB endpoint axis
/// and pick its 4-bit weight.
fn eval_weights_mode6_rgb(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    hr: i32,
    hg: i32,
    hb: i32,
    p0: u32,
    p1: u32,
) {
    let lr = from_7_p(lr as u32, p0) as i32;
    let lg = from_7_p(lg as u32, p0) as i32;
    let lb = from_7_p(lb as u32, p0) as i32;
    let hr = from_7_p(hr as u32, p1) as i32;
    let hg = from_7_p(hg as u32, p1) as i32;
    let hb = from_7_p(hb as u32, p1) as i32;

    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;

    let f = 15.0f32 / ((dr * dr + dg * dg + db * db) as f32 + 0.000_001_25);
    let sofs = -(lr * dr + lg * dg + lb * db);

    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            + sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 15 {
            sel = (!sel >> 31) & 15;
        }
        weights[i] = sel as u8;
    }
}

/// `eval_weights_mode6_rgb_sse`: like [`eval_weights_mode6_rgb`] but also
/// returns the RGBA squared error (packed alpha is always 127).
fn eval_weights_mode6_rgb_sse(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    hr: i32,
    hg: i32,
    hb: i32,
    p0: u32,
    p1: u32,
) -> u32 {
    let lr = from_7_p(lr as u32, p0) as i32;
    let lg = from_7_p(lg as u32, p0) as i32;
    let lb = from_7_p(lb as u32, p0) as i32;
    let hr = from_7_p(hr as u32, p1) as i32;
    let hg = from_7_p(hg as u32, p1) as i32;
    let hb = from_7_p(hb as u32, p1) as i32;

    let la = from_7_p(127, p0) as i32;
    let ha = from_7_p(127, p1) as i32;
    let da = ha - la;

    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;

    let f = 15.0f32 / ((dr * dr + dg * dg + db * db) as f32 + 0.000_001_25);
    let sofs = -(lr * dr + lg * dg + lb * db);

    let mut sse = 0u32;
    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            + sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 15 {
            sel = (!sel >> 31) & 15;
        }
        weights[i] = sel as u8;
        sse += bc7_sse_4(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            pixels[i][3] as i32,
            lr,
            lg,
            lb,
            la,
            dr,
            dg,
            db,
            da,
            BC7_WEIGHTS4[sel as usize] as i32,
        );
    }
    sse
}

/// Like [`eval_weights_mode6_rgb`] but projects each pixel onto the 4D RGBA
/// endpoint axis (alpha is a real endpoint channel, not fixed at 127).
fn eval_weights_mode6_rgba(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    la: i32,
    p0: i32,
    hr: i32,
    hg: i32,
    hb: i32,
    ha: i32,
    p1: i32,
) {
    let lr = from_7_p(lr as u32, p0 as u32) as i32;
    let lg = from_7_p(lg as u32, p0 as u32) as i32;
    let lb = from_7_p(lb as u32, p0 as u32) as i32;
    let la = from_7_p(la as u32, p0 as u32) as i32;
    let hr = from_7_p(hr as u32, p1 as u32) as i32;
    let hg = from_7_p(hg as u32, p1 as u32) as i32;
    let hb = from_7_p(hb as u32, p1 as u32) as i32;
    let ha = from_7_p(ha as u32, p1 as u32) as i32;

    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    let da = ha - la;

    let f = 15.0f32 / ((dr * dr + dg * dg + db * db + da * da) as f32 + 0.000_001_25);
    let sofs = -(lr * dr + lg * dg + lb * db + la * da);

    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            + pixels[i][3] as i32 * da
            + sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 15 {
            sel = (!sel >> 31) & 15;
        }
        weights[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode6_rgba`]: assigns the same weights
/// and also returns the RGBA squared error.
fn eval_weights_mode6_rgba_sse(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    la: i32,
    p0: i32,
    hr: i32,
    hg: i32,
    hb: i32,
    ha: i32,
    p1: i32,
) -> u32 {
    let lr = from_7_p(lr as u32, p0 as u32) as i32;
    let lg = from_7_p(lg as u32, p0 as u32) as i32;
    let lb = from_7_p(lb as u32, p0 as u32) as i32;
    let la = from_7_p(la as u32, p0 as u32) as i32;
    let hr = from_7_p(hr as u32, p1 as u32) as i32;
    let hg = from_7_p(hg as u32, p1 as u32) as i32;
    let hb = from_7_p(hb as u32, p1 as u32) as i32;
    let ha = from_7_p(ha as u32, p1 as u32) as i32;

    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    let da = ha - la;

    let f = 15.0f32 / ((dr * dr + dg * dg + db * db + da * da) as f32 + 0.000_001_25);
    let sofs = -(lr * dr + lg * dg + lb * db + la * da);

    let mut sse = 0u32;
    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            + pixels[i][3] as i32 * da
            + sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 15 {
            sel = (!sel >> 31) & 15;
        }
        weights[i] = sel as u8;
        sse += bc7_sse_4(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            pixels[i][3] as i32,
            lr,
            lg,
            lb,
            la,
            dr,
            dg,
            db,
            da,
            BC7_WEIGHTS4[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode1_rgb` (3-bit weights, 2 subsets, shared pbits).
fn eval_weights_mode1_rgb(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 2],
    blg: &[u32; 2],
    blb: &[u32; 2],
    bhr: &[u32; 2],
    bhg: &[u32; 2],
    bhb: &[u32; 2],
    pbits: &[u32; 2],
    subset_bitmask: u32,
) {
    let mut lr = [0i32; 2];
    let mut lg = [0i32; 2];
    let mut lb = [0i32; 2];
    let mut dr = [0i32; 2];
    let mut dg = [0i32; 2];
    let mut db = [0i32; 2];
    for s in 0..2 {
        lr[s] = from_6_p(blr[s], pbits[s]) as i32;
        lg[s] = from_6_p(blg[s], pbits[s]) as i32;
        lb[s] = from_6_p(blb[s], pbits[s]) as i32;
        let hr = from_6_p(bhr[s], pbits[s]) as i32;
        let hg = from_6_p(bhg[s], pbits[s]) as i32;
        let hb = from_6_p(bhb[s], pbits[s]) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
    }
    let f = [
        7.0f32 / ((dr[0] * dr[0] + dg[0] * dg[0] + db[0] * db[0]) as f32 + 0.000_001_25),
        7.0f32 / ((dr[1] * dr[1] + dg[1] * dg[1] + db[1] * db[1]) as f32 + 0.000_001_25),
    ];
    let sofs = [
        lr[0] * dr[0] + lg[0] * dg[0] + lb[0] * db[0],
        lr[1] * dr[1] + lg[1] * dg[1] + lb[1] * db[1],
    ];
    for i in 0..16 {
        let s = ((subset_bitmask >> i) & 1) as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 7 {
            sel = (!sel >> 31) & 7;
        }
        weights[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode1_rgb`]: assigns the same weights
/// and also returns the RGB squared error.
fn eval_weights_mode1_rgb_sse(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 2],
    blg: &[u32; 2],
    blb: &[u32; 2],
    bhr: &[u32; 2],
    bhg: &[u32; 2],
    bhb: &[u32; 2],
    pbits: &[u32; 2],
    subset_bitmask: u32,
) -> u32 {
    let mut lr = [0i32; 2];
    let mut lg = [0i32; 2];
    let mut lb = [0i32; 2];
    let mut dr = [0i32; 2];
    let mut dg = [0i32; 2];
    let mut db = [0i32; 2];
    for s in 0..2 {
        lr[s] = from_6_p(blr[s], pbits[s]) as i32;
        lg[s] = from_6_p(blg[s], pbits[s]) as i32;
        lb[s] = from_6_p(blb[s], pbits[s]) as i32;
        let hr = from_6_p(bhr[s], pbits[s]) as i32;
        let hg = from_6_p(bhg[s], pbits[s]) as i32;
        let hb = from_6_p(bhb[s], pbits[s]) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
    }
    let f = [
        7.0f32 / ((dr[0] * dr[0] + dg[0] * dg[0] + db[0] * db[0]) as f32 + 0.000_001_25),
        7.0f32 / ((dr[1] * dr[1] + dg[1] * dg[1] + db[1] * db[1]) as f32 + 0.000_001_25),
    ];
    let sofs = [
        lr[0] * dr[0] + lg[0] * dg[0] + lb[0] * db[0],
        lr[1] * dr[1] + lg[1] * dg[1] + lb[1] * db[1],
    ];
    let mut sse = 0u32;
    for i in 0..16 {
        let s = ((subset_bitmask >> i) & 1) as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 7 {
            sel = (!sel >> 31) & 7;
        }
        weights[i] = sel as u8;
        sse += bc7_sse_3(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            lr[s],
            lg[s],
            lb[s],
            dr[s],
            dg[s],
            db[s],
            BC7_WEIGHTS3[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode7_rgba` (2-bit weights, 2 subsets, unique pbits).
fn eval_weights_mode7_rgba(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 2],
    blg: &[u32; 2],
    blb: &[u32; 2],
    bla: &[u32; 2],
    bhr: &[u32; 2],
    bhg: &[u32; 2],
    bhb: &[u32; 2],
    bha: &[u32; 2],
    pbits: &[u32; 4],
    subset_bitmask: u32,
) {
    let mut lr = [0i32; 2];
    let mut lg = [0i32; 2];
    let mut lb = [0i32; 2];
    let mut la = [0i32; 2];
    let mut dr = [0i32; 2];
    let mut dg = [0i32; 2];
    let mut db = [0i32; 2];
    let mut da = [0i32; 2];
    for s in 0..2 {
        let l_pbit = pbits[s * 2];
        let h_pbit = pbits[s * 2 + 1];
        lr[s] = from_5_p(blr[s], l_pbit) as i32;
        lg[s] = from_5_p(blg[s], l_pbit) as i32;
        lb[s] = from_5_p(blb[s], l_pbit) as i32;
        la[s] = from_5_p(bla[s], l_pbit) as i32;
        let hr = from_5_p(bhr[s], h_pbit) as i32;
        let hg = from_5_p(bhg[s], h_pbit) as i32;
        let hb = from_5_p(bhb[s], h_pbit) as i32;
        let ha = from_5_p(bha[s], h_pbit) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
        da[s] = ha - la[s];
    }
    let f = [
        3.0f32
            / ((dr[0] * dr[0] + dg[0] * dg[0] + db[0] * db[0] + da[0] * da[0]) as f32
                + 0.000_001_25),
        3.0f32
            / ((dr[1] * dr[1] + dg[1] * dg[1] + db[1] * db[1] + da[1] * da[1]) as f32
                + 0.000_001_25),
    ];
    let sofs = [
        lr[0] * dr[0] + lg[0] * dg[0] + lb[0] * db[0] + la[0] * da[0],
        lr[1] * dr[1] + lg[1] * dg[1] + lb[1] * db[1] + la[1] * da[1],
    ];
    for i in 0..16 {
        let s = ((subset_bitmask >> i) & 1) as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            + pixels[i][3] as i32 * da[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode7_rgba`]: assigns the same weights
/// and also returns the RGBA squared error.
fn eval_weights_mode7_rgba_sse(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 2],
    blg: &[u32; 2],
    blb: &[u32; 2],
    bla: &[u32; 2],
    bhr: &[u32; 2],
    bhg: &[u32; 2],
    bhb: &[u32; 2],
    bha: &[u32; 2],
    pbits: &[u32; 4],
    subset_bitmask: u32,
) -> u32 {
    let mut lr = [0i32; 2];
    let mut lg = [0i32; 2];
    let mut lb = [0i32; 2];
    let mut la = [0i32; 2];
    let mut dr = [0i32; 2];
    let mut dg = [0i32; 2];
    let mut db = [0i32; 2];
    let mut da = [0i32; 2];
    for s in 0..2 {
        let l_pbit = pbits[s * 2];
        let h_pbit = pbits[s * 2 + 1];
        lr[s] = from_5_p(blr[s], l_pbit) as i32;
        lg[s] = from_5_p(blg[s], l_pbit) as i32;
        lb[s] = from_5_p(blb[s], l_pbit) as i32;
        la[s] = from_5_p(bla[s], l_pbit) as i32;
        let hr = from_5_p(bhr[s], h_pbit) as i32;
        let hg = from_5_p(bhg[s], h_pbit) as i32;
        let hb = from_5_p(bhb[s], h_pbit) as i32;
        let ha = from_5_p(bha[s], h_pbit) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
        da[s] = ha - la[s];
    }
    let f = [
        3.0f32
            / ((dr[0] * dr[0] + dg[0] * dg[0] + db[0] * db[0] + da[0] * da[0]) as f32
                + 0.000_001_25),
        3.0f32
            / ((dr[1] * dr[1] + dg[1] * dg[1] + db[1] * db[1] + da[1] * da[1]) as f32
                + 0.000_001_25),
    ];
    let sofs = [
        lr[0] * dr[0] + lg[0] * dg[0] + lb[0] * db[0] + la[0] * da[0],
        lr[1] * dr[1] + lg[1] * dg[1] + lb[1] * db[1] + la[1] * da[1],
    ];
    let mut sse = 0u32;
    for i in 0..16 {
        let s = ((subset_bitmask >> i) & 1) as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            + pixels[i][3] as i32 * da[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights[i] = sel as u8;
        sse += bc7_sse_4(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            pixels[i][3] as i32,
            lr[s],
            lg[s],
            lb[s],
            la[s],
            dr[s],
            dg[s],
            db[s],
            da[s],
            BC7_WEIGHTS2[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode3_rgb` (2-bit weights, 2 subsets, unique pbits).
fn eval_weights_mode3_rgb(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 2],
    blg: &[u32; 2],
    blb: &[u32; 2],
    bhr: &[u32; 2],
    bhg: &[u32; 2],
    bhb: &[u32; 2],
    pbits: &[u32; 4],
    subset_bitmask: u32,
) {
    let mut lr = [0i32; 2];
    let mut lg = [0i32; 2];
    let mut lb = [0i32; 2];
    let mut dr = [0i32; 2];
    let mut dg = [0i32; 2];
    let mut db = [0i32; 2];
    for s in 0..2 {
        lr[s] = from_7_p(blr[s], pbits[s * 2]) as i32;
        lg[s] = from_7_p(blg[s], pbits[s * 2]) as i32;
        lb[s] = from_7_p(blb[s], pbits[s * 2]) as i32;
        let hr = from_7_p(bhr[s], pbits[s * 2 + 1]) as i32;
        let hg = from_7_p(bhg[s], pbits[s * 2 + 1]) as i32;
        let hb = from_7_p(bhb[s], pbits[s * 2 + 1]) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
    }
    let f = [
        3.0f32 / ((dr[0] * dr[0] + dg[0] * dg[0] + db[0] * db[0]) as f32 + 0.000_001_25),
        3.0f32 / ((dr[1] * dr[1] + dg[1] * dg[1] + db[1] * db[1]) as f32 + 0.000_001_25),
    ];
    let sofs = [
        lr[0] * dr[0] + lg[0] * dg[0] + lb[0] * db[0],
        lr[1] * dr[1] + lg[1] * dg[1] + lb[1] * db[1],
    ];
    for i in 0..16 {
        let s = ((subset_bitmask >> i) & 1) as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode3_rgb`]: assigns the same weights
/// and also returns the RGB squared error.
fn eval_weights_mode3_rgb_sse(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 2],
    blg: &[u32; 2],
    blb: &[u32; 2],
    bhr: &[u32; 2],
    bhg: &[u32; 2],
    bhb: &[u32; 2],
    pbits: &[u32; 4],
    subset_bitmask: u32,
) -> u32 {
    let mut lr = [0i32; 2];
    let mut lg = [0i32; 2];
    let mut lb = [0i32; 2];
    let mut dr = [0i32; 2];
    let mut dg = [0i32; 2];
    let mut db = [0i32; 2];
    for s in 0..2 {
        lr[s] = from_7_p(blr[s], pbits[s * 2]) as i32;
        lg[s] = from_7_p(blg[s], pbits[s * 2]) as i32;
        lb[s] = from_7_p(blb[s], pbits[s * 2]) as i32;
        let hr = from_7_p(bhr[s], pbits[s * 2 + 1]) as i32;
        let hg = from_7_p(bhg[s], pbits[s * 2 + 1]) as i32;
        let hb = from_7_p(bhb[s], pbits[s * 2 + 1]) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
    }
    let f = [
        3.0f32 / ((dr[0] * dr[0] + dg[0] * dg[0] + db[0] * db[0]) as f32 + 0.000_001_25),
        3.0f32 / ((dr[1] * dr[1] + dg[1] * dg[1] + db[1] * db[1]) as f32 + 0.000_001_25),
    ];
    let sofs = [
        lr[0] * dr[0] + lg[0] * dg[0] + lb[0] * db[0],
        lr[1] * dr[1] + lg[1] * dg[1] + lb[1] * db[1],
    ];
    let mut sse = 0u32;
    for i in 0..16 {
        let s = ((subset_bitmask >> i) & 1) as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights[i] = sel as u8;
        sse += bc7_sse_3(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            lr[s],
            lg[s],
            lb[s],
            dr[s],
            dg[s],
            db[s],
            BC7_WEIGHTS2[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode0_rgb` (3-bit weights, 3 subsets, unique pbits).
fn eval_weights_mode0_rgb(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 3],
    blg: &[u32; 3],
    blb: &[u32; 3],
    bhr: &[u32; 3],
    bhg: &[u32; 3],
    bhb: &[u32; 3],
    pbits: &[u32; 6],
    pat_index: u32,
) {
    let mut lr = [0i32; 3];
    let mut lg = [0i32; 3];
    let mut lb = [0i32; 3];
    let mut dr = [0i32; 3];
    let mut dg = [0i32; 3];
    let mut db = [0i32; 3];
    for s in 0..3 {
        lr[s] = from_4_p(blr[s], pbits[s * 2]) as i32;
        lg[s] = from_4_p(blg[s], pbits[s * 2]) as i32;
        lb[s] = from_4_p(blb[s], pbits[s * 2]) as i32;
        let hr = from_4_p(bhr[s], pbits[s * 2 + 1]) as i32;
        let hg = from_4_p(bhg[s], pbits[s * 2 + 1]) as i32;
        let hb = from_4_p(bhb[s], pbits[s * 2 + 1]) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
    }
    let mut f = [0f32; 3];
    let mut sofs = [0i32; 3];
    for s in 0..3 {
        f[s] = 7.0f32 / ((dr[s] * dr[s] + dg[s] * dg[s] + db[s] * db[s]) as f32 + 0.000_001_25);
        sofs[s] = lr[s] * dr[s] + lg[s] * dg[s] + lb[s] * db[s];
    }
    let part_map = &G_BC7_PARTITION3[(pat_index as usize) * 16..(pat_index as usize) * 16 + 16];
    for i in 0..16 {
        let s = part_map[i] as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 7 {
            sel = (!sel >> 31) & 7;
        }
        weights[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode0_rgb`]: assigns the same weights
/// and also returns the RGB squared error.
fn eval_weights_mode0_rgb_sse(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 3],
    blg: &[u32; 3],
    blb: &[u32; 3],
    bhr: &[u32; 3],
    bhg: &[u32; 3],
    bhb: &[u32; 3],
    pbits: &[u32; 6],
    pat_index: u32,
) -> u32 {
    let mut lr = [0i32; 3];
    let mut lg = [0i32; 3];
    let mut lb = [0i32; 3];
    let mut dr = [0i32; 3];
    let mut dg = [0i32; 3];
    let mut db = [0i32; 3];
    for s in 0..3 {
        lr[s] = from_4_p(blr[s], pbits[s * 2]) as i32;
        lg[s] = from_4_p(blg[s], pbits[s * 2]) as i32;
        lb[s] = from_4_p(blb[s], pbits[s * 2]) as i32;
        let hr = from_4_p(bhr[s], pbits[s * 2 + 1]) as i32;
        let hg = from_4_p(bhg[s], pbits[s * 2 + 1]) as i32;
        let hb = from_4_p(bhb[s], pbits[s * 2 + 1]) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
    }
    let mut f = [0f32; 3];
    let mut sofs = [0i32; 3];
    for s in 0..3 {
        f[s] = 7.0f32 / ((dr[s] * dr[s] + dg[s] * dg[s] + db[s] * db[s]) as f32 + 0.000_001_25);
        sofs[s] = lr[s] * dr[s] + lg[s] * dg[s] + lb[s] * db[s];
    }
    let part_map = &G_BC7_PARTITION3[(pat_index as usize) * 16..(pat_index as usize) * 16 + 16];
    let mut sse = 0u32;
    for i in 0..16 {
        let s = part_map[i] as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 7 {
            sel = (!sel >> 31) & 7;
        }
        weights[i] = sel as u8;
        sse += bc7_sse_3(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            lr[s],
            lg[s],
            lb[s],
            dr[s],
            dg[s],
            db[s],
            BC7_WEIGHTS3[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode2_rgb` (2-bit weights, 3 subsets, no pbits).
fn eval_weights_mode2_rgb(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 3],
    blg: &[u32; 3],
    blb: &[u32; 3],
    bhr: &[u32; 3],
    bhg: &[u32; 3],
    bhb: &[u32; 3],
    pat_index: u32,
) {
    let mut lr = [0i32; 3];
    let mut lg = [0i32; 3];
    let mut lb = [0i32; 3];
    let mut dr = [0i32; 3];
    let mut dg = [0i32; 3];
    let mut db = [0i32; 3];
    for s in 0..3 {
        lr[s] = from_5(blr[s]) as i32;
        lg[s] = from_5(blg[s]) as i32;
        lb[s] = from_5(blb[s]) as i32;
        let hr = from_5(bhr[s]) as i32;
        let hg = from_5(bhg[s]) as i32;
        let hb = from_5(bhb[s]) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
    }
    let mut f = [0f32; 3];
    let mut sofs = [0i32; 3];
    for s in 0..3 {
        f[s] = 3.0f32 / ((dr[s] * dr[s] + dg[s] * dg[s] + db[s] * db[s]) as f32 + 0.000_001_25);
        sofs[s] = lr[s] * dr[s] + lg[s] * dg[s] + lb[s] * db[s];
    }
    let part_map = &G_BC7_PARTITION3[(pat_index as usize) * 16..(pat_index as usize) * 16 + 16];
    for i in 0..16 {
        let s = part_map[i] as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode2_rgb`]: assigns the same weights
/// and also returns the RGB squared error.
fn eval_weights_mode2_rgb_sse(
    pixels: &[Rgba; 16],
    weights: &mut [u8; 16],
    blr: &[u32; 3],
    blg: &[u32; 3],
    blb: &[u32; 3],
    bhr: &[u32; 3],
    bhg: &[u32; 3],
    bhb: &[u32; 3],
    pat_index: u32,
) -> u32 {
    let mut lr = [0i32; 3];
    let mut lg = [0i32; 3];
    let mut lb = [0i32; 3];
    let mut dr = [0i32; 3];
    let mut dg = [0i32; 3];
    let mut db = [0i32; 3];
    for s in 0..3 {
        lr[s] = from_5(blr[s]) as i32;
        lg[s] = from_5(blg[s]) as i32;
        lb[s] = from_5(blb[s]) as i32;
        let hr = from_5(bhr[s]) as i32;
        let hg = from_5(bhg[s]) as i32;
        let hb = from_5(bhb[s]) as i32;
        dr[s] = hr - lr[s];
        dg[s] = hg - lg[s];
        db[s] = hb - lb[s];
    }
    let mut f = [0f32; 3];
    let mut sofs = [0i32; 3];
    for s in 0..3 {
        f[s] = 3.0f32 / ((dr[s] * dr[s] + dg[s] * dg[s] + db[s] * db[s]) as f32 + 0.000_001_25);
        sofs[s] = lr[s] * dr[s] + lg[s] * dg[s] + lb[s] * db[s];
    }
    let part_map = &G_BC7_PARTITION3[(pat_index as usize) * 16..(pat_index as usize) * 16 + 16];
    let mut sse = 0u32;
    for i in 0..16 {
        let s = part_map[i] as usize;
        let mut sel = ((pixels[i][0] as i32 * dr[s]
            + pixels[i][1] as i32 * dg[s]
            + pixels[i][2] as i32 * db[s]
            - sofs[s]) as f32
            * f[s]
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights[i] = sel as u8;
        sse += bc7_sse_3(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            lr[s],
            lg[s],
            lb[s],
            dr[s],
            dg[s],
            db[s],
            BC7_WEIGHTS2[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode4_3bit_rgb` (single subset, 5-bit endpoints, 3-bit
/// color weights).
fn eval_weights_mode4_3bit_rgb(
    pixels: &[Rgba; 16],
    weights0: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    hr: i32,
    hg: i32,
    hb: i32,
) {
    let lr = from_5(lr as u32) as i32;
    let lg = from_5(lg as u32) as i32;
    let lb = from_5(lb as u32) as i32;
    let hr = from_5(hr as u32) as i32;
    let hg = from_5(hg as u32) as i32;
    let hb = from_5(hb as u32) as i32;
    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    let f = 7.0f32 / ((dr * dr + dg * dg + db * db) as f32 + 0.000_001_25);
    let sofs = lr * dr + lg * dg + lb * db;
    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            - sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 7 {
            sel = (!sel >> 31) & 7;
        }
        weights0[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode4_3bit_rgb`]: assigns the same
/// weights and also returns the RGB squared error.
fn eval_weights_mode4_3bit_rgb_sse(
    pixels: &[Rgba; 16],
    weights0: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    hr: i32,
    hg: i32,
    hb: i32,
) -> u32 {
    let lr = from_5(lr as u32) as i32;
    let lg = from_5(lg as u32) as i32;
    let lb = from_5(lb as u32) as i32;
    let hr = from_5(hr as u32) as i32;
    let hg = from_5(hg as u32) as i32;
    let hb = from_5(hb as u32) as i32;
    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    let f = 7.0f32 / ((dr * dr + dg * dg + db * db) as f32 + 0.000_001_25);
    let sofs = lr * dr + lg * dg + lb * db;
    let mut sse = 0u32;
    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            - sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 7 {
            sel = (!sel >> 31) & 7;
        }
        weights0[i] = sel as u8;
        sse += bc7_sse_3(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            lr,
            lg,
            lb,
            dr,
            dg,
            db,
            BC7_WEIGHTS3[sel as usize] as i32,
        );
    }
    sse
}

/// The 2-bit-color-weight twin of [`eval_weights_mode4_3bit_rgb`] (used when
/// mode 4 assigns 3 index bits to the alpha plane instead of RGB).
fn eval_weights_mode4_2bit_rgb(
    pixels: &[Rgba; 16],
    weights0: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    hr: i32,
    hg: i32,
    hb: i32,
) {
    let lr = from_5(lr as u32) as i32;
    let lg = from_5(lg as u32) as i32;
    let lb = from_5(lb as u32) as i32;
    let hr = from_5(hr as u32) as i32;
    let hg = from_5(hg as u32) as i32;
    let hb = from_5(hb as u32) as i32;
    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    let f = 3.0f32 / ((dr * dr + dg * dg + db * db) as f32 + 0.000_001_25);
    let sofs = lr * dr + lg * dg + lb * db;
    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            - sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights0[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode4_2bit_rgb`]: assigns the same
/// weights and also returns the RGB squared error.
fn eval_weights_mode4_2bit_rgb_sse(
    pixels: &[Rgba; 16],
    weights0: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    hr: i32,
    hg: i32,
    hb: i32,
) -> u32 {
    let lr = from_5(lr as u32) as i32;
    let lg = from_5(lg as u32) as i32;
    let lb = from_5(lb as u32) as i32;
    let hr = from_5(hr as u32) as i32;
    let hg = from_5(hg as u32) as i32;
    let hb = from_5(hb as u32) as i32;
    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    let f = 3.0f32 / ((dr * dr + dg * dg + db * db) as f32 + 0.000_001_25);
    let sofs = lr * dr + lg * dg + lb * db;
    let mut sse = 0u32;
    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            - sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights0[i] = sel as u8;
        sse += bc7_sse_3(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            lr,
            lg,
            lb,
            dr,
            dg,
            db,
            BC7_WEIGHTS2[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode4_2bit_a` (alpha plane, 6-bit endpoints).
fn eval_weights_mode4_2bit_a(pixels: &[Rgba; 16], weights1: &mut [u8; 16], la: i32, ha: i32) {
    let la = from_6(la as u32) as i32;
    let ha = from_6(ha as u32) as i32;
    let da = ha - la;
    let f = 3.0f32 / (da as f32 + 0.000_001_25);
    for i in 0..16 {
        let mut sel = ((pixels[i][3] as i32 - la) as f32 * f + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights1[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode4_2bit_a`]: assigns the same alpha
/// weights and also returns the alpha squared error.
fn eval_weights_mode4_2bit_a_sse(
    pixels: &[Rgba; 16],
    weights1: &mut [u8; 16],
    la: i32,
    ha: i32,
) -> u32 {
    let la = from_6(la as u32) as i32;
    let ha = from_6(ha as u32) as i32;
    let da = ha - la;
    let f = 3.0f32 / (da as f32 + 0.000_001_25);
    let mut sse = 0u32;
    for i in 0..16 {
        let mut sel = ((pixels[i][3] as i32 - la) as f32 * f + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights1[i] = sel as u8;
        sse += bc7_sse_1(
            pixels[i][3] as i32,
            la,
            da,
            BC7_WEIGHTS2[sel as usize] as i32,
        );
    }
    sse
}

/// The 3-bit-weight twin of [`eval_weights_mode4_2bit_a`] (alpha plane, 6-bit
/// endpoints), used when mode 4 gives the alpha plane the 3 index bits.
fn eval_weights_mode4_3bit_a(pixels: &[Rgba; 16], weights1: &mut [u8; 16], la: i32, ha: i32) {
    let la = from_6(la as u32) as i32;
    let ha = from_6(ha as u32) as i32;
    let da = ha - la;
    let f = 7.0f32 / (da as f32 + 0.000_001_25);
    for i in 0..16 {
        let mut sel = ((pixels[i][3] as i32 - la) as f32 * f + 0.5) as i32;
        if sel as u32 > 7 {
            sel = (!sel >> 31) & 7;
        }
        weights1[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode4_3bit_a`]: assigns the same alpha
/// weights and also returns the alpha squared error.
fn eval_weights_mode4_3bit_a_sse(
    pixels: &[Rgba; 16],
    weights1: &mut [u8; 16],
    la: i32,
    ha: i32,
) -> u32 {
    let la = from_6(la as u32) as i32;
    let ha = from_6(ha as u32) as i32;
    let da = ha - la;
    let f = 7.0f32 / (da as f32 + 0.000_001_25);
    let mut sse = 0u32;
    for i in 0..16 {
        let mut sel = ((pixels[i][3] as i32 - la) as f32 * f + 0.5) as i32;
        if sel as u32 > 7 {
            sel = (!sel >> 31) & 7;
        }
        weights1[i] = sel as u8;
        sse += bc7_sse_1(
            pixels[i][3] as i32,
            la,
            da,
            BC7_WEIGHTS3[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode5_2bit_rgb` (single subset, 7-bit endpoints).
fn eval_weights_mode5_2bit_rgb(
    pixels: &[Rgba; 16],
    weights0: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    hr: i32,
    hg: i32,
    hb: i32,
) {
    let lr = from_7(lr as u32) as i32;
    let lg = from_7(lg as u32) as i32;
    let lb = from_7(lb as u32) as i32;
    let hr = from_7(hr as u32) as i32;
    let hg = from_7(hg as u32) as i32;
    let hb = from_7(hb as u32) as i32;
    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    let f = 3.0f32 / ((dr * dr + dg * dg + db * db) as f32 + 0.000_001_25);
    let sofs = lr * dr + lg * dg + lb * db;
    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            - sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights0[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode5_2bit_rgb`]: assigns the same
/// weights and also returns the RGB squared error.
fn eval_weights_mode5_2bit_rgb_sse(
    pixels: &[Rgba; 16],
    weights0: &mut [u8; 16],
    lr: i32,
    lg: i32,
    lb: i32,
    hr: i32,
    hg: i32,
    hb: i32,
) -> u32 {
    let lr = from_7(lr as u32) as i32;
    let lg = from_7(lg as u32) as i32;
    let lb = from_7(lb as u32) as i32;
    let hr = from_7(hr as u32) as i32;
    let hg = from_7(hg as u32) as i32;
    let hb = from_7(hb as u32) as i32;
    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    let f = 3.0f32 / ((dr * dr + dg * dg + db * db) as f32 + 0.000_001_25);
    let sofs = lr * dr + lg * dg + lb * db;
    let mut sse = 0u32;
    for i in 0..16 {
        let mut sel = ((pixels[i][0] as i32 * dr
            + pixels[i][1] as i32 * dg
            + pixels[i][2] as i32 * db
            - sofs) as f32
            * f
            + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights0[i] = sel as u8;
        sse += bc7_sse_3(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            lr,
            lg,
            lb,
            dr,
            dg,
            db,
            BC7_WEIGHTS2[sel as usize] as i32,
        );
    }
    sse
}

/// `eval_weights_mode5_2bit_a` (alpha plane, 8-bit endpoints, no expansion).
fn eval_weights_mode5_2bit_a(pixels: &[Rgba; 16], weights1: &mut [u8; 16], la: i32, ha: i32) {
    let da = ha - la;
    let f = 3.0f32 / (da as f32 + 0.000_001_25);
    for i in 0..16 {
        let mut sel = ((pixels[i][3] as i32 - la) as f32 * f + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights1[i] = sel as u8;
    }
}

/// SSE-returning twin of [`eval_weights_mode5_2bit_a`]: assigns the same alpha
/// weights and also returns the alpha squared error.
fn eval_weights_mode5_2bit_a_sse(
    pixels: &[Rgba; 16],
    weights1: &mut [u8; 16],
    la: i32,
    ha: i32,
) -> u32 {
    let da = ha - la;
    let f = 3.0f32 / (da as f32 + 0.000_001_25);
    let mut sse = 0u32;
    for i in 0..16 {
        let mut sel = ((pixels[i][3] as i32 - la) as f32 * f + 0.5) as i32;
        if sel as u32 > 3 {
            sel = (!sel >> 31) & 3;
        }
        weights1[i] = sel as u8;
        sse += bc7_sse_1(
            pixels[i][3] as i32,
            la,
            da,
            BC7_WEIGHTS2[sel as usize] as i32,
        );
    }
    sse
}

/// `determine_unique_pbits`: pick per-endpoint pbits for [0,1]-normalized
/// endpoints, quantizing each endpoint at both parities and keeping the
/// lower-error one independently.
pub(crate) fn determine_unique_pbits(
    total_comps: usize,
    comp_bits: u32,
    xl: &[f32; 4],
    xh: &[f32; 4],
    best_min_color: &mut Rgba,
    best_max_color: &mut Rgba,
    best_pbits: &mut [u32; 2],
) {
    let total_bits = comp_bits + 1;
    let iscalep = (1i32 << total_bits) - 1;
    let scalep = iscalep as f32;

    let mut best_err0 = 1e9f32;
    let mut best_err1 = 1e9f32;

    for p in 0..2i32 {
        let mut x_min_color = [0u32; 4];
        let mut x_max_color = [0u32; 4];
        for c in 0..4 {
            x_min_color[c] = ((((xl[c] * scalep - p as f32) * (1.0 / 2.0) + 0.5) as i32) * 2 + p)
                .clamp(p, iscalep - 1 + p) as u32;
            x_max_color[c] = ((((xh[c] * scalep - p as f32) * (1.0 / 2.0) + 0.5) as i32) * 2 + p)
                .clamp(p, iscalep - 1 + p) as u32;
        }
        let mut scaled_low = [0u32; 4];
        let mut scaled_high = [0u32; 4];
        for i in 0..4 {
            scaled_low[i] = x_min_color[i] << (8 - total_bits);
            scaled_low[i] |= scaled_low[i] >> total_bits;
            scaled_high[i] = x_max_color[i] << (8 - total_bits);
            scaled_high[i] |= scaled_high[i] >> total_bits;
        }
        let mut err0 = 0f32;
        let mut err1 = 0f32;
        for i in 0..total_comps {
            let e0 = scaled_low[i] as f32 - xl[i] * 255.0;
            err0 += e0 * e0;
            let e1 = scaled_high[i] as f32 - xh[i] * 255.0;
            err1 += e1 * e1;
        }
        if err0 < best_err0 {
            best_err0 = err0;
            best_pbits[0] = p as u32;
            for j in 0..4 {
                best_min_color[j] = (x_min_color[j] >> 1) as u8;
            }
        }
        if err1 < best_err1 {
            best_err1 = err1;
            best_pbits[1] = p as u32;
            for j in 0..4 {
                best_max_color[j] = (x_max_color[j] >> 1) as u8;
            }
        }
    }
}

/// `determine_shared_pbits`: like [`determine_unique_pbits`] but one parity
/// shared by both endpoints, scored on [0,1]-normalized error.
pub(crate) fn determine_shared_pbits(
    total_comps: usize,
    comp_bits: u32,
    xl: &[f32; 4],
    xh: &[f32; 4],
    best_min_color: &mut Rgba,
    best_max_color: &mut Rgba,
    best_pbits: &mut [u32; 2],
) {
    let total_bits = comp_bits + 1;
    let iscalep = (1i32 << total_bits) - 1;
    let scalep = iscalep as f32;

    let mut best_err = 1e9f32;

    for p in 0..2i32 {
        let mut x_min_color = [0u32; 4];
        let mut x_max_color = [0u32; 4];
        for c in 0..4 {
            x_min_color[c] = ((((xl[c] * scalep - p as f32) * (1.0 / 2.0) + 0.5) as i32) * 2 + p)
                .clamp(p, iscalep - 1 + p) as u32;
            x_max_color[c] = ((((xh[c] * scalep - p as f32) * (1.0 / 2.0) + 0.5) as i32) * 2 + p)
                .clamp(p, iscalep - 1 + p) as u32;
        }
        let mut scaled_low = [0u32; 4];
        let mut scaled_high = [0u32; 4];
        for i in 0..4 {
            scaled_low[i] = x_min_color[i] << (8 - total_bits);
            scaled_low[i] |= scaled_low[i] >> total_bits;
            scaled_high[i] = x_max_color[i] << (8 - total_bits);
            scaled_high[i] |= scaled_high[i] >> total_bits;
        }
        let mut err = 0f32;
        for i in 0..total_comps {
            let e0 = scaled_low[i] as f32 * (1.0 / 255.0) - xl[i];
            let e1 = scaled_high[i] as f32 * (1.0 / 255.0) - xh[i];
            err += e0 * e0 + e1 * e1;
        }
        if err < best_err {
            best_err = err;
            best_pbits[0] = p as u32;
            best_pbits[1] = p as u32;
            for j in 0..4 {
                best_min_color[j] = (x_min_color[j] >> 1) as u8;
                best_max_color[j] = (x_max_color[j] >> 1) as u8;
            }
        }
    }
}

// A BC7 mode 0-7 decoder, used by calc_sse to score candidate blocks in the
// partially-analytical pipelines.

/// Dequantize a BC7 endpoint code with its pbit appended.
#[inline]
fn bc7u_dequant_p(val: u32, pbit: u32, val_bits: u32) -> u32 {
    let total_bits = val_bits + 1;
    let mut v = (val << 1) | pbit;
    v <<= 8 - total_bits;
    v |= v >> total_bits;
    v
}

/// `bc7u::bc7_dequant` (no pbit).
#[inline]
fn bc7u_dequant(val: u32, val_bits: u32) -> u32 {
    let mut v = val << (8 - val_bits);
    v |= v >> val_bits;
    v
}

/// `bc7u::bc7_interp` over the 2/3/4-bit weight ramps.
#[inline]
fn bc7u_interp(l: u32, h: u32, w: u32, bits: u32) -> u32 {
    let ramp = match bits {
        2 => BC7_WEIGHTS2[w as usize],
        3 => BC7_WEIGHTS3[w as usize],
        4 => BC7_WEIGHTS4[w as usize],
        _ => return 0,
    };
    (l * (64 - ramp) + h * ramp + 32) >> 6
}

/// `bc7u::read_bits32`.
#[inline]
fn read_bits32(buf: &[u8; 16], bit_offset: &mut u32, codesize: u32) -> u32 {
    let mut bits = 0u32;
    let mut total_bits = 0u32;
    while total_bits < codesize {
        let byte_bit_offset = *bit_offset & 7;
        let bits_to_read = (codesize - total_bits).min(8 - byte_bit_offset);
        let mut byte_bits = (buf[(*bit_offset >> 3) as usize] as u32) >> byte_bit_offset;
        byte_bits &= (1 << bits_to_read) - 1;
        bits |= byte_bits << total_bits;
        total_bits += bits_to_read;
        *bit_offset += bits_to_read;
    }
    bits
}

/// `bc7u::unpack_bc7_mode0_2`.
fn unpack_bc7_mode0_2(mode: u32, block: &[u8; 16], pixels: &mut [Rgba; 16]) -> bool {
    let weight_bits: u32 = if mode == 0 { 3 } else { 2 };
    let endpoint_bits: u32 = if mode == 0 { 4 } else { 5 };
    let pbit_count: usize = if mode == 0 { 6 } else { 0 };
    let weight_vals = 1usize << weight_bits;

    let mut bit_offset = 0u32;
    if read_bits32(block, &mut bit_offset, mode + 1) != (1 << mode) {
        return false;
    }
    let part = read_bits32(block, &mut bit_offset, if mode == 0 { 4 } else { 6 }) as usize;

    let mut endpoints = [[0u32; 4]; 6];
    for c in 0..3 {
        for e in 0..6 {
            endpoints[e][c] = read_bits32(block, &mut bit_offset, endpoint_bits);
        }
    }
    let mut pbits = [0u32; 6];
    for p in 0..pbit_count {
        pbits[p] = read_bits32(block, &mut bit_offset, 1);
    }
    let mut weights = [0u32; 16];
    for i in 0..16 {
        let anchored = i == 0
            || i == G_BC7_ANCHOR_THIRD_1[part] as usize
            || i == G_BC7_ANCHOR_THIRD_2[part] as usize;
        weights[i] = read_bits32(
            block,
            &mut bit_offset,
            if anchored {
                weight_bits - 1
            } else {
                weight_bits
            },
        );
    }

    for e in 0..6 {
        for c in 0..4 {
            endpoints[e][c] = if c == 3 {
                255
            } else if pbit_count != 0 {
                bc7u_dequant_p(endpoints[e][c], pbits[e], endpoint_bits)
            } else {
                bc7u_dequant(endpoints[e][c], endpoint_bits)
            };
        }
    }

    let mut block_colors = [[[0u8; 4]; 8]; 3];
    for s in 0..3 {
        for i in 0..weight_vals {
            for c in 0..3 {
                block_colors[s][i][c] = bc7u_interp(
                    endpoints[s * 2][c],
                    endpoints[s * 2 + 1][c],
                    i as u32,
                    weight_bits,
                ) as u8;
            }
            block_colors[s][i][3] = 255;
        }
    }
    for i in 0..16 {
        pixels[i] = block_colors[G_BC7_PARTITION3[part * 16 + i] as usize][weights[i] as usize];
    }
    true
}

/// `bc7u::unpack_bc7_mode1_3_7`.
fn unpack_bc7_mode1_3_7(mode: u32, block: &[u8; 16], pixels: &mut [Rgba; 16]) -> bool {
    let comps: usize = if mode == 7 { 4 } else { 3 };
    let weight_bits: u32 = if mode == 1 { 3 } else { 2 };
    let endpoint_bits: u32 = if mode == 7 {
        5
    } else if mode == 1 {
        6
    } else {
        7
    };
    let pbit_count: usize = if mode == 1 { 2 } else { 4 };
    let shared_pbits = mode == 1;
    let weight_vals = 1usize << weight_bits;

    let mut bit_offset = 0u32;
    if read_bits32(block, &mut bit_offset, mode + 1) != (1 << mode) {
        return false;
    }
    let part = read_bits32(block, &mut bit_offset, 6) as usize;

    let mut endpoints = [[0u32; 4]; 4];
    for c in 0..comps {
        for e in 0..4 {
            endpoints[e][c] = read_bits32(block, &mut bit_offset, endpoint_bits);
        }
    }
    let mut pbits = [0u32; 4];
    for p in 0..pbit_count {
        pbits[p] = read_bits32(block, &mut bit_offset, 1);
    }
    let mut weights = [0u32; 16];
    for i in 0..16 {
        let anchored = i == 0 || i == G_BC7_ANCHOR_SECOND[part] as usize;
        weights[i] = read_bits32(
            block,
            &mut bit_offset,
            if anchored {
                weight_bits - 1
            } else {
                weight_bits
            },
        );
    }

    for e in 0..4 {
        for c in 0..4 {
            let alpha_cutoff: usize = if mode == 7 { 4 } else { 3 };
            endpoints[e][c] = if c == alpha_cutoff {
                255
            } else {
                bc7u_dequant_p(
                    endpoints[e][c],
                    pbits[if shared_pbits { e >> 1 } else { e }],
                    endpoint_bits,
                )
            };
        }
    }

    let mut block_colors = [[[0u8; 4]; 8]; 2];
    for s in 0..2 {
        for i in 0..weight_vals {
            for c in 0..comps {
                block_colors[s][i][c] = bc7u_interp(
                    endpoints[s * 2][c],
                    endpoints[s * 2 + 1][c],
                    i as u32,
                    weight_bits,
                ) as u8;
            }
            if comps == 3 {
                block_colors[s][i][3] = 255;
            }
        }
    }
    for i in 0..16 {
        pixels[i] = block_colors[G_BC7_PARTITION2[part * 16 + i] as usize][weights[i] as usize];
    }
    true
}

/// `bc7u::unpack_bc7_mode4_5`.
fn unpack_bc7_mode4_5(mode: u32, block: &[u8; 16], pixels: &mut [Rgba; 16]) -> bool {
    let weight_bits: u32 = 2;
    let a_weight_bits: u32 = if mode == 4 { 3 } else { 2 };
    let endpoint_bits: u32 = if mode == 4 { 5 } else { 7 };
    let a_endpoint_bits: u32 = if mode == 4 { 6 } else { 8 };

    let mut bit_offset = 0u32;
    if read_bits32(block, &mut bit_offset, mode + 1) != (1 << mode) {
        return false;
    }
    let comp_rot = read_bits32(block, &mut bit_offset, 2) as usize;
    let index_mode = if mode == 4 {
        read_bits32(block, &mut bit_offset, 1) as usize
    } else {
        0
    };

    let mut endpoints = [[0u32; 4]; 2];
    for c in 0..4 {
        for e in 0..2 {
            endpoints[e][c] = read_bits32(
                block,
                &mut bit_offset,
                if c == 3 {
                    a_endpoint_bits
                } else {
                    endpoint_bits
                },
            );
        }
    }

    let wb = [
        if index_mode != 0 {
            a_weight_bits
        } else {
            weight_bits
        },
        if index_mode != 0 {
            weight_bits
        } else {
            a_weight_bits
        },
    ];

    let mut weights = [0u32; 16];
    let mut a_weights = [0u32; 16];
    for i in 0..16 {
        let v = read_bits32(block, &mut bit_offset, wb[index_mode] - u32::from(i == 0));
        if index_mode != 0 {
            a_weights[i] = v;
        } else {
            weights[i] = v;
        }
    }
    for i in 0..16 {
        let v = read_bits32(
            block,
            &mut bit_offset,
            wb[1 - index_mode] - u32::from(i == 0),
        );
        if index_mode != 0 {
            weights[i] = v;
        } else {
            a_weights[i] = v;
        }
    }

    for e in 0..2 {
        for c in 0..4 {
            endpoints[e][c] = bc7u_dequant(
                endpoints[e][c],
                if c == 3 {
                    a_endpoint_bits
                } else {
                    endpoint_bits
                },
            );
        }
    }

    let mut block_colors = [[0u8; 4]; 8];
    for i in 0..(1usize << wb[0]) {
        for c in 0..3 {
            block_colors[i][c] =
                bc7u_interp(endpoints[0][c], endpoints[1][c], i as u32, wb[0]) as u8;
        }
    }
    for i in 0..(1usize << wb[1]) {
        block_colors[i][3] = bc7u_interp(endpoints[0][3], endpoints[1][3], i as u32, wb[1]) as u8;
    }

    for i in 0..16 {
        pixels[i] = block_colors[weights[i] as usize];
        pixels[i][3] = block_colors[a_weights[i] as usize][3];
        if comp_rot >= 1 {
            pixels[i].swap(3, comp_rot - 1);
        }
    }
    true
}

/// `bc7u::unpack_bc7_mode6`.
fn unpack_bc7_mode6(block: &[u8; 16], pixels: &mut [Rgba; 16]) -> bool {
    let lo = u64::from_le_bytes(block[0..8].try_into().unwrap());
    let hi = u64::from_le_bytes(block[8..16].try_into().unwrap());

    if (lo & 0x7F) as u32 != (1 << 6) {
        return false;
    }
    let f = |ofs: u32| ((lo >> ofs) & 0x7F) as u32;
    let p0 = ((lo >> 63) & 1) as u32;
    let p1 = (hi & 1) as u32;

    let r0 = (f(7) << 1) | p0;
    let g0 = (f(21) << 1) | p0;
    let b0 = (f(35) << 1) | p0;
    let a0 = (f(49) << 1) | p0;
    let r1 = (f(14) << 1) | p1;
    let g1 = (f(28) << 1) | p1;
    let b1 = (f(42) << 1) | p1;
    let a1 = (f(56) << 1) | p1;

    let mut vals = [[0u8; 4]; 16];
    for i in 0..16 {
        let w = BC7_WEIGHTS4[i];
        let iw = 64 - w;
        vals[i] = [
            ((r0 * iw + r1 * w + 32) >> 6) as u8,
            ((g0 * iw + g1 * w + 32) >> 6) as u8,
            ((b0 * iw + b1 * w + 32) >> 6) as u8,
            ((a0 * iw + a1 * w + 32) >> 6) as u8,
        ];
    }

    // Selectors: texel 0 is 3 bits at hi bit 1; the rest are 4 bits.
    let mut ofs = 1u32;
    for i in 0..16usize {
        let bits = if i == 0 { 3 } else { 4 };
        let s = ((hi >> ofs) & ((1 << bits) - 1)) as usize;
        pixels[i] = vals[s];
        ofs += bits;
    }
    true
}

/// `bc7u::unpack_bc7`.
fn unpack_bc7(block: &[u8; 16], pixels: &mut [Rgba; 16]) -> bool {
    let first_byte = block[0] as u32;
    for mode in 0..=7u32 {
        if first_byte & (1 << mode) != 0 {
            return match mode {
                0 | 2 => unpack_bc7_mode0_2(mode, block, pixels),
                1 | 3 | 7 => unpack_bc7_mode1_3_7(mode, block, pixels),
                4 | 5 => unpack_bc7_mode4_5(mode, block, pixels),
                6 => unpack_bc7_mode6(block, pixels),
                _ => false,
            };
        }
    }
    false
}

/// `calc_sse`: decode a candidate block and compute its true RGBA squared
/// error against the source pixels (`u32::MAX` on an undecodable block).
fn calc_sse(block: &[u8; 16], pixels: &[Rgba; 16]) -> u32 {
    let mut unpacked = [[0u8; 4]; 16];
    if !unpack_bc7(block, &mut unpacked) {
        return u32::MAX;
    }
    let mut sse = 0u32;
    for i in 0..16 {
        for c in 0..4 {
            let d = pixels[i][c] as i32 - unpacked[i][c] as i32;
            sse += (d * d) as u32;
        }
    }
    sse
}

const SMALL_FLOAT_VAL: f32 = 0.000_012_5;

/// `analytical_quant_est_sse` (multi-channel): predicted quantization SSE
/// from the endpoint/weight level counts and per-channel spans.
fn analytical_quant_est_sse(
    e_levels: i32,
    w_levels: i32,
    num_chans: i32,
    spans: &[i32; 4],
    span_weights: Option<&[f32; 4]>,
    endpoint_weight_scale: f32,
    num_pixels: i32,
) -> f32 {
    let dep = 1.0f32 / (e_levels - 1) as f32;
    let dw = 1.0f32 / (w_levels - 1) as f32;
    let n = w_levels as f32;
    let ab_sum = (2.0 * n - 1.0) / (3.0 * (n - 1.0));
    let mut pixel_sse = if e_levels == 256 {
        0.0
    } else {
        (dep * dep)
            * ((1.0 / 12.0) * ab_sum * (255.0 * 255.0))
            * num_chans as f32
            * endpoint_weight_scale
    };
    let k = (dw * dw) * (1.0 / 12.0);
    for i in 0..num_chans as usize {
        pixel_sse += k * (spans[i] * spans[i]) as f32 * span_weights.map_or(1.0, |sw| sw[i]);
    }
    pixel_sse * num_pixels as f32
}

/// `analytical_quant_est_sse` (single channel).
fn analytical_quant_est_sse_1(
    e_levels: i32,
    w_levels: i32,
    span: i32,
    span_weight: f32,
    endpoint_weight_scale: f32,
    num_pixels: i32,
) -> f32 {
    let dep = 1.0f32 / (e_levels - 1) as f32;
    let dw = 1.0f32 / (w_levels - 1) as f32;
    let n = w_levels as f32;
    let ab_sum = (2.0 * n - 1.0) / (3.0 * (n - 1.0));
    let mut pixel_sse = if e_levels == 256 {
        0.0
    } else {
        (dep * dep) * ((1.0 / 12.0) * ab_sum * (255.0 * 255.0)) * endpoint_weight_scale
    };
    pixel_sse += (dw * dw) * (1.0 / 12.0) * (span * span) as f32 * span_weight;
    pixel_sse * num_pixels as f32
}

/// `estimate_slam_to_line_sse_3D`: variance unexplained by the given axis
/// (Rayleigh quotient residual over the 3x3 covariance).
fn estimate_slam_to_line_sse_3d(
    cov: &[f32; 6],
    mut xr: f32,
    mut yr: f32,
    mut zr: f32,
    ortho_ratio: Option<&mut f32>,
) -> f32 {
    let total_var = cov[0] + cov[3] + cov[5];
    let mut l = crate::mathf::sqrtf(xr * xr + yr * yr + zr * zr);
    if l < SMALL_FLOAT_VAL {
        xr = 0.577_350_26;
        yr = 0.577_350_26;
        zr = 0.577_350_26;
    } else {
        l = 1.0 / l;
        xr *= l;
        yr *= l;
        zr *= l;
    }
    let xr2 = cov[0] * xr + cov[1] * yr + cov[2] * zr;
    let xg2 = cov[1] * xr + cov[3] * yr + cov[4] * zr;
    let xb2 = cov[2] * xr + cov[4] * yr + cov[5] * zr;
    let principal_axis_var = xr2 * xr + xg2 * yr + xb2 * zr;
    let ortho_var = 0.0f32.max(total_var - principal_axis_var);
    if let Some(r) = ortho_ratio {
        *r = if total_var > SMALL_FLOAT_VAL {
            ortho_var / total_var
        } else {
            0.0
        };
    }
    ortho_var
}

/// `estimate_slam_to_line_sse_4D`.
fn estimate_slam_to_line_sse_4d(
    cov: &[f32; 10],
    mut xr: f32,
    mut yr: f32,
    mut zr: f32,
    mut wr: f32,
    ortho_ratio: Option<&mut f32>,
) -> f32 {
    let total_var = cov[0] + cov[4] + cov[7] + cov[9];
    let mut l = crate::mathf::sqrtf(xr * xr + yr * yr + zr * zr + wr * wr);
    if l < SMALL_FLOAT_VAL {
        xr = 0.5;
        yr = 0.5;
        zr = 0.5;
        wr = 0.5;
    } else {
        l = 1.0 / l;
        xr *= l;
        yr *= l;
        zr *= l;
        wr *= l;
    }
    let xr2 = cov[0] * xr + cov[1] * yr + cov[2] * zr + cov[3] * wr;
    let xg2 = cov[1] * xr + cov[4] * yr + cov[5] * zr + cov[6] * wr;
    let xb2 = cov[2] * xr + cov[5] * yr + cov[7] * zr + cov[8] * wr;
    let xa2 = cov[3] * xr + cov[6] * yr + cov[8] * zr + cov[9] * wr;
    let principal_axis_var = xr2 * xr + xg2 * yr + xb2 * zr + xa2 * wr;
    let ortho_var = 0.0f32.max(total_var - principal_axis_var);
    if let Some(r) = ortho_ratio {
        *r = if total_var > SMALL_FLOAT_VAL {
            ortho_var / total_var
        } else {
            0.0
        };
    }
    ortho_var
}

/// `dist3`: squared RGB distance.
#[inline]
fn dist3(lr: i32, lg: i32, lb: i32, hr: i32, hg: i32, hb: i32) -> i32 {
    let dr = hr - lr;
    let dg = hg - lg;
    let db = hb - lb;
    dr * dr + dg * dg + db * db
}

/// `determine_3subsets`: split the block into 3 subsets by cutting the
/// principal-axis halves, then re-splitting the worse half between its
/// extreme-luma pixels. False when a degenerate split falls out.
fn determine_3subsets(
    final_3subsets: &mut [u8; 16],
    pixels: &[Rgba; 16],
    block_xr: f32,
    block_xg: f32,
    block_xb: f32,
    block_mean_r: i32,
    block_mean_g: i32,
    block_mean_b: i32,
) -> bool {
    let mut subset_indices = [0usize; 16];
    let mut subset_means = [[0i32; 3]; 2];
    let mut subset_total = [0i32; 2];

    for i in 0..16 {
        let rd = pixels[i][0] as i32 - block_mean_r;
        let gd = pixels[i][1] as i32 - block_mean_g;
        let bd = pixels[i][2] as i32 - block_mean_b;
        let subset_index =
            usize::from(rd as f32 * block_xr + gd as f32 * block_xg + bd as f32 * block_xb > 0.0);
        subset_indices[i] = subset_index;
        subset_means[subset_index][0] += pixels[i][0] as i32;
        subset_means[subset_index][1] += pixels[i][1] as i32;
        subset_means[subset_index][2] += pixels[i][2] as i32;
        subset_total[subset_index] += 1;
    }

    for i in 0..2 {
        let t = subset_total[i];
        if t == 0 {
            return false;
        }
        for c in 0..3 {
            subset_means[i][c] = (subset_means[i][c] + (t >> 1)) / t;
        }
    }

    let mut subset_sses = [0i32; 2];
    for i in 0..16 {
        let s = subset_indices[i];
        subset_sses[s] += dist3(
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            subset_means[s][0],
            subset_means[s][1],
            subset_means[s][2],
        );
    }

    let subset_to_split = usize::from(subset_sses[1] > subset_sses[0]);
    if subset_total[subset_to_split] < 2 {
        return false;
    }

    let mut lo_y = i32::MAX;
    let mut hi_y = 0i32;
    for i in 0..16 {
        if subset_indices[i] != subset_to_split {
            continue;
        }
        let y = ((pixels[i][0] as i32 + pixels[i][1] as i32 + pixels[i][2] as i32) << 4) + i as i32;
        lo_y = lo_y.min(y);
        hi_y = hi_y.max(y);
    }

    let lo_y_index = (lo_y & 15) as usize;
    let hi_y_index = (hi_y & 15) as usize;
    if lo_y_index == hi_y_index {
        return false;
    }

    let (lr, lg, lb) = (
        pixels[lo_y_index][0] as i32,
        pixels[lo_y_index][1] as i32,
        pixels[lo_y_index][2] as i32,
    );
    let (hr, hg, hb) = (
        pixels[hi_y_index][0] as i32,
        pixels[hi_y_index][1] as i32,
        pixels[hi_y_index][2] as i32,
    );

    final_3subsets.fill(2);
    for i in 0..16 {
        if subset_indices[i] == subset_to_split {
            let dist0 = dist3(
                lr,
                lg,
                lb,
                pixels[i][0] as i32,
                pixels[i][1] as i32,
                pixels[i][2] as i32,
            );
            let dist1 = dist3(
                hr,
                hg,
                hb,
                pixels[i][0] as i32,
                pixels[i][1] as i32,
                pixels[i][2] as i32,
            );
            final_3subsets[i] = u8::from(dist1 > dist0);
        }
    }
    true
}

/// `pick_3subset_pat_index`: best 3-subset partition pattern by maximum
/// permuted overlap (popcount matching); also reports the best of the first
/// 16 patterns (mode 0's 4-bit id range).
fn pick_3subset_pat_index(desired_subsets: &[u8; 16], best_pat_index_first16: &mut u32) -> i32 {
    let t = tables();
    *best_pat_index_first16 = 0;

    let mut m = [0u16; 3];
    for i in 0..16 {
        m[desired_subsets[i] as usize] |= 1 << i;
    }
    let n0 = m[0].count_ones() as i32;
    let n1 = m[1].count_ones() as i32;
    let n2 = 16 - n0 - n1;

    let mut best_score = -1i32;
    let mut best_pat = 0i32;

    for p in 0..MAX_PATTERNS3_TO_CHECK {
        let s0 = (t.part3_bitmasks[p] & 0xFFFF) as u16;
        let s1 = (t.part3_bitmasks[p] >> 16) as u16;

        let c00 = (m[0] & s0).count_ones() as i32;
        let c01 = (m[0] & s1).count_ones() as i32;
        let c02 = n0 - c00 - c01;
        let c10 = (m[1] & s0).count_ones() as i32;
        let c11 = (m[1] & s1).count_ones() as i32;
        let c12 = n1 - c10 - c11;
        let c20 = (m[2] & s0).count_ones() as i32;
        let c21 = (m[2] & s1).count_ones() as i32;
        let c22 = n2 - c20 - c21;

        let mut s = c00 + c11 + c22;
        s = s.max(c00 + c12 + c21);
        s = s.max(c01 + c10 + c22);
        s = s.max(c01 + c12 + c20);
        s = s.max(c02 + c10 + c21);
        s = s.max(c02 + c11 + c20);

        if s > best_score {
            best_score = s;
            best_pat = p as i32;
            if s == 16 {
                if p <= 15 {
                    *best_pat_index_first16 = best_pat as u32;
                }
                break;
            }
        }
        if p == 15 {
            *best_pat_index_first16 = best_pat as u32;
        }
    }
    best_pat
}

/// `pack_mode1_or_3_rgb`: fit a 2-subset partition from the principal axis,
/// estimate mode 1 (6-bit/shared-pbit/3-bit-weight) vs mode 3
/// (7-bit/unique-pbit/2-bit-weight) quantization SSE, then encode the winner
/// with one least-squares refinement pass. Returns false when the estimate
/// cannot beat `sse_est_to_beat` (the caller's bailout).
fn pack_mode1_or_3_rgb(
    block: &mut [u8; 16],
    pixels: &[Rgba; 16],
    block_xr: f32,
    block_xg: f32,
    block_xb: f32,
    block_mean_r: i32,
    block_mean_g: i32,
    block_mean_b: i32,
    sse_est_to_beat: f32,
    fl: u32,
    mut final_sse_est: Option<&mut f32>,
    actual_sse: Option<&mut u32>,
) -> bool {
    let t = tables();

    let mut desired_pat_bits = 0u32;
    for i in 0..16 {
        let r = (pixels[i][0] as i32 - block_mean_r) as f32;
        let g = (pixels[i][1] as i32 - block_mean_g) as f32;
        let b = (pixels[i][2] as i32 - block_mean_b) as f32;
        let subset = u32::from(r * block_xr + g * block_xg + b * block_xb > 0.0);
        desired_pat_bits |= subset << i;
    }

    let mut best_diff = u32::MAX;
    for p in 0..MAX_PATTERNS2_TO_CHECK {
        let pat_bits = t.part2_bitmasks[p] as u32;
        let diff = (pat_bits ^ desired_pat_bits).count_ones() as i32;
        let diff_inv = 16 - diff;
        let min_diff = ((diff.min(diff_inv) as u32) << 8) | p as u32;
        if min_diff < best_diff {
            best_diff = min_diff;
        }
    }
    let best_pat_index = best_diff & 0xFF;
    let best_pat_bits = t.part2_bitmasks[best_pat_index as usize] as u32;

    let mut total_r = [0i32; 2];
    let mut total_g = [0i32; 2];
    let mut total_b = [0i32; 2];
    let mut total_c = [0i32; 2];
    for i in 0..16 {
        let s = ((best_pat_bits >> i) & 1) as usize;
        total_r[s] += pixels[i][0] as i32;
        total_g[s] += pixels[i][1] as i32;
        total_b[s] += pixels[i][2] as i32;
        total_c[s] += 1;
    }

    let mut mean_r = [0i32; 2];
    let mut mean_g = [0i32; 2];
    let mut mean_b = [0i32; 2];
    for s in 0..2 {
        let tc = total_c[s];
        let h = tc >> 1;
        mean_r[s] = (total_r[s] + h) / tc;
        mean_g[s] = (total_g[s] + h) / tc;
        mean_b[s] = (total_b[s] + h) / tc;
    }

    let mut icov = [[0i32; 6]; 2];
    for i in 0..16 {
        let s = ((best_pat_bits >> i) & 1) as usize;
        let r = pixels[i][0] as i32 - mean_r[s];
        let g = pixels[i][1] as i32 - mean_g[s];
        let b = pixels[i][2] as i32 - mean_b[s];
        icov[s][0] += r * r;
        icov[s][1] += r * g;
        icov[s][2] += r * b;
        icov[s][3] += g * g;
        icov[s][4] += g * b;
        icov[s][5] += b * b;
    }

    let mut ar = [0i32; 2];
    let mut ag = [0i32; 2];
    let mut ab = [0i32; 2];
    let mut slam_to_line_sse_est = 0.0f32;
    for s in 0..2 {
        let block_max_var = icov[s][0].max(icov[s][3]).max(icov[s][5]);
        let mut cov = [0f32; 6];
        for i in 0..6 {
            cov[i] = icov[s][i] as f32;
        }
        let sc = 1.0f32 / (block_max_var as f32 + 0.000_012_5);
        let wx = sc * cov[0];
        let wy = sc * cov[3];
        let wz = sc * cov[5];
        let alt_xr = cov[0] * wx + cov[1] * wy + cov[2] * wz;
        let alt_xg = cov[1] * wx + cov[3] * wy + cov[4] * wz;
        let alt_xb = cov[2] * wx + cov[4] * wy + cov[5] * wz;
        slam_to_line_sse_est += estimate_slam_to_line_sse_3d(&cov, alt_xr, alt_xg, alt_xb, None);

        let mut saxis_r = 306i32;
        let mut saxis_g = 601i32;
        let mut saxis_b = 117i32;
        let k = crate::mathf::fabsf(alt_xr)
            .max(crate::mathf::fabsf(alt_xg))
            .max(crate::mathf::fabsf(alt_xb));
        if crate::mathf::fabsf(k) >= SMALL_FLOAT_VAL {
            let m = 2048.0f32 / k;
            saxis_r = (alt_xr * m) as i32;
            saxis_g = (alt_xg * m) as i32;
            saxis_b = (alt_xb * m) as i32;
        }
        ar[s] = ((saxis_r as u32) << 4) as i32;
        ag[s] = ((saxis_g as u32) << 4) as i32;
        ab[s] = ((saxis_b as u32) << 4) as i32;
    }

    let mut low_dot = [i32::MAX; 2];
    let mut high_dot = [i32::MIN; 2];
    for i in 0..16 {
        let s = ((best_pat_bits >> i) & 1) as usize;
        let dot = pixels[i][0] as i32 * ar[s]
            + pixels[i][1] as i32 * ag[s]
            + pixels[i][2] as i32 * ab[s]
            + i as i32;
        low_dot[s] = low_dot[s].min(dot);
        high_dot[s] = high_dot[s].max(dot);
    }
    let low_c = [(low_dot[0] & 15) as usize, (low_dot[1] & 15) as usize];
    let high_c = [(high_dot[0] & 15) as usize, (high_dot[1] & 15) as usize];

    let mut spans = [0i32; 4];
    let mut quant_err_sse_est = [0f32; 2];
    for subset in 0..2 {
        let lp = low_c[subset];
        let hp = high_c[subset];
        for c in 0..3 {
            spans[c] = pixels[hp][c] as i32 - pixels[lp][c] as i32;
        }
        quant_err_sse_est[0] += analytical_quant_est_sse(
            64,
            8,
            3,
            &spans,
            None,
            if fl & flags::PBIT_OPT != 0 {
                UNIQUE_PBIT_DISCOUNT
            } else {
                1.0
            },
            total_c[subset],
        );
        quant_err_sse_est[1] += analytical_quant_est_sse(
            128,
            4,
            3,
            &spans,
            None,
            if fl & flags::PBIT_OPT != 0 {
                SHARED_PBIT_DISCOUNT
            } else {
                1.0
            },
            total_c[subset],
        );
    }

    let total_mode1_est_sse = slam_to_line_sse_est + quant_err_sse_est[0];
    let total_mode3_est_sse = slam_to_line_sse_est + quant_err_sse_est[1];

    if total_mode1_est_sse < total_mode3_est_sse {
        // Mode 1: large span.
        if let Some(est) = final_sse_est.as_deref_mut() {
            *est = total_mode1_est_sse;
        }
        if total_mode1_est_sse >= sse_est_to_beat {
            return false;
        }

        let mut lr = [0u32; 2];
        let mut lg = [0u32; 2];
        let mut lb = [0u32; 2];
        let mut hr = [0u32; 2];
        let mut hg = [0u32; 2];
        let mut hb = [0u32; 2];
        let mut pbits = [0u32; 2];

        for s in 0..2 {
            let lc = low_c[s];
            let hc = high_c[s];
            if fl & flags::PBIT_OPT != 0 {
                let q = 1.0f32 / 255.0;
                let sxl = [
                    pixels[lc][0] as f32 * q,
                    pixels[lc][1] as f32 * q,
                    pixels[lc][2] as f32 * q,
                    0.0,
                ];
                let sxh = [
                    pixels[hc][0] as f32 * q,
                    pixels[hc][1] as f32 * q,
                    pixels[hc][2] as f32 * q,
                    0.0,
                ];
                let mut bmin = [0u8; 4];
                let mut bmax = [0u8; 4];
                let mut bp = [0u32; 2];
                determine_shared_pbits(3, 6, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
                pbits[s] = bp[0];
                lr[s] = bmin[0] as u32;
                lg[s] = bmin[1] as u32;
                lb[s] = bmin[2] as u32;
                hr[s] = bmax[0] as u32;
                hg[s] = bmax[1] as u32;
                hb[s] = bmax[2] as u32;
            } else {
                let l = pixels[lc][0] as i32 + pixels[lc][1] as i32 + pixels[lc][2] as i32;
                let h = pixels[hc][0] as i32 + pixels[hc][1] as i32 + pixels[hc][2] as i32;
                if l.max(h) >= 129 * 3 {
                    pbits[s] = 1;
                }
                lr[s] = to_6_int_p(pixels[lc][0] as i32, pbits[s] as i32) as u32;
                lg[s] = to_6_int_p(pixels[lc][1] as i32, pbits[s] as i32) as u32;
                lb[s] = to_6_int_p(pixels[lc][2] as i32, pbits[s] as i32) as u32;
                hr[s] = to_6_int_p(pixels[hc][0] as i32, pbits[s] as i32) as u32;
                hg[s] = to_6_int_p(pixels[hc][1] as i32, pbits[s] as i32) as u32;
                hb[s] = to_6_int_p(pixels[hc][2] as i32, pbits[s] as i32) as u32;
            }
        }

        let mut cur_weights = [0u8; 16];
        eval_weights_mode1_rgb(
            pixels,
            &mut cur_weights,
            &lr,
            &lg,
            &lb,
            &hr,
            &hg,
            &hb,
            &pbits,
            best_pat_bits,
        );

        let mut z00 = [0f32; 2];
        let mut z10 = [0f32; 2];
        let mut z11 = [0f32; 2];
        let mut q00_r = [0f32; 2];
        let mut q00_g = [0f32; 2];
        let mut q00_b = [0f32; 2];
        for i in 0..16 {
            let s = ((best_pat_bits >> i) & 1) as usize;
            let sel = cur_weights[i] as usize;
            z00[s] += t.ls3[sel][0];
            z10[s] += t.ls3[sel][1];
            z11[s] += t.ls3[sel][2];
            let w = t.ls3[sel][3];
            q00_r[s] += w * pixels[i][0] as f32;
            q00_g[s] += w * pixels[i][1] as f32;
            q00_b[s] += w * pixels[i][2] as f32;
        }
        for s in 0..2 {
            let q10_r = total_r[s] as f32 - q00_r[s];
            let q10_g = total_g[s] as f32 - q00_g[s];
            let q10_b = total_b[s] as f32 - q00_b[s];
            let z01 = z10[s];
            let mut det = z00[s] * z11[s] - z01 * z10[s];
            if crate::mathf::fabsf(det) < 1e-8 {
                continue;
            }
            det = 1.0 / det;
            let iz00 = z11[s] * det;
            let iz01 = -z01 * det;
            let iz10 = -z10[s] * det;
            let iz11 = z00[s] * det;
            let shr = iz00 * q00_r[s] + iz01 * q10_r;
            let slr = iz10 * q00_r[s] + iz11 * q10_r;
            let shg = iz00 * q00_g[s] + iz01 * q10_g;
            let slg = iz10 * q00_g[s] + iz11 * q10_g;
            let shb = iz00 * q00_b[s] + iz01 * q10_b;
            let slb = iz10 * q00_b[s] + iz11 * q10_b;

            if fl & flags::PBIT_OPT != 0 {
                let q = 1.0f32 / 255.0;
                let sxl = [
                    (slr * q).clamp(0.0, 1.0),
                    (slg * q).clamp(0.0, 1.0),
                    (slb * q).clamp(0.0, 1.0),
                    0.0,
                ];
                let sxh = [
                    (shr * q).clamp(0.0, 1.0),
                    (shg * q).clamp(0.0, 1.0),
                    (shb * q).clamp(0.0, 1.0),
                    0.0,
                ];
                let mut bmin = [0u8; 4];
                let mut bmax = [0u8; 4];
                let mut bp = [0u32; 2];
                determine_shared_pbits(3, 6, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
                pbits[s] = bp[0];
                lr[s] = bmin[0] as u32;
                lg[s] = bmin[1] as u32;
                lb[s] = bmin[2] as u32;
                hr[s] = bmax[0] as u32;
                hg[s] = bmax[1] as u32;
                hb[s] = bmax[2] as u32;
            } else {
                let l = slr + slg + slb;
                let h = shr + shg + shb;
                pbits[s] = u32::from(l.max(h) >= 129.0 * 3.0);
                lr[s] = to_6_clamp(slr, pbits[s] as i32) as u32;
                hr[s] = to_6_clamp(shr, pbits[s] as i32) as u32;
                lg[s] = to_6_clamp(slg, pbits[s] as i32) as u32;
                hg[s] = to_6_clamp(shg, pbits[s] as i32) as u32;
                lb[s] = to_6_clamp(slb, pbits[s] as i32) as u32;
                hb[s] = to_6_clamp(shb, pbits[s] as i32) as u32;
            }
        }

        if let Some(sse) = actual_sse {
            *sse = eval_weights_mode1_rgb_sse(
                pixels,
                &mut cur_weights,
                &lr,
                &lg,
                &lb,
                &hr,
                &hg,
                &hb,
                &pbits,
                best_pat_bits,
            );
        } else {
            eval_weights_mode1_rgb(
                pixels,
                &mut cur_weights,
                &lr,
                &lg,
                &lb,
                &hr,
                &hg,
                &hb,
                &pbits,
                best_pat_bits,
            );
        }

        encode_mode1_rgb_block(
            block,
            best_pat_index,
            &mut lr,
            &mut lg,
            &mut lb,
            &mut hr,
            &mut hg,
            &mut hb,
            pbits[0],
            pbits[1],
            &cur_weights,
        );
    } else {
        // Mode 3: small span.
        if let Some(est) = final_sse_est {
            *est = total_mode3_est_sse;
        }
        if total_mode3_est_sse >= sse_est_to_beat {
            return false;
        }

        let mut lr = [0u32; 2];
        let mut lg = [0u32; 2];
        let mut lb = [0u32; 2];
        let mut hr = [0u32; 2];
        let mut hg = [0u32; 2];
        let mut hb = [0u32; 2];
        let mut pbits = [0u32; 4];

        for s in 0..2 {
            let lc = low_c[s];
            let hc = high_c[s];
            if fl & flags::PBIT_OPT != 0 {
                let q = 1.0f32 / 255.0;
                let sxl = [
                    pixels[lc][0] as f32 * q,
                    pixels[lc][1] as f32 * q,
                    pixels[lc][2] as f32 * q,
                    0.0,
                ];
                let sxh = [
                    pixels[hc][0] as f32 * q,
                    pixels[hc][1] as f32 * q,
                    pixels[hc][2] as f32 * q,
                    0.0,
                ];
                let mut bmin = [0u8; 4];
                let mut bmax = [0u8; 4];
                let mut bp = [0u32; 2];
                determine_unique_pbits(3, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
                pbits[s * 2] = bp[0];
                pbits[s * 2 + 1] = bp[1];
                lr[s] = bmin[0] as u32;
                lg[s] = bmin[1] as u32;
                lb[s] = bmin[2] as u32;
                hr[s] = bmax[0] as u32;
                hg[s] = bmax[1] as u32;
                hb[s] = bmax[2] as u32;
            } else {
                let l = pixels[lc][0] as i32 + pixels[lc][1] as i32 + pixels[lc][2] as i32;
                let l_pbit = i32::from(l >= 129);
                pbits[s * 2] = l_pbit as u32;
                lr[s] = to_7_int_p(pixels[lc][0] as i32, l_pbit) as u32;
                lg[s] = to_7_int_p(pixels[lc][1] as i32, l_pbit) as u32;
                lb[s] = to_7_int_p(pixels[lc][2] as i32, l_pbit) as u32;
                let h = pixels[hc][0] as i32 + pixels[hc][1] as i32 + pixels[hc][2] as i32;
                let h_pbit = i32::from(h >= 129);
                pbits[s * 2 + 1] = h_pbit as u32;
                hr[s] = to_7_int_p(pixels[hc][0] as i32, h_pbit) as u32;
                hg[s] = to_7_int_p(pixels[hc][1] as i32, h_pbit) as u32;
                hb[s] = to_7_int_p(pixels[hc][2] as i32, h_pbit) as u32;
            }
        }

        let mut cur_weights = [0u8; 16];
        eval_weights_mode3_rgb(
            pixels,
            &mut cur_weights,
            &lr,
            &lg,
            &lb,
            &hr,
            &hg,
            &hb,
            &pbits,
            best_pat_bits,
        );

        let mut z00 = [0f32; 2];
        let mut z10 = [0f32; 2];
        let mut z11 = [0f32; 2];
        let mut q00_r = [0f32; 2];
        let mut q00_g = [0f32; 2];
        let mut q00_b = [0f32; 2];
        for i in 0..16 {
            let s = ((best_pat_bits >> i) & 1) as usize;
            let sel = cur_weights[i] as usize;
            z00[s] += t.ls2[sel][0];
            z10[s] += t.ls2[sel][1];
            z11[s] += t.ls2[sel][2];
            let w = t.ls2[sel][3];
            q00_r[s] += w * pixels[i][0] as f32;
            q00_g[s] += w * pixels[i][1] as f32;
            q00_b[s] += w * pixels[i][2] as f32;
        }
        for s in 0..2 {
            let q10_r = total_r[s] as f32 - q00_r[s];
            let q10_g = total_g[s] as f32 - q00_g[s];
            let q10_b = total_b[s] as f32 - q00_b[s];
            let z01 = z10[s];
            let mut det = z00[s] * z11[s] - z01 * z10[s];
            if crate::mathf::fabsf(det) < 1e-8 {
                continue;
            }
            det = 1.0 / det;
            let iz00 = z11[s] * det;
            let iz01 = -z01 * det;
            let iz10 = -z10[s] * det;
            let iz11 = z00[s] * det;
            let shr = iz00 * q00_r[s] + iz01 * q10_r;
            let slr = iz10 * q00_r[s] + iz11 * q10_r;
            let shg = iz00 * q00_g[s] + iz01 * q10_g;
            let slg = iz10 * q00_g[s] + iz11 * q10_g;
            let shb = iz00 * q00_b[s] + iz01 * q10_b;
            let slb = iz10 * q00_b[s] + iz11 * q10_b;

            if fl & flags::PBIT_OPT != 0 {
                let q = 1.0f32 / 255.0;
                let sxl = [
                    (slr * q).clamp(0.0, 1.0),
                    (slg * q).clamp(0.0, 1.0),
                    (slb * q).clamp(0.0, 1.0),
                    0.0,
                ];
                let sxh = [
                    (shr * q).clamp(0.0, 1.0),
                    (shg * q).clamp(0.0, 1.0),
                    (shb * q).clamp(0.0, 1.0),
                    0.0,
                ];
                let mut bmin = [0u8; 4];
                let mut bmax = [0u8; 4];
                let mut bp = [0u32; 2];
                determine_unique_pbits(3, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
                pbits[s * 2] = bp[0];
                pbits[s * 2 + 1] = bp[1];
                lr[s] = bmin[0] as u32;
                lg[s] = bmin[1] as u32;
                lb[s] = bmin[2] as u32;
                hr[s] = bmax[0] as u32;
                hg[s] = bmax[1] as u32;
                hb[s] = bmax[2] as u32;
            } else {
                let l = slr + slg + slb;
                let l_pbit = i32::from(l >= 129.0 * 3.0);
                pbits[s * 2] = l_pbit as u32;
                lr[s] = to_7_clamp(slr, l_pbit) as u32;
                lg[s] = to_7_clamp(slg, l_pbit) as u32;
                lb[s] = to_7_clamp(slb, l_pbit) as u32;
                let h = shr + shg + shb;
                let h_pbit = i32::from(h >= 129.0 * 3.0);
                pbits[s * 2 + 1] = h_pbit as u32;
                hr[s] = to_7_clamp(shr, h_pbit) as u32;
                hg[s] = to_7_clamp(shg, h_pbit) as u32;
                hb[s] = to_7_clamp(shb, h_pbit) as u32;
            }
        }

        if let Some(sse) = actual_sse {
            *sse = eval_weights_mode3_rgb_sse(
                pixels,
                &mut cur_weights,
                &lr,
                &lg,
                &lb,
                &hr,
                &hg,
                &hb,
                &pbits,
                best_pat_bits,
            );
        } else {
            eval_weights_mode3_rgb(
                pixels,
                &mut cur_weights,
                &lr,
                &lg,
                &lb,
                &hr,
                &hg,
                &hb,
                &pbits,
                best_pat_bits,
            );
        }

        let mut p4: [u32; 4] = pbits;
        encode_mode3_rgb_block(
            block,
            best_pat_index,
            &mut lr,
            &mut lg,
            &mut lb,
            &mut hr,
            &mut hg,
            &mut hb,
            &mut p4,
            &cur_weights,
        );
    }
    true
}

/// `pack_mode0_or_2_rgb`: 3-subset packing. Splits the block into three
/// subsets, matches the best partition pattern for both the mode-0 (first 16
/// patterns) and mode-2 (all 64) id ranges, estimates both modes'
/// quantization SSE, and encodes the winner with one least-squares
/// refinement. Returns false on a degenerate split or when the estimate
/// cannot beat `sse_est_to_beat`.
fn pack_mode0_or_2_rgb(
    block: &mut [u8; 16],
    pixels: &[Rgba; 16],
    block_xr: f32,
    block_xg: f32,
    block_xb: f32,
    block_mean_r: i32,
    block_mean_g: i32,
    block_mean_b: i32,
    sse_est_to_beat: f32,
    _fl: u32,
    mut final_sse_est: Option<&mut f32>,
    actual_sse: Option<&mut u32>,
) -> bool {
    let t = tables();

    let mut desired_3subsets = [0u8; 16];
    if !determine_3subsets(
        &mut desired_3subsets,
        pixels,
        block_xr,
        block_xg,
        block_xb,
        block_mean_r,
        block_mean_g,
        block_mean_b,
    ) {
        if let Some(est) = final_sse_est.as_deref_mut() {
            *est = 1e9;
        }
        return false;
    }

    let mut best_pat_indices = [0u32; 2];
    best_pat_indices[1] =
        pick_3subset_pat_index(&desired_3subsets, &mut best_pat_indices[0]) as u32;

    let mut total_quant_sse_mode = [0f32; 2];
    let mut total_slam_to_line_sse_mode = [0f32; 2];
    let mut mode_total_c = [[0i32; 3]; 2];
    let mut mode_low_c = [[0i32; 3]; 2];
    let mut mode_high_c = [[0i32; 3]; 2];
    let mut mode_total_r = [[0i32; 3]; 2];
    let mut mode_total_g = [[0i32; 3]; 2];
    let mut mode_total_b = [[0i32; 3]; 2];
    let mut spans = [0i32; 4];

    for mode_iter in 0..2usize {
        if mode_iter == 1 && best_pat_indices[0] == best_pat_indices[1] {
            for s in 0..3 {
                mode_total_c[1][s] = mode_total_c[0][s];
                mode_low_c[1][s] = mode_low_c[0][s];
                mode_high_c[1][s] = mode_high_c[0][s];
                mode_total_r[1][s] = mode_total_r[0][s];
                mode_total_g[1][s] = mode_total_g[0][s];
                mode_total_b[1][s] = mode_total_b[0][s];
                total_slam_to_line_sse_mode[1] = total_slam_to_line_sse_mode[0];
            }
        } else {
            let best_pat_index = best_pat_indices[mode_iter] as usize;
            let best_pat = &G_BC7_PARTITION3[best_pat_index * 16..best_pat_index * 16 + 16];

            for i in 0..16 {
                let s = best_pat[i] as usize;
                mode_total_r[mode_iter][s] += pixels[i][0] as i32;
                mode_total_g[mode_iter][s] += pixels[i][1] as i32;
                mode_total_b[mode_iter][s] += pixels[i][2] as i32;
                mode_total_c[mode_iter][s] += 1;
            }

            let mut mean_r = [0i32; 3];
            let mut mean_g = [0i32; 3];
            let mut mean_b = [0i32; 3];
            for s in 0..3 {
                let tc = mode_total_c[mode_iter][s];
                let h = tc >> 1;
                mean_r[s] = (mode_total_r[mode_iter][s] + h) / tc;
                mean_g[s] = (mode_total_g[mode_iter][s] + h) / tc;
                mean_b[s] = (mode_total_b[mode_iter][s] + h) / tc;
            }

            let mut icov = [[0i32; 6]; 3];
            for i in 0..16 {
                let s = best_pat[i] as usize;
                let r = pixels[i][0] as i32 - mean_r[s];
                let g = pixels[i][1] as i32 - mean_g[s];
                let b = pixels[i][2] as i32 - mean_b[s];
                icov[s][0] += r * r;
                icov[s][1] += r * g;
                icov[s][2] += r * b;
                icov[s][3] += g * g;
                icov[s][4] += g * b;
                icov[s][5] += b * b;
            }

            let mut ar = [0i32; 3];
            let mut ag = [0i32; 3];
            let mut ab = [0i32; 3];
            let mut total_slam = 0f32;
            for s in 0..3 {
                let block_max_var = icov[s][0].max(icov[s][3]).max(icov[s][5]);
                let mut cov = [0f32; 6];
                for i in 0..6 {
                    cov[i] = icov[s][i] as f32;
                }
                let sc = 1.0f32 / (block_max_var as f32 + 0.000_012_5);
                let wx = sc * cov[0];
                let wy = sc * cov[3];
                let wz = sc * cov[5];
                let alt_xr = cov[0] * wx + cov[1] * wy + cov[2] * wz;
                let alt_xg = cov[1] * wx + cov[3] * wy + cov[4] * wz;
                let alt_xb = cov[2] * wx + cov[4] * wy + cov[5] * wz;
                total_slam += estimate_slam_to_line_sse_3d(&cov, alt_xr, alt_xg, alt_xb, None);

                let mut saxis_r = 306i32;
                let mut saxis_g = 601i32;
                let mut saxis_b = 117i32;
                let k = crate::mathf::fabsf(alt_xr)
                    .max(crate::mathf::fabsf(alt_xg))
                    .max(crate::mathf::fabsf(alt_xb));
                if crate::mathf::fabsf(k) >= SMALL_FLOAT_VAL {
                    let m = 2048.0f32 / k;
                    saxis_r = (alt_xr * m) as i32;
                    saxis_g = (alt_xg * m) as i32;
                    saxis_b = (alt_xb * m) as i32;
                }
                ar[s] = ((saxis_r as u32) << 4) as i32;
                ag[s] = ((saxis_g as u32) << 4) as i32;
                ab[s] = ((saxis_b as u32) << 4) as i32;
            }
            total_slam_to_line_sse_mode[mode_iter] = total_slam;

            let mut low_dot = [i32::MAX; 3];
            let mut high_dot = [i32::MIN; 3];
            for i in 0..16 {
                let s = best_pat[i] as usize;
                let dot = pixels[i][0] as i32 * ar[s]
                    + pixels[i][1] as i32 * ag[s]
                    + pixels[i][2] as i32 * ab[s]
                    + i as i32;
                low_dot[s] = low_dot[s].min(dot);
                high_dot[s] = high_dot[s].max(dot);
            }
            for s in 0..3 {
                mode_low_c[mode_iter][s] = low_dot[s] & 15;
                mode_high_c[mode_iter][s] = high_dot[s] & 15;
            }
        }

        for subset in 0..3 {
            let lp = mode_low_c[mode_iter][subset] as usize;
            let hp = mode_high_c[mode_iter][subset] as usize;
            for c in 0..3 {
                spans[c] = pixels[hp][c] as i32 - pixels[lp][c] as i32;
            }
            let subset_sse = if mode_iter == 0 {
                analytical_quant_est_sse(
                    16,
                    8,
                    3,
                    &spans,
                    None,
                    UNIQUE_PBIT_DISCOUNT,
                    mode_total_c[mode_iter][subset],
                )
            } else {
                analytical_quant_est_sse(
                    32,
                    4,
                    3,
                    &spans,
                    None,
                    1.0,
                    mode_total_c[mode_iter][subset],
                )
            };
            total_quant_sse_mode[mode_iter] += subset_sse;
        }
    }

    let total_sse_est_mode0 = total_quant_sse_mode[0] + total_slam_to_line_sse_mode[0];
    let total_sse_est_mode2 = total_quant_sse_mode[1] + total_slam_to_line_sse_mode[1];

    if total_sse_est_mode0 < total_sse_est_mode2 {
        // Mode 0 (high span).
        if let Some(est) = final_sse_est.as_deref_mut() {
            *est = total_sse_est_mode0;
        }
        if total_sse_est_mode0 >= sse_est_to_beat {
            return false;
        }

        let best_pat_index = best_pat_indices[0];
        let best_pat =
            &G_BC7_PARTITION3[(best_pat_index as usize) * 16..(best_pat_index as usize) * 16 + 16];
        let low_c = &mode_low_c[0];
        let high_c = &mode_high_c[0];
        let total_r = &mode_total_r[0];
        let total_g = &mode_total_g[0];
        let total_b = &mode_total_b[0];

        let mut xl = [[0f32; 4]; 3];
        let mut xh = [[0f32; 4]; 3];
        for s in 0..3 {
            let lc = low_c[s] as usize;
            let hc = high_c[s] as usize;
            for c in 0..3 {
                xl[s][c] = pixels[lc][c] as f32 * (1.0 / 255.0);
                xh[s][c] = pixels[hc][c] as f32 * (1.0 / 255.0);
            }
            xl[s][3] = 0.0;
            xh[s][3] = 0.0;
        }

        let mut lr = [0u32; 3];
        let mut lg = [0u32; 3];
        let mut lb = [0u32; 3];
        let mut hr = [0u32; 3];
        let mut hg = [0u32; 3];
        let mut hb = [0u32; 3];
        let mut pbits = [0u32; 6];

        for s in 0..3 {
            let mut el = [0u8; 4];
            let mut eh = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(3, 4, &xl[s], &xh[s], &mut el, &mut eh, &mut bp);
            pbits[s * 2] = bp[0];
            pbits[s * 2 + 1] = bp[1];
            lr[s] = el[0] as u32;
            lg[s] = el[1] as u32;
            lb[s] = el[2] as u32;
            hr[s] = eh[0] as u32;
            hg[s] = eh[1] as u32;
            hb[s] = eh[2] as u32;
        }

        let mut cur_weights = [0u8; 16];
        eval_weights_mode0_rgb(
            pixels,
            &mut cur_weights,
            &lr,
            &lg,
            &lb,
            &hr,
            &hg,
            &hb,
            &pbits,
            best_pat_index,
        );

        let mut z00 = [0f32; 3];
        let mut z10 = [0f32; 3];
        let mut z11 = [0f32; 3];
        let mut q00_r = [0f32; 3];
        let mut q00_g = [0f32; 3];
        let mut q00_b = [0f32; 3];
        for i in 0..16 {
            let s = best_pat[i] as usize;
            let sel = cur_weights[i] as usize;
            z00[s] += t.ls3[sel][0];
            z10[s] += t.ls3[sel][1];
            z11[s] += t.ls3[sel][2];
            let w = t.ls3[sel][3];
            q00_r[s] += w * pixels[i][0] as f32;
            q00_g[s] += w * pixels[i][1] as f32;
            q00_b[s] += w * pixels[i][2] as f32;
        }
        for s in 0..3 {
            let q10_r = total_r[s] as f32 - q00_r[s];
            let q10_g = total_g[s] as f32 - q00_g[s];
            let q10_b = total_b[s] as f32 - q00_b[s];
            let z01 = z10[s];
            let mut det = z00[s] * z11[s] - z01 * z10[s];
            if crate::mathf::fabsf(det) < 1e-8 {
                continue;
            }
            det = 1.0 / det;
            let iz00 = z11[s] * det;
            let iz01 = -z01 * det;
            let iz10 = -z10[s] * det;
            let iz11 = z00[s] * det;
            let q = 1.0f32 / 255.0;
            xl[s][0] = (q * (iz10 * q00_r[s] + iz11 * q10_r)).clamp(0.0, 1.0);
            xh[s][0] = (q * (iz00 * q00_r[s] + iz01 * q10_r)).clamp(0.0, 1.0);
            xl[s][1] = (q * (iz10 * q00_g[s] + iz11 * q10_g)).clamp(0.0, 1.0);
            xh[s][1] = (q * (iz00 * q00_g[s] + iz01 * q10_g)).clamp(0.0, 1.0);
            xl[s][2] = (q * (iz10 * q00_b[s] + iz11 * q10_b)).clamp(0.0, 1.0);
            xh[s][2] = (q * (iz00 * q00_b[s] + iz01 * q10_b)).clamp(0.0, 1.0);
        }
        for s in 0..3 {
            let mut el = [0u8; 4];
            let mut eh = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(3, 4, &xl[s], &xh[s], &mut el, &mut eh, &mut bp);
            pbits[s * 2] = bp[0];
            pbits[s * 2 + 1] = bp[1];
            lr[s] = el[0] as u32;
            lg[s] = el[1] as u32;
            lb[s] = el[2] as u32;
            hr[s] = eh[0] as u32;
            hg[s] = eh[1] as u32;
            hb[s] = eh[2] as u32;
        }

        if let Some(sse) = actual_sse {
            *sse = eval_weights_mode0_rgb_sse(
                pixels,
                &mut cur_weights,
                &lr,
                &lg,
                &lb,
                &hr,
                &hg,
                &hb,
                &pbits,
                best_pat_index,
            );
        } else {
            eval_weights_mode0_rgb(
                pixels,
                &mut cur_weights,
                &lr,
                &lg,
                &lb,
                &hr,
                &hg,
                &hb,
                &pbits,
                best_pat_index,
            );
        }

        encode_mode0_rgb_block(
            block,
            best_pat_index,
            &mut lr,
            &mut lg,
            &mut lb,
            &mut hr,
            &mut hg,
            &mut hb,
            &mut pbits,
            &cur_weights,
        );
    } else {
        // Mode 2 (low span).
        if let Some(est) = final_sse_est {
            *est = total_sse_est_mode2;
        }
        if total_sse_est_mode2 >= sse_est_to_beat {
            return false;
        }

        let best_pat_index = best_pat_indices[1];
        let best_pat =
            &G_BC7_PARTITION3[(best_pat_index as usize) * 16..(best_pat_index as usize) * 16 + 16];
        let low_c = &mode_low_c[1];
        let high_c = &mode_high_c[1];
        let total_r = &mode_total_r[1];
        let total_g = &mode_total_g[1];
        let total_b = &mode_total_b[1];

        let mut lr = [0u32; 3];
        let mut lg = [0u32; 3];
        let mut lb = [0u32; 3];
        let mut hr = [0u32; 3];
        let mut hg = [0u32; 3];
        let mut hb = [0u32; 3];

        for s in 0..3 {
            let lc = low_c[s] as usize;
            let hc = high_c[s] as usize;
            lr[s] = to_5_int(pixels[lc][0] as i32) as u32;
            lg[s] = to_5_int(pixels[lc][1] as i32) as u32;
            lb[s] = to_5_int(pixels[lc][2] as i32) as u32;
            hr[s] = to_5_int(pixels[hc][0] as i32) as u32;
            hg[s] = to_5_int(pixels[hc][1] as i32) as u32;
            hb[s] = to_5_int(pixels[hc][2] as i32) as u32;
        }

        let mut cur_weights = [0u8; 16];
        eval_weights_mode2_rgb(
            pixels,
            &mut cur_weights,
            &lr,
            &lg,
            &lb,
            &hr,
            &hg,
            &hb,
            best_pat_index,
        );

        let mut z00 = [0f32; 3];
        let mut z10 = [0f32; 3];
        let mut z11 = [0f32; 3];
        let mut q00_r = [0f32; 3];
        let mut q00_g = [0f32; 3];
        let mut q00_b = [0f32; 3];
        for i in 0..16 {
            let s = best_pat[i] as usize;
            let sel = cur_weights[i] as usize;
            z00[s] += t.ls2[sel][0];
            z10[s] += t.ls2[sel][1];
            z11[s] += t.ls2[sel][2];
            let w = t.ls2[sel][3];
            q00_r[s] += w * pixels[i][0] as f32;
            q00_g[s] += w * pixels[i][1] as f32;
            q00_b[s] += w * pixels[i][2] as f32;
        }
        for s in 0..3 {
            let q10_r = total_r[s] as f32 - q00_r[s];
            let q10_g = total_g[s] as f32 - q00_g[s];
            let q10_b = total_b[s] as f32 - q00_b[s];
            let z01 = z10[s];
            let mut det = z00[s] * z11[s] - z01 * z10[s];
            if crate::mathf::fabsf(det) < 1e-8 {
                continue;
            }
            det = 1.0 / det;
            let iz00 = z11[s] * det;
            let iz01 = -z01 * det;
            let iz10 = -z10[s] * det;
            let iz11 = z00[s] * det;
            hr[s] = to_5_clamp_np(iz00 * q00_r[s] + iz01 * q10_r) as u32;
            lr[s] = to_5_clamp_np(iz10 * q00_r[s] + iz11 * q10_r) as u32;
            hg[s] = to_5_clamp_np(iz00 * q00_g[s] + iz01 * q10_g) as u32;
            lg[s] = to_5_clamp_np(iz10 * q00_g[s] + iz11 * q10_g) as u32;
            hb[s] = to_5_clamp_np(iz00 * q00_b[s] + iz01 * q10_b) as u32;
            lb[s] = to_5_clamp_np(iz10 * q00_b[s] + iz11 * q10_b) as u32;
        }

        if let Some(sse) = actual_sse {
            *sse = eval_weights_mode2_rgb_sse(
                pixels,
                &mut cur_weights,
                &lr,
                &lg,
                &lb,
                &hr,
                &hg,
                &hb,
                best_pat_index,
            );
        } else {
            eval_weights_mode2_rgb(
                pixels,
                &mut cur_weights,
                &lr,
                &lg,
                &lb,
                &hr,
                &hg,
                &hb,
                best_pat_index,
            );
        }

        encode_mode2_rgb_block(
            block,
            best_pat_index,
            &mut lr,
            &mut lg,
            &mut lb,
            &mut hr,
            &mut hg,
            &mut hb,
            &cur_weights,
        );
    }
    true
}

/// `pack_mode4_or_5`: single-subset dual-plane packing. Optionally swaps the
/// dual-plane channel into alpha, estimates mode 5 vs mode 4's two index
/// configurations, and encodes the winner with least-squares refinement of
/// both planes. `dp_chan_index` 3 means no rotation.
fn pack_mode4_or_5(
    block: &mut [u8; 16],
    orig_pixels: &[Rgba; 16],
    dp_chan_index: usize,
    sse_est_to_beat: f32,
    _fl: u32,
    mut final_sse_est: Option<&mut f32>,
    mut actual_sse: Option<&mut u32>,
) -> bool {
    let t = tables();

    let mut swapped;
    let pixels: &[Rgba; 16] = if dp_chan_index != 3 {
        swapped = *orig_pixels;
        for px in swapped.iter_mut() {
            px.swap(dp_chan_index, 3);
        }
        &swapped
    } else {
        orig_pixels
    };

    let mut total_r = 0i32;
    let mut total_g = 0i32;
    let mut total_b = 0i32;
    let mut total_a = 0i32;
    let mut min_a = 255i32;
    let mut max_a = 0i32;
    for i in 0..16 {
        total_r += pixels[i][0] as i32;
        total_g += pixels[i][1] as i32;
        total_b += pixels[i][2] as i32;
        let a = pixels[i][3] as i32;
        total_a += a;
        min_a = min_a.min(a);
        max_a = max_a.max(a);
    }

    let mean_r = (total_r + 8) >> 4;
    let mean_g = (total_g + 8) >> 4;
    let mean_b = (total_b + 8) >> 4;

    let mut icov = [0i32; 6];
    for i in 0..16 {
        let r = pixels[i][0] as i32 - mean_r;
        let g = pixels[i][1] as i32 - mean_g;
        let b = pixels[i][2] as i32 - mean_b;
        icov[0] += r * r;
        icov[1] += r * g;
        icov[2] += r * b;
        icov[3] += g * g;
        icov[4] += g * b;
        icov[5] += b * b;
    }
    let mut cov3 = [0f32; 6];
    for i in 0..6 {
        cov3[i] = icov[i] as f32;
    }
    let block_max_var3 = icov[0].max(icov[3]).max(icov[5]);
    let sc3 = if block_max_var3 != 0 {
        1.0f32 / block_max_var3 as f32
    } else {
        0.0
    };
    let wx3 = sc3 * cov3[0];
    let wy3 = sc3 * cov3[3];
    let wz3 = sc3 * cov3[5];
    let alt_xr = cov3[0] * wx3 + cov3[1] * wy3 + cov3[2] * wz3;
    let alt_xg = cov3[1] * wx3 + cov3[3] * wy3 + cov3[4] * wz3;
    let alt_xb = cov3[2] * wx3 + cov3[4] * wy3 + cov3[5] * wz3;

    let rgb_slam_to_line_sse_est =
        estimate_slam_to_line_sse_3d(&cov3, alt_xr, alt_xg, alt_xb, None);

    let mut saxis_r = 306i32;
    let mut saxis_g = 601i32;
    let mut saxis_b = 117i32;
    let k = crate::mathf::fabsf(alt_xr)
        .max(crate::mathf::fabsf(alt_xg))
        .max(crate::mathf::fabsf(alt_xb));
    if crate::mathf::fabsf(k) >= SMALL_FLOAT_VAL {
        let m = 2048.0f32 / k;
        saxis_r = (alt_xr * m) as i32;
        saxis_g = (alt_xg * m) as i32;
        saxis_b = (alt_xb * m) as i32;
    }
    saxis_r = ((saxis_r as u32) << 4) as i32;
    saxis_g = ((saxis_g as u32) << 4) as i32;
    saxis_b = ((saxis_b as u32) << 4) as i32;

    let mut low_dot = i32::MAX;
    let mut high_dot = i32::MIN;
    for i in 0..16 {
        let dot = pixels[i][0] as i32 * saxis_r
            + pixels[i][1] as i32 * saxis_g
            + pixels[i][2] as i32 * saxis_b
            + i as i32;
        low_dot = low_dot.min(dot);
        high_dot = high_dot.max(dot);
    }
    let low_c = (low_dot & 15) as usize;
    let high_c = (high_dot & 15) as usize;

    let rgb_spans = [
        pixels[high_c][0] as i32 - pixels[low_c][0] as i32,
        pixels[high_c][1] as i32 - pixels[low_c][1] as i32,
        pixels[high_c][2] as i32 - pixels[low_c][2] as i32,
        0,
    ];
    let a_span = max_a - min_a;
    const SECOND_PLANE_SPAN_WEIGHT: f32 = 1.0;

    let mode_4_rgb_3bit = analytical_quant_est_sse(32, 8, 3, &rgb_spans, None, 1.0, 16);
    let mode_4_a_2bit =
        analytical_quant_est_sse_1(64, 4, a_span, SECOND_PLANE_SPAN_WEIGHT, 1.0, 16);
    let mode_4_rgb_2bit = analytical_quant_est_sse(32, 4, 3, &rgb_spans, None, 1.0, 16);
    let mode_4_a_3bit =
        analytical_quant_est_sse_1(64, 8, a_span, SECOND_PLANE_SPAN_WEIGHT, 1.0, 16);

    let total_mode_4_rgb3_a2 = rgb_slam_to_line_sse_est + mode_4_rgb_3bit + mode_4_a_2bit;
    let total_mode_4_rgb2_a3 = rgb_slam_to_line_sse_est + mode_4_rgb_2bit + mode_4_a_3bit;

    let mode_5_rgb = analytical_quant_est_sse(128, 4, 3, &rgb_spans, None, 1.0, 16);
    let mode_5_a = analytical_quant_est_sse_1(256, 4, a_span, SECOND_PLANE_SPAN_WEIGHT, 1.0, 16);
    let total_mode_5_rgba = rgb_slam_to_line_sse_est + mode_5_rgb + mode_5_a;

    let rot = ((dp_chan_index + 1) & 3) as u32;

    if total_mode_5_rgba < total_mode_4_rgb3_a2.min(total_mode_4_rgb2_a3) {
        // Mode 5: low RGB/A span.
        if let Some(est) = final_sse_est.as_deref_mut() {
            *est = total_mode_5_rgba;
        }
        if total_mode_5_rgba >= sse_est_to_beat {
            return false;
        }

        let mut lr = to_7_int(pixels[low_c][0] as i32);
        let mut lg = to_7_int(pixels[low_c][1] as i32);
        let mut lb = to_7_int(pixels[low_c][2] as i32);
        let mut la = min_a;
        let mut hr = to_7_int(pixels[high_c][0] as i32);
        let mut hg = to_7_int(pixels[high_c][1] as i32);
        let mut hb = to_7_int(pixels[high_c][2] as i32);
        let mut ha = max_a;

        let mut cur_weights0 = [0u8; 16];
        if let Some(sse) = actual_sse.as_deref_mut() {
            *sse =
                eval_weights_mode5_2bit_rgb_sse(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
        } else {
            eval_weights_mode5_2bit_rgb(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
        }

        let mut xl = [0f32; 4];
        let mut xh = [0f32; 4];
        if compute_least_squares_endpoints_3d(
            16,
            &cur_weights0,
            &t.ls2,
            &mut xl,
            &mut xh,
            pixels,
            total_r as f32,
            total_g as f32,
            total_b as f32,
        ) {
            lr = fast_roundf_int(xl[0] * (127.0 / 255.0));
            lg = fast_roundf_int(xl[1] * (127.0 / 255.0));
            lb = fast_roundf_int(xl[2] * (127.0 / 255.0));
            hr = fast_roundf_int(xh[0] * (127.0 / 255.0));
            hg = fast_roundf_int(xh[1] * (127.0 / 255.0));
            hb = fast_roundf_int(xh[2] * (127.0 / 255.0));
            if let Some(sse) = actual_sse.as_deref_mut() {
                *sse = eval_weights_mode5_2bit_rgb_sse(
                    pixels,
                    &mut cur_weights0,
                    lr,
                    lg,
                    lb,
                    hr,
                    hg,
                    hb,
                );
            } else {
                eval_weights_mode5_2bit_rgb(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
            }
        }

        let mut cur_weights1 = [0u8; 16];
        let mut a_sse = 0u32;
        if actual_sse.is_some() {
            a_sse = eval_weights_mode5_2bit_a_sse(pixels, &mut cur_weights1, la, ha);
        } else {
            eval_weights_mode5_2bit_a(pixels, &mut cur_weights1, la, ha);
        }

        let mut nal = 0f32;
        let mut nah = 0f32;
        if compute_least_squares_endpoints_1d(
            16,
            &cur_weights1,
            &t.ls2,
            &mut nal,
            &mut nah,
            pixels,
            3,
            total_a as f32,
        ) {
            la = fast_roundf_int(nal);
            ha = fast_roundf_int(nah);
            if actual_sse.is_some() {
                a_sse = eval_weights_mode5_2bit_a_sse(pixels, &mut cur_weights1, la, ha);
            } else {
                eval_weights_mode5_2bit_a(pixels, &mut cur_weights1, la, ha);
            }
        }
        if let Some(sse) = actual_sse.as_deref_mut() {
            *sse += a_sse;
        }

        encode_mode5_rgba_block(
            block,
            lr as u32,
            lg as u32,
            lb as u32,
            la as u32,
            hr as u32,
            hg as u32,
            hb as u32,
            ha as u32,
            &cur_weights0,
            &cur_weights1,
            rot,
        );
    } else if total_mode_4_rgb3_a2 < total_mode_4_rgb2_a3 {
        // Mode 4, RGB 3-bit / alpha 2-bit (index_flag = 1).
        if let Some(est) = final_sse_est.as_deref_mut() {
            *est = total_mode_4_rgb3_a2;
        }
        if total_mode_4_rgb3_a2 >= sse_est_to_beat {
            return false;
        }

        let mut lr = to_5_int(pixels[low_c][0] as i32);
        let mut lg = to_5_int(pixels[low_c][1] as i32);
        let mut lb = to_5_int(pixels[low_c][2] as i32);
        let mut la = to_6_int(min_a);
        let mut hr = to_5_int(pixels[high_c][0] as i32);
        let mut hg = to_5_int(pixels[high_c][1] as i32);
        let mut hb = to_5_int(pixels[high_c][2] as i32);
        let mut ha = to_6_int(max_a);

        let mut cur_weights0 = [0u8; 16];
        if let Some(sse) = actual_sse.as_deref_mut() {
            *sse =
                eval_weights_mode4_3bit_rgb_sse(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
        } else {
            eval_weights_mode4_3bit_rgb(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
        }

        let mut xl = [0f32; 4];
        let mut xh = [0f32; 4];
        if compute_least_squares_endpoints_3d(
            16,
            &cur_weights0,
            &t.ls3,
            &mut xl,
            &mut xh,
            pixels,
            total_r as f32,
            total_g as f32,
            total_b as f32,
        ) {
            lr = fast_roundf_int(xl[0] * (31.0 / 255.0));
            lg = fast_roundf_int(xl[1] * (31.0 / 255.0));
            lb = fast_roundf_int(xl[2] * (31.0 / 255.0));
            hr = fast_roundf_int(xh[0] * (31.0 / 255.0));
            hg = fast_roundf_int(xh[1] * (31.0 / 255.0));
            hb = fast_roundf_int(xh[2] * (31.0 / 255.0));
            if let Some(sse) = actual_sse.as_deref_mut() {
                *sse = eval_weights_mode4_3bit_rgb_sse(
                    pixels,
                    &mut cur_weights0,
                    lr,
                    lg,
                    lb,
                    hr,
                    hg,
                    hb,
                );
            } else {
                eval_weights_mode4_3bit_rgb(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
            }
        }

        let mut cur_weights1 = [0u8; 16];
        let mut a_sse = 0u32;
        if actual_sse.is_some() {
            a_sse = eval_weights_mode4_2bit_a_sse(pixels, &mut cur_weights1, la, ha);
        } else {
            eval_weights_mode4_2bit_a(pixels, &mut cur_weights1, la, ha);
        }

        let mut nal = 0f32;
        let mut nah = 0f32;
        if compute_least_squares_endpoints_1d(
            16,
            &cur_weights1,
            &t.ls2,
            &mut nal,
            &mut nah,
            pixels,
            3,
            total_a as f32,
        ) {
            la = fast_roundf_int(nal * (63.0 / 255.0));
            ha = fast_roundf_int(nah * (63.0 / 255.0));
            if actual_sse.is_some() {
                a_sse = eval_weights_mode4_2bit_a_sse(pixels, &mut cur_weights1, la, ha);
            } else {
                eval_weights_mode4_2bit_a(pixels, &mut cur_weights1, la, ha);
            }
        }
        if let Some(sse) = actual_sse.as_deref_mut() {
            *sse += a_sse;
        }

        encode_mode4_rgba_block(
            block,
            lr as u32,
            lg as u32,
            lb as u32,
            la as u32,
            hr as u32,
            hg as u32,
            hb as u32,
            ha as u32,
            &cur_weights0,
            &cur_weights1,
            rot,
            1,
        );
    } else {
        // Mode 4, RGB 2-bit / alpha 3-bit (index_flag = 0).
        if let Some(est) = final_sse_est {
            *est = total_mode_4_rgb2_a3;
        }
        if total_mode_4_rgb2_a3 >= sse_est_to_beat {
            return false;
        }

        let mut lr = to_5_int(pixels[low_c][0] as i32);
        let mut lg = to_5_int(pixels[low_c][1] as i32);
        let mut lb = to_5_int(pixels[low_c][2] as i32);
        let mut la = to_6_int(min_a);
        let mut hr = to_5_int(pixels[high_c][0] as i32);
        let mut hg = to_5_int(pixels[high_c][1] as i32);
        let mut hb = to_5_int(pixels[high_c][2] as i32);
        let mut ha = to_6_int(max_a);

        let mut cur_weights0 = [0u8; 16];
        if let Some(sse) = actual_sse.as_deref_mut() {
            *sse =
                eval_weights_mode4_2bit_rgb_sse(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
        } else {
            eval_weights_mode4_2bit_rgb(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
        }

        let mut xl = [0f32; 4];
        let mut xh = [0f32; 4];
        if compute_least_squares_endpoints_3d(
            16,
            &cur_weights0,
            &t.ls2,
            &mut xl,
            &mut xh,
            pixels,
            total_r as f32,
            total_g as f32,
            total_b as f32,
        ) {
            lr = fast_roundf_int(xl[0] * (31.0 / 255.0));
            lg = fast_roundf_int(xl[1] * (31.0 / 255.0));
            lb = fast_roundf_int(xl[2] * (31.0 / 255.0));
            hr = fast_roundf_int(xh[0] * (31.0 / 255.0));
            hg = fast_roundf_int(xh[1] * (31.0 / 255.0));
            hb = fast_roundf_int(xh[2] * (31.0 / 255.0));
            if let Some(sse) = actual_sse.as_deref_mut() {
                *sse = eval_weights_mode4_2bit_rgb_sse(
                    pixels,
                    &mut cur_weights0,
                    lr,
                    lg,
                    lb,
                    hr,
                    hg,
                    hb,
                );
            } else {
                eval_weights_mode4_2bit_rgb(pixels, &mut cur_weights0, lr, lg, lb, hr, hg, hb);
            }
        }

        let mut cur_weights1 = [0u8; 16];
        let mut a_sse = 0u32;
        if actual_sse.is_some() {
            a_sse = eval_weights_mode4_3bit_a_sse(pixels, &mut cur_weights1, la, ha);
        } else {
            eval_weights_mode4_3bit_a(pixels, &mut cur_weights1, la, ha);
        }

        let mut nal = 0f32;
        let mut nah = 0f32;
        if compute_least_squares_endpoints_1d(
            16,
            &cur_weights1,
            &t.ls3,
            &mut nal,
            &mut nah,
            pixels,
            3,
            total_a as f32,
        ) {
            la = fast_roundf_int(nal * (63.0 / 255.0));
            ha = fast_roundf_int(nah * (63.0 / 255.0));
            if actual_sse.is_some() {
                a_sse = eval_weights_mode4_3bit_a_sse(pixels, &mut cur_weights1, la, ha);
            } else {
                eval_weights_mode4_3bit_a(pixels, &mut cur_weights1, la, ha);
            }
        }
        if let Some(sse) = actual_sse {
            *sse += a_sse;
        }

        encode_mode4_rgba_block(
            block,
            lr as u32,
            lg as u32,
            lb as u32,
            la as u32,
            hr as u32,
            hg as u32,
            hb as u32,
            ha as u32,
            &cur_weights0,
            &cur_weights1,
            rot,
            0,
        );
    }
    true
}

/// `pack_mode7_rgba`: 2-subset RGBA packing along a 4D principal axis
/// (power-iterated once), 5-bit endpoints with unique pbits and 2-bit
/// weights, refined by one 4D least-squares pass.
fn pack_mode7_rgba(
    block: &mut [u8; 16],
    pixels: &[Rgba; 16],
    block_xr: f32,
    block_xg: f32,
    block_xb: f32,
    block_xa: f32,
    block_mean_r: i32,
    block_mean_g: i32,
    block_mean_b: i32,
    block_mean_a: i32,
    sse_est_to_beat: f32,
    fl: u32,
    final_sse_est: Option<&mut f32>,
    actual_sse: Option<&mut u32>,
) -> bool {
    let t = tables();

    let mut desired_pat_bits = 0u32;
    for i in 0..16 {
        let r = (pixels[i][0] as i32 - block_mean_r) as f32;
        let g = (pixels[i][1] as i32 - block_mean_g) as f32;
        let b = (pixels[i][2] as i32 - block_mean_b) as f32;
        let a = (pixels[i][3] as i32 - block_mean_a) as f32;
        let subset = u32::from(r * block_xr + g * block_xg + b * block_xb + a * block_xa > 0.0);
        desired_pat_bits |= subset << i;
    }

    let mut best_diff = u32::MAX;
    for p in 0..MAX_PATTERNS2_TO_CHECK {
        let pat_bits = t.part2_bitmasks[p] as u32;
        let diff = (pat_bits ^ desired_pat_bits).count_ones() as i32;
        let diff_inv = 16 - diff;
        let min_diff = ((diff.min(diff_inv) as u32) << 8) | p as u32;
        if min_diff < best_diff {
            best_diff = min_diff;
        }
    }
    let best_pat_index = best_diff & 0xFF;
    let best_pat_bits = t.part2_bitmasks[best_pat_index as usize] as u32;

    let mut total_r = [0i32; 2];
    let mut total_g = [0i32; 2];
    let mut total_b = [0i32; 2];
    let mut total_a = [0i32; 2];
    let mut total_c = [0i32; 2];
    for i in 0..16 {
        let s = ((best_pat_bits >> i) & 1) as usize;
        total_r[s] += pixels[i][0] as i32;
        total_g[s] += pixels[i][1] as i32;
        total_b[s] += pixels[i][2] as i32;
        total_a[s] += pixels[i][3] as i32;
        total_c[s] += 1;
    }

    let mut mean_r = [0i32; 2];
    let mut mean_g = [0i32; 2];
    let mut mean_b = [0i32; 2];
    let mut mean_a = [0i32; 2];
    for s in 0..2 {
        let tc = total_c[s];
        let h = tc >> 1;
        mean_r[s] = (total_r[s] + h) / tc;
        mean_g[s] = (total_g[s] + h) / tc;
        mean_b[s] = (total_b[s] + h) / tc;
        mean_a[s] = (total_a[s] + h) / tc;
    }

    let mut icov4 = [[0i32; 10]; 2];
    for i in 0..16 {
        let s = ((best_pat_bits >> i) & 1) as usize;
        let r = pixels[i][0] as i32 - mean_r[s];
        let g = pixels[i][1] as i32 - mean_g[s];
        let b = pixels[i][2] as i32 - mean_b[s];
        let a = pixels[i][3] as i32 - mean_a[s];
        icov4[s][0] += r * r;
        icov4[s][1] += r * g;
        icov4[s][2] += r * b;
        icov4[s][3] += r * a;
        icov4[s][4] += g * g;
        icov4[s][5] += g * b;
        icov4[s][6] += g * a;
        icov4[s][7] += b * b;
        icov4[s][8] += b * a;
        icov4[s][9] += a * a;
    }

    let mut ar = [0i32; 2];
    let mut ag = [0i32; 2];
    let mut ab = [0i32; 2];
    let mut aa = [0i32; 2];
    let mut slam_to_line_sse_est = 0f32;
    for s in 0..2 {
        let block_max_var4 = icov4[s][0]
            .max(icov4[s][4])
            .max(icov4[s][7])
            .max(icov4[s][9]);
        let mut cov4 = [0f32; 10];
        for i in 0..10 {
            cov4[i] = icov4[s][i] as f32;
        }
        let sc4 = if block_max_var4 != 0 {
            1.0f32 / block_max_var4 as f32
        } else {
            0.0
        };
        let wx = sc4 * cov4[0];
        let wy = sc4 * cov4[4];
        let wz = sc4 * cov4[7];
        let wa = sc4 * cov4[9];

        let x0 = cov4[0] * wx + cov4[1] * wy + cov4[2] * wz + cov4[3] * wa;
        let y0 = cov4[1] * wx + cov4[4] * wy + cov4[5] * wz + cov4[6] * wa;
        let z0 = cov4[2] * wx + cov4[5] * wy + cov4[7] * wz + cov4[8] * wa;
        let w0 = cov4[3] * wx + cov4[6] * wy + cov4[8] * wz + cov4[9] * wa;

        let x1 = cov4[0] * x0 + cov4[1] * y0 + cov4[2] * z0 + cov4[3] * w0;
        let y1 = cov4[1] * x0 + cov4[4] * y0 + cov4[5] * z0 + cov4[6] * w0;
        let z1 = cov4[2] * x0 + cov4[5] * y0 + cov4[7] * z0 + cov4[8] * w0;
        let w1 = cov4[3] * x0 + cov4[6] * y0 + cov4[8] * z0 + cov4[9] * w0;

        slam_to_line_sse_est += estimate_slam_to_line_sse_4d(&cov4, x1, y1, z1, w1, None);

        let mut saxis_r = 256i32;
        let mut saxis_g = 256i32;
        let mut saxis_b = 256i32;
        let mut saxis_a = 256i32;
        let k = crate::mathf::fabsf(x1)
            .max(crate::mathf::fabsf(y1))
            .max(crate::mathf::fabsf(z1))
            .max(crate::mathf::fabsf(w1));
        if crate::mathf::fabsf(k) >= SMALL_FLOAT_VAL {
            let m = 2048.0f32 / k;
            saxis_r = (x1 * m) as i32;
            saxis_g = (y1 * m) as i32;
            saxis_b = (z1 * m) as i32;
            saxis_a = (w1 * m) as i32;
        }
        ar[s] = ((saxis_r as u32) << 4) as i32;
        ag[s] = ((saxis_g as u32) << 4) as i32;
        ab[s] = ((saxis_b as u32) << 4) as i32;
        aa[s] = ((saxis_a as u32) << 4) as i32;
    }

    let mut low_dot = [i32::MAX; 2];
    let mut high_dot = [i32::MIN; 2];
    for i in 0..16 {
        let s = ((best_pat_bits >> i) & 1) as usize;
        let dot = pixels[i][0] as i32 * ar[s]
            + pixels[i][1] as i32 * ag[s]
            + pixels[i][2] as i32 * ab[s]
            + pixels[i][3] as i32 * aa[s]
            + i as i32;
        low_dot[s] = low_dot[s].min(dot);
        high_dot[s] = high_dot[s].max(dot);
    }
    let low_c = [(low_dot[0] & 15) as usize, (low_dot[1] & 15) as usize];
    let high_c = [(high_dot[0] & 15) as usize, (high_dot[1] & 15) as usize];

    let mut quant_err_sse_est = 0f32;
    for subset in 0..2 {
        let lp = low_c[subset];
        let hp = high_c[subset];
        let mut spans = [0i32; 4];
        for c in 0..4 {
            spans[c] = pixels[hp][c] as i32 - pixels[lp][c] as i32;
        }
        quant_err_sse_est += analytical_quant_est_sse(
            32,
            4,
            4,
            &spans,
            None,
            if fl & flags::PBIT_OPT != 0 {
                UNIQUE_PBIT_DISCOUNT
            } else {
                1.0
            },
            total_c[subset],
        );
    }

    let total_mode7_est_sse = slam_to_line_sse_est + quant_err_sse_est;
    if let Some(est) = final_sse_est {
        *est = total_mode7_est_sse;
    }
    if total_mode7_est_sse >= sse_est_to_beat {
        return false;
    }

    let mut lr = [0u32; 2];
    let mut lg = [0u32; 2];
    let mut lb = [0u32; 2];
    let mut la = [0u32; 2];
    let mut hr = [0u32; 2];
    let mut hg = [0u32; 2];
    let mut hb = [0u32; 2];
    let mut ha = [0u32; 2];
    let mut pbits = [0u32; 4];

    for s in 0..2 {
        let lc = low_c[s];
        let hc = high_c[s];
        if fl & flags::PBIT_OPT != 0 {
            let q = 1.0f32 / 255.0;
            let sxl = [
                pixels[lc][0] as f32 * q,
                pixels[lc][1] as f32 * q,
                pixels[lc][2] as f32 * q,
                pixels[lc][3] as f32 * q,
            ];
            let sxh = [
                pixels[hc][0] as f32 * q,
                pixels[hc][1] as f32 * q,
                pixels[hc][2] as f32 * q,
                pixels[hc][3] as f32 * q,
            ];
            let mut bmin = [0u8; 4];
            let mut bmax = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(4, 5, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
            pbits[s * 2] = bp[0];
            pbits[s * 2 + 1] = bp[1];
            lr[s] = bmin[0] as u32;
            lg[s] = bmin[1] as u32;
            lb[s] = bmin[2] as u32;
            la[s] = bmin[3] as u32;
            hr[s] = bmax[0] as u32;
            hg[s] = bmax[1] as u32;
            hb[s] = bmax[2] as u32;
            ha[s] = bmax[3] as u32;
        } else {
            let l_pbit = i32::from(pixels[lc][3] >= 129);
            let h_pbit = i32::from(pixels[hc][3] >= 129);
            pbits[s * 2] = l_pbit as u32;
            pbits[s * 2 + 1] = h_pbit as u32;
            lr[s] = to_5_int_p(pixels[lc][0] as i32, l_pbit) as u32;
            lg[s] = to_5_int_p(pixels[lc][1] as i32, l_pbit) as u32;
            lb[s] = to_5_int_p(pixels[lc][2] as i32, l_pbit) as u32;
            la[s] = to_5_int_p(pixels[lc][3] as i32, l_pbit) as u32;
            hr[s] = to_5_int_p(pixels[hc][0] as i32, h_pbit) as u32;
            hg[s] = to_5_int_p(pixels[hc][1] as i32, h_pbit) as u32;
            hb[s] = to_5_int_p(pixels[hc][2] as i32, h_pbit) as u32;
            ha[s] = to_5_int_p(pixels[hc][3] as i32, h_pbit) as u32;
        }
    }

    let mut cur_weights = [0u8; 16];
    eval_weights_mode7_rgba(
        pixels,
        &mut cur_weights,
        &lr,
        &lg,
        &lb,
        &la,
        &hr,
        &hg,
        &hb,
        &ha,
        &pbits,
        best_pat_bits,
    );

    let mut z00 = [0f32; 2];
    let mut z10 = [0f32; 2];
    let mut z11 = [0f32; 2];
    let mut q00_r = [0f32; 2];
    let mut q00_g = [0f32; 2];
    let mut q00_b = [0f32; 2];
    let mut q00_a = [0f32; 2];
    for i in 0..16 {
        let s = ((best_pat_bits >> i) & 1) as usize;
        let sel = cur_weights[i] as usize;
        z00[s] += t.ls2[sel][0];
        z10[s] += t.ls2[sel][1];
        z11[s] += t.ls2[sel][2];
        let w = t.ls2[sel][3];
        q00_r[s] += w * pixels[i][0] as f32;
        q00_g[s] += w * pixels[i][1] as f32;
        q00_b[s] += w * pixels[i][2] as f32;
        q00_a[s] += w * pixels[i][3] as f32;
    }
    for s in 0..2 {
        let q10_r = total_r[s] as f32 - q00_r[s];
        let q10_g = total_g[s] as f32 - q00_g[s];
        let q10_b = total_b[s] as f32 - q00_b[s];
        let q10_a = total_a[s] as f32 - q00_a[s];
        let z01 = z10[s];
        let mut det = z00[s] * z11[s] - z01 * z10[s];
        if crate::mathf::fabsf(det) < 1e-8 {
            continue;
        }
        det = 1.0 / det;
        let iz00 = z11[s] * det;
        let iz01 = -z01 * det;
        let iz10 = -z10[s] * det;
        let iz11 = z00[s] * det;

        let slr = iz10 * q00_r[s] + iz11 * q10_r;
        let shr = iz00 * q00_r[s] + iz01 * q10_r;
        let slg = iz10 * q00_g[s] + iz11 * q10_g;
        let shg = iz00 * q00_g[s] + iz01 * q10_g;
        let slb = iz10 * q00_b[s] + iz11 * q10_b;
        let shb = iz00 * q00_b[s] + iz01 * q10_b;
        let sla = iz10 * q00_a[s] + iz11 * q10_a;
        let sha = iz00 * q00_a[s] + iz01 * q10_a;

        if fl & flags::PBIT_OPT != 0 {
            let q = 1.0f32 / 255.0;
            let sxl = [
                (slr * q).clamp(0.0, 1.0),
                (slg * q).clamp(0.0, 1.0),
                (slb * q).clamp(0.0, 1.0),
                (sla * q).clamp(0.0, 1.0),
            ];
            let sxh = [
                (shr * q).clamp(0.0, 1.0),
                (shg * q).clamp(0.0, 1.0),
                (shb * q).clamp(0.0, 1.0),
                (sha * q).clamp(0.0, 1.0),
            ];
            let mut bmin = [0u8; 4];
            let mut bmax = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(4, 5, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
            pbits[s * 2] = bp[0];
            pbits[s * 2 + 1] = bp[1];
            lr[s] = bmin[0] as u32;
            lg[s] = bmin[1] as u32;
            lb[s] = bmin[2] as u32;
            la[s] = bmin[3] as u32;
            hr[s] = bmax[0] as u32;
            hg[s] = bmax[1] as u32;
            hb[s] = bmax[2] as u32;
            ha[s] = bmax[3] as u32;
        } else {
            let l_pbit = i32::from(sla >= 129.0);
            let h_pbit = i32::from(sha >= 129.0);
            pbits[s * 2] = l_pbit as u32;
            pbits[s * 2 + 1] = h_pbit as u32;
            lr[s] = to_5_clamp(slr, l_pbit) as u32;
            lg[s] = to_5_clamp(slg, l_pbit) as u32;
            lb[s] = to_5_clamp(slb, l_pbit) as u32;
            la[s] = to_5_clamp(sla, l_pbit) as u32;
            hr[s] = to_5_clamp(shr, h_pbit) as u32;
            hg[s] = to_5_clamp(shg, h_pbit) as u32;
            hb[s] = to_5_clamp(shb, h_pbit) as u32;
            ha[s] = to_5_clamp(sha, h_pbit) as u32;
        }
    }

    if let Some(sse) = actual_sse {
        *sse = eval_weights_mode7_rgba_sse(
            pixels,
            &mut cur_weights,
            &lr,
            &lg,
            &lb,
            &la,
            &hr,
            &hg,
            &hb,
            &ha,
            &pbits,
            best_pat_bits,
        );
    } else {
        eval_weights_mode7_rgba(
            pixels,
            &mut cur_weights,
            &lr,
            &lg,
            &lb,
            &la,
            &hr,
            &hg,
            &hb,
            &ha,
            &pbits,
            best_pat_bits,
        );
    }

    encode_mode7_rgba_block(
        block,
        best_pat_index,
        &mut lr,
        &mut lg,
        &mut lb,
        &mut la,
        &mut hr,
        &mut hg,
        &mut hb,
        &mut ha,
        &mut pbits,
        &cur_weights,
    );
    true
}

// Pipeline decision thresholds.
const TRIVIAL_BLOCK_THRESH_RGB: i32 = 20 * 16;
const TRIVIAL_BLOCK_THRESH_RGBA: i32 = 2 * 16;
const DP_BLOCK_VAR_THRESH: i32 = 2 * 16;
const STRONG_CORR_THRESH: f32 = 0.85;
const MIN_BLOCK_MAX_VAR_23SUBSETS: i32 = 100 * 16;
const HIGH_ORTHO_ENERGY_THRESH: f32 = 1.0 * 16.0;
const ORTHO_RATIO_23SUBSET_RATIO_THRESH: f32 = 0.004;
const DP_BLOCK_VAR_THRESH_RGBA: i32 = 16; // 1 per texel, 16 texels
const ALPHA_DECORR_THRESHOLD: f32 = 0.995;
const STRONG_DECORR_THRESH_RGBA: f32 = 0.85;
const MIN_BLOCK_MAX_VAR_3SUBSETS: i32 = 500 * 16;

/// Shared mode-6 tail of the RGB pipelines: project onto the principal axis,
/// pick low/high pixels, quantize to 7-bit + pbits (fixed 1 or optimized),
/// evaluate weights, one least-squares refinement, and encode mode 6 with
/// alpha endpoints 127 (decoding to opaque under pbit 1).
fn pack_mode6_rgb_tail(
    block: &mut [u8; 16],
    pixels: &[Rgba; 16],
    alt_xr: f32,
    alt_xg: f32,
    alt_xb: f32,
    total_r: i32,
    total_g: i32,
    total_b: i32,
    fl: u32,
) {
    let t = tables();

    let mut saxis_r = 306i32;
    let mut saxis_g = 601i32;
    let mut saxis_b = 117i32;
    let k = crate::mathf::fabsf(alt_xr)
        .max(crate::mathf::fabsf(alt_xg))
        .max(crate::mathf::fabsf(alt_xb));
    if crate::mathf::fabsf(k) >= SMALL_FLOAT_VAL {
        let m = 2048.0f32 / k;
        saxis_r = (alt_xr * m) as i32;
        saxis_g = (alt_xg * m) as i32;
        saxis_b = (alt_xb * m) as i32;
    }
    saxis_r = ((saxis_r as u32) << 4) as i32;
    saxis_g = ((saxis_g as u32) << 4) as i32;
    saxis_b = ((saxis_b as u32) << 4) as i32;

    let mut low_dot = i32::MAX;
    let mut high_dot = i32::MIN;
    for i in 0..16 {
        let dot = pixels[i][0] as i32 * saxis_r
            + pixels[i][1] as i32 * saxis_g
            + pixels[i][2] as i32 * saxis_b
            + i as i32;
        low_dot = low_dot.min(dot);
        high_dot = high_dot.max(dot);
    }
    let low_c = (low_dot & 15) as usize;
    let high_c = (high_dot & 15) as usize;

    let mut p0: i32;
    let mut p1: i32;
    let mut lr: i32;
    let mut lg: i32;
    let mut lb: i32;
    let mut hr: i32;
    let mut hg: i32;
    let mut hb: i32;

    if fl & flags::PBIT_OPT_MODE6 != 0 {
        let q = 1.0f32 / 255.0;
        let sxl = [
            pixels[low_c][0] as f32 * q,
            pixels[low_c][1] as f32 * q,
            pixels[low_c][2] as f32 * q,
            0.0,
        ];
        let sxh = [
            pixels[high_c][0] as f32 * q,
            pixels[high_c][1] as f32 * q,
            pixels[high_c][2] as f32 * q,
            0.0,
        ];
        let mut bmin = [0u8; 4];
        let mut bmax = [0u8; 4];
        let mut bp = [0u32; 2];
        determine_unique_pbits(3, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
        p0 = bp[0] as i32;
        p1 = bp[1] as i32;
        lr = bmin[0] as i32;
        lg = bmin[1] as i32;
        lb = bmin[2] as i32;
        hr = bmax[0] as i32;
        hg = bmax[1] as i32;
        hb = bmax[2] as i32;
    } else {
        p0 = 1;
        p1 = 1;
        lr = to_7_int_p(pixels[low_c][0] as i32, p0);
        lg = to_7_int_p(pixels[low_c][1] as i32, p0);
        lb = to_7_int_p(pixels[low_c][2] as i32, p0);
        hr = to_7_int_p(pixels[high_c][0] as i32, p1);
        hg = to_7_int_p(pixels[high_c][1] as i32, p1);
        hb = to_7_int_p(pixels[high_c][2] as i32, p1);
    }

    let mut cur_weights = [0u8; 16];
    eval_weights_mode6_rgb(
        pixels,
        &mut cur_weights,
        lr,
        lg,
        lb,
        hr,
        hg,
        hb,
        p0 as u32,
        p1 as u32,
    );

    let mut xl = [0f32; 4];
    let mut xh = [0f32; 4];
    if compute_least_squares_endpoints_3d(
        16,
        &cur_weights,
        &t.ls4,
        &mut xl,
        &mut xh,
        pixels,
        total_r as f32,
        total_g as f32,
        total_b as f32,
    ) {
        if fl & flags::PBIT_OPT_MODE6 != 0 {
            let q = 1.0f32 / 255.0;
            let sxl = [xl[0] * q, xl[1] * q, xl[2] * q, 0.0];
            let sxh = [xh[0] * q, xh[1] * q, xh[2] * q, 0.0];
            let mut bmin = [0u8; 4];
            let mut bmax = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(3, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
            p0 = bp[0] as i32;
            p1 = bp[1] as i32;
            lr = bmin[0] as i32;
            lg = bmin[1] as i32;
            lb = bmin[2] as i32;
            hr = bmax[0] as i32;
            hg = bmax[1] as i32;
            hb = bmax[2] as i32;
        } else {
            p0 = 1;
            p1 = 1;
            lr = to_7_f(xl[0], p0);
            lg = to_7_f(xl[1], p0);
            lb = to_7_f(xl[2], p0);
            hr = to_7_f(xh[0], p1);
            hg = to_7_f(xh[1], p1);
            hb = to_7_f(xh[2], p1);
        }
        eval_weights_mode6_rgb(
            pixels,
            &mut cur_weights,
            lr,
            lg,
            lb,
            hr,
            hg,
            hb,
            p0 as u32,
            p1 as u32,
        );
    }

    encode_mode6_rgba_block(
        block,
        lr as u32,
        lg as u32,
        lb as u32,
        127,
        p0 as u32,
        hr as u32,
        hg as u32,
        hb as u32,
        127,
        p1 as u32,
        &cur_weights,
    );
}

/// Shared RGB block statistics: totals, min/max, mean, integer covariance,
/// and the trivial mode-6 encode for low-variance blocks. Returns `None`
/// when the block was fully handled (solid or trivial), else the stats.
struct RgbStats {
    total_r: i32,
    total_g: i32,
    total_b: i32,
    min_r: i32,
    min_g: i32,
    min_b: i32,
    max_r: i32,
    max_g: i32,
    max_b: i32,
    mean_r: i32,
    mean_g: i32,
    mean_b: i32,
    icov: [i32; 6],
    block_max_var: i32,
    desired_dp_chan: i32,
}

/// The common head of both RGB pipelines: solid check, stats/covariance,
/// dual-plane channel selection, and the trivial mode-6 path.
fn rgb_pipeline_head(block: &mut [u8; 16], pixels: &[Rgba; 16], fl: u32) -> Option<RgbStats> {
    // Solid block check (all four bytes; A's are assumed 255).
    if pixels[0] == pixels[15] && pixels[1..15].iter().all(|&p| p == pixels[0]) {
        pack_mode5_solid(block, pixels[0]);
        return None;
    }

    let mut total_r = 0i32;
    let mut total_g = 0i32;
    let mut total_b = 0i32;
    let mut min_r = 255i32;
    let mut min_g = 255i32;
    let mut min_b = 255i32;
    let mut max_r = 0i32;
    let mut max_g = 0i32;
    let mut max_b = 0i32;
    for i in 0..16 {
        let (r, g, b) = (
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
        );
        total_r += r;
        total_g += g;
        total_b += b;
        min_r = min_r.min(r);
        min_g = min_g.min(g);
        min_b = min_b.min(b);
        max_r = max_r.max(r);
        max_g = max_g.max(g);
        max_b = max_b.max(b);
    }
    let mean_r = (total_r + 8) >> 4;
    let mean_g = (total_g + 8) >> 4;
    let mean_b = (total_b + 8) >> 4;

    let mut icov = [0i32; 6];
    for i in 0..16 {
        let r = pixels[i][0] as i32 - mean_r;
        let g = pixels[i][1] as i32 - mean_g;
        let b = pixels[i][2] as i32 - mean_b;
        icov[0] += r * r;
        icov[1] += r * g;
        icov[2] += r * b;
        icov[3] += g * g;
        icov[4] += g * b;
        icov[5] += b * b;
    }
    let block_max_var = icov[0].max(icov[3]).max(icov[5]);
    if block_max_var == 0 {
        // Not redundant: the u32 solid test can be fooled by stray alpha.
        pack_mode5_solid(block, pixels[0]);
        return None;
    }

    // Dual plane: switch to modes 4/5 when one channel is strongly
    // decorrelated from the others.
    let mut desired_dp_chan = -1i32;
    if fl & flags::USE_DUAL_PLANE_RGB != 0 && block_max_var >= DP_BLOCK_VAR_THRESH {
        let has_r = icov[0] > 16;
        let has_g = icov[3] > 16;
        let has_b = icov[5] > 16;
        let total_active_chans = i32::from(has_r) + i32::from(has_g) + i32::from(has_b);
        if total_active_chans >= 2 {
            let r_var = icov[0] as f32;
            let g_var = icov[3] as f32;
            let b_var = icov[5] as f32;
            let rg_corr = if has_r && has_g {
                crate::mathf::fabsf(icov[1] as f32 / crate::mathf::sqrtf(r_var * g_var))
            } else {
                1.0
            };
            let rb_corr = if has_r && has_b {
                crate::mathf::fabsf(icov[2] as f32 / crate::mathf::sqrtf(r_var * b_var))
            } else {
                1.0
            };
            let gb_corr = if has_g && has_b {
                crate::mathf::fabsf(icov[4] as f32 / crate::mathf::sqrtf(g_var * b_var))
            } else {
                1.0
            };
            let min_p = rg_corr.min(rb_corr).min(gb_corr);
            if min_p < STRONG_CORR_THRESH {
                if total_active_chans == 2 {
                    if !has_r {
                        desired_dp_chan = 1;
                    } else {
                        desired_dp_chan = 0;
                    }
                } else if rg_corr < gb_corr && rb_corr < gb_corr {
                    desired_dp_chan = 0;
                } else if rg_corr < rb_corr && gb_corr < rb_corr {
                    desired_dp_chan = 1;
                } else {
                    desired_dp_chan = 2;
                }
            }
        }
    }

    // Trivial mode 6 for low-variance blocks: luma-extreme endpoints, no
    // least-squares refinement.
    if fl & flags::USE_TRIVIAL_MODE6 != 0
        && desired_dp_chan == -1
        && block_max_var < TRIVIAL_BLOCK_THRESH_RGB
    {
        let mut low_c = i32::MAX;
        let mut high_c = 0i32;
        for i in 0..16 {
            let y = (16 * 2) * pixels[i][0] as i32
                + (16 * 4) * pixels[i][1] as i32
                + 16 * pixels[i][2] as i32
                + i as i32;
            low_c = low_c.min(y);
            high_c = high_c.max(y);
        }
        let low_c = (low_c & 0xF) as usize;
        let high_c = (high_c & 0xF) as usize;

        let p0: i32;
        let p1: i32;
        let lr: i32;
        let lg: i32;
        let lb: i32;
        let hr: i32;
        let hg: i32;
        let hb: i32;
        if fl & flags::PBIT_OPT_MODE6 != 0 {
            let q = 1.0f32 / 255.0;
            let sxl = [
                pixels[low_c][0] as f32 * q,
                pixels[low_c][1] as f32 * q,
                pixels[low_c][2] as f32 * q,
                0.0,
            ];
            let sxh = [
                pixels[high_c][0] as f32 * q,
                pixels[high_c][1] as f32 * q,
                pixels[high_c][2] as f32 * q,
                0.0,
            ];
            let mut bmin = [0u8; 4];
            let mut bmax = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(3, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
            p0 = bp[0] as i32;
            p1 = bp[1] as i32;
            lr = bmin[0] as i32;
            lg = bmin[1] as i32;
            lb = bmin[2] as i32;
            hr = bmax[0] as i32;
            hg = bmax[1] as i32;
            hb = bmax[2] as i32;
        } else {
            p0 = 1;
            p1 = 1;
            lr = to_7_int_p(pixels[low_c][0] as i32, p0);
            lg = to_7_int_p(pixels[low_c][1] as i32, p0);
            lb = to_7_int_p(pixels[low_c][2] as i32, p0);
            hr = to_7_int_p(pixels[high_c][0] as i32, p1);
            hg = to_7_int_p(pixels[high_c][1] as i32, p1);
            hb = to_7_int_p(pixels[high_c][2] as i32, p1);
        }

        let mut cur_weights = [0u8; 16];
        eval_weights_mode6_rgb(
            pixels,
            &mut cur_weights,
            lr,
            lg,
            lb,
            hr,
            hg,
            hb,
            p0 as u32,
            p1 as u32,
        );
        encode_mode6_rgba_block(
            block,
            lr as u32,
            lg as u32,
            lb as u32,
            127,
            p0 as u32,
            hr as u32,
            hg as u32,
            hb as u32,
            127,
            p1 as u32,
            &cur_weights,
        );
        return None;
    }

    Some(RgbStats {
        total_r,
        total_g,
        total_b,
        min_r,
        min_g,
        min_b,
        max_r,
        max_g,
        max_b,
        mean_r,
        mean_g,
        mean_b,
        icov,
        block_max_var,
        desired_dp_chan,
    })
}

/// `fast_pack_bc7_rgb_analytical`: the RGB pipeline that trusts the SSE
/// estimates (no actual-SSE comparisons).
pub(crate) fn fast_pack_bc7_rgb_analytical(block: &mut [u8; 16], pixels: &[Rgba; 16], fl: u32) {
    let Some(st) = rgb_pipeline_head(block, pixels, fl) else {
        return;
    };

    let mut cov = [0f32; 6];
    for i in 0..6 {
        cov[i] = st.icov[i] as f32;
    }
    let sc = if st.block_max_var != 0 {
        1.0f32 / st.block_max_var as f32
    } else {
        0.0
    };
    let wx = sc * cov[0];
    let wy = sc * cov[3];
    let wz = sc * cov[5];
    let alt_xr = cov[0] * wx + cov[1] * wy + cov[2] * wz;
    let alt_xg = cov[1] * wx + cov[3] * wy + cov[4] * wz;
    let alt_xb = cov[2] * wx + cov[4] * wy + cov[5] * wz;

    let spans = [
        st.max_r - st.min_r,
        st.max_g - st.min_g,
        st.max_b - st.min_b,
        0,
    ];
    let need_sse_estimates = (fl & flags::USE_2_SUBSETS_RGB != 0) || st.desired_dp_chan >= 0;

    let mut mode6_ortho_ratio = 0f32;
    let mode6_slam_to_line_sse_est = if need_sse_estimates {
        estimate_slam_to_line_sse_3d(&cov, alt_xr, alt_xg, alt_xb, Some(&mut mode6_ortho_ratio))
    } else {
        0.0
    };
    let mode6_sse_est = if need_sse_estimates {
        mode6_slam_to_line_sse_est + analytical_quant_est_sse(128, 16, 3, &spans, None, 1.0, 16)
    } else {
        0.0
    };

    // Prefer 2/3-subsets over dual plane.
    if fl & flags::USE_2_SUBSETS_RGB != 0
        && st.block_max_var >= MIN_BLOCK_MAX_VAR_23SUBSETS
        && mode6_ortho_ratio > ORTHO_RATIO_23SUBSET_RATIO_THRESH
    {
        let high_ortho_energy_flag = mode6_slam_to_line_sse_est >= HIGH_ORTHO_ENERGY_THRESH;
        if high_ortho_energy_flag {
            if fl & flags::USE_3_SUBSETS_RGB != 0 && st.block_max_var >= MIN_BLOCK_MAX_VAR_3SUBSETS
            {
                let mut mode0_or_2_sse_est = 1e9f32;
                if pack_mode0_or_2_rgb(
                    block,
                    pixels,
                    alt_xr,
                    alt_xg,
                    alt_xb,
                    st.mean_r,
                    st.mean_g,
                    st.mean_b,
                    mode6_sse_est,
                    fl,
                    Some(&mut mode0_or_2_sse_est),
                    None,
                ) {
                    let mut mode1_or_3_sse_est = 1e9f32;
                    let mut temp_2subset_block = [0u8; 16];
                    if pack_mode1_or_3_rgb(
                        &mut temp_2subset_block,
                        pixels,
                        alt_xr,
                        alt_xg,
                        alt_xb,
                        st.mean_r,
                        st.mean_g,
                        st.mean_b,
                        mode0_or_2_sse_est,
                        fl,
                        Some(&mut mode1_or_3_sse_est),
                        None,
                    ) {
                        *block = temp_2subset_block;
                    }
                    return;
                }
            }
            if pack_mode1_or_3_rgb(
                block,
                pixels,
                alt_xr,
                alt_xg,
                alt_xb,
                st.mean_r,
                st.mean_g,
                st.mean_b,
                mode6_sse_est,
                fl,
                None,
                None,
            ) {
                return;
            }
        }
    }

    // Use dual plane over mode 6.
    if st.desired_dp_chan >= 0
        && pack_mode4_or_5(
            block,
            pixels,
            st.desired_dp_chan as usize,
            mode6_sse_est,
            fl,
            None,
            None,
        )
    {
        return;
    }

    pack_mode6_rgb_tail(
        block, pixels, alt_xr, alt_xg, alt_xb, st.total_r, st.total_g, st.total_b, fl,
    );
}

/// `fast_pack_bc7_rgb_partial_analytical`: like the analytical RGB pipeline
/// but every candidate mode's true SSE is computed (via the weight
/// evaluators' `_sse` twins) and the best actual block wins, tie-breaking
/// mode 4/5 > mode 0/2 > mode 1/3 > mode 6. Returns the winning SSE.
fn fast_pack_bc7_rgb_partial_analytical(block: &mut [u8; 16], pixels: &[Rgba; 16], fl: u32) -> u32 {
    let t = tables();

    // Solid block check.
    if pixels[0] == pixels[15] && pixels[1..15].iter().all(|&p| p == pixels[0]) {
        pack_mode5_solid(block, pixels[0]);
        return 0;
    }

    let mut total_r = 0i32;
    let mut total_g = 0i32;
    let mut total_b = 0i32;
    for i in 0..16 {
        total_r += pixels[i][0] as i32;
        total_g += pixels[i][1] as i32;
        total_b += pixels[i][2] as i32;
    }
    let mean_r = (total_r + 8) >> 4;
    let mean_g = (total_g + 8) >> 4;
    let mean_b = (total_b + 8) >> 4;

    let mut icov = [0i32; 6];
    for i in 0..16 {
        let r = pixels[i][0] as i32 - mean_r;
        let g = pixels[i][1] as i32 - mean_g;
        let b = pixels[i][2] as i32 - mean_b;
        icov[0] += r * r;
        icov[1] += r * g;
        icov[2] += r * b;
        icov[3] += g * g;
        icov[4] += g * b;
        icov[5] += b * b;
    }
    let block_max_var = icov[0].max(icov[3]).max(icov[5]);
    if block_max_var == 0 {
        pack_mode5_solid(block, pixels[0]);
        return 0;
    }

    let mut desired_dp_chan = -1i32;
    // This flag is never enabled on the transcode path, so it stays false; the
    // non-analytical branches below are kept but never taken.
    let non_analytical_flag = false;
    if fl & flags::USE_DUAL_PLANE_RGB != 0
        && ((!non_analytical_flag && block_max_var >= DP_BLOCK_VAR_THRESH)
            || (non_analytical_flag && block_max_var >= 16))
    {
        let has_r = icov[0] > 16;
        let has_g = icov[3] > 16;
        let has_b = icov[5] > 16;
        let total_active_chans = i32::from(has_r) + i32::from(has_g) + i32::from(has_b);
        if total_active_chans >= 2 {
            let r_var = icov[0] as f32;
            let g_var = icov[3] as f32;
            let b_var = icov[5] as f32;
            let rg_corr = if has_r && has_g {
                crate::mathf::fabsf(icov[1] as f32 / crate::mathf::sqrtf(r_var * g_var))
            } else {
                1.0
            };
            let rb_corr = if has_r && has_b {
                crate::mathf::fabsf(icov[2] as f32 / crate::mathf::sqrtf(r_var * b_var))
            } else {
                1.0
            };
            let gb_corr = if has_g && has_b {
                crate::mathf::fabsf(icov[4] as f32 / crate::mathf::sqrtf(g_var * b_var))
            } else {
                1.0
            };
            let min_p = rg_corr.min(rb_corr).min(gb_corr);
            let corr_thresh = if non_analytical_flag {
                0.999
            } else {
                STRONG_CORR_THRESH
            };
            if min_p < corr_thresh {
                if total_active_chans == 2 {
                    if !has_r {
                        desired_dp_chan = 1;
                    } else {
                        desired_dp_chan = 0;
                    }
                } else if rg_corr < gb_corr && rb_corr < gb_corr {
                    desired_dp_chan = 0;
                } else if rg_corr < rb_corr && gb_corr < rb_corr {
                    desired_dp_chan = 1;
                } else {
                    desired_dp_chan = 2;
                }
            }
        }
    }

    if fl & flags::USE_TRIVIAL_MODE6 != 0
        && desired_dp_chan == -1
        && block_max_var < TRIVIAL_BLOCK_THRESH_RGB
    {
        let mut low_c = i32::MAX;
        let mut high_c = 0i32;
        for i in 0..16 {
            let y = (16 * 2) * pixels[i][0] as i32
                + (16 * 4) * pixels[i][1] as i32
                + 16 * pixels[i][2] as i32
                + i as i32;
            low_c = low_c.min(y);
            high_c = high_c.max(y);
        }
        let low_c = (low_c & 0xF) as usize;
        let high_c = (high_c & 0xF) as usize;

        let p0: i32;
        let p1: i32;
        let lr: i32;
        let lg: i32;
        let lb: i32;
        let hr: i32;
        let hg: i32;
        let hb: i32;
        if fl & flags::PBIT_OPT_MODE6 != 0 {
            let q = 1.0f32 / 255.0;
            let sxl = [
                pixels[low_c][0] as f32 * q,
                pixels[low_c][1] as f32 * q,
                pixels[low_c][2] as f32 * q,
                0.0,
            ];
            let sxh = [
                pixels[high_c][0] as f32 * q,
                pixels[high_c][1] as f32 * q,
                pixels[high_c][2] as f32 * q,
                0.0,
            ];
            let mut bmin = [0u8; 4];
            let mut bmax = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(3, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
            p0 = bp[0] as i32;
            p1 = bp[1] as i32;
            lr = bmin[0] as i32;
            lg = bmin[1] as i32;
            lb = bmin[2] as i32;
            hr = bmax[0] as i32;
            hg = bmax[1] as i32;
            hb = bmax[2] as i32;
        } else {
            p0 = 1;
            p1 = 1;
            lr = to_7_int_p(pixels[low_c][0] as i32, p0);
            lg = to_7_int_p(pixels[low_c][1] as i32, p0);
            lb = to_7_int_p(pixels[low_c][2] as i32, p0);
            hr = to_7_int_p(pixels[high_c][0] as i32, p1);
            hg = to_7_int_p(pixels[high_c][1] as i32, p1);
            hb = to_7_int_p(pixels[high_c][2] as i32, p1);
        }

        let mut cur_weights = [0u8; 16];
        let mode6_actual_sse = eval_weights_mode6_rgb_sse(
            pixels,
            &mut cur_weights,
            lr,
            lg,
            lb,
            hr,
            hg,
            hb,
            p0 as u32,
            p1 as u32,
        );
        encode_mode6_rgba_block(
            block,
            lr as u32,
            lg as u32,
            lb as u32,
            127,
            p0 as u32,
            hr as u32,
            hg as u32,
            hb as u32,
            127,
            p1 as u32,
            &cur_weights,
        );
        return mode6_actual_sse;
    }

    let mut cov = [0f32; 6];
    for i in 0..6 {
        cov[i] = icov[i] as f32;
    }
    let sc = if block_max_var != 0 {
        1.0f32 / block_max_var as f32
    } else {
        0.0
    };
    let wx = sc * cov[0];
    let wy = sc * cov[3];
    let wz = sc * cov[5];
    let alt_xr = cov[0] * wx + cov[1] * wy + cov[2] * wz;
    let alt_xg = cov[1] * wx + cov[3] * wy + cov[4] * wz;
    let alt_xb = cov[2] * wx + cov[4] * wy + cov[5] * wz;

    let mut saxis_r = 306i32;
    let mut saxis_g = 601i32;
    let mut saxis_b = 117i32;
    let k = crate::mathf::fabsf(alt_xr)
        .max(crate::mathf::fabsf(alt_xg))
        .max(crate::mathf::fabsf(alt_xb));
    if crate::mathf::fabsf(k) >= SMALL_FLOAT_VAL {
        let m = 2048.0f32 / k;
        saxis_r = (alt_xr * m) as i32;
        saxis_g = (alt_xg * m) as i32;
        saxis_b = (alt_xb * m) as i32;
    }
    saxis_r = ((saxis_r as u32) << 4) as i32;
    saxis_g = ((saxis_g as u32) << 4) as i32;
    saxis_b = ((saxis_b as u32) << 4) as i32;

    let mut low_dot = i32::MAX;
    let mut high_dot = i32::MIN;
    for i in 0..16 {
        let dot = pixels[i][0] as i32 * saxis_r
            + pixels[i][1] as i32 * saxis_g
            + pixels[i][2] as i32 * saxis_b
            + i as i32;
        low_dot = low_dot.min(dot);
        high_dot = high_dot.max(dot);
    }
    let low_c = (low_dot & 15) as usize;
    let high_c = (high_dot & 15) as usize;

    let mut p0: i32;
    let mut p1: i32;
    let mut lr: i32;
    let mut lg: i32;
    let mut lb: i32;
    let mut hr: i32;
    let mut hg: i32;
    let mut hb: i32;
    if fl & flags::PBIT_OPT_MODE6 != 0 {
        let q = 1.0f32 / 255.0;
        let sxl = [
            pixels[low_c][0] as f32 * q,
            pixels[low_c][1] as f32 * q,
            pixels[low_c][2] as f32 * q,
            0.0,
        ];
        let sxh = [
            pixels[high_c][0] as f32 * q,
            pixels[high_c][1] as f32 * q,
            pixels[high_c][2] as f32 * q,
            0.0,
        ];
        let mut bmin = [0u8; 4];
        let mut bmax = [0u8; 4];
        let mut bp = [0u32; 2];
        determine_unique_pbits(3, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
        p0 = bp[0] as i32;
        p1 = bp[1] as i32;
        lr = bmin[0] as i32;
        lg = bmin[1] as i32;
        lb = bmin[2] as i32;
        hr = bmax[0] as i32;
        hg = bmax[1] as i32;
        hb = bmax[2] as i32;
    } else {
        p0 = 1;
        p1 = 1;
        lr = to_7_int_p(pixels[low_c][0] as i32, p0);
        lg = to_7_int_p(pixels[low_c][1] as i32, p0);
        lb = to_7_int_p(pixels[low_c][2] as i32, p0);
        hr = to_7_int_p(pixels[high_c][0] as i32, p1);
        hg = to_7_int_p(pixels[high_c][1] as i32, p1);
        hb = to_7_int_p(pixels[high_c][2] as i32, p1);
    }

    let mut cur_weights = [0u8; 16];
    let mut mode6_actual_sse = eval_weights_mode6_rgb_sse(
        pixels,
        &mut cur_weights,
        lr,
        lg,
        lb,
        hr,
        hg,
        hb,
        p0 as u32,
        p1 as u32,
    );

    if mode6_actual_sse != 0 {
        let mut xl = [0f32; 4];
        let mut xh = [0f32; 4];
        if compute_least_squares_endpoints_3d(
            16,
            &cur_weights,
            &t.ls4,
            &mut xl,
            &mut xh,
            pixels,
            total_r as f32,
            total_g as f32,
            total_b as f32,
        ) {
            let trial_p0: i32;
            let trial_p1: i32;
            let trial_lr: i32;
            let trial_lg: i32;
            let trial_lb: i32;
            let trial_hr: i32;
            let trial_hg: i32;
            let trial_hb: i32;
            if fl & flags::PBIT_OPT_MODE6 != 0 {
                let q = 1.0f32 / 255.0;
                let sxl = [xl[0] * q, xl[1] * q, xl[2] * q, 0.0];
                let sxh = [xh[0] * q, xh[1] * q, xh[2] * q, 0.0];
                let mut bmin = [0u8; 4];
                let mut bmax = [0u8; 4];
                let mut bp = [0u32; 2];
                determine_unique_pbits(3, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
                trial_p0 = bp[0] as i32;
                trial_p1 = bp[1] as i32;
                trial_lr = bmin[0] as i32;
                trial_lg = bmin[1] as i32;
                trial_lb = bmin[2] as i32;
                trial_hr = bmax[0] as i32;
                trial_hg = bmax[1] as i32;
                trial_hb = bmax[2] as i32;
            } else {
                trial_p0 = 1;
                trial_p1 = 1;
                trial_lr = to_7_f(xl[0], trial_p0);
                trial_lg = to_7_f(xl[1], trial_p0);
                trial_lb = to_7_f(xl[2], trial_p0);
                trial_hr = to_7_f(xh[0], trial_p1);
                trial_hg = to_7_f(xh[1], trial_p1);
                trial_hb = to_7_f(xh[2], trial_p1);
            }
            let mut trial_weights = [0u8; 16];
            let mode6_ls_actual_sse = eval_weights_mode6_rgb_sse(
                pixels,
                &mut trial_weights,
                trial_lr,
                trial_lg,
                trial_lb,
                trial_hr,
                trial_hg,
                trial_hb,
                trial_p0 as u32,
                trial_p1 as u32,
            );
            if mode6_ls_actual_sse < mode6_actual_sse {
                mode6_actual_sse = mode6_ls_actual_sse;
                cur_weights = trial_weights;
                p0 = trial_p0;
                p1 = trial_p1;
                lr = trial_lr;
                lg = trial_lg;
                lb = trial_lb;
                hr = trial_hr;
                hg = trial_hg;
                hb = trial_hb;
            }
        }
    }

    let mut mode02_actual_sse = u32::MAX;
    let mut mode02_candidate_block = [0u8; 16];
    let mut mode13_actual_sse = u32::MAX;
    let mut mode13_candidate_block = [0u8; 16];
    let mut mode45_actual_sse = u32::MAX;
    let mut mode45_candidate_block = [0u8; 16];

    if mode6_actual_sse != 0 {
        let mut mode6_ortho_ratio = 0f32;
        let mode6_slam_to_line_sse_est = estimate_slam_to_line_sse_3d(
            &cov,
            alt_xr,
            alt_xg,
            alt_xb,
            Some(&mut mode6_ortho_ratio),
        );

        if fl & flags::USE_2_SUBSETS_RGB != 0
            && block_max_var >= MIN_BLOCK_MAX_VAR_23SUBSETS
            && mode6_ortho_ratio > ORTHO_RATIO_23SUBSET_RATIO_THRESH
        {
            let high_ortho_energy_flag = mode6_slam_to_line_sse_est >= HIGH_ORTHO_ENERGY_THRESH;
            if high_ortho_energy_flag {
                if fl & flags::USE_3_SUBSETS_RGB != 0 && block_max_var >= MIN_BLOCK_MAX_VAR_3SUBSETS
                {
                    pack_mode0_or_2_rgb(
                        &mut mode02_candidate_block,
                        pixels,
                        alt_xr,
                        alt_xg,
                        alt_xb,
                        mean_r,
                        mean_g,
                        mean_b,
                        1e9,
                        fl,
                        None,
                        Some(&mut mode02_actual_sse),
                    );
                    pack_mode1_or_3_rgb(
                        &mut mode13_candidate_block,
                        pixels,
                        alt_xr,
                        alt_xg,
                        alt_xb,
                        mean_r,
                        mean_g,
                        mean_b,
                        1e9,
                        fl,
                        None,
                        Some(&mut mode13_actual_sse),
                    );
                } else {
                    pack_mode1_or_3_rgb(
                        &mut mode13_candidate_block,
                        pixels,
                        alt_xr,
                        alt_xg,
                        alt_xb,
                        mean_r,
                        mean_g,
                        mean_b,
                        1e9,
                        fl,
                        None,
                        Some(&mut mode13_actual_sse),
                    );
                }
            }
        }

        if desired_dp_chan >= 0 {
            pack_mode4_or_5(
                &mut mode45_candidate_block,
                pixels,
                desired_dp_chan as usize,
                1e9,
                fl,
                None,
                Some(&mut mode45_actual_sse),
            );
        }
    }

    let best_actual_sse = mode6_actual_sse
        .min(mode02_actual_sse)
        .min(mode13_actual_sse)
        .min(mode45_actual_sse);

    if mode45_actual_sse != u32::MAX && best_actual_sse == mode45_actual_sse {
        *block = mode45_candidate_block;
    } else if mode02_actual_sse != u32::MAX && best_actual_sse == mode02_actual_sse {
        *block = mode02_candidate_block;
    } else if mode13_actual_sse != u32::MAX && best_actual_sse == mode13_actual_sse {
        *block = mode13_candidate_block;
    } else {
        encode_mode6_rgba_block(
            block,
            lr as u32,
            lg as u32,
            lb as u32,
            127,
            p0 as u32,
            hr as u32,
            hg as u32,
            hb as u32,
            127,
            p1 as u32,
            &cur_weights,
        );
    }
    // Sanity check: the incrementally tracked SSE must equal a fresh decode of
    // the winning block.
    debug_assert_eq!(calc_sse(block, pixels), best_actual_sse);
    best_actual_sse
}

/// Shared RGBA stats + dual-plane selection + trivial-mode6 head of both
/// RGBA pipelines. Returns `None` when fully handled.
struct RgbaStats {
    total_r: i32,
    total_g: i32,
    total_b: i32,
    total_a: i32,
    spans: [i32; 4],
    mean_r: i32,
    mean_g: i32,
    mean_b: i32,
    mean_a: i32,
    icov4: [i32; 10],
    block_max_var4: i32,
    desired_dp_chan: i32,
}

/// Head outcome: fully handled (with the actual SSE when requested), or the
/// stats for the main pipeline.
enum RgbaHead {
    Done(u32),
    Stats(RgbaStats),
}

/// Shared head of both RGBA pipelines: short-circuit a solid block, then gather
/// the block means, the 4x4 covariance, and the dual-plane channel choice.
/// Returns `Done` when the block is already fully packed (the solid case),
/// otherwise `Stats` for the main pipeline to continue.
fn rgba_pipeline_head(
    block: &mut [u8; 16],
    pixels: &[Rgba; 16],
    fl: u32,
    want_sse: bool,
) -> RgbaHead {
    // Solid block check.
    if pixels[0] == pixels[15] && pixels[1..15].iter().all(|&p| p == pixels[0]) {
        pack_mode5_solid(block, pixels[0]);
        return RgbaHead::Done(0);
    }

    let mut total_r = 0i32;
    let mut total_g = 0i32;
    let mut total_b = 0i32;
    let mut total_a = 0i32;
    let mut min = [255i32; 4];
    let mut max = [0i32; 4];
    for i in 0..16 {
        let (r, g, b, a) = (
            pixels[i][0] as i32,
            pixels[i][1] as i32,
            pixels[i][2] as i32,
            pixels[i][3] as i32,
        );
        total_r += r;
        total_g += g;
        total_b += b;
        total_a += a;
        for (c, &v) in [r, g, b, a].iter().enumerate() {
            min[c] = min[c].min(v);
            max[c] = max[c].max(v);
        }
    }
    let mean_r = (total_r + 8) >> 4;
    let mean_g = (total_g + 8) >> 4;
    let mean_b = (total_b + 8) >> 4;
    let mean_a = (total_a + 8) >> 4;

    let mut icov4 = [0i32; 10];
    for i in 0..16 {
        let r = pixels[i][0] as i32 - mean_r;
        let g = pixels[i][1] as i32 - mean_g;
        let b = pixels[i][2] as i32 - mean_b;
        let a = pixels[i][3] as i32 - mean_a;
        icov4[0] += r * r;
        icov4[1] += r * g;
        icov4[2] += r * b;
        icov4[3] += r * a;
        icov4[4] += g * g;
        icov4[5] += g * b;
        icov4[6] += g * a;
        icov4[7] += b * b;
        icov4[8] += b * a;
        icov4[9] += a * a;
    }
    let block_max_var4 = icov4[0].max(icov4[4]).max(icov4[7]).max(icov4[9]);

    let mut desired_dp_chan = -1i32;
    if fl & flags::USE_DUAL_PLANE_RGBA != 0 && block_max_var4 >= DP_BLOCK_VAR_THRESH_RGBA {
        let r_var = icov4[0] as f32;
        let g_var = icov4[4] as f32;
        let b_var = icov4[7] as f32;
        let a_var = icov4[9] as f32;
        let has_a = icov4[9] > 0;
        if has_a {
            let p_03 = if icov4[0] != 0 {
                crate::mathf::fabsf(icov4[3] as f32 / crate::mathf::sqrtf(r_var * a_var))
            } else {
                1.0
            };
            let p_13 = if icov4[4] != 0 {
                crate::mathf::fabsf(icov4[6] as f32 / crate::mathf::sqrtf(g_var * a_var))
            } else {
                1.0
            };
            let p_23 = if icov4[7] != 0 {
                crate::mathf::fabsf(icov4[8] as f32 / crate::mathf::sqrtf(b_var * a_var))
            } else {
                1.0
            };
            let min_p = p_03.min(p_13).min(p_23);
            if min_p < ALPHA_DECORR_THRESHOLD {
                desired_dp_chan = 3;
            }
        }
        if desired_dp_chan < 0 {
            let has_r = icov4[0] > 16;
            let has_g = icov4[4] > 16;
            let has_b = icov4[7] > 16;
            let total_active_chans_rgb = i32::from(has_r) + i32::from(has_g) + i32::from(has_b);
            if total_active_chans_rgb >= 2 {
                let rg_corr = if has_r && has_g {
                    crate::mathf::fabsf(icov4[1] as f32 / crate::mathf::sqrtf(r_var * g_var))
                } else {
                    1.0
                };
                let rb_corr = if has_r && has_b {
                    crate::mathf::fabsf(icov4[2] as f32 / crate::mathf::sqrtf(r_var * b_var))
                } else {
                    1.0
                };
                let gb_corr = if has_g && has_b {
                    crate::mathf::fabsf(icov4[5] as f32 / crate::mathf::sqrtf(g_var * b_var))
                } else {
                    1.0
                };
                let min_p = rg_corr.min(rb_corr).min(gb_corr);
                if min_p < STRONG_DECORR_THRESH_RGBA {
                    if total_active_chans_rgb == 2 {
                        if !has_r {
                            desired_dp_chan = 1;
                        } else {
                            desired_dp_chan = 0;
                        }
                    } else if rg_corr < gb_corr && rb_corr < gb_corr {
                        desired_dp_chan = 0;
                    } else if rg_corr < rb_corr && gb_corr < rb_corr {
                        desired_dp_chan = 1;
                    } else {
                        desired_dp_chan = 2;
                    }
                }
            }
        }
    }

    if fl & flags::USE_TRIVIAL_MODE6 != 0
        && desired_dp_chan == -1
        && block_max_var4 < TRIVIAL_BLOCK_THRESH_RGBA
    {
        let mut low_c = i32::MAX;
        let mut high_c = 0i32;
        for i in 0..16 {
            let mut y = (16 * 2) * pixels[i][0] as i32
                + (16 * 4) * pixels[i][1] as i32
                + 16 * pixels[i][2] as i32
                + (16 * 4) * pixels[i][3] as i32;
            y += i as i32;
            low_c = low_c.min(y);
            high_c = high_c.max(y);
        }
        let low_c = (low_c & 0xF) as usize;
        let high_c = (high_c & 0xF) as usize;

        let (p0, p1, lr, lg, lb, la, hr, hg, hb, ha);
        if fl & flags::PBIT_OPT_MODE6 != 0 {
            let q = 1.0f32 / 255.0;
            let sxl = [
                pixels[low_c][0] as f32 * q,
                pixels[low_c][1] as f32 * q,
                pixels[low_c][2] as f32 * q,
                pixels[low_c][3] as f32 * q,
            ];
            let sxh = [
                pixels[high_c][0] as f32 * q,
                pixels[high_c][1] as f32 * q,
                pixels[high_c][2] as f32 * q,
                pixels[high_c][3] as f32 * q,
            ];
            let mut bmin = [0u8; 4];
            let mut bmax = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(4, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
            p0 = bp[0] as i32;
            p1 = bp[1] as i32;
            lr = bmin[0] as i32;
            lg = bmin[1] as i32;
            lb = bmin[2] as i32;
            la = bmin[3] as i32;
            hr = bmax[0] as i32;
            hg = bmax[1] as i32;
            hb = bmax[2] as i32;
            ha = bmax[3] as i32;
        } else {
            p0 = i32::from(pixels[low_c][3] > 128);
            p1 = i32::from(pixels[high_c][3] > 128);
            lr = to_7_int_p(pixels[low_c][0] as i32, p0);
            lg = to_7_int_p(pixels[low_c][1] as i32, p0);
            lb = to_7_int_p(pixels[low_c][2] as i32, p0);
            la = to_7_int_p(pixels[low_c][3] as i32, p0);
            hr = to_7_int_p(pixels[high_c][0] as i32, p1);
            hg = to_7_int_p(pixels[high_c][1] as i32, p1);
            hb = to_7_int_p(pixels[high_c][2] as i32, p1);
            ha = to_7_int_p(pixels[high_c][3] as i32, p1);
        }

        let mut cur_weights = [0u8; 16];
        let sse = if want_sse {
            eval_weights_mode6_rgba_sse(
                pixels,
                &mut cur_weights,
                lr,
                lg,
                lb,
                la,
                p0,
                hr,
                hg,
                hb,
                ha,
                p1,
            )
        } else {
            eval_weights_mode6_rgba(
                pixels,
                &mut cur_weights,
                lr,
                lg,
                lb,
                la,
                p0,
                hr,
                hg,
                hb,
                ha,
                p1,
            );
            0
        };
        encode_mode6_rgba_block(
            block,
            lr as u32,
            lg as u32,
            lb as u32,
            la as u32,
            p0 as u32,
            hr as u32,
            hg as u32,
            hb as u32,
            ha as u32,
            p1 as u32,
            &cur_weights,
        );
        return RgbaHead::Done(sse);
    }

    RgbaHead::Stats(RgbaStats {
        total_r,
        total_g,
        total_b,
        total_a,
        spans: [
            max[0] - min[0],
            max[1] - min[1],
            max[2] - min[2],
            max[3] - min[3],
        ],
        mean_r,
        mean_g,
        mean_b,
        mean_a,
        icov4,
        block_max_var4,
        desired_dp_chan,
    })
}

/// The RGBA 4D principal axis: four power iterations with normalization.
fn rgba_power_iterate(cov4: &[f32; 10], block_max_var4: i32) -> (f32, f32, f32, f32) {
    let sc4 = if block_max_var4 != 0 {
        1.0f32 / block_max_var4 as f32
    } else {
        0.0
    };
    let mut wx = sc4 * cov4[0];
    let mut wy = sc4 * cov4[4];
    let mut wz = sc4 * cov4[7];
    let mut wa = sc4 * cov4[9];
    let mut x1 = 0f32;
    let mut y1 = 0f32;
    let mut z1 = 0f32;
    let mut w1 = 0f32;
    for _ in 0..4 {
        x1 = cov4[0] * wx + cov4[1] * wy + cov4[2] * wz + cov4[3] * wa;
        y1 = cov4[1] * wx + cov4[4] * wy + cov4[5] * wz + cov4[6] * wa;
        z1 = cov4[2] * wx + cov4[5] * wy + cov4[7] * wz + cov4[8] * wa;
        w1 = cov4[3] * wx + cov4[6] * wy + cov4[8] * wz + cov4[9] * wa;
        let mut t = crate::mathf::sqrtf(x1 * x1 + y1 * y1 + z1 * z1 + w1 * w1);
        if t > SMALL_FLOAT_VAL {
            t = 1.0 / t;
            x1 *= t;
            y1 *= t;
            z1 *= t;
            w1 *= t;
        } else {
            x1 = 0.25;
            y1 = 0.25;
            z1 = 0.25;
            w1 = 0.25;
        }
        wx = x1;
        wy = y1;
        wz = z1;
        wa = w1;
    }
    (x1, y1, z1, w1)
}

/// Shared mode-6 RGBA tail: axis projection, 7-bit + pbit quantization,
/// weight eval, one 4D least-squares refinement, encode. Returns the actual
/// SSE when `want_sse` (partial pipeline), else 0.
fn pack_mode6_rgba_tail(
    block: &mut [u8; 16],
    pixels: &[Rgba; 16],
    x1: f32,
    y1: f32,
    z1: f32,
    w1: f32,
    st: &RgbaStats,
    fl: u32,
    want_sse: bool,
) -> u32 {
    let t = tables();

    let mut saxis_r = 256i32;
    let mut saxis_g = 256i32;
    let mut saxis_b = 256i32;
    let mut saxis_a = 256i32;
    let k = crate::mathf::fabsf(x1)
        .max(crate::mathf::fabsf(y1))
        .max(crate::mathf::fabsf(z1))
        .max(crate::mathf::fabsf(w1));
    if crate::mathf::fabsf(k) >= SMALL_FLOAT_VAL {
        let m = 2048.0f32 / k;
        saxis_r = (x1 * m) as i32;
        saxis_g = (y1 * m) as i32;
        saxis_b = (z1 * m) as i32;
        saxis_a = (w1 * m) as i32;
    }
    saxis_r = ((saxis_r as u32) << 4) as i32;
    saxis_g = ((saxis_g as u32) << 4) as i32;
    saxis_b = ((saxis_b as u32) << 4) as i32;
    saxis_a = ((saxis_a as u32) << 4) as i32;

    let mut low_dot = i32::MAX;
    let mut high_dot = i32::MIN;
    for i in 0..16 {
        let dot = pixels[i][0] as i32 * saxis_r
            + pixels[i][1] as i32 * saxis_g
            + pixels[i][2] as i32 * saxis_b
            + pixels[i][3] as i32 * saxis_a
            + i as i32;
        low_dot = low_dot.min(dot);
        high_dot = high_dot.max(dot);
    }
    let low_c = (low_dot & 15) as usize;
    let high_c = (high_dot & 15) as usize;

    let (mut p0, mut p1, mut lr, mut lg, mut lb, mut la, mut hr, mut hg, mut hb, mut ha);
    if fl & flags::PBIT_OPT_MODE6 != 0 {
        let q = 1.0f32 / 255.0;
        let sxl = [
            pixels[low_c][0] as f32 * q,
            pixels[low_c][1] as f32 * q,
            pixels[low_c][2] as f32 * q,
            pixels[low_c][3] as f32 * q,
        ];
        let sxh = [
            pixels[high_c][0] as f32 * q,
            pixels[high_c][1] as f32 * q,
            pixels[high_c][2] as f32 * q,
            pixels[high_c][3] as f32 * q,
        ];
        let mut bmin = [0u8; 4];
        let mut bmax = [0u8; 4];
        let mut bp = [0u32; 2];
        determine_unique_pbits(4, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
        p0 = bp[0] as i32;
        p1 = bp[1] as i32;
        lr = bmin[0] as i32;
        lg = bmin[1] as i32;
        lb = bmin[2] as i32;
        la = bmin[3] as i32;
        hr = bmax[0] as i32;
        hg = bmax[1] as i32;
        hb = bmax[2] as i32;
        ha = bmax[3] as i32;
    } else {
        p0 = i32::from(pixels[low_c][3] > 128);
        p1 = i32::from(pixels[high_c][3] > 128);
        lr = to_7_int_p(pixels[low_c][0] as i32, p0);
        lg = to_7_int_p(pixels[low_c][1] as i32, p0);
        lb = to_7_int_p(pixels[low_c][2] as i32, p0);
        la = to_7_int_p(pixels[low_c][3] as i32, p0);
        hr = to_7_int_p(pixels[high_c][0] as i32, p1);
        hg = to_7_int_p(pixels[high_c][1] as i32, p1);
        hb = to_7_int_p(pixels[high_c][2] as i32, p1);
        ha = to_7_int_p(pixels[high_c][3] as i32, p1);
    }

    let mut cur_weights = [0u8; 16];
    eval_weights_mode6_rgba(
        pixels,
        &mut cur_weights,
        lr,
        lg,
        lb,
        la,
        p0,
        hr,
        hg,
        hb,
        ha,
        p1,
    );

    let mut xl = [0f32; 4];
    let mut xh = [0f32; 4];
    if compute_least_squares_endpoints_4d(
        16,
        &cur_weights,
        &t.ls4,
        &mut xl,
        &mut xh,
        pixels,
        st.total_r as f32,
        st.total_g as f32,
        st.total_b as f32,
        st.total_a as f32,
    ) {
        if fl & flags::PBIT_OPT_MODE6 != 0 {
            let q = 1.0f32 / 255.0;
            let sxl = [xl[0] * q, xl[1] * q, xl[2] * q, xl[3] * q];
            let sxh = [xh[0] * q, xh[1] * q, xh[2] * q, xh[3] * q];
            let mut bmin = [0u8; 4];
            let mut bmax = [0u8; 4];
            let mut bp = [0u32; 2];
            determine_unique_pbits(4, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
            p0 = bp[0] as i32;
            p1 = bp[1] as i32;
            lr = bmin[0] as i32;
            lg = bmin[1] as i32;
            lb = bmin[2] as i32;
            la = bmin[3] as i32;
            hr = bmax[0] as i32;
            hg = bmax[1] as i32;
            hb = bmax[2] as i32;
            ha = bmax[3] as i32;
        } else {
            p0 = i32::from(xl[3] >= 129.0);
            lr = to_7_f(xl[0], p0);
            lg = to_7_f(xl[1], p0);
            lb = to_7_f(xl[2], p0);
            la = to_7_f(xl[3], p0);
            p1 = i32::from(xh[3] >= 129.0);
            hr = to_7_f(xh[0], p1);
            hg = to_7_f(xh[1], p1);
            hb = to_7_f(xh[2], p1);
            ha = to_7_f(xh[3], p1);
        }
        eval_weights_mode6_rgba(
            pixels,
            &mut cur_weights,
            lr,
            lg,
            lb,
            la,
            p0,
            hr,
            hg,
            hb,
            ha,
            p1,
        );
    }

    let sse = if want_sse {
        eval_weights_mode6_rgba_sse(
            pixels,
            &mut cur_weights,
            lr,
            lg,
            lb,
            la,
            p0,
            hr,
            hg,
            hb,
            ha,
            p1,
        )
    } else {
        0
    };

    encode_mode6_rgba_block(
        block,
        lr as u32,
        lg as u32,
        lb as u32,
        la as u32,
        p0 as u32,
        hr as u32,
        hg as u32,
        hb as u32,
        ha as u32,
        p1 as u32,
        &cur_weights,
    );
    sse
}

/// `fast_pack_bc7_rgba_analytical`.
fn fast_pack_bc7_rgba_analytical(block: &mut [u8; 16], pixels: &[Rgba; 16], fl: u32) {
    let RgbaHead::Stats(st) = rgba_pipeline_head(block, pixels, fl, false) else {
        return;
    };

    let mut cov4 = [0f32; 10];
    for i in 0..10 {
        cov4[i] = st.icov4[i] as f32;
    }
    let (x1, y1, z1, w1) = rgba_power_iterate(&cov4, st.block_max_var4);

    let mut mode6_ortho_ratio = 0f32;
    let mode6_slam_to_line_sse_est =
        estimate_slam_to_line_sse_4d(&cov4, x1, y1, z1, w1, Some(&mut mode6_ortho_ratio));
    let mode6_sse_est =
        mode6_slam_to_line_sse_est + analytical_quant_est_sse(128, 16, 4, &st.spans, None, 1.0, 16);

    let mut mode45_sse_est = 1e9f32;
    let mut mode7_sse_est = 1e9f32;

    let mut mode45_block = [0u8; 16];
    if st.desired_dp_chan >= 0 {
        pack_mode4_or_5(
            &mut mode45_block,
            pixels,
            st.desired_dp_chan as usize,
            mode6_sse_est,
            fl,
            Some(&mut mode45_sse_est),
            None,
        );
    }

    let mut mode7_block = [0u8; 16];
    if fl & flags::USE_2_SUBSETS_RGBA != 0
        && st.block_max_var4 >= MIN_BLOCK_MAX_VAR_23SUBSETS
        && mode6_ortho_ratio > ORTHO_RATIO_23SUBSET_RATIO_THRESH
    {
        let high_ortho_energy_flag = mode6_slam_to_line_sse_est >= HIGH_ORTHO_ENERGY_THRESH;
        if high_ortho_energy_flag {
            pack_mode7_rgba(
                &mut mode7_block,
                pixels,
                x1,
                y1,
                z1,
                w1,
                st.mean_r,
                st.mean_g,
                st.mean_b,
                st.mean_a,
                mode6_sse_est,
                fl,
                Some(&mut mode7_sse_est),
                None,
            );
        }
    }

    if mode45_sse_est < mode7_sse_est && mode45_sse_est < mode6_sse_est {
        *block = mode45_block;
        return;
    } else if mode7_sse_est < mode45_sse_est && mode7_sse_est < mode6_sse_est {
        *block = mode7_block;
        return;
    }

    pack_mode6_rgba_tail(block, pixels, x1, y1, z1, w1, &st, fl, false);
}

/// The partial pipeline's mode-6 RGBA candidate: like the analytical tail
/// but scores everything with actual SSE and keeps the least-squares
/// refinement only when it wins. Does not encode; returns the candidate.
struct Mode6RgbaCandidate {
    p0: i32,
    p1: i32,
    lr: i32,
    lg: i32,
    lb: i32,
    la: i32,
    hr: i32,
    hg: i32,
    hb: i32,
    ha: i32,
    weights: [u8; 16],
    sse: u32,
}

/// Build one BC7 mode-6 candidate from the RGBA endpoint axis `(x1, y1, z1,
/// w1)`: project the texels onto the scaled integer axis to find the extreme
/// endpoints, optionally optimize the pbits, then return the packed endpoints,
/// weights, and SSE.
fn mode6_rgba_candidate(
    pixels: &[Rgba; 16],
    x1: f32,
    y1: f32,
    z1: f32,
    w1: f32,
    st: &RgbaStats,
    fl: u32,
) -> Mode6RgbaCandidate {
    let t = tables();

    let mut saxis_r = 256i32;
    let mut saxis_g = 256i32;
    let mut saxis_b = 256i32;
    let mut saxis_a = 256i32;
    let k = crate::mathf::fabsf(x1)
        .max(crate::mathf::fabsf(y1))
        .max(crate::mathf::fabsf(z1))
        .max(crate::mathf::fabsf(w1));
    if crate::mathf::fabsf(k) >= SMALL_FLOAT_VAL {
        let m = 2048.0f32 / k;
        saxis_r = (x1 * m) as i32;
        saxis_g = (y1 * m) as i32;
        saxis_b = (z1 * m) as i32;
        saxis_a = (w1 * m) as i32;
    }
    saxis_r = ((saxis_r as u32) << 4) as i32;
    saxis_g = ((saxis_g as u32) << 4) as i32;
    saxis_b = ((saxis_b as u32) << 4) as i32;
    saxis_a = ((saxis_a as u32) << 4) as i32;

    let mut low_dot = i32::MAX;
    let mut high_dot = i32::MIN;
    for i in 0..16 {
        let dot = pixels[i][0] as i32 * saxis_r
            + pixels[i][1] as i32 * saxis_g
            + pixels[i][2] as i32 * saxis_b
            + pixels[i][3] as i32 * saxis_a
            + i as i32;
        low_dot = low_dot.min(dot);
        high_dot = high_dot.max(dot);
    }
    let low_c = (low_dot & 15) as usize;
    let high_c = (high_dot & 15) as usize;

    let (mut p0, mut p1, mut lr, mut lg, mut lb, mut la, mut hr, mut hg, mut hb, mut ha);
    if fl & flags::PBIT_OPT_MODE6 != 0 {
        let q = 1.0f32 / 255.0;
        let sxl = [
            pixels[low_c][0] as f32 * q,
            pixels[low_c][1] as f32 * q,
            pixels[low_c][2] as f32 * q,
            pixels[low_c][3] as f32 * q,
        ];
        let sxh = [
            pixels[high_c][0] as f32 * q,
            pixels[high_c][1] as f32 * q,
            pixels[high_c][2] as f32 * q,
            pixels[high_c][3] as f32 * q,
        ];
        let mut bmin = [0u8; 4];
        let mut bmax = [0u8; 4];
        let mut bp = [0u32; 2];
        determine_unique_pbits(4, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
        p0 = bp[0] as i32;
        p1 = bp[1] as i32;
        lr = bmin[0] as i32;
        lg = bmin[1] as i32;
        lb = bmin[2] as i32;
        la = bmin[3] as i32;
        hr = bmax[0] as i32;
        hg = bmax[1] as i32;
        hb = bmax[2] as i32;
        ha = bmax[3] as i32;
    } else {
        p0 = i32::from(pixels[low_c][3] > 128);
        p1 = i32::from(pixels[high_c][3] > 128);
        lr = to_7_int_p(pixels[low_c][0] as i32, p0);
        lg = to_7_int_p(pixels[low_c][1] as i32, p0);
        lb = to_7_int_p(pixels[low_c][2] as i32, p0);
        la = to_7_int_p(pixels[low_c][3] as i32, p0);
        hr = to_7_int_p(pixels[high_c][0] as i32, p1);
        hg = to_7_int_p(pixels[high_c][1] as i32, p1);
        hb = to_7_int_p(pixels[high_c][2] as i32, p1);
        ha = to_7_int_p(pixels[high_c][3] as i32, p1);
    }

    let mut cur_weights = [0u8; 16];
    let mut mode6_actual_sse = eval_weights_mode6_rgba_sse(
        pixels,
        &mut cur_weights,
        lr,
        lg,
        lb,
        la,
        p0,
        hr,
        hg,
        hb,
        ha,
        p1,
    );

    if mode6_actual_sse != 0 {
        let mut xl = [0f32; 4];
        let mut xh = [0f32; 4];
        if compute_least_squares_endpoints_4d(
            16,
            &cur_weights,
            &t.ls4,
            &mut xl,
            &mut xh,
            pixels,
            st.total_r as f32,
            st.total_g as f32,
            st.total_b as f32,
            st.total_a as f32,
        ) {
            let (
                trial_p0,
                trial_p1,
                trial_lr,
                trial_lg,
                trial_lb,
                trial_la,
                trial_hr,
                trial_hg,
                trial_hb,
                trial_ha,
            );
            if fl & flags::PBIT_OPT_MODE6 != 0 {
                let q = 1.0f32 / 255.0;
                let sxl = [xl[0] * q, xl[1] * q, xl[2] * q, xl[3] * q];
                let sxh = [xh[0] * q, xh[1] * q, xh[2] * q, xh[3] * q];
                let mut bmin = [0u8; 4];
                let mut bmax = [0u8; 4];
                let mut bp = [0u32; 2];
                determine_unique_pbits(4, 7, &sxl, &sxh, &mut bmin, &mut bmax, &mut bp);
                trial_p0 = bp[0] as i32;
                trial_p1 = bp[1] as i32;
                trial_lr = bmin[0] as i32;
                trial_lg = bmin[1] as i32;
                trial_lb = bmin[2] as i32;
                trial_la = bmin[3] as i32;
                trial_hr = bmax[0] as i32;
                trial_hg = bmax[1] as i32;
                trial_hb = bmax[2] as i32;
                trial_ha = bmax[3] as i32;
            } else {
                trial_p0 = i32::from(xl[3] >= 129.0);
                trial_lr = to_7_f(xl[0], trial_p0);
                trial_lg = to_7_f(xl[1], trial_p0);
                trial_lb = to_7_f(xl[2], trial_p0);
                trial_la = to_7_f(xl[3], trial_p0);
                trial_p1 = i32::from(xh[3] >= 129.0);
                trial_hr = to_7_f(xh[0], trial_p1);
                trial_hg = to_7_f(xh[1], trial_p1);
                trial_hb = to_7_f(xh[2], trial_p1);
                trial_ha = to_7_f(xh[3], trial_p1);
            }
            let mut trial_weights = [0u8; 16];
            let mode6_ls_actual_sse = eval_weights_mode6_rgba_sse(
                pixels,
                &mut trial_weights,
                trial_lr,
                trial_lg,
                trial_lb,
                trial_la,
                trial_p0,
                trial_hr,
                trial_hg,
                trial_hb,
                trial_ha,
                trial_p1,
            );
            if mode6_ls_actual_sse < mode6_actual_sse {
                mode6_actual_sse = mode6_ls_actual_sse;
                cur_weights = trial_weights;
                p0 = trial_p0;
                p1 = trial_p1;
                lr = trial_lr;
                lg = trial_lg;
                lb = trial_lb;
                la = trial_la;
                hr = trial_hr;
                hg = trial_hg;
                hb = trial_hb;
                ha = trial_ha;
            }
        }
    }

    Mode6RgbaCandidate {
        p0,
        p1,
        lr,
        lg,
        lb,
        la,
        hr,
        hg,
        hb,
        ha,
        weights: cur_weights,
        sse: mode6_actual_sse,
    }
}

/// `fast_pack_bc7_rgba_partial_analytical`: like the analytical RGBA
/// pipeline but the mode-6/7/4-5 candidates compete on actual SSE
/// (tie-break mode 4/5 > mode 7 > mode 6). Returns the winning SSE.
fn fast_pack_bc7_rgba_partial_analytical(
    block: &mut [u8; 16],
    pixels: &[Rgba; 16],
    fl: u32,
) -> u32 {
    let st = match rgba_pipeline_head(block, pixels, fl, true) {
        RgbaHead::Done(sse) => return sse,
        RgbaHead::Stats(st) => st,
    };

    let mut cov4 = [0f32; 10];
    for i in 0..10 {
        cov4[i] = st.icov4[i] as f32;
    }
    let (x1, y1, z1, w1) = rgba_power_iterate(&cov4, st.block_max_var4);

    let m6 = mode6_rgba_candidate(pixels, x1, y1, z1, w1, &st, fl);

    let mut mode7_actual_sse = u32::MAX;
    let mut mode7_candidate_block = [0u8; 16];
    let mut mode45_actual_sse = u32::MAX;
    let mut mode45_candidate_block = [0u8; 16];

    if m6.sse != 0 {
        let mut mode6_ortho_ratio = 0f32;
        let mode6_slam_to_line_sse_est =
            estimate_slam_to_line_sse_4d(&cov4, x1, y1, z1, w1, Some(&mut mode6_ortho_ratio));
        if fl & flags::USE_2_SUBSETS_RGBA != 0
            && st.block_max_var4 >= MIN_BLOCK_MAX_VAR_23SUBSETS
            && mode6_ortho_ratio > ORTHO_RATIO_23SUBSET_RATIO_THRESH
        {
            let high_ortho_energy_flag = mode6_slam_to_line_sse_est >= HIGH_ORTHO_ENERGY_THRESH;
            if high_ortho_energy_flag {
                pack_mode7_rgba(
                    &mut mode7_candidate_block,
                    pixels,
                    x1,
                    y1,
                    z1,
                    w1,
                    st.mean_r,
                    st.mean_g,
                    st.mean_b,
                    st.mean_a,
                    1e9,
                    fl,
                    None,
                    Some(&mut mode7_actual_sse),
                );
            }
        }
        if st.desired_dp_chan >= 0 {
            pack_mode4_or_5(
                &mut mode45_candidate_block,
                pixels,
                st.desired_dp_chan as usize,
                1e9,
                fl,
                None,
                Some(&mut mode45_actual_sse),
            );
        }
    }

    let best_actual_sse = m6.sse.min(mode45_actual_sse).min(mode7_actual_sse);

    if mode45_actual_sse != u32::MAX && best_actual_sse == mode45_actual_sse {
        *block = mode45_candidate_block;
    } else if mode7_actual_sse != u32::MAX && best_actual_sse == mode7_actual_sse {
        *block = mode7_candidate_block;
    } else {
        encode_mode6_rgba_block(
            block,
            m6.lr as u32,
            m6.lg as u32,
            m6.lb as u32,
            m6.la as u32,
            m6.p0 as u32,
            m6.hr as u32,
            m6.hg as u32,
            m6.hb as u32,
            m6.ha as u32,
            m6.p1 as u32,
            &m6.weights,
        );
    }
    // Sanity check: the incrementally tracked SSE must equal a fresh decode of
    // the winning block.
    debug_assert_eq!(calc_sse(block, pixels), best_actual_sse);
    best_actual_sse
}

/// `fast_pack_bc7_auto_rgb`: the known-opaque entry (the XUASTC generic
/// path uses it when the stream header declares no alpha). No alpha scan;
/// straight to the RGB pipelines.
pub fn fast_pack_bc7_auto_rgb(block: &mut [u8; 16], pixels: &[Rgba; 16], fl: u32) -> u32 {
    if fl & flags::PARTIALLY_ANALYTICAL_RGB != 0 {
        return fast_pack_bc7_rgb_partial_analytical(block, pixels, fl);
    }
    fast_pack_bc7_rgb_analytical(block, pixels, fl);
    0
}

/// `fast_pack_bc7_auto_rgba`: the entry the raw-ASTC decode paths use. A
/// per-row alpha scan routes to the RGBA pipelines when any texel is
/// translucent, else the RGB pipelines; the flags choose analytical vs
/// partially-analytical.
pub fn fast_pack_bc7_auto_rgba(block: &mut [u8; 16], pixels: &[Rgba; 16], fl: u32) -> u32 {
    for i in (0..16).step_by(4) {
        if pixels[i][3] < 255
            || pixels[i + 1][3] < 255
            || pixels[i + 2][3] < 255
            || pixels[i + 3][3] < 255
        {
            if fl & flags::PARTIALLY_ANALYTICAL_RGBA != 0 {
                return fast_pack_bc7_rgba_partial_analytical(block, pixels, fl);
            }
            fast_pack_bc7_rgba_analytical(block, pixels, fl);
            return 0;
        }
    }
    if fl & flags::PARTIALLY_ANALYTICAL_RGB != 0 {
        return fast_pack_bc7_rgb_partial_analytical(block, pixels, fl);
    }
    fast_pack_bc7_rgb_analytical(block, pixels, fl);
    0
}

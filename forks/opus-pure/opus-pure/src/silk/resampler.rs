use crate::silk::macros::*;

const RESAMPLER_DOWN_ORDER_FIR2: usize = 36;

pub const SILK_RESAMPLER_DOWN2_0: i32 = 9872;
pub const SILK_RESAMPLER_DOWN2_1: i32 = 39809 - 65536;

pub const SILK_RESAMPLER_2_3_COEFS_LQ: [i16; 6] = [-2797, -6507, 4697, 10739, 1567, 8276];

const SILK_RESAMPLER_UP2_HQ_0: [i16; 3] = [1746, 14986, (39083 - 65536) as i16];
const SILK_RESAMPLER_UP2_HQ_1: [i16; 3] = [6854, 25769, (55542 - 65536) as i16];

const SILK_RESAMPLER_FRAC_FIR_12: [[i16; 4]; 12] = [
    [189, -600, 617, 30567],
    [117, -159, -1070, 29704],
    [52, 221, -2392, 28276],
    [-4, 529, -3350, 26341],
    [-48, 758, -3956, 23973],
    [-80, 905, -4235, 21254],
    [-99, 972, -4222, 18278],
    [-107, 967, -3957, 15143],
    [-103, 896, -3487, 11950],
    [-91, 773, -2865, 8798],
    [-71, 611, -2143, 5784],
    [-46, 425, -1375, 2996],
];

const RESAMPLER_MAX_BATCH_SIZE_MS: i32 = 10;
const RESAMPLER_ORDER_FIR_12: usize = 8;

/// Contiguous 8-tap coefficients per fractional phase, `[phase][tap]`, laid out
/// exactly as the scalar interpolator consumes them:
/// `[t[ti][0..3], t[11-ti][3], t[11-ti][2], t[11-ti][1], t[11-ti][0]]`.
/// This makes the per-sample interpolation a single 8-wide dot product.
const fn build_fir12_8() -> [[i16; 8]; 12] {
    let mut o = [[0i16; 8]; 12];
    let mut ti = 0;
    while ti < 12 {
        o[ti][0] = SILK_RESAMPLER_FRAC_FIR_12[ti][0];
        o[ti][1] = SILK_RESAMPLER_FRAC_FIR_12[ti][1];
        o[ti][2] = SILK_RESAMPLER_FRAC_FIR_12[ti][2];
        o[ti][3] = SILK_RESAMPLER_FRAC_FIR_12[ti][3];
        o[ti][4] = SILK_RESAMPLER_FRAC_FIR_12[11 - ti][3];
        o[ti][5] = SILK_RESAMPLER_FRAC_FIR_12[11 - ti][2];
        o[ti][6] = SILK_RESAMPLER_FRAC_FIR_12[11 - ti][1];
        o[ti][7] = SILK_RESAMPLER_FRAC_FIR_12[11 - ti][0];
        ti += 1;
    }
    o
}
const FIR_COEFS_12_8: [[i16; 8]; 12] = build_fir12_8();

/// Bit-exact 8-tap fractional-FIR dot product: `Σ buf[bi+j] * coef[ti][j]` in
/// wrapping i32. Matches the scalar `silk_smlabb` chain exactly (i32 addition is
/// associative under wrapping, so `madd`'s pairwise sum is identical). SSE2
/// `_mm_madd_epi16` on x86, scalar elsewhere.
#[inline]
fn resampler_fir12_8(buf: &[i16], bi: usize, ti: usize) -> i32 {
    let c = &FIR_COEFS_12_8[ti];
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("sse2") {
            return unsafe { resampler_fir12_8_sse2(&buf[bi..bi + 8], c) };
        }
    }
    let mut r = 0i32;
    for j in 0..8 {
        r = r.wrapping_add((buf[bi + j] as i32) * (c[j] as i32));
    }
    r
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn resampler_fir12_8_sse2(buf8: &[i16], c: &[i16; 8]) -> i32 {
    use std::arch::x86_64::*;
    let b = _mm_loadu_si128(buf8.as_ptr() as *const __m128i);
    let cc = _mm_loadu_si128(c.as_ptr() as *const __m128i);
    let m = _mm_madd_epi16(b, cc); // 4x i32: pairwise products summed
    // Horizontal sum the 4 lanes (i32 add is associative under wrapping).
    let t = _mm_add_epi32(m, _mm_shuffle_epi32(m, 0b01_00_11_10)); // [2,3,0,1]
    let t = _mm_add_epi32(t, _mm_shuffle_epi32(t, 0b00_00_00_01)); // + lane 1
    _mm_cvtsi128_si32(t)
}

const DELAY_MATRIX_DEC: [[i8; 6]; 3] =
    [[4, 0, 2, 0, 0, 0], [0, 9, 4, 7, 4, 4], [0, 3, 12, 7, 7, 7]];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResamplerMode {
    Copy,
    Up2HQ,
    IirFir,
}

#[derive(Clone)]
pub struct SilkResampler {
    s_iir: [i32; 6],

    s_fir: [i16; RESAMPLER_ORDER_FIR_12],

    delay_buf: [i16; 48],

    input_delay: i32,

    fs_in_khz: i32,

    fs_out_khz: i32,

    batch_size: i32,

    inv_ratio_q16: i32,

    mode: ResamplerMode,
}

impl Default for SilkResampler {
    fn default() -> Self {
        Self {
            s_iir: [0; 6],
            s_fir: [0; RESAMPLER_ORDER_FIR_12],
            delay_buf: [0; 48],
            input_delay: 0,
            fs_in_khz: 0,
            fs_out_khz: 0,
            batch_size: 0,
            inv_ratio_q16: 0,
            mode: ResamplerMode::Copy,
        }
    }
}

fn rate_id(rate_hz: i32) -> usize {
    match rate_hz {
        8000 => 0,
        12000 => 1,
        16000 => 2,
        24000 => 3,
        48000 => 4,
        _ => 5,
    }
}

impl SilkResampler {
    pub fn init(&mut self, fs_hz_in: i32, fs_hz_out: i32) -> i32 {
        *self = Self::default();

        let in_id = rate_id(fs_hz_in);
        let out_id = rate_id(fs_hz_out);

        if in_id > 2 || out_id > 5 {
            return -1;
        }

        self.input_delay = DELAY_MATRIX_DEC[in_id][out_id] as i32;
        self.fs_in_khz = fs_hz_in / 1000;
        self.fs_out_khz = fs_hz_out / 1000;
        self.batch_size = self.fs_in_khz * RESAMPLER_MAX_BATCH_SIZE_MS;

        if fs_hz_out == fs_hz_in {
            self.mode = ResamplerMode::Copy;
        } else if fs_hz_out == fs_hz_in * 2 {
            self.mode = ResamplerMode::Up2HQ;
        } else {
            self.mode = ResamplerMode::IirFir;
        }

        let up2x = if self.mode == ResamplerMode::IirFir {
            1
        } else {
            0
        };
        self.inv_ratio_q16 = ((((fs_hz_in as i64) << (14 + up2x)) / fs_hz_out as i64) << 2) as i32;

        while silk_smulww(self.inv_ratio_q16, fs_hz_out) < (fs_hz_in << up2x) {
            self.inv_ratio_q16 += 1;
        }

        0
    }

    /// Output samples `process` will write for `in_len` input samples.
    ///
    /// Returns `None` when the resampler has not been initialized, which would
    /// otherwise give a zero input rate and a nonsense length.
    pub fn output_len(&self, in_len: i32) -> Option<usize> {
        if self.fs_in_khz <= 0 || self.fs_out_khz <= 0 || in_len < 0 {
            return None;
        }
        let in_len = in_len as i64;
        Some(match self.mode {
            ResamplerMode::Copy => in_len as usize,
            ResamplerMode::Up2HQ => (in_len * 2) as usize,
            ResamplerMode::IirFir => {
                (in_len * self.fs_out_khz as i64 / self.fs_in_khz as i64) as usize
            }
        })
    }

    /// Resample `in_len` samples of `input` into `out`, returning 0 on success
    /// and -1 if the call is inconsistent.
    ///
    /// The length checks are load-bearing, not defensive padding: the decoder
    /// derives `out`'s length from the *packet's* declared bandwidth, so a
    /// malformed packet can disagree with how this resampler was configured.
    /// Before these checks that wrote past the end of the output buffer.
    pub fn process(&mut self, out: &mut [i16], input: &[i16], in_len: i32) -> i32 {
        if in_len < self.fs_in_khz {
            return -1;
        }
        if input.len() < in_len as usize {
            return -1;
        }
        match self.output_len(in_len) {
            Some(need) if out.len() >= need => {}
            _ => return -1,
        }
        // The `Copy` and `Up2HQ` paths index from `fs_out_khz`, which must be
        // inside `out` for a well-formed configuration.
        if (self.fs_out_khz as usize) > out.len() {
            return -1;
        }

        let n_samples = self.fs_in_khz - self.input_delay;

        self.delay_buf[self.input_delay as usize..self.fs_in_khz as usize]
            .copy_from_slice(&input[..n_samples as usize]);

        match self.mode {
            ResamplerMode::Copy => {
                out[..self.fs_in_khz as usize]
                    .copy_from_slice(&self.delay_buf[..self.fs_in_khz as usize]);
                let remaining = (in_len - self.fs_in_khz) as usize;
                out[self.fs_out_khz as usize..self.fs_out_khz as usize + remaining]
                    .copy_from_slice(&input[n_samples as usize..n_samples as usize + remaining]);
            }
            ResamplerMode::Up2HQ => {
                silk_resampler_private_up2_hq(
                    &mut self.s_iir,
                    &mut out[..],
                    &self.delay_buf[..self.fs_in_khz as usize],
                    self.fs_in_khz,
                );
                silk_resampler_private_up2_hq(
                    &mut self.s_iir,
                    &mut out[self.fs_out_khz as usize..],
                    &input[n_samples as usize..],
                    in_len - self.fs_in_khz,
                );
            }
            ResamplerMode::IirFir => {
                self.iir_fir_resample(
                    out,
                    &self.delay_buf.clone(),
                    self.fs_in_khz,
                    &input[n_samples as usize..],
                    in_len - self.fs_in_khz,
                );
            }
        }

        let delay = self.input_delay as usize;
        if delay > 0 {
            let src_start = (in_len as usize).saturating_sub(delay);
            self.delay_buf[..delay].copy_from_slice(&input[src_start..src_start + delay]);
        }

        0
    }

    fn iir_fir_resample(
        &mut self,
        out: &mut [i16],
        first_block: &[i16],
        first_len: i32,
        rest: &[i16],
        rest_len: i32,
    ) {
        // libopus calls silk_resampler_private_IIR_FIR once per block; so do we.
        let out_idx = self.iir_fir_block(out, 0, first_block, first_len);
        self.iir_fir_block(out, out_idx, rest, rest_len);
    }

    /// Resample one contiguous input block, appending at `out_idx` and
    /// returning the next output position.
    fn iir_fir_block(
        &mut self,
        out: &mut [i16],
        mut out_idx: usize,
        input: &[i16],
        len: i32,
    ) -> usize {
        const MAX_BATCH_IN: usize = 480;
        const MAX_BUF: usize = 2 * MAX_BATCH_IN + RESAMPLER_ORDER_FIR_12;

        let mut in_idx = 0usize;
        let mut remaining = len;

        while remaining > 0 {
            let n_samples_in = remaining.min(self.batch_size) as usize;

            let buf_len = 2 * n_samples_in + RESAMPLER_ORDER_FIR_12;
            let mut buf_arr = [0i16; MAX_BUF];
            let buf = &mut buf_arr[..buf_len];

            buf[..RESAMPLER_ORDER_FIR_12].copy_from_slice(&self.s_fir);

            silk_resampler_private_up2_hq(
                &mut self.s_iir,
                &mut buf[RESAMPLER_ORDER_FIR_12..],
                &input[in_idx..in_idx + n_samples_in],
                n_samples_in as i32,
            );

            let max_index_q16 = (n_samples_in as i32) << 17;
            let index_increment_q16 = self.inv_ratio_q16;

            let mut index_q16 = 0i32;
            while index_q16 < max_index_q16 {
                let table_index = silk_smulwb(index_q16 & 0xFFFF, 12) as usize;
                let buf_idx = (index_q16 >> 16) as usize;

                let res_q15 = resampler_fir12_8(buf, buf_idx, table_index);

                if out_idx < out.len() {
                    out[out_idx] = silk_sat16(silk_rshift_round(res_q15, 15)) as i16;
                    out_idx += 1;
                }
                index_q16 += index_increment_q16;
            }

            in_idx += n_samples_in;
            remaining -= n_samples_in as i32;

            self.s_fir
                .copy_from_slice(&buf[2 * n_samples_in..2 * n_samples_in + RESAMPLER_ORDER_FIR_12]);
        }

        out_idx
    }
}

pub fn silk_resampler_private_up2_hq(s: &mut [i32], out: &mut [i16], input: &[i16], len: i32) {
    for k in 0..len as usize {
        let in32 = (input[k] as i32) << 10;

        let y = in32 - s[0];
        let x = silk_smulwb(y, SILK_RESAMPLER_UP2_HQ_0[0] as i32);
        let out32_1 = s[0] + x;
        s[0] = in32 + x;

        let y = out32_1 - s[1];
        let x = silk_smulwb(y, SILK_RESAMPLER_UP2_HQ_0[1] as i32);
        let out32_2 = s[1] + x;
        s[1] = out32_1 + x;

        let y = out32_2 - s[2];
        let x = silk_smlawb(y, y, SILK_RESAMPLER_UP2_HQ_0[2] as i32);
        let out32_1 = s[2] + x;
        s[2] = out32_2 + x;

        out[2 * k] = silk_sat16(silk_rshift_round(out32_1, 10)) as i16;

        let y = in32 - s[3];
        let x = silk_smulwb(y, SILK_RESAMPLER_UP2_HQ_1[0] as i32);
        let out32_1 = s[3] + x;
        s[3] = in32 + x;

        let y = out32_1 - s[4];
        let x = silk_smulwb(y, SILK_RESAMPLER_UP2_HQ_1[1] as i32);
        let out32_2 = s[4] + x;
        s[4] = out32_1 + x;

        let y = out32_2 - s[5];
        let x = silk_smlawb(y, y, SILK_RESAMPLER_UP2_HQ_1[2] as i32);
        let out32_1 = s[5] + x;
        s[5] = out32_2 + x;

        out[2 * k + 1] = silk_sat16(silk_rshift_round(out32_1, 10)) as i16;
    }
}

pub fn silk_resampler_down2(s: &mut [i32], out: &mut [i16], input: &[i16], in_len: i32) {
    let len2 = in_len >> 1;
    let mut in32: i32;
    let mut out32: i32;
    let mut y: i32;
    let mut x: i32;

    for k in 0..len2 as usize {
        in32 = (input[2 * k] as i32) << 10;

        y = in32.wrapping_sub(s[0]);
        x = silk_smlawb(y, y, SILK_RESAMPLER_DOWN2_1);
        out32 = s[0].wrapping_add(x);
        s[0] = in32.wrapping_add(x);

        in32 = (input[2 * k + 1] as i32) << 10;

        y = in32.wrapping_sub(s[1]);
        x = silk_smulwb(y, SILK_RESAMPLER_DOWN2_0);
        out32 = out32.wrapping_add(s[1]);
        out32 = out32.wrapping_add(x);
        s[1] = in32.wrapping_add(x);

        out[k] = silk_sat16(silk_rshift_round(out32, 11)) as i16;
    }
}

pub fn silk_resampler_private_ar2(
    s: &mut [i32],
    out_q8: &mut [i32],
    input: &[i16],
    a_q14: &[i16],
    len: i32,
) {
    let mut out32: i32;
    for k in 0..len as usize {
        out32 = s[0].wrapping_add((input[k] as i32) << 8);
        s[0] = s[1].wrapping_add(silk_smlawb(out32, out32, a_q14[0] as i32));
        s[1] = silk_smlawb(0, out32, a_q14[1] as i32);
        out_q8[k] = out32;
    }
}

const RESAMPLER_MAX_BATCH_SIZE_IN: i32 = 480;
const ORDER_FIR: usize = 4;

pub fn silk_resampler_down2_3(s: &mut [i32], out: &mut [i16], input: &[i16], in_len: i32) {
    let mut n_samples_in: i32;
    let mut counter: i32;
    let mut res_q6: i32;
    let mut buf = [0i32; (RESAMPLER_MAX_BATCH_SIZE_IN as usize) + ORDER_FIR];
    let mut in_idx = 0;
    let mut out_idx = 0;
    let mut remaining_len = in_len;

    buf[0..ORDER_FIR].copy_from_slice(&s[0..ORDER_FIR]);

    while remaining_len > 0 {
        n_samples_in = remaining_len.min(RESAMPLER_MAX_BATCH_SIZE_IN);

        silk_resampler_private_ar2(
            &mut s[ORDER_FIR..ORDER_FIR + 2],
            &mut buf[ORDER_FIR..ORDER_FIR + n_samples_in as usize],
            &input[in_idx..in_idx + n_samples_in as usize],
            &SILK_RESAMPLER_2_3_COEFS_LQ,
            n_samples_in,
        );

        let mut buf_ptr = 0;
        counter = n_samples_in;
        while counter > 2 {
            res_q6 = silk_smulwb(buf[buf_ptr], SILK_RESAMPLER_2_3_COEFS_LQ[2] as i32);
            res_q6 = silk_smlawb(
                res_q6,
                buf[buf_ptr + 1],
                SILK_RESAMPLER_2_3_COEFS_LQ[3] as i32,
            );
            res_q6 = silk_smlawb(
                res_q6,
                buf[buf_ptr + 2],
                SILK_RESAMPLER_2_3_COEFS_LQ[5] as i32,
            );
            res_q6 = silk_smlawb(
                res_q6,
                buf[buf_ptr + 3],
                SILK_RESAMPLER_2_3_COEFS_LQ[4] as i32,
            );

            out[out_idx] = silk_sat16(silk_rshift_round(res_q6, 9)) as i16;
            out_idx += 1;

            res_q6 = silk_smulwb(buf[buf_ptr + 1], SILK_RESAMPLER_2_3_COEFS_LQ[4] as i32);
            res_q6 = silk_smlawb(
                res_q6,
                buf[buf_ptr + 2],
                SILK_RESAMPLER_2_3_COEFS_LQ[5] as i32,
            );
            res_q6 = silk_smlawb(
                res_q6,
                buf[buf_ptr + 3],
                SILK_RESAMPLER_2_3_COEFS_LQ[3] as i32,
            );
            res_q6 = silk_smlawb(
                res_q6,
                buf[buf_ptr + 4],
                SILK_RESAMPLER_2_3_COEFS_LQ[2] as i32,
            );

            out[out_idx] = silk_sat16(silk_rshift_round(res_q6, 8)) as i16;
            out_idx += 1;

            buf_ptr += 3;
            counter -= 3;
        }

        in_idx += n_samples_in as usize;
        remaining_len -= n_samples_in;

        if remaining_len > 0 {
            for i in 0..ORDER_FIR {
                buf[i] = buf[n_samples_in as usize + i];
            }
        } else {
            s[0..ORDER_FIR]
                .copy_from_slice(&buf[n_samples_in as usize..n_samples_in as usize + ORDER_FIR]);
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Encoder-side downsampling resampler (silk_resampler_private_down_FIR):
// 2nd-order AR filter followed by polyphase FIR interpolation. This is what
// silk_Encode uses internally for 48->16 (1:3, FIR2) and 24->16 (2:3, FIR0);
// the ad-hoc down_1_3/down2_3 decimators it replaces here have a different
// passband, which cost SILK quality at every >16 kHz API rate.

const RESAMPLER_DOWN_ORDER_FIR0: usize = 18;
const RESAMPLER_DOWN_ORDER_FIR1: usize = 24;

// Downsampling filters from libopus `silk/resampler_rom.c`. The first two
// entries of each table are the AR2 IIR coefficients; the rest are the FIR
// taps. FIR0 tables are polyphase (one bank per interpolation fraction);
// FIR1 and FIR2 tables are symmetric, holding half the taps.
static RESAMPLER_3_4_COEFS: [i16; 2 + 3 * (RESAMPLER_DOWN_ORDER_FIR0 / 2)] = [
    -20694, -13867, //
    -49, 64, 17, -157, 353, -496, 163, 11047, 22205, //
    -39, 6, 91, -170, 186, 23, -896, 6336, 19928, //
    -19, -36, 102, -89, -24, 328, -951, 2568, 15909,
];
static RESAMPLER_2_3_COEFS: [i16; 2 + 2 * (RESAMPLER_DOWN_ORDER_FIR0 / 2)] = [
    -14457, -14019, //
    64, 128, -122, 36, 310, -768, 584, 9267, 17733, //
    12, 128, 18, -142, 288, -117, -865, 4123, 14459,
];
static RESAMPLER_1_2_COEFS: [i16; 2 + RESAMPLER_DOWN_ORDER_FIR1 / 2] = [
    616, -14323, //
    -10, 39, 58, -46, -84, 120, 184, -315, -541, 1284, 5380, 9024,
];
static RESAMPLER_1_3_COEFS: [i16; 2 + RESAMPLER_DOWN_ORDER_FIR2 / 2] = [
    16102, -15162, //
    -13, 0, 20, 26, 5, -31, -43, -4, 65, 90, 7, -157, -248, -44, 593, 1583, 2612, 3271,
];
static RESAMPLER_1_4_COEFS: [i16; 2 + RESAMPLER_DOWN_ORDER_FIR2 / 2] = [
    22500, -15099, //
    3, -14, -20, -15, 2, 25, 37, 25, -16, -71, -107, -79, 50, 292, 623, 982, 1288, 1464,
];
static RESAMPLER_1_6_COEFS: [i16; 2 + RESAMPLER_DOWN_ORDER_FIR2 / 2] = [
    27540, -15257, //
    17, 12, 8, 1, -10, -22, -30, -32, -22, 3, 44, 100, 168, 243, 317, 381, 429, 455,
];

/// silk_resampler_private_AR2, exact port. (The older silk_resampler_private_ar2
/// above has a different state recurrence and is kept for its down2_3 caller.)
fn ar2_q14_exact(s: &mut [i32; 2], out_q8: &mut [i32], input: &[i16], a_q14: &[i16]) {
    for (k, &x) in input.iter().enumerate() {
        let mut out32 = s[0].wrapping_add((x as i32) << 8);
        out_q8[k] = out32;
        out32 = out32.wrapping_shl(2);
        s[0] = silk_smlawb(s[1], out32, a_q14[0] as i32);
        s[1] = silk_smulwb(out32, a_q14[1] as i32);
    }
}

#[inline]
fn sat16_round_q6(a: i32) -> i16 {
    let r = ((a >> 5) + 1) >> 1;
    r.clamp(-32768, 32767) as i16
}

/// `delay_matrix_enc[rateID(in)][rateID(out)]` (`resampler.c:53`), the samples
/// of input the encoder-side resampler holds back.
///
/// These exist to equalise the codec's *total* delay across every rate pair, so
/// that a stream sounds the same distance behind its input whatever internal
/// rate SILK picked. That makes them part of the resampler, not an optional
/// extra: the equal-rate pairs carry non-zero entries too, which is why a
/// pass-through still has to run one.
fn enc_input_delay(fs_hz_in: i32, fs_hz_out: i32) -> Option<usize> {
    const DELAY_MATRIX_ENC: [[i8; 3]; 5] =
        [[6, 0, 3], [0, 7, 3], [0, 1, 10], [0, 2, 6], [18, 10, 12]];
    let rid_in = match fs_hz_in {
        8000 => 0,
        12000 => 1,
        16000 => 2,
        24000 => 3,
        48000 => 4,
        _ => return None,
    };
    let rid_out = match fs_hz_out {
        8000 => 0,
        12000 => 1,
        16000 => 2,
        _ => return None,
    };
    Some(DELAY_MATRIX_ENC[rid_in][rid_out] as usize)
}

/// The encoder-side resampler: API rate in, SILK's internal rate out.
///
/// libopus keeps one type and selects a kernel (`resampler_function`); this
/// splits the ratio conversion from the pass-through because they share no
/// state but the delay buffer. What they must not split is the delay itself.
/// A pass-through is a resampler that happens not to change the rate, not a
/// copy the caller may skip, and this crate learned that the hard way: the
/// pass-through's delay lived downstream in `silk_encode` instead, so every
/// rate pair that *did* resample got it twice and came out 10 samples of
/// SILK-internal rate late — 30 at 48 kHz, 15 at 24 kHz. Keep the delay here,
/// where there is exactly one of it.
pub enum SilkEncoderResampler {
    /// The API rate is above SILK's internal rate, so the samples go through
    /// the AR2 + FIR chain.
    Ratio(SilkDownFirResampler),
    /// The API rate already is SILK's internal rate
    /// (`USE_silk_resampler_copy`).
    Pass(SilkPassthroughResampler),
}

impl SilkEncoderResampler {
    pub fn new(fs_hz_in: i32, fs_hz_out: i32) -> Option<Self> {
        if fs_hz_in == fs_hz_out {
            SilkPassthroughResampler::new(fs_hz_in).map(SilkEncoderResampler::Pass)
        } else {
            SilkDownFirResampler::new(fs_hz_in, fs_hz_out).map(SilkEncoderResampler::Ratio)
        }
    }

    /// `input.len()` must be a whole number of milliseconds, at least one.
    pub fn process(&mut self, out: &mut [i16], input: &[i16]) {
        match self {
            SilkEncoderResampler::Ratio(r) => r.process(out, input),
            SilkEncoderResampler::Pass(r) => r.process(out, input),
        }
    }
}

/// `USE_silk_resampler_copy`: sample values pass straight through, but the
/// delay buffer still runs.
pub struct SilkPassthroughResampler {
    delay_buf: [i16; 48],
    input_delay: usize,
    fs_khz: usize,
}

impl SilkPassthroughResampler {
    fn new(fs_hz: i32) -> Option<Self> {
        Some(SilkPassthroughResampler {
            delay_buf: [0; 48],
            input_delay: enc_input_delay(fs_hz, fs_hz)?,
            fs_khz: (fs_hz / 1000) as usize,
        })
    }

    /// The `default` branch of `silk_resampler()`: one millisecond out of the
    /// delay buffer, the rest straight from the input, and the last
    /// `input_delay` samples held back for the next call.
    fn process(&mut self, out: &mut [i16], input: &[i16]) {
        let in_len = input.len();
        debug_assert!(in_len >= self.fs_khz && in_len >= self.input_delay);
        let n = self.fs_khz - self.input_delay;
        self.delay_buf[self.input_delay..self.fs_khz].copy_from_slice(&input[..n]);
        out[..self.fs_khz].copy_from_slice(&self.delay_buf[..self.fs_khz]);
        out[self.fs_khz..in_len].copy_from_slice(&input[n..in_len - self.input_delay]);
        self.delay_buf[..self.input_delay].copy_from_slice(&input[in_len - self.input_delay..]);
    }
}

pub struct SilkDownFirResampler {
    s_iir: [i32; 2],
    s_fir: [i32; RESAMPLER_DOWN_ORDER_FIR2],
    delay_buf: [i16; 48],
    input_delay: usize,
    fir_order: usize,
    fir_fracs: i32,
    batch_size: usize,
    inv_ratio_q16: i32,
    fs_in_khz: usize,
    fs_out_khz: usize,
    coefs: &'static [i16],
}

impl SilkDownFirResampler {
    /// Every downsampling ratio SILK defines (`silk_resampler_init`): 3:4, 2:3,
    /// 1:2, 1:3, 1:4 and 1:6. Between them these cover each API rate down to
    /// each SILK-internal rate, so the encoder can honour any bandwidth the
    /// input can carry.
    pub fn new(fs_hz_in: i32, fs_hz_out: i32) -> Option<Self> {
        let (coefs, fir_order, fir_fracs): (&'static [i16], usize, i32) =
            if fs_hz_out * 4 == fs_hz_in * 3 {
                (&RESAMPLER_3_4_COEFS, RESAMPLER_DOWN_ORDER_FIR0, 3)
            } else if fs_hz_out * 3 == fs_hz_in * 2 {
                (&RESAMPLER_2_3_COEFS, RESAMPLER_DOWN_ORDER_FIR0, 2)
            } else if fs_hz_out * 2 == fs_hz_in {
                (&RESAMPLER_1_2_COEFS, RESAMPLER_DOWN_ORDER_FIR1, 1)
            } else if fs_hz_out * 3 == fs_hz_in {
                (&RESAMPLER_1_3_COEFS, RESAMPLER_DOWN_ORDER_FIR2, 1)
            } else if fs_hz_out * 4 == fs_hz_in {
                (&RESAMPLER_1_4_COEFS, RESAMPLER_DOWN_ORDER_FIR2, 1)
            } else if fs_hz_out * 6 == fs_hz_in {
                (&RESAMPLER_1_6_COEFS, RESAMPLER_DOWN_ORDER_FIR2, 1)
            } else {
                return None;
            };
        let input_delay = enc_input_delay(fs_hz_in, fs_hz_out)?;
        let mut inv_ratio_q16 = ((((fs_hz_in as i64) << 14) / fs_hz_out as i64) << 2) as i32;
        while (((inv_ratio_q16 as i64) * fs_hz_out as i64) >> 16) < fs_hz_in as i64 {
            inv_ratio_q16 += 1;
        }
        Some(SilkDownFirResampler {
            s_iir: [0; 2],
            s_fir: [0; RESAMPLER_DOWN_ORDER_FIR2],
            delay_buf: [0; 48],
            input_delay,
            fir_order,
            fir_fracs,
            batch_size: (fs_hz_in / 1000) as usize * 10, // RESAMPLER_MAX_BATCH_SIZE_MS
            inv_ratio_q16,
            fs_in_khz: (fs_hz_in / 1000) as usize,
            fs_out_khz: (fs_hz_out / 1000) as usize,
            coefs,
        })
    }

    fn interpol(&self, buf: &[i32], out: &mut [i16], max_index_q16: i32) -> usize {
        let inc = self.inv_ratio_q16;
        let fir_coefs = &self.coefs[2..];
        let mut n_out = 0usize;
        let mut index_q16 = 0i32;
        if self.fir_order == RESAMPLER_DOWN_ORDER_FIR0 {
            while index_q16 < max_index_q16 {
                let b = (index_q16 >> 16) as usize;
                let interpol_ind =
                    (((index_q16 & 0xffff) as i64 * self.fir_fracs as i64) >> 16) as usize;
                let p = &fir_coefs[RESAMPLER_DOWN_ORDER_FIR0 / 2 * interpol_ind..];
                let mut res = 0i32;
                for j in 0..9 {
                    res = silk_smlawb(res, buf[b + j], p[j] as i32);
                }
                let p2 = &fir_coefs[RESAMPLER_DOWN_ORDER_FIR0 / 2
                    * (self.fir_fracs as usize - 1 - interpol_ind)..];
                for j in 0..9 {
                    res = silk_smlawb(res, buf[b + 17 - j], p2[j] as i32);
                }
                out[n_out] = sat16_round_q6(res);
                n_out += 1;
                index_q16 += inc;
            }
        } else {
            // Symmetric FIR: order 24 (1:2) or 36 (1:3, 1:4, 1:6). Half the taps
            // are stored; each is applied to a mirrored pair.
            let order = self.fir_order;
            let half = order / 2;
            while index_q16 < max_index_q16 {
                let b = (index_q16 >> 16) as usize;
                let mut res = 0i32;
                for j in 0..half {
                    let sum = buf[b + j].wrapping_add(buf[b + order - 1 - j]);
                    res = silk_smlawb(res, sum, fir_coefs[j] as i32);
                }
                out[n_out] = sat16_round_q6(res);
                n_out += 1;
                index_q16 += inc;
            }
        }
        n_out
    }

    fn down_fir(&mut self, input: &[i16], out: &mut [i16]) -> usize {
        let mut buf = [0i32; 480 + RESAMPLER_DOWN_ORDER_FIR2];
        buf[..self.fir_order].copy_from_slice(&self.s_fir[..self.fir_order]);
        let mut in_pos = 0usize;
        let mut out_pos = 0usize;
        let mut n_in;
        loop {
            n_in = (input.len() - in_pos).min(self.batch_size);
            {
                let fir_order = self.fir_order;
                let mut s = self.s_iir;
                ar2_q14_exact(
                    &mut s,
                    &mut buf[fir_order..fir_order + n_in],
                    &input[in_pos..in_pos + n_in],
                    &self.coefs[..2],
                );
                self.s_iir = s;
            }
            let max_index_q16 = (n_in as i32) << 16;
            out_pos += self.interpol(&buf, &mut out[out_pos..], max_index_q16);
            in_pos += n_in;
            if input.len() - in_pos > 1 {
                buf.copy_within(n_in..n_in + self.fir_order, 0);
            } else {
                break;
            }
        }
        self.s_fir[..self.fir_order].copy_from_slice(&buf[n_in..n_in + self.fir_order]);
        out_pos
    }

    /// silk_resampler() top level: 1 ms through the delay buffer, the rest
    /// direct, and the last input_delay samples buffered for the next call.
    /// input.len() must be a whole number of ms (10/20 ms frames are).
    pub fn process(&mut self, out: &mut [i16], input: &[i16]) {
        let in_len = input.len();
        let n = self.fs_in_khz - self.input_delay;
        self.delay_buf[self.input_delay..self.fs_in_khz].copy_from_slice(&input[..n]);
        let first_ms: [i16; 48] = self.delay_buf;
        let produced = self.down_fir(&first_ms[..self.fs_in_khz], &mut out[..self.fs_out_khz]);
        debug_assert_eq!(produced, self.fs_out_khz);
        self.down_fir(
            &input[n..in_len - self.input_delay],
            &mut out[self.fs_out_khz..],
        );
        self.delay_buf[..self.input_delay].copy_from_slice(&input[in_len - self.input_delay..]);
    }
}

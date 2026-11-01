use crate::celt::pitch::pitch_xcorr;

pub fn lpc(lpc: &mut [f32], ac: &[f32], p: usize) {
    let mut error = ac[0];
    if error <= 1e-10 {
        for x in lpc.iter_mut() {
            *x = 0.0;
        }
        return;
    }

    for i in 0..p {
        let mut rr = 0.0f32;
        for j in 0..i {
            rr += lpc[j] * ac[i - j];
        }
        rr += ac[i + 1];
        let r = -rr / error;

        lpc[i] = r;
        for j in 0..i.div_ceil(2) {
            let tmp1 = lpc[j];
            let tmp2 = lpc[i - 1 - j];
            lpc[j] = tmp1 + r * tmp2;
            lpc[i - 1 - j] = tmp2 + r * tmp1;
        }

        error = error - r * r * error;

        if error <= 0.001 * ac[0] {
            break;
        }
    }
}

pub fn autocorr(
    x: &[f32],
    ac: &mut [f32],
    window: Option<&[f32]>,
    overlap: usize,
    lag: usize,
    n: usize,
) {
    let xx_vec;
    let xx: &[f32] = if let Some(win) = window {
        if x.len() < n {
            return;
        }
        xx_vec = {
            let mut v = x[0..n].to_vec();
            for i in 0..overlap {
                v[i] *= win[i];
                v[n - 1 - i] *= win[i];
            }
            v
        };
        &xx_vec
    } else {
        &x[0..n]
    };

    let fast_n = n - lag;

    pitch_xcorr(xx, xx, ac, fast_n, lag + 1);

    for k in 0..=lag {
        let mut d = 0.0f32;
        for i in (k + fast_n)..n {
            d += xx[i] * xx[i - k];
        }
        ac[k] += d;
    }
}

pub fn celt_fir(x: &[f32], num: &[f32], y: &mut [f32], n: usize, ord: usize) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        celt_fir_neon(x, num, y, n, ord);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        for i in 0..n {
            let mut sum = x[i];
            for j in 0..ord {
                if i > j {
                    sum += num[j] * x[i - j - 1];
                }
            }
            y[i] = sum;
        }
    }
}

pub fn celt_iir(x: &[f32], den: &[f32], y: &mut [f32], n: usize, ord: usize, mem: &mut [f32]) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        celt_iir_neon(x, den, y, n, ord, mem);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        for i in 0..n {
            let mut sum = x[i];
            for j in 0..ord {
                sum -= den[j] * mem[j];
            }
            for j in (1..ord).rev() {
                mem[j] = mem[j - 1];
            }
            mem[0] = sum;
            y[i] = sum;
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn celt_fir_neon(x: &[f32], num: &[f32], y: &mut [f32], n: usize, ord: usize) {
    use std::arch::aarch64::*;

    // The kernel reads `ord` taps and `n` samples of history and writes `n`
    // outputs; trimming to those lengths here is what puts the loads below in
    // bounds.
    let x = &x[..n];
    let num = &num[..ord];
    let y = &mut y[..n];

    if ord < 4 {
        for i in 0..n {
            let mut sum = x[i];
            for j in 0..ord {
                if i > j {
                    sum += num[j] * x[i - j - 1];
                }
            }
            y[i] = sum;
        }
        return;
    }

    for i in 0..n {
        // The accumulator starts empty and `x[i]` is added once, after the
        // horizontal reduction. Seeding the vector with `x[i]` instead would
        // contribute it four times over — one per lane.
        let mut sum = vdupq_n_f32(0.0);

        let mut j = 0;
        while j + 4 <= ord && i > j + 3 {
            let coeff = vld1q_f32(num.as_ptr().add(j));
            let x_vals = vld1q_f32(x.as_ptr().add(i - j - 4));
            let x_reversed = vrev64q_f32(x_vals);
            let x_reversed = vextq_f32(x_reversed, x_reversed, 2);
            sum = vfmaq_f32(sum, coeff, x_reversed);
            j += 4;
        }

        let sum_low = vget_low_f32(sum);
        let sum_high = vget_high_f32(sum);
        let sum_pair = vadd_f32(sum_low, sum_high);
        let mut result = x[i] + vget_lane_f32(sum_pair, 0) + vget_lane_f32(sum_pair, 1);

        while j < ord {
            if i > j {
                result += num[j] * x[i - j - 1];
            }
            j += 1;
        }

        y[i] = result;
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn celt_iir_neon(
    x: &[f32],
    den: &[f32],
    y: &mut [f32],
    n: usize,
    ord: usize,
    mem: &mut [f32],
) {
    use std::arch::aarch64::*;

    // The kernel reads `ord` denominator taps against `ord` memory slots and
    // walks `n` samples; trimming to those lengths here is what puts the loads
    // below in bounds.
    let x = &x[..n];
    let den = &den[..ord];
    let y = &mut y[..n];
    let mem = &mut mem[..ord];

    if ord < 4 {
        for i in 0..n {
            let mut sum = x[i];
            for j in 0..ord {
                sum -= den[j] * mem[j];
            }
            for j in (1..ord).rev() {
                mem[j] = mem[j - 1];
            }
            mem[0] = sum;
            y[i] = sum;
        }
        return;
    }

    for i in 0..n {
        let mut feedback = vdupq_n_f32(0.0);

        let mut j = 0;
        while j + 4 <= ord {
            let coeff = vld1q_f32(den.as_ptr().add(j));
            let mem_vals = vld1q_f32(mem.as_ptr().add(j));
            feedback = vfmaq_f32(feedback, coeff, mem_vals);
            j += 4;
        }

        let fb_low = vget_low_f32(feedback);
        let fb_high = vget_high_f32(feedback);
        let fb_pair = vadd_f32(fb_low, fb_high);
        let mut fb_sum = vget_lane_f32(fb_pair, 0) + vget_lane_f32(fb_pair, 1);

        while j < ord {
            fb_sum += den[j] * mem[j];
            j += 1;
        }

        let sum = x[i] - fb_sum;

        for j in (1..ord).rev() {
            mem[j] = mem[j - 1];
        }
        mem[0] = sum;
        y[i] = sum;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `celt_fir` straight from libopus `celt_fir_c`: the sample plus `ord`
    /// weighted history taps, with everything before the slice read as zero.
    fn celt_fir_reference(x: &[f32], num: &[f32], n: usize, ord: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let mut sum = x[i];
                for j in 0..ord {
                    if i > j {
                        sum += num[j] * x[i - j - 1];
                    }
                }
                sum
            })
            .collect()
    }

    /// `celt_iir` straight from libopus `celt_iir_c`, with `mem[j]` holding
    /// output `i - 1 - j`.
    fn celt_iir_reference(
        x: &[f32],
        den: &[f32],
        n: usize,
        ord: usize,
        mem: &mut [f32],
    ) -> Vec<f32> {
        let mut y = Vec::with_capacity(n);
        for &xi in x.iter().take(n) {
            let mut sum = xi;
            for j in 0..ord {
                sum -= den[j] * mem[j];
            }
            for j in (1..ord).rev() {
                mem[j] = mem[j - 1];
            }
            mem[0] = sum;
            y.push(sum);
        }
        y
    }

    fn ramp(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1 << 23) as f32 - 1.0
            })
            .collect()
    }

    /// The SIMD kernels must agree with the scalar definition they replace.
    ///
    /// This is not hypothetical: the aarch64 `celt_fir` seeded its vector
    /// accumulator with `x[i]` broadcast across all four lanes, so the
    /// horizontal reduction added `x[i]` four times. It only surfaced in the
    /// CELT concealment, the one caller, where it inflated the LPC residual
    /// until the explosion guard zeroed the frame.
    #[test]
    fn simd_fir_matches_the_scalar_definition() {
        for &ord in &[1usize, 3, 4, 5, 8, 16, 24] {
            for &n in &[ord + 1, 32, 97, 776] {
                let x = ramp(n, 0x1234_5678 ^ (n as u32));
                let num = ramp(ord, 0x9E37_79B9 ^ (ord as u32));
                let want = celt_fir_reference(&x, &num, n, ord);
                let mut got = vec![0.0f32; n];
                celt_fir(&x, &num, &mut got, n, ord);
                for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                    assert!(
                        (g - w).abs() <= 1e-4 * (1.0 + w.abs()),
                        "celt_fir ord={ord} n={n} sample {i}: {g} vs {w}"
                    );
                }
            }
        }
    }

    #[test]
    fn simd_iir_matches_the_scalar_definition() {
        for &ord in &[1usize, 3, 4, 5, 8, 16, 24] {
            for &n in &[1usize, 32, 97, 360] {
                let x = ramp(n, 0xDEAD_BEEF ^ (n as u32));
                // Small coefficients keep the recursion stable so the
                // comparison measures the kernel, not divergence.
                let den: Vec<f32> = ramp(ord, 0xFEED_FACE ^ (ord as u32))
                    .iter()
                    .map(|v| v * 0.1)
                    .collect();
                let seed = ramp(ord, 0x0BAD_C0DE);
                let mut mem_ref = seed.clone();
                let want = celt_iir_reference(&x, &den, n, ord, &mut mem_ref);
                let mut mem = seed.clone();
                let mut got = vec![0.0f32; n];
                celt_iir(&x, &den, &mut got, n, ord, &mut mem);
                for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                    assert!(
                        (g - w).abs() <= 1e-4 * (1.0 + w.abs()),
                        "celt_iir ord={ord} n={n} sample {i}: {g} vs {w}"
                    );
                }
                assert_eq!(mem.len(), mem_ref.len());
            }
        }
    }
}

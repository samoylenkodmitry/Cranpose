#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::cpu_features::FeatureCache;
use crate::silk::define::*;
use crate::silk::macros::*;

#[inline]
pub fn silk_k2a_q16(a_q24: &mut [i32], rc_q16: &[i32], order: usize) {
    for k in 0..order {
        let rc = rc_q16[k];
        for n in 0..((k + 1) >> 1) {
            let tmp1 = a_q24[n];
            let tmp2 = a_q24[k - n - 1];
            a_q24[n] = silk_smlaww(tmp1, tmp2, rc);
            a_q24[k - n - 1] = silk_smlaww(tmp2, tmp1, rc);
        }
        a_q24[k] = -(rc << 8);
    }
}

pub fn silk_schur64(rc_q16: &mut [i32], c: &[i32], order: usize) -> i32 {
    let mut c_matrix = [[0i32; 2]; MAX_SHAPE_LPC_ORDER + 1];
    let mut c_tmp1_q30: i32;
    let mut c_tmp2_q30: i32;
    let mut rc_tmp_q31: i32;

    if c[0] <= 0 {
        for v in rc_q16.iter_mut().take(order) {
            *v = 0;
        }
        return 0;
    }

    for k in 0..=order {
        c_matrix[k][0] = c[k];
        c_matrix[k][1] = c[k];
    }

    let mut k = 0;
    while k < order {
        if c_matrix[k + 1][0].wrapping_abs() >= c_matrix[0][1] {
            if c_matrix[k + 1][0] > 0 {
                rc_q16[k] = -64880;
            } else {
                rc_q16[k] = 64880;
            }
            k += 1;
            break;
        }

        rc_tmp_q31 = silk_div32_varq(-c_matrix[k + 1][0], c_matrix[0][1], 31);

        rc_q16[k] = silk_rshift_round(rc_tmp_q31, 15);

        for n in 0..(order - k) {
            c_tmp1_q30 = c_matrix[n + k + 1][0];
            c_tmp2_q30 = c_matrix[n][1];

            c_matrix[n + k + 1][0] =
                c_tmp1_q30.wrapping_add(silk_smmul(c_tmp2_q30 << 1, rc_tmp_q31));
            c_matrix[n][1] = c_tmp2_q30.wrapping_add(silk_smmul(c_tmp1_q30 << 1, rc_tmp_q31));
        }
        k += 1;
    }

    while k < order {
        rc_q16[k] = 0;
        k += 1;
    }

    c_matrix[0][1].max(1)
}

#[inline]
fn xcorr_kernel_c(x: &[i16], y: &[i16], sum: &mut [i32; 4], len: usize) {
    #[cfg(target_arch = "aarch64")]
    {
        unsafe {
            xcorr_kernel_neon_s16(x, y, sum, len);
        }
    }
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        unsafe { xcorr_kernel_avx2(x, y, sum, len) };
        return;
    }
    #[cfg(not(target_arch = "aarch64"))]
    xcorr_kernel_scalar(x, y, sum, len);
}

#[cfg_attr(target_arch = "aarch64", allow(dead_code))]
#[inline]
fn xcorr_kernel_scalar(x: &[i16], y: &[i16], sum: &mut [i32; 4], len: usize) {
    let mut j = 0;
    let mut y_0 = y[0];
    let mut y_1 = y[1];
    let mut y_2 = y[2];
    let mut y_3: i16 = 0;
    let mut yi = 3;
    while j + 3 < len {
        let tmp = x[j];
        y_3 = y[yi];
        yi += 1;
        sum[0] = mac16_16(sum[0], tmp, y_0);
        sum[1] = mac16_16(sum[1], tmp, y_1);
        sum[2] = mac16_16(sum[2], tmp, y_2);
        sum[3] = mac16_16(sum[3], tmp, y_3);
        let tmp = x[j + 1];
        y_0 = y[yi];
        yi += 1;
        sum[0] = mac16_16(sum[0], tmp, y_1);
        sum[1] = mac16_16(sum[1], tmp, y_2);
        sum[2] = mac16_16(sum[2], tmp, y_3);
        sum[3] = mac16_16(sum[3], tmp, y_0);
        let tmp = x[j + 2];
        y_1 = y[yi];
        yi += 1;
        sum[0] = mac16_16(sum[0], tmp, y_2);
        sum[1] = mac16_16(sum[1], tmp, y_3);
        sum[2] = mac16_16(sum[2], tmp, y_0);
        sum[3] = mac16_16(sum[3], tmp, y_1);
        let tmp = x[j + 3];
        y_2 = y[yi];
        yi += 1;
        sum[0] = mac16_16(sum[0], tmp, y_3);
        sum[1] = mac16_16(sum[1], tmp, y_0);
        sum[2] = mac16_16(sum[2], tmp, y_1);
        sum[3] = mac16_16(sum[3], tmp, y_2);
        j += 4;
    }

    if j < len {
        let tmp = x[j];
        j += 1;
        y_3 = y[yi];
        yi += 1;
        sum[0] = mac16_16(sum[0], tmp, y_0);
        sum[1] = mac16_16(sum[1], tmp, y_1);
        sum[2] = mac16_16(sum[2], tmp, y_2);
        sum[3] = mac16_16(sum[3], tmp, y_3);
    }
    if j < len {
        let tmp = x[j];
        j += 1;
        y_0 = y[yi];
        yi += 1;
        sum[0] = mac16_16(sum[0], tmp, y_1);
        sum[1] = mac16_16(sum[1], tmp, y_2);
        sum[2] = mac16_16(sum[2], tmp, y_3);
        sum[3] = mac16_16(sum[3], tmp, y_0);
    }
    if j < len {
        let tmp = x[j];
        y_1 = y[yi];
        sum[0] = mac16_16(sum[0], tmp, y_2);
        sum[1] = mac16_16(sum[1], tmp, y_3);
        sum[2] = mac16_16(sum[2], tmp, y_0);
        sum[3] = mac16_16(sum[3], tmp, y_1);
    }
    let _ = (y_0, y_1, y_2, y_3);
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn xcorr_kernel_neon_s16(x: &[i16], y: &[i16], sum: &mut [i32; 4], mut len: usize) {
    use std::arch::aarch64::*;

    // Callers owe `len` taps and `len + 3` history samples: tap `j` is
    // correlated against `y[j ..= j + 3]`.
    let x = &x[..len];
    assert!(
        y.len() >= len + 3,
        "xcorr kernel: y holds {} samples, needs len + 3 = {}",
        y.len(),
        len + 3
    );

    let mut acc = vld1q_s32(sum.as_ptr());

    let mut xi = x.as_ptr();
    let mut yi = y.as_ptr();
    let mut off = 0usize;

    // A step uses seven `y` lanes, but `vld1q_s16` takes eight and the second
    // load runs four ahead, so a step needs twelve lanes of headroom -- more
    // than the `len + 3` a caller owes. The vector path therefore runs only
    // while that headroom lasts and the scalar loop below finishes the taps.
    // Before this bound the kernel read up to four `i16` past `y` whenever the
    // caller supplied exactly `len + 3`, which `silk_pitch_xcorr` does at its
    // last lag.
    if y.len() >= 12 {
        let mut yy = vld1q_s16(yi);

        while len > 4 && off + 12 <= y.len() {
            yi = yi.add(4);
            let yy1 = vld1q_s16(yi);

            let xj0 = vld1_dup_s16(xi);
            acc = vmlal_s16(acc, vget_low_s16(yy), xj0);

            let xj1 = vld1_dup_s16(xi.add(1));
            let ye1 = vextq_s16(yy, yy1, 1);
            acc = vmlal_s16(acc, vget_low_s16(ye1), xj1);

            let xj2 = vld1_dup_s16(xi.add(2));
            let ye2 = vextq_s16(yy, yy1, 2);
            acc = vmlal_s16(acc, vget_low_s16(ye2), xj2);

            let xj3 = vld1_dup_s16(xi.add(3));
            let ye3 = vextq_s16(yy, yy1, 3);
            acc = vmlal_s16(acc, vget_low_s16(ye3), xj3);

            xi = xi.add(4);
            yy = yy1;
            off += 4;
            len -= 4;
        }
    }

    vst1q_s32(sum.as_mut_ptr(), acc);

    for k in 0..len {
        let xv = *xi.add(k) as i32;

        sum[0] = sum[0].wrapping_add(xv * (*yi.add(k) as i32));
        sum[1] = sum[1].wrapping_add(xv * (*yi.add(k + 1) as i32));
        sum[2] = sum[2].wrapping_add(xv * (*yi.add(k + 2) as i32));
        sum[3] = sum[3].wrapping_add(xv * (*yi.add(k + 3) as i32));
    }
}

#[inline(always)]
fn mac16_16(a: i32, b: i16, c: i16) -> i32 {
    a.wrapping_add((b as i32).wrapping_mul(c as i32))
}

pub fn silk_autocorr(
    results: &mut [i32],
    scale: &mut i32,
    input_data: &[i16],
    input_data_size: usize,
    correlation_count: usize,
) {
    #[inline]
    fn ec_ilog(x: u32) -> i32 {
        if x == 0 {
            0
        } else {
            32 - x.leading_zeros() as i32
        }
    }

    let n = input_data_size;
    let mut shift: i32;

    let xptr = input_data;

    let ac0_shift = ec_ilog((n + (n >> 4)) as u32) - 1;

    let mut ac0: i32 = 1 + ((n as i32) << 7);
    let mut i = n & 1;
    if n & 1 != 0 {
        ac0 += ((xptr[0] as i32) * (xptr[0] as i32)) >> ac0_shift;
    }
    while i < n {
        ac0 += ((xptr[i] as i32) * (xptr[i] as i32)) >> ac0_shift;
        ac0 += ((xptr[i + 1] as i32) * (xptr[i + 1] as i32)) >> ac0_shift;
        i += 2;
    }

    ac0 += ac0 >> 7;

    let ac0_log2 = ec_ilog(ac0 as u32) - 1;
    shift = ac0_log2 - 30 + ac0_shift + 1;
    shift /= 2;

    let mut xx_buf = [0i16; PE_MAX_FRAME_LENGTH];
    let xptr: &[i16];

    if shift > 0 {
        for j in 0..n {
            xx_buf[j] = silk_rshift_round(input_data[j] as i32, shift) as i16;
        }
        xptr = &xx_buf[..n];
    } else {
        shift = 0;
        xptr = input_data;
    }

    let lag = correlation_count - 1;
    let fast_n = n - lag;
    let max_pitch = lag + 1;

    let mut lag_idx = 0;
    while lag_idx + 3 < max_pitch {
        let mut sum = [0i32; 4];
        xcorr_kernel_c(xptr, &xptr[lag_idx..], &mut sum, fast_n);
        results[lag_idx] = sum[0];
        results[lag_idx + 1] = sum[1];
        results[lag_idx + 2] = sum[2];
        results[lag_idx + 3] = sum[3];
        lag_idx += 4;
    }

    while lag_idx < max_pitch {
        let mut sum = 0i32;
        for j in 0..fast_n {
            sum = sum.wrapping_add((xptr[j] as i32).wrapping_mul(xptr[j + lag_idx] as i32));
        }
        results[lag_idx] = sum;
        lag_idx += 1;
    }

    for k in 0..correlation_count {
        let mut d: i32 = 0;
        for i in (k + fast_n)..n {
            d = d.wrapping_add((xptr[i] as i32).wrapping_mul(xptr[i - k] as i32));
        }
        results[k] = results[k].wrapping_add(d);
    }

    shift *= 2;
    if shift <= 0 {
        let add_shift = (-shift).min(30);
        results[0] += 1i32 << add_shift;
    }
    if results[0] > 0 && results[0] < 268435456 {
        let shift2 = 29 - ec_ilog(results[0] as u32);
        for v in results[..correlation_count].iter_mut() {
            *v <<= shift2;
        }
        shift -= shift2;
    } else if results[0] >= 536870912 {
        let mut shift2 = 1;
        if results[0] >= 1073741824 {
            shift2 += 1;
        }
        for v in results[..correlation_count].iter_mut() {
            *v >>= shift2;
        }
        shift += shift2;
    }

    *scale = shift;
}

pub fn silk_sum_sqr_shift(energy: &mut i32, shift: &mut i32, x: &[i16], len: usize) {
    let mut i: usize;
    let mut shft: i32;
    let mut nrg_tmp: u32;
    let mut nrg: i32;

    shft = 31 - silk_clz32(len as i32);

    nrg = len as i32;
    i = 0;
    while i < len - 1 {
        nrg_tmp = silk_smulbb(x[i] as i32, x[i] as i32) as u32;
        nrg_tmp = nrg_tmp.wrapping_add(silk_smulbb(x[i + 1] as i32, x[i + 1] as i32) as u32);
        nrg = nrg.wrapping_add((nrg_tmp >> shft) as i32);
        i += 2;
    }
    if i < len {
        nrg_tmp = silk_smulbb(x[i] as i32, x[i] as i32) as u32;
        nrg = nrg.wrapping_add((nrg_tmp >> shft) as i32);
    }

    shft = (shft + 3 - silk_clz32(nrg)).max(0);

    #[cfg(target_arch = "aarch64")]
    {
        nrg = unsafe { silk_sum_sqr_shift_neon(x, len, shft) };
    }
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        nrg = unsafe { silk_sum_sqr_shift_avx2(x, len, shft) };
    } else {
        nrg = 0;
        i = 0;
        while i < len - 1 {
            nrg_tmp = silk_smulbb(x[i] as i32, x[i] as i32) as u32;
            nrg_tmp = nrg_tmp.wrapping_add(silk_smulbb(x[i + 1] as i32, x[i + 1] as i32) as u32);
            nrg = nrg.wrapping_add((nrg_tmp >> shft) as i32);
            i += 2;
        }
        if i < len {
            nrg_tmp = silk_smulbb(x[i] as i32, x[i] as i32) as u32;
            nrg = nrg.wrapping_add((nrg_tmp >> shft) as i32);
        }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        nrg = 0;
        i = 0;
        while i < len - 1 {
            nrg_tmp = silk_smulbb(x[i] as i32, x[i] as i32) as u32;
            nrg_tmp = nrg_tmp.wrapping_add(silk_smulbb(x[i + 1] as i32, x[i + 1] as i32) as u32);
            nrg = nrg.wrapping_add((nrg_tmp >> shft) as i32);
            i += 2;
        }
        if i < len {
            nrg_tmp = silk_smulbb(x[i] as i32, x[i] as i32) as u32;
            nrg = nrg.wrapping_add((nrg_tmp >> shft) as i32);
        }
    }

    *shift = shft;
    *energy = nrg;
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn silk_sum_sqr_shift_neon(x: &[i16], len: usize, shft: i32) -> i32 {
    use std::arch::aarch64::*;

    // The loads below walk `len` samples; trimming to `len` here is what puts
    // them in bounds.
    let x = &x[..len];

    // The reference shifts each *pair* sum, not each square, so the truncation
    // lands once per pair. Shifting the squares individually — as this kernel
    // used to — truncates once per sample and drifts low by roughly half an
    // LSB per extra sample. SILK is fixed point: that difference reaches the
    // bitstream.
    let mut acc = vdupq_n_s64(0i64);
    let shift_vec = vdupq_n_s64(-shft as i64);
    let mut i = 0;

    while i + 8 <= len {
        let v = vld1q_s16(x.as_ptr().add(i));

        let lo = vget_low_s16(v);
        let hi = vget_high_s16(v);
        let sq_lo = vmull_s16(lo, lo);
        let sq_hi = vmull_s16(hi, hi);

        // Pairwise-widen first: each lane holds x[2k]^2 + x[2k+1]^2, which is
        // what the reference shifts. Two squares of i16 fit an i64 exactly.
        acc = vaddq_s64(acc, vshlq_s64(vpaddlq_s32(sq_lo), shift_vec));
        acc = vaddq_s64(acc, vshlq_s64(vpaddlq_s32(sq_hi), shift_vec));
        i += 8;
    }

    let mut nrg = vaddvq_s64(acc) as i32;

    while i + 1 < len {
        let a = x[i] as i32;
        let b = x[i + 1] as i32;
        let sq = ((a * a) as u32).wrapping_add((b * b) as u32);
        nrg = nrg.wrapping_add((sq >> shft) as i32);
        i += 2;
    }
    if i < len {
        let v = x[i] as i32;
        let sq = (v * v) as u32;
        nrg = nrg.wrapping_add((sq >> shft) as i32);
    }
    nrg
}

/// Inner product with each term right-shifted before accumulation, so a long
/// or loud pair of vectors cannot overflow the accumulator (libopus
/// `silk_inner_prod_aligned_scale`). Shifting per term rather than once at the
/// end is what the reference does and what the callers' Q formats assume, so
/// this stays scalar: a vectorised version would have to reproduce the same
/// per-term truncation to be worth anything.
pub fn silk_inner_prod_aligned_scale(ptr1: &[i16], ptr2: &[i16], scale: i32, len: usize) -> i32 {
    let mut sum = 0i32;
    for i in 0..len {
        sum = silk_add_rshift32(sum, silk_smulbb(ptr1[i] as i32, ptr2[i] as i32), scale);
    }
    sum
}

#[inline(always)]
pub fn silk_inner_prod_aligned(ptr1: &[i16], ptr2: &[i16], len: usize) -> i32 {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        silk_inner_prod_aligned_neon(ptr1, ptr2, len)
    }
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        return unsafe { silk_inner_prod_aligned_avx2(ptr1, ptr2, len) };
    }
    #[cfg(not(target_arch = "aarch64"))]
    silk_inner_prod_aligned_scalar(ptr1, ptr2, len)
}

#[cfg_attr(target_arch = "aarch64", allow(dead_code))]
#[inline(always)]
fn silk_inner_prod_aligned_scalar(ptr1: &[i16], ptr2: &[i16], len: usize) -> i32 {
    let ptr1 = &ptr1[..len];
    let ptr2 = &ptr2[..len];
    let mut i = 0;
    let mut sum0 = 0i32;
    let mut sum1 = 0i32;
    let mut sum2 = 0i32;
    let mut sum3 = 0i32;
    let len4 = (len / 4) * 4;

    while i < len4 {
        sum0 = sum0.wrapping_add((ptr1[i] as i32).wrapping_mul(ptr2[i] as i32));
        sum1 = sum1.wrapping_add((ptr1[i + 1] as i32).wrapping_mul(ptr2[i + 1] as i32));
        sum2 = sum2.wrapping_add((ptr1[i + 2] as i32).wrapping_mul(ptr2[i + 2] as i32));
        sum3 = sum3.wrapping_add((ptr1[i + 3] as i32).wrapping_mul(ptr2[i + 3] as i32));
        i += 4;
    }

    while i < len {
        sum0 = sum0.wrapping_add((ptr1[i] as i32).wrapping_mul(ptr2[i] as i32));
        i += 1;
    }

    sum0.wrapping_add(sum1)
        .wrapping_add(sum2)
        .wrapping_add(sum3)
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn silk_inner_prod_aligned_neon(ptr1: &[i16], ptr2: &[i16], len: usize) -> i32 {
    use std::arch::aarch64::*;

    // The loads below walk `len` elements of each input; trimming to `len`
    // here is what puts them in bounds.
    let ptr1 = &ptr1[..len];
    let ptr2 = &ptr2[..len];

    let mut acc0 = vdupq_n_s32(0i32);
    let mut acc1 = vdupq_n_s32(0i32);
    let mut acc2 = vdupq_n_s32(0i32);
    let mut acc3 = vdupq_n_s32(0i32);

    let mut i = 0;

    while i + 32 <= len {
        let a0 = vld1q_s16(ptr1.as_ptr().add(i));
        let b0 = vld1q_s16(ptr2.as_ptr().add(i));
        let a1 = vld1q_s16(ptr1.as_ptr().add(i + 8));
        let b1 = vld1q_s16(ptr2.as_ptr().add(i + 8));
        let a2 = vld1q_s16(ptr1.as_ptr().add(i + 16));
        let b2 = vld1q_s16(ptr2.as_ptr().add(i + 16));
        let a3 = vld1q_s16(ptr1.as_ptr().add(i + 24));
        let b3 = vld1q_s16(ptr2.as_ptr().add(i + 24));

        acc0 = vmlal_s16(acc0, vget_low_s16(a0), vget_low_s16(b0));
        acc0 = vmlal_high_s16(acc0, a0, b0);
        acc1 = vmlal_s16(acc1, vget_low_s16(a1), vget_low_s16(b1));
        acc1 = vmlal_high_s16(acc1, a1, b1);
        acc2 = vmlal_s16(acc2, vget_low_s16(a2), vget_low_s16(b2));
        acc2 = vmlal_high_s16(acc2, a2, b2);
        acc3 = vmlal_s16(acc3, vget_low_s16(a3), vget_low_s16(b3));
        acc3 = vmlal_high_s16(acc3, a3, b3);

        i += 32;
    }

    while i + 8 <= len {
        let a0 = vld1q_s16(ptr1.as_ptr().add(i));
        let b0 = vld1q_s16(ptr2.as_ptr().add(i));
        acc0 = vmlal_s16(acc0, vget_low_s16(a0), vget_low_s16(b0));
        acc0 = vmlal_high_s16(acc0, a0, b0);
        i += 8;
    }

    let sum01 = vpaddq_s32(acc0, acc1);
    let sum23 = vpaddq_s32(acc2, acc3);
    let sum = vpaddq_s32(sum01, sum23);
    let wide = vpaddlq_s32(sum);
    let mut result = vaddvq_s64(wide);

    while i < len {
        result += (ptr1[i] as i64) * (ptr2[i] as i64);
        i += 1;
    }

    result as i32
}

pub fn silk_corr_vector_fix(
    x: &[i16],
    t: &[i16],
    l: usize,
    order: usize,
    xt: &mut [i32],
    rshifts: i32,
) {
    let mut ptr1_idx = order - 1;
    if rshifts > 0 {
        for xt_val in xt[..order].iter_mut() {
            let mut inner_prod: i32 = 0;
            for i in 0..l {
                inner_prod = silk_add_rshift32(
                    inner_prod,
                    silk_smulbb(x[ptr1_idx + i] as i32, t[i] as i32),
                    rshifts,
                );
            }
            *xt_val = inner_prod;
            ptr1_idx = ptr1_idx.saturating_sub(1);
        }
    } else {
        for xt_val in xt[..order].iter_mut() {
            *xt_val = silk_inner_prod_aligned(&x[ptr1_idx..], t, l);
            ptr1_idx = ptr1_idx.saturating_sub(1);
        }
    }
}

pub fn silk_corr_matrix_fix(
    x: &[i16],
    l: usize,
    order: usize,
    xx: &mut [i32],
    nrg: &mut i32,
    rshifts: &mut i32,
) {
    silk_sum_sqr_shift(nrg, rshifts, x, l + order - 1);
    let mut energy = *nrg;

    for xi in x.iter().take(order - 1) {
        energy -= silk_rshift32(silk_smulbb(*xi as i32, *xi as i32), *rshifts);
    }

    xx[0] = energy;
    let ptr1_start_idx = order - 1;
    for j in 1..order {
        energy = silk_sub32(
            energy,
            silk_rshift32(
                silk_smulbb(
                    x[ptr1_start_idx + l - j] as i32,
                    x[ptr1_start_idx + l - j] as i32,
                ),
                *rshifts,
            ),
        );
        energy = silk_add32(
            energy,
            silk_rshift32(
                silk_smulbb(x[ptr1_start_idx - j] as i32, x[ptr1_start_idx - j] as i32),
                *rshifts,
            ),
        );
        xx[j * order + j] = energy;
    }

    for lag in 1..order {
        let ptr1_idx = ptr1_start_idx;
        let ptr2_idx = ptr1_start_idx - lag;
        let mut inner_prod: i32 = 0;
        if *rshifts > 0 {
            for i in 0..l {
                inner_prod = silk_add_rshift32(
                    inner_prod,
                    silk_smulbb(x[ptr1_idx + i] as i32, x[ptr2_idx + i] as i32),
                    *rshifts,
                );
            }
        } else {
            inner_prod = silk_inner_prod_aligned(&x[ptr1_idx..], &x[ptr2_idx..], l);
        }
        xx[lag] = inner_prod;
        xx[lag * order] = inner_prod;

        for j in 1..(order - lag) {
            inner_prod = silk_sub32(
                inner_prod,
                silk_rshift32(
                    silk_smulbb(x[ptr1_idx + l - j] as i32, x[ptr2_idx + l - j] as i32),
                    *rshifts,
                ),
            );
            inner_prod = silk_add32(
                inner_prod,
                silk_rshift32(
                    silk_smulbb(x[ptr1_idx - j] as i32, x[ptr2_idx - j] as i32),
                    *rshifts,
                ),
            );
            xx[j * order + (lag + j)] = inner_prod;
            xx[(lag + j) * order + j] = inner_prod;
        }
    }
}

const FREQ_TABLE_Q16: [i16; 27] = [
    12111, 9804, 8235, 7100, 6239, 5565, 5022, 4575, 4202, 3885, 3612, 3375, 3167, 2984, 2820,
    2674, 2542, 2422, 2313, 2214, 2123, 2038, 1961, 1889, 1822, 1760, 1702,
];

pub fn silk_apply_sine_window(px_win: &mut [i16], px: &[i16], win_type: i32, length: usize) {
    let mut s0_q16: i32;
    let mut s1_q16: i32;

    let idx = (length >> 2) - 4;
    let f_q16: i32 = FREQ_TABLE_Q16[idx] as i32;

    let c_q16: i32 = silk_smulwb(f_q16, -f_q16);

    if win_type == 1 {
        s0_q16 = 0;
        s1_q16 = f_q16 + (length as i32 >> 3);
    } else {
        s0_q16 = 1 << 16;
        s1_q16 = (1 << 16) + (c_q16 >> 1) + (length as i32 >> 4);
    }

    for k in (0..length).step_by(4) {
        px_win[k] = silk_smulwb((s0_q16 + s1_q16) >> 1, px[k] as i32) as i16;
        px_win[k + 1] = silk_smulwb(s1_q16, px[k + 1] as i32) as i16;
        s0_q16 = silk_smulwb(s1_q16, c_q16) + (s1_q16 << 1) - s0_q16 + 1;
        s0_q16 = s0_q16.min(1 << 16);

        px_win[k + 2] = silk_smulwb((s0_q16 + s1_q16) >> 1, px[k + 2] as i32) as i16;
        px_win[k + 3] = silk_smulwb(s0_q16, px[k + 3] as i32) as i16;
        s1_q16 = silk_smulwb(s0_q16, c_q16) + (s0_q16 << 1) - s1_q16;
        s1_q16 = s1_q16.min(1 << 16);
    }
}

#[inline(always)]
pub fn silk_pitch_xcorr(x: &[i16], y: &[i16], xcorr: &mut [i32], len: usize, max_pitch: usize) {
    debug_assert!(max_pitch > 0);
    debug_assert!(x.len() >= len);
    debug_assert!(xcorr.len() >= max_pitch);

    let y_len = y.len();

    let mut i = 0;
    while i + 3 < max_pitch {
        if y_len >= i + len + 3 {
            let mut sum = [0i32; 4];
            xcorr_kernel_c(x, &y[i..], &mut sum, len);
            xcorr[i] = sum[0];
            xcorr[i + 1] = sum[1];
            xcorr[i + 2] = sum[2];
            xcorr[i + 3] = sum[3];
            i += 4;
        } else {
            for k in i..i.saturating_add(4).min(max_pitch) {
                let avail = len.min(y_len.saturating_sub(k));
                let mut sum = 0i32;
                for j in 0..avail {
                    sum = mac16_16(sum, x[j], y[k + j]);
                }
                xcorr[k] = sum;
            }
            i = i.saturating_add(4).min(max_pitch);
        }
    }

    while i < max_pitch {
        let avail = len.min(y_len.saturating_sub(i));
        let mut sum = 0i32;
        for j in 0..avail {
            sum = mac16_16(sum, x[j], y[i + j]);
        }
        xcorr[i] = sum;
        i += 1;
    }
}

/// Correlation MAC over the warped states: `corr_qc[i] += (state[i]·state[0])>>16`
/// for `i in 0..=order`. Scalar oracle; the AVX2 twin (below) must match it
/// byte-for-byte (i64 accumulate, arithmetic `>>16`).
#[inline(always)]
fn warped_corr_update_scalar(corr_qc: &mut [i64], state_qs: &[i32], order: usize) {
    let state0 = state_qs[0];
    for i in 0..=order {
        corr_qc[i] += silk_rshift64(silk_smull(state_qs[i], state0), 2 * 13 - 10);
    }
}

/// Cached AVX2 dispatch for the warped-correlation MAC (mirrors the NSQ knob;
/// the scalar twin below is the correctness oracle).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn warped_corr_avx2_enabled() -> bool {
    static CACHE: FeatureCache = FeatureCache::new();
    CACHE.get(|| is_x86_feature_detected!("avx2"))
}

/// AVX2 twin: 4 taps/iteration, `(state[i] as i64 * state0 as i64) >> 16`
/// accumulated into `corr_qc` (i64). Byte-identical to the scalar.
///
/// SAFETY: `corr_qc.len() >= order+1` and `state_qs.len() >= order+1` (the caller
/// sizes both `MAX_SHAPE_LPC_ORDER+1`); AVX2 checked by the caller.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn warped_corr_update_avx2(corr_qc: &mut [i64], state_qs: &[i32], order: usize) {
    use core::arch::x86_64::*;

    // The kernel updates `order + 1` correlation lags against the same number
    // of state slots; trimming both here is what puts the loads and stores
    // below in bounds.
    let corr_qc = &mut corr_qc[..order + 1];
    let state_qs = &state_qs[..order + 1];
    let n = order + 1;
    let s0v = _mm256_set1_epi64x(state_qs[0] as i64); // state0 in low 32 of each lane
    let cp = corr_qc.as_mut_ptr();
    let sp = state_qs.as_ptr();
    let mut i = 0usize;
    while i + 4 <= n {
        // Sign-extend 4 i32 states to 4 i64 lanes; mul_epi32 uses the low 32 bits.
        let s4 = _mm256_cvtepi32_epi64(_mm_loadu_si128(sp.add(i) as *const __m128i));
        let prod = _mm256_mul_epi32(s4, s0v); // 4× (state[i+k] * state0) as i64
        // Arithmetic >>16 of packed i64 (logical shift + sign fill at bit 48).
        let sign = _mm256_cmpgt_epi64(_mm256_setzero_si256(), prod);
        let shifted = _mm256_or_si256(_mm256_srli_epi64(prod, 16), _mm256_slli_epi64(sign, 48));
        let cur = _mm256_loadu_si256(cp.add(i) as *const __m256i);
        _mm256_storeu_si256(cp.add(i) as *mut __m256i, _mm256_add_epi64(cur, shifted));
        i += 4;
    }
    while i < n {
        corr_qc[i] += silk_rshift64(silk_smull(state_qs[i], state_qs[0]), 2 * 13 - 10);
        i += 1;
    }
}

#[inline(always)]
fn warped_corr_update(corr_qc: &mut [i64], state_qs: &[i32], order: usize) {
    #[cfg(target_arch = "x86_64")]
    {
        if warped_corr_avx2_enabled() {
            // SAFETY: avx2 checked; caller sizes both slices to order+1.
            unsafe { warped_corr_update_avx2(corr_qc, state_qs, order) };
            return;
        }
    }
    warped_corr_update_scalar(corr_qc, state_qs, order);
}

pub fn silk_warped_autocorrelation_fix(
    corr: &mut [i32],
    scale: &mut i32,
    input: &[i16],
    warping_q16: i32,
    length: usize,
    order: usize,
) {
    const QC: i32 = 10;
    const QS: i32 = 13;

    let mut tmp1_qs: i32;
    let mut tmp2_qs: i32;
    let mut state_qs = [0i32; MAX_SHAPE_LPC_ORDER + 1];
    let mut corr_qc = [0i64; MAX_SHAPE_LPC_ORDER + 1];

    debug_assert!((order & 1) == 0);

    for &input_n in input.iter().take(length) {
        tmp1_qs = (input_n as i32) << QS;

        // Warped all-pass state update (serial recurrence — inherently scalar).
        let mut i = 0;
        while i < order {
            tmp2_qs = silk_smlaww(state_qs[i], state_qs[i + 1] - tmp1_qs, warping_q16);
            state_qs[i] = tmp1_qs;
            tmp1_qs = silk_smlaww(state_qs[i + 1], state_qs[i + 2] - tmp2_qs, warping_q16);
            state_qs[i + 1] = tmp2_qs;
            i += 2;
        }
        state_qs[order] = tmp1_qs;

        // Correlation accumulation: corr_qc[i] += (state_qs[i]·state_qs[0]) >> 16
        // for all i — a vector×scalar i64 MAC (state_qs[0] = input<<QS is the
        // per-sample scalar). Vectorizable now that it's split from the recurrence.
        warped_corr_update(&mut corr_qc, &state_qs, order);
    }

    let mut lsh = silk_clz64(corr_qc[0]) - 35;
    lsh = silk_limit_32(lsh, -12 - QC, 30 - QC);
    *scale = -(QC + lsh);
    if lsh >= 0 {
        for i in 0..=order {
            corr[i] = (corr_qc[i] << lsh) as i32;
        }
    } else {
        for i in 0..=order {
            corr[i] = (corr_qc[i] >> (-lsh)) as i32;
        }
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod warped_corr_avx2_tests {
    use super::*;

    #[test]
    fn avx2_matches_scalar() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }
        let mut s: u64 = 0xDEAD_BEEF_1357_9BDF;
        let mut rng = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for order in [10usize, 12, 14, 16] {
            for _ in 0..50_000 {
                let mut state = [0i32; MAX_SHAPE_LPC_ORDER + 1];
                for v in state.iter_mut().take(order + 1) {
                    // Warped states reach ~i16<<QS magnitudes; cover full sign range.
                    *v = (rng() as i32) >> (rng() as u32 % 4);
                }
                // Random non-zero starting accumulators to catch add mistakes.
                let mut a = [0i64; MAX_SHAPE_LPC_ORDER + 1];
                let mut b = [0i64; MAX_SHAPE_LPC_ORDER + 1];
                for k in 0..=order {
                    let init = ((rng() as i64) << 20) >> (rng() as u32 % 20);
                    a[k] = init;
                    b[k] = init;
                }
                warped_corr_update_scalar(&mut a, &state, order);
                unsafe { warped_corr_update_avx2(&mut b, &state, order) };
                assert_eq!(a, b, "order={order}");
            }
        }
    }
}

pub fn silk_schur(rc_q15: &mut [i16], c: &[i32], order: usize) -> i32 {
    let mut c_inner = [[0i32; 2]; MAX_LPC_ORDER + 1];
    let mut ctmp1: i32;
    let mut ctmp2: i32;
    let mut rc_tmp_q15: i32;

    assert!(order <= MAX_LPC_ORDER);

    let lz = c[0].leading_zeros() as i32;

    if lz < 2 {
        for i in 0..=order {
            c_inner[i][0] = c[i] >> 1;
            c_inner[i][1] = c[i] >> 1;
        }
    } else if lz > 2 {
        let lz_adj = lz - 2;
        for i in 0..=order {
            c_inner[i][0] = c[i] << lz_adj;
            c_inner[i][1] = c[i] << lz_adj;
        }
    } else {
        for i in 0..=order {
            c_inner[i][0] = c[i];
            c_inner[i][1] = c[i];
        }
    }

    for k in 0..order {
        if c_inner[k + 1][0].abs() >= c_inner[0][1] {
            if c_inner[k + 1][0] > 0 {
                rc_q15[k] = -32440;
            } else {
                rc_q15[k] = 32440;
            }
            return c_inner[0][1];
        }

        rc_tmp_q15 = -silk_div32_16(c_inner[k + 1][0], (c_inner[0][1] >> 15).max(1));

        rc_tmp_q15 = silk_sat16(rc_tmp_q15);

        rc_q15[k] = rc_tmp_q15 as i16;

        for n in 0..order - k {
            ctmp1 = c_inner[n + k + 1][0];
            ctmp2 = c_inner[n][1];
            c_inner[n + k + 1][0] = silk_smlawb(ctmp1, ctmp2 << 1, rc_tmp_q15);
            c_inner[n][1] = silk_smlawb(ctmp2, ctmp1 << 1, rc_tmp_q15);
        }
    }

    c_inner[0][1]
}

pub fn silk_k2a(a_q24: &mut [i32], rc_q15: &[i16], order: usize) {
    for k in 0..order {
        let rc = rc_q15[k] as i32;
        for n in 0..(k + 1) >> 1 {
            let tmp1 = a_q24[n];
            let tmp2 = a_q24[k - n - 1];
            a_q24[n] = silk_smlawb(tmp1, tmp2 << 1, rc);
            a_q24[k - n - 1] = silk_smlawb(tmp2, tmp1 << 1, rc);
        }
        a_q24[k] = -(rc << 9);
    }
}

pub fn silk_bwexpander(ar: &mut [i16], d: usize, mut chirp_q16: i32) {
    let chirp_minus_one_q16 = chirp_q16 - 65536;

    for ar_val in ar[..d - 1].iter_mut() {
        *ar_val = silk_rshift_round((*ar_val as i32).wrapping_mul(chirp_q16), 16) as i16;
        chirp_q16 += silk_rshift_round(chirp_q16.wrapping_mul(chirp_minus_one_q16), 16);
    }
    ar[d - 1] = silk_rshift_round((ar[d - 1] as i32).wrapping_mul(chirp_q16), 16) as i16;
}

pub fn silk_lpc_analysis_filter(
    out: &mut [i16],
    input: &[i16],
    b: &[i16],
    len: usize,
    d: usize,
    _arch: i32,
) {
    assert!(d >= 6);
    assert!((d & 1) == 0);
    assert!(d <= len);

    #[cfg(target_arch = "x86_64")]
    if d <= 16 && is_x86_feature_detected!("avx2") {
        unsafe {
            silk_lpc_analysis_filter_avx2(out, input, b, len, d);
        }
        return;
    }

    #[cfg(target_arch = "aarch64")]
    {
        // NEON-backed inner-product path helps most for larger orders (e.g. d=16)
        // and can regress small orders due to setup overhead.
        if d >= 16 {
            silk_lpc_analysis_filter_aarch64(out, input, b, len, d);
        } else {
            silk_lpc_analysis_filter_scalar(out, input, b, len, d);
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        silk_lpc_analysis_filter_scalar(out, input, b, len, d);
    }
}

#[inline(always)]
#[cfg_attr(target_arch = "aarch64", allow(dead_code))]
fn silk_lpc_analysis_filter_scalar(
    out: &mut [i16],
    input: &[i16],
    b: &[i16],
    len: usize,
    d: usize,
) {
    for out_val in out[..d].iter_mut() {
        *out_val = 0;
    }

    // The filter reads `input[ix - d ..= ix]` and writes `out[ix]`. Trimming
    // both to `len` and the taps to `d` up front states every bound once, so
    // the loop below indexes slices whose lengths the compiler already knows.
    let input = &input[..len];
    let out = &mut out[..len];
    let b = &b[..d];

    for ix in d..len {
        // `hist[d - 1 - j]` is `input[ix - j - 1]`, so pairing the taps with
        // the history in reverse walks the same products as the reference.
        let hist = &input[ix - d..ix];
        let mut out32_q12 = 0i32;
        for (&bj, &xj) in b.iter().zip(hist.iter().rev()) {
            out32_q12 = out32_q12.wrapping_add(silk_smulbb(xj as i32, bj as i32));
        }

        out32_q12 = ((input[ix] as i32) << 12).wrapping_sub(out32_q12);

        let out32 = silk_rshift_round(out32_q12, 12);

        out[ix] = silk_sat16(out32) as i16;
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn silk_lpc_analysis_filter_aarch64(
    out: &mut [i16],
    input: &[i16],
    b: &[i16],
    len: usize,
    d: usize,
) {
    for out_val in out[..d].iter_mut() {
        *out_val = 0;
    }

    let mut b_rev = [0i16; MAX_LPC_ORDER];
    for k in 0..d {
        b_rev[d - 1 - k] = b[k];
    }

    for ix in d..len {
        let s = silk_inner_prod_aligned(&input[ix - d..ix], &b_rev[..d], d);
        let out32_q12 = ((input[ix] as i32) << 12).wrapping_sub(s);
        out[ix] = silk_sat16(silk_rshift_round(out32_q12, 12)) as i16;
    }
}

pub fn silk_scale_copy_vector16(
    data_out: &mut [i16],
    data_in: &[i16],
    gain_q16: i32,
    data_size: usize,
) {
    for i in 0..data_size {
        let tmp32 = silk_smulwb(gain_q16, data_in[i] as i32);
        data_out[i] = silk_sat16(tmp32) as i16;
    }
}

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn silk_inner_prod_aligned_avx2(ptr1: &[i16], ptr2: &[i16], len: usize) -> i32 {
    // The loads below walk `len` elements of each input; trimming to `len`
    // here is what puts them in bounds.
    let ptr1 = &ptr1[..len];
    let ptr2 = &ptr2[..len];

    let mut acc0 = _mm256_setzero_si256();
    let mut acc1 = _mm256_setzero_si256();
    let mut i = 0;

    while i + 32 <= len {
        let a0 = _mm256_loadu_si256(ptr1.as_ptr().add(i) as *const __m256i);
        let b0 = _mm256_loadu_si256(ptr2.as_ptr().add(i) as *const __m256i);
        let a1 = _mm256_loadu_si256(ptr1.as_ptr().add(i + 16) as *const __m256i);
        let b1 = _mm256_loadu_si256(ptr2.as_ptr().add(i + 16) as *const __m256i);

        acc0 = _mm256_add_epi32(acc0, _mm256_madd_epi16(a0, b0));
        acc1 = _mm256_add_epi32(acc1, _mm256_madd_epi16(a1, b1));
        i += 32;
    }
    while i + 16 <= len {
        let a = _mm256_loadu_si256(ptr1.as_ptr().add(i) as *const __m256i);
        let b = _mm256_loadu_si256(ptr2.as_ptr().add(i) as *const __m256i);
        acc0 = _mm256_add_epi32(acc0, _mm256_madd_epi16(a, b));
        i += 16;
    }

    let acc = _mm256_add_epi32(acc0, acc1);
    let hi = _mm256_extracti128_si256(acc, 1);
    let lo = _mm256_castsi256_si128(acc);
    let sum4 = _mm_add_epi32(lo, hi);
    let sum2 = _mm_add_epi32(sum4, _mm_srli_si128(sum4, 8));
    let sum1 = _mm_add_epi32(sum2, _mm_srli_si128(sum2, 4));
    let mut result = _mm_cvtsi128_si32(sum1);

    if i + 8 <= len {
        let a = _mm_loadu_si128(ptr1.as_ptr().add(i) as *const __m128i);
        let b = _mm_loadu_si128(ptr2.as_ptr().add(i) as *const __m128i);
        let p = _mm_madd_epi16(a, b);
        let p2 = _mm_add_epi32(p, _mm_srli_si128(p, 8));
        let p1 = _mm_add_epi32(p2, _mm_srli_si128(p2, 4));
        result = result.wrapping_add(_mm_cvtsi128_si32(p1));
        i += 8;
    }

    while i < len {
        result = result.wrapping_add((ptr1[i] as i32).wrapping_mul(ptr2[i] as i32));
        i += 1;
    }

    result
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn xcorr_kernel_avx2(x: &[i16], y: &[i16], sum: &mut [i32; 4], len: usize) {
    // Callers owe `len` taps and `len + 3` history samples: tap `j` is
    // correlated against `y[j ..= j + 3]`.
    let x = &x[..len];
    assert!(
        y.len() >= len + 3,
        "xcorr kernel: y holds {} samples, needs len + 3 = {}",
        y.len(),
        len + 3
    );

    let mut acc = _mm_setzero_si128();
    let mut i = 0;

    // `_mm_loadu_si128` takes eight `i16` where a step uses seven, so the last
    // full block would read one past `y[len + 2]` when the caller supplied
    // exactly `len + 3`. Stop the vector path when that eighth lane is not
    // there and let the scalar loop below finish the taps.
    while i + 4 <= len && i + 8 <= y.len() {
        let x4 = _mm_loadl_epi64(x.as_ptr().add(i) as *const __m128i);
        let xdup = _mm_unpacklo_epi64(x4, x4);

        let y8 = _mm_loadu_si128(y.as_ptr().add(i) as *const __m128i);
        let y1 = _mm_bsrli_si128(y8, 2);
        let y2 = _mm_bsrli_si128(y8, 4);
        let y3 = _mm_bsrli_si128(y8, 6);

        let y4_01 = _mm_unpacklo_epi64(y8, y1);

        let y4_23 = _mm_unpacklo_epi64(y2, y3);

        let p01 = _mm_madd_epi16(xdup, y4_01);
        let p23 = _mm_madd_epi16(xdup, y4_23);

        acc = _mm_add_epi32(acc, _mm_hadd_epi32(p01, p23));

        i += 4;
    }

    let mut tmp = [0i32; 4];
    _mm_storeu_si128(tmp.as_mut_ptr() as *mut __m128i, acc);
    sum[0] = sum[0].wrapping_add(tmp[0]);
    sum[1] = sum[1].wrapping_add(tmp[1]);
    sum[2] = sum[2].wrapping_add(tmp[2]);
    sum[3] = sum[3].wrapping_add(tmp[3]);

    let mut y_0 = y[i];
    let mut y_1 = y[i + 1];
    let mut y_2 = y[i + 2];
    for j in i..len {
        let y_3 = y[j + 3];
        let xv = x[j] as i32;
        sum[0] = sum[0].wrapping_add(xv.wrapping_mul(y_0 as i32));
        sum[1] = sum[1].wrapping_add(xv.wrapping_mul(y_1 as i32));
        sum[2] = sum[2].wrapping_add(xv.wrapping_mul(y_2 as i32));
        sum[3] = sum[3].wrapping_add(xv.wrapping_mul(y_3 as i32));
        y_0 = y_1;
        y_1 = y_2;
        y_2 = y_3;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn silk_sum_sqr_shift_avx2(x: &[i16], len: usize, shft: i32) -> i32 {
    // The loads below walk `len` samples; trimming to `len` here is what puts
    // them in bounds.
    let x = &x[..len];

    let mut acc = _mm256_setzero_si256();
    let mut i = 0;

    while i + 16 <= len {
        let v = _mm256_loadu_si256(x.as_ptr().add(i) as *const __m256i);

        let sq = _mm256_madd_epi16(v, v);

        let vshift = _mm256_set1_epi32(shft);
        let shifted = if shft > 0 {
            _mm256_srlv_epi32(sq, vshift)
        } else {
            sq
        };
        acc = _mm256_add_epi32(acc, shifted);
        i += 16;
    }

    let hi = _mm256_extracti128_si256(acc, 1);
    let lo = _mm256_castsi256_si128(acc);
    let sum4 = _mm_add_epi32(lo, hi);
    let sum2 = _mm_add_epi32(sum4, _mm_srli_si128(sum4, 8));
    let sum1 = _mm_add_epi32(sum2, _mm_srli_si128(sum2, 4));
    let mut nrg = _mm_cvtsi128_si32(sum1);

    // `_mm256_madd_epi16` above already sums each pair before the shift, which
    // is what the reference does; the tail has to keep that pairing. Walking it
    // one sample at a time — as this kernel used to — truncates once per sample
    // instead of once per pair and drifts low by roughly half an LSB per extra
    // sample. SILK is fixed point: that difference reaches the bitstream. The
    // vector loop consumes 16 at a time, so `i` is even here and these pairs
    // line up with the reference's.
    while i + 1 < len {
        let a = x[i] as i32;
        let b = x[i + 1] as i32;
        let sq = ((a * a) as u32).wrapping_add((b * b) as u32);
        nrg = nrg.wrapping_add((sq >> shft) as i32);
        i += 2;
    }
    if i < len {
        let v = x[i] as i32;
        let sq = (v * v) as u32;
        nrg = nrg.wrapping_add((sq >> shft) as i32);
    }

    nrg
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn silk_lpc_analysis_filter_avx2(
    out: &mut [i16],
    input: &[i16],
    b: &[i16],
    len: usize,
    d: usize,
) {
    // The filter reads `input[ix - d ..= ix]` and writes `out[ix]` for
    // `ix < len`, against `d` taps; trimming all three here is what puts the
    // loads below in bounds.
    let input = &input[..len];
    let out = &mut out[..len];
    let b = &b[..d];

    debug_assert!(d >= 6 && (d & 1) == 0 && d <= 16);

    let mut b_rev_pad = [0i16; 16];
    for k in 0..d {
        b_rev_pad[15 - k] = b[k];
    }

    for v in out[..d].iter_mut() {
        *v = 0;
    }

    let avx_start = 16usize.max(d);
    for ix in d..avx_start.min(len) {
        let mut s: i32 = 0;
        for k in 0..d {
            s = s.wrapping_add((input[ix - 1 - k] as i32).wrapping_mul(b[k] as i32));
        }
        let out32_q12 = ((input[ix] as i32) << 12).wrapping_sub(s);
        out[ix] = silk_sat16(silk_rshift_round(out32_q12, 12)) as i16;
    }

    if avx_start >= len {
        return;
    }

    let b_lo = _mm256_cvtepi16_epi32(_mm_loadu_si128(b_rev_pad.as_ptr() as *const __m128i));
    let b_hi = _mm256_cvtepi16_epi32(_mm_loadu_si128(b_rev_pad.as_ptr().add(8) as *const __m128i));

    for ix in avx_start..len {
        let base = input.as_ptr().add(ix - 16);
        let inp_lo = _mm256_cvtepi16_epi32(_mm_loadu_si128(base as *const __m128i));
        let inp_hi = _mm256_cvtepi16_epi32(_mm_loadu_si128(base.add(8) as *const __m128i));

        let prod_lo = _mm256_mullo_epi32(inp_lo, b_lo);
        let prod_hi = _mm256_mullo_epi32(inp_hi, b_hi);

        let sum256 = _mm256_add_epi32(prod_lo, prod_hi);
        let hi128 = _mm256_extracti128_si256(sum256, 1);
        let lo128 = _mm256_castsi256_si128(sum256);
        let sum128 = _mm_add_epi32(lo128, hi128);
        let sum64 = _mm_add_epi32(sum128, _mm_srli_si128(sum128, 8));
        let sum32 = _mm_add_epi32(sum64, _mm_srli_si128(sum64, 4));
        let s = _mm_cvtsi128_si32(sum32);

        let out32_q12 = ((input[ix] as i32) << 12).wrapping_sub(s);
        out[ix] = silk_sat16(silk_rshift_round(out32_q12, 12)) as i16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lpc_analysis_filter_ref(out: &mut [i16], input: &[i16], b: &[i16], len: usize, d: usize) {
        out[..d].fill(0);
        for ix in d..len {
            let mut s = 0i32;
            for k in 0..d {
                s = s.wrapping_add((input[ix - 1 - k] as i32).wrapping_mul(b[k] as i32));
            }
            let out32_q12 = ((input[ix] as i32) << 12).wrapping_sub(s);
            out[ix] = silk_sat16(silk_rshift_round(out32_q12, 12)) as i16;
        }
    }

    fn make_signal(len: usize, seed: u32) -> Vec<i16> {
        let mut s = seed;
        let mut out = vec![0i16; len];
        for v in &mut out {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (s >> 16) as i16;
        }
        out
    }

    #[test]
    fn test_silk_lpc_analysis_filter_matches_reference() {
        for d in (6usize..=16).step_by(2) {
            for &len in &[d + 8, d + 31, 160, 320] {
                let input = make_signal(len, (d as u32) * 17 + len as u32);
                let b = make_signal(d, 0x1234_5678 ^ d as u32);

                let mut out_opt = vec![0i16; len];
                let mut out_ref = vec![0i16; len];

                silk_lpc_analysis_filter(&mut out_opt, &input, &b, len, d, 0);
                lpc_analysis_filter_ref(&mut out_ref, &input, &b, len, d);

                assert_eq!(out_opt, out_ref, "mismatch for d={d}, len={len}");
            }
        }
    }
}

/// The SILK kernels are fixed-point, so unlike the CELT ones they must agree
/// with their scalar definitions *exactly* — a single-LSB difference changes
/// the bitstream. The AVX2 twins already have tests; these cover the NEON ones
/// and the dispatchers on every target.
#[cfg(test)]
mod simd_tests {
    use super::*;

    fn i16_noise(n: usize, seed: u32) -> Vec<i16> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((s >> 12) as i32 % 65536 - 32768) as i16
            })
            .collect()
    }

    #[test]
    fn simd_inner_prod_matches_the_scalar_definition() {
        for &len in &[0usize, 1, 3, 4, 7, 8, 15, 16, 31, 32, 100, 480] {
            let a = i16_noise(len, 0xA1 ^ len as u32);
            let b = i16_noise(len, 0xB1 ^ len as u32);
            let got = silk_inner_prod_aligned(&a, &b, len);
            let want = silk_inner_prod_aligned_scalar(&a, &b, len);
            assert_eq!(got, want, "silk_inner_prod_aligned len={len}");
        }
    }

    #[test]
    fn simd_sum_sqr_shift_matches_the_scalar_definition() {
        for &len in &[1usize, 3, 4, 8, 15, 16, 31, 64, 480] {
            for &amp in &[1i32, 100, 10_000, 32_767] {
                let x: Vec<i16> = i16_noise(len, 0xC1 ^ len as u32)
                    .iter()
                    .map(|v| ((*v as i32 * amp) / 32_768) as i16)
                    .collect();

                let (mut energy, mut shift) = (0i32, 0i32);
                silk_sum_sqr_shift(&mut energy, &mut shift, &x, len);

                // libopus `silk_sum_sqr_shift` shifts each *pair* sum rather
                // than the total, so the truncation accumulates per pair. The
                // scalar branch of the function above is the definition; a
                // vectorised path that accumulates in wider precision and
                // shifts once at the end would be more accurate but would not
                // be the same number, and the bitstream depends on it.
                let mut want = 0i32;
                let mut i = 0usize;
                while i + 1 < len {
                    let t = (x[i] as i32 * x[i] as i32) as u32;
                    let t = t.wrapping_add((x[i + 1] as i32 * x[i + 1] as i32) as u32);
                    want = want.wrapping_add((t >> shift) as i32);
                    i += 2;
                }
                if i < len {
                    let t = (x[i] as i32 * x[i] as i32) as u32;
                    want = want.wrapping_add((t >> shift) as i32);
                }
                assert_eq!(
                    energy, want,
                    "silk_sum_sqr_shift len={len} amp={amp} shift={shift}"
                );
            }
        }
    }

    #[test]
    fn simd_xcorr_kernel_matches_the_scalar_definition() {
        for &len in &[1usize, 2, 4, 5, 8, 12, 16, 33, 120] {
            let x = i16_noise(len, 0xD1 ^ len as u32);
            let y = i16_noise(len + 3, 0xE1 ^ len as u32);

            let mut want = [0i32; 4];
            xcorr_kernel_scalar(&x, &y, &mut want, len);

            let mut got = [0i32; 4];
            xcorr_kernel_c(&x, &y, &mut got, len);
            assert_eq!(got, want, "xcorr_kernel len={len}");
        }
    }

    #[test]
    fn simd_warped_corr_update_matches_the_scalar_definition() {
        for &order in &[2usize, 4, 6, 8, 16, 24] {
            let state: Vec<i32> = i16_noise(order + 1, 0xF1 ^ order as u32)
                .iter()
                .map(|v| *v as i32 * 4096)
                .collect();
            let seed: Vec<i64> = (0..order + 1).map(|i| (i as i64) * 7919 - 1000).collect();

            let mut got = seed.clone();
            warped_corr_update(&mut got, &state, order);
            let mut want = seed.clone();
            warped_corr_update_scalar(&mut want, &state, order);
            assert_eq!(got, want, "warped_corr_update order={order}");
        }
    }

    #[test]
    fn simd_lpc_analysis_filter_matches_the_scalar_definition() {
        for &order in &[8usize, 10, 12, 16] {
            for &len in &[order + 1, 32, 97, 480] {
                let x = i16_noise(len, 0x1A ^ len as u32);
                let b: Vec<i16> = i16_noise(order, 0x2A ^ order as u32)
                    .iter()
                    .map(|v| (*v as i32 / 8) as i16)
                    .collect();

                let mut got = vec![0i16; len];
                silk_lpc_analysis_filter(&mut got, &x, &b, len, order, 0);
                let mut want = vec![0i16; len];
                silk_lpc_analysis_filter_scalar(&mut want, &x, &b, len, order);
                assert_eq!(
                    got, want,
                    "silk_lpc_analysis_filter order={order} len={len}"
                );
            }
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use super::have_avx_fma;
use crate::celt::lpc::{autocorr, lpc};

pub fn inner_prod(x: &[f32], y: &[f32], n: usize) -> f32 {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    unsafe {
        if have_avx_fma() {
            return inner_prod_avx(x, y, n);
        }
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        inner_prod_neon(x, y, n)
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
    unsafe {
        inner_prod_sse(x, y, n)
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", target_feature = "sse")
    )))]
    {
        let mut sum = 0.0f32;
        for i in 0..n {
            sum += x[i] * y[i];
        }
        sum
    }
}

pub fn dual_inner_prod(x: &[f32], y1: &[f32], y2: &[f32], n: usize) -> (f32, f32) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    unsafe {
        if have_avx_fma() {
            return dual_inner_prod_avx(x, y1, y2, n);
        }
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        dual_inner_prod_neon(x, y1, y2, n)
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
    unsafe {
        dual_inner_prod_sse(x, y1, y2, n)
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", target_feature = "sse")
    )))]
    {
        let mut xy1 = 0.0f32;
        let mut xy2 = 0.0f32;
        for i in 0..n {
            xy1 += x[i] * y1[i];
            xy2 += x[i] * y2[i];
        }
        (xy1, xy2)
    }
}

pub fn pitch_xcorr(x: &[f32], y: &[f32], xcorr: &mut [f32], len: usize, max_pitch: usize) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    unsafe {
        if have_avx_fma() {
            return pitch_xcorr_avx(x, y, xcorr, len, max_pitch);
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if max_pitch >= 32 {
            unsafe {
                return pitch_xcorr_neon(x, y, xcorr, len, max_pitch);
            }
        }
        for i in 0..max_pitch {
            xcorr[i] = unsafe { inner_prod_neon(x, &y[i..], len) };
        }
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
    unsafe {
        pitch_xcorr_sse(x, y, xcorr, len, max_pitch)
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", target_feature = "sse")
    )))]
    {
        for i in 0..max_pitch {
            xcorr[i] = inner_prod(x, &y[i..], len);
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn inner_prod_neon(x: &[f32], y: &[f32], n: usize) -> f32 {
    use std::arch::aarch64::*;

    // The loads below walk `n` elements of each input; trimming to `n` here is
    // what puts them in bounds.
    let x = &x[..n];
    let y = &y[..n];

    let mut xy = vdupq_n_f32(0.0);
    let mut i = 0;

    while i + 8 <= n {
        let x0 = vld1q_f32(x.as_ptr().add(i));
        let y0 = vld1q_f32(y.as_ptr().add(i));
        xy = vfmaq_f32(xy, x0, y0);

        let x1 = vld1q_f32(x.as_ptr().add(i + 4));
        let y1 = vld1q_f32(y.as_ptr().add(i + 4));
        xy = vfmaq_f32(xy, x1, y1);

        i += 8;
    }

    if i + 4 <= n {
        let x0 = vld1q_f32(x.as_ptr().add(i));
        let y0 = vld1q_f32(y.as_ptr().add(i));
        xy = vfmaq_f32(xy, x0, y0);
        i += 4;
    }

    let mut sum = vaddvq_f32(xy);

    for j in i..n {
        sum += x[j] * y[j];
    }

    sum
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn dual_inner_prod_neon(x: &[f32], y1: &[f32], y2: &[f32], n: usize) -> (f32, f32) {
    use std::arch::aarch64::*;

    // The loads below walk `n` elements of each input; trimming to `n` here is
    // what puts them in bounds.
    let x = &x[..n];
    let y1 = &y1[..n];
    let y2 = &y2[..n];

    let mut xy1 = vdupq_n_f32(0.0);
    let mut xy2 = vdupq_n_f32(0.0);
    let mut i = 0;

    while i + 8 <= n {
        let x0 = vld1q_f32(x.as_ptr().add(i));
        let x4 = vld1q_f32(x.as_ptr().add(i + 4));

        let y1_0 = vld1q_f32(y1.as_ptr().add(i));
        let y1_4 = vld1q_f32(y1.as_ptr().add(i + 4));
        let y2_0 = vld1q_f32(y2.as_ptr().add(i));
        let y2_4 = vld1q_f32(y2.as_ptr().add(i + 4));

        xy1 = vfmaq_f32(xy1, x0, y1_0);
        xy2 = vfmaq_f32(xy2, x0, y2_0);
        xy1 = vfmaq_f32(xy1, x4, y1_4);
        xy2 = vfmaq_f32(xy2, x4, y2_4);

        i += 8;
    }

    if i + 4 <= n {
        let x0 = vld1q_f32(x.as_ptr().add(i));
        let y1_0 = vld1q_f32(y1.as_ptr().add(i));
        let y2_0 = vld1q_f32(y2.as_ptr().add(i));
        xy1 = vfmaq_f32(xy1, x0, y1_0);
        xy2 = vfmaq_f32(xy2, x0, y2_0);
        i += 4;
    }

    let sum1 = vaddvq_f32(xy1);
    let sum2 = vaddvq_f32(xy2);

    let mut s1 = sum1;
    let mut s2 = sum2;
    for j in i..n {
        s1 += x[j] * y1[j];
        s2 += x[j] * y2[j];
    }

    (s1, s2)
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn xcorr_kernel_neon(x: &[f32], y: &[f32], sum: &mut [f32; 4], mut len: usize) {
    use std::arch::aarch64::*;

    if len == 0 {
        *sum = [0.0; 4];
        return;
    }

    // The kernel consumes `len` taps from `x` and, for each of them, a
    // four-wide window of `y`, so the last window reaches `y[len + 2]`.
    // Trimming to those lengths is what puts the pointer walk below in bounds.
    let x = &x[..len];
    let y = &y[..len + 3];

    let mut summ = vdupq_n_f32(0.0);
    let mut xi = x.as_ptr();
    let mut yi = y.as_ptr();

    let mut yy = vld1q_f32(yi);

    while len > 8 {
        yi = yi.add(4);
        let yy1 = vld1q_f32(yi);
        yi = yi.add(4);
        let yy2 = vld1q_f32(yi);

        let xx0 = vld1q_f32(xi);
        xi = xi.add(4);
        let xx1 = vld1q_f32(xi);
        xi = xi.add(4);

        summ = vfmaq_lane_f32(summ, yy, vget_low_f32(xx0), 0);
        let yext = vextq_f32(yy, yy1, 1);
        summ = vfmaq_lane_f32(summ, yext, vget_low_f32(xx0), 1);
        let yext = vextq_f32(yy, yy1, 2);
        summ = vfmaq_lane_f32(summ, yext, vget_high_f32(xx0), 0);
        let yext = vextq_f32(yy, yy1, 3);
        summ = vfmaq_lane_f32(summ, yext, vget_high_f32(xx0), 1);

        summ = vfmaq_lane_f32(summ, yy1, vget_low_f32(xx1), 0);
        let yext = vextq_f32(yy1, yy2, 1);
        summ = vfmaq_lane_f32(summ, yext, vget_low_f32(xx1), 1);
        let yext = vextq_f32(yy1, yy2, 2);
        summ = vfmaq_lane_f32(summ, yext, vget_high_f32(xx1), 0);
        let yext = vextq_f32(yy1, yy2, 3);
        summ = vfmaq_lane_f32(summ, yext, vget_high_f32(xx1), 1);

        yy = yy2;
        len -= 8;
    }

    if len > 4 {
        yi = yi.add(4);
        let yy1 = vld1q_f32(yi);

        let xx0 = vld1q_f32(xi);
        xi = xi.add(4);

        summ = vfmaq_lane_f32(summ, yy, vget_low_f32(xx0), 0);
        let yext = vextq_f32(yy, yy1, 1);
        summ = vfmaq_lane_f32(summ, yext, vget_low_f32(xx0), 1);
        let yext = vextq_f32(yy, yy1, 2);
        summ = vfmaq_lane_f32(summ, yext, vget_high_f32(xx0), 0);
        let yext = vextq_f32(yy, yy1, 3);
        summ = vfmaq_lane_f32(summ, yext, vget_high_f32(xx0), 1);

        yy = yy1;
        len -= 4;
    }

    while len > 1 {
        let xx = vld1_dup_f32(xi);
        xi = xi.add(1);
        summ = vfmaq_lane_f32(summ, yy, xx, 0);
        yi = yi.add(1);
        yy = vld1q_f32(yi);
        len -= 1;
    }

    let xx = vld1_dup_f32(xi);
    summ = vfmaq_lane_f32(summ, yy, xx, 0);

    vst1q_f32(sum.as_mut_ptr(), summ);
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn pitch_xcorr_neon(x: &[f32], y: &[f32], xcorr: &mut [f32], len: usize, max_pitch: usize) {
    debug_assert!(x.len() >= len, "pitch_xcorr_neon: x too short");
    debug_assert!(
        y.len() >= max_pitch + len - 1,
        "pitch_xcorr_neon: y too short"
    );
    debug_assert!(
        xcorr.len() >= max_pitch,
        "pitch_xcorr_neon: xcorr too short"
    );

    let mut i = 0;

    while i + 4 <= max_pitch {
        let mut sum = [0.0f32; 4];
        unsafe { xcorr_kernel_neon(x, &y[i..], &mut sum, len) };
        xcorr[i] = sum[0];
        xcorr[i + 1] = sum[1];
        xcorr[i + 2] = sum[2];
        xcorr[i + 3] = sum[3];
        i += 4;
    }

    for j in i..max_pitch {
        xcorr[j] = unsafe { inner_prod_neon(x, &y[j..], len) };
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn inner_prod_sse(x: &[f32], y: &[f32], n: usize) -> f32 {
    use std::arch::x86_64::*;

    // The loads below walk `n` elements of each input; trimming to `n` here is
    // what puts them in bounds.
    let x = &x[..n];
    let y = &y[..n];

    let mut sum0 = _mm_setzero_ps();
    let mut sum1 = _mm_setzero_ps();
    let mut i = 0;

    while i + 8 <= n {
        let x0 = _mm_loadu_ps(x.as_ptr().add(i));
        let y0 = _mm_loadu_ps(y.as_ptr().add(i));
        sum0 = _mm_add_ps(sum0, _mm_mul_ps(x0, y0));
        let x1 = _mm_loadu_ps(x.as_ptr().add(i + 4));
        let y1 = _mm_loadu_ps(y.as_ptr().add(i + 4));
        sum1 = _mm_add_ps(sum1, _mm_mul_ps(x1, y1));
        i += 8;
    }

    if i + 4 <= n {
        let x0 = _mm_loadu_ps(x.as_ptr().add(i));
        let y0 = _mm_loadu_ps(y.as_ptr().add(i));
        sum0 = _mm_add_ps(sum0, _mm_mul_ps(x0, y0));
        i += 4;
    }

    let sum = _mm_add_ps(sum0, sum1);
    let tmp = _mm_movehl_ps(sum, sum);
    let sum = _mm_add_ps(sum, tmp);
    let tmp2 = _mm_shuffle_ps(sum, sum, 0x55);
    let sum = _mm_add_ss(sum, tmp2);
    let mut result = _mm_cvtss_f32(sum);

    while i < n {
        result += x[i] * y[i];
        i += 1;
    }

    result
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn dual_inner_prod_sse(x: &[f32], y1: &[f32], y2: &[f32], n: usize) -> (f32, f32) {
    use std::arch::x86_64::*;

    // The loads below walk `n` elements of each input; trimming to `n` here is
    // what puts them in bounds.
    let x = &x[..n];
    let y1 = &y1[..n];
    let y2 = &y2[..n];

    let mut xy1 = _mm_setzero_ps();
    let mut xy2 = _mm_setzero_ps();
    let mut i = 0;

    while i + 4 <= n {
        let xi = _mm_loadu_ps(x.as_ptr().add(i));
        let y1i = _mm_loadu_ps(y1.as_ptr().add(i));
        let y2i = _mm_loadu_ps(y2.as_ptr().add(i));
        xy1 = _mm_add_ps(xy1, _mm_mul_ps(xi, y1i));
        xy2 = _mm_add_ps(xy2, _mm_mul_ps(xi, y2i));
        i += 4;
    }

    let tmp = _mm_movehl_ps(xy1, xy1);
    let xy1 = _mm_add_ps(xy1, tmp);
    let tmp2 = _mm_shuffle_ps(xy1, xy1, 0x55);
    let xy1 = _mm_add_ss(xy1, tmp2);
    let mut s1 = _mm_cvtss_f32(xy1);

    let tmp = _mm_movehl_ps(xy2, xy2);
    let xy2 = _mm_add_ps(xy2, tmp);
    let tmp2 = _mm_shuffle_ps(xy2, xy2, 0x55);
    let xy2 = _mm_add_ss(xy2, tmp2);
    let mut s2 = _mm_cvtss_f32(xy2);

    while i < n {
        s1 += x[i] * y1[i];
        s2 += x[i] * y2[i];
        i += 1;
    }

    (s1, s2)
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn xcorr_kernel_sse(x: &[f32], y: &[f32], sum: &mut [f32; 4], len: usize) {
    use std::arch::x86_64::*;

    // Four taps of `x` per step, each against a four-wide window of `y` that
    // starts three elements later, so the reads reach `x[len - 1]` and
    // `y[len + 2]`.
    let x = &x[..len];
    let y = &y[..len + 3];

    let mut xsum1 = _mm_loadu_ps(sum.as_ptr());
    let mut xsum2 = _mm_setzero_ps();

    let mut j = 0;

    while j + 4 <= len {
        let x0 = _mm_loadu_ps(x.as_ptr().add(j));
        let yj = _mm_loadu_ps(y.as_ptr().add(j));
        let y3 = _mm_loadu_ps(y.as_ptr().add(j + 3));

        xsum1 = _mm_add_ps(xsum1, _mm_mul_ps(_mm_shuffle_ps(x0, x0, 0x00), yj));

        xsum2 = _mm_add_ps(
            xsum2,
            _mm_mul_ps(_mm_shuffle_ps(x0, x0, 0x55), _mm_shuffle_ps(yj, y3, 0x49)),
        );

        xsum1 = _mm_add_ps(
            xsum1,
            _mm_mul_ps(_mm_shuffle_ps(x0, x0, 0xaa), _mm_shuffle_ps(yj, y3, 0x9e)),
        );

        xsum2 = _mm_add_ps(xsum2, _mm_mul_ps(_mm_shuffle_ps(x0, x0, 0xff), y3));

        j += 4;
    }

    if j < len {
        xsum1 = _mm_add_ps(
            xsum1,
            _mm_mul_ps(
                _mm_set1_ps(*x.as_ptr().add(j)),
                _mm_loadu_ps(y.as_ptr().add(j)),
            ),
        );
        j += 1;
        if j < len {
            xsum2 = _mm_add_ps(
                xsum2,
                _mm_mul_ps(
                    _mm_set1_ps(*x.as_ptr().add(j)),
                    _mm_loadu_ps(y.as_ptr().add(j)),
                ),
            );
            j += 1;
            if j < len {
                xsum1 = _mm_add_ps(
                    xsum1,
                    _mm_mul_ps(
                        _mm_set1_ps(*x.as_ptr().add(j)),
                        _mm_loadu_ps(y.as_ptr().add(j)),
                    ),
                );
            }
        }
    }

    _mm_storeu_ps(sum.as_mut_ptr(), _mm_add_ps(xsum1, xsum2));
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn pitch_xcorr_sse(x: &[f32], y: &[f32], xcorr: &mut [f32], len: usize, max_pitch: usize) {
    let mut i = 0;

    while i + 4 <= max_pitch {
        let mut sum = [0.0f32; 4];
        xcorr_kernel_sse(x, &y[i..], &mut sum, len);
        xcorr[i] = sum[0];
        xcorr[i + 1] = sum[1];
        xcorr[i + 2] = sum[2];
        xcorr[i + 3] = sum[3];
        i += 4;
    }

    for j in i..max_pitch {
        xcorr[j] = inner_prod_sse(x, &y[j..], len);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx,fma")]
unsafe fn inner_prod_avx(x: &[f32], y: &[f32], n: usize) -> f32 {
    use std::arch::x86_64::*;

    // The loads below walk `n` elements of each input; trimming to `n` here is
    // what puts them in bounds.
    let x = &x[..n];
    let y = &y[..n];

    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();
    let mut i = 0usize;

    while i + 16 <= n {
        let x0 = _mm256_loadu_ps(x.as_ptr().add(i));
        let y0 = _mm256_loadu_ps(y.as_ptr().add(i));
        acc0 = _mm256_fmadd_ps(x0, y0, acc0);

        let x1 = _mm256_loadu_ps(x.as_ptr().add(i + 8));
        let y1 = _mm256_loadu_ps(y.as_ptr().add(i + 8));
        acc1 = _mm256_fmadd_ps(x1, y1, acc1);
        i += 16;
    }

    while i + 8 <= n {
        let x0 = _mm256_loadu_ps(x.as_ptr().add(i));
        let y0 = _mm256_loadu_ps(y.as_ptr().add(i));
        acc0 = _mm256_fmadd_ps(x0, y0, acc0);
        i += 8;
    }

    let acc = _mm256_add_ps(acc0, acc1);
    let hi = _mm256_extractf128_ps(acc, 1);
    let lo = _mm256_castps256_ps128(acc);
    let sum4 = _mm_add_ps(lo, hi);
    let tmp = _mm_movehl_ps(sum4, sum4);
    let sum2 = _mm_add_ps(sum4, tmp);
    let tmp2 = _mm_shuffle_ps(sum2, sum2, 0x55);
    let mut result = _mm_cvtss_f32(_mm_add_ss(sum2, tmp2));

    while i < n {
        result += x[i] * y[i];
        i += 1;
    }

    result
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx,fma")]
unsafe fn dual_inner_prod_avx(x: &[f32], y1: &[f32], y2: &[f32], n: usize) -> (f32, f32) {
    use std::arch::x86_64::*;

    // The loads below walk `n` elements of each input; trimming to `n` here is
    // what puts them in bounds.
    let x = &x[..n];
    let y1 = &y1[..n];
    let y2 = &y2[..n];

    let mut acc1 = _mm256_setzero_ps();
    let mut acc2 = _mm256_setzero_ps();
    let mut acc1b = _mm256_setzero_ps();
    let mut acc2b = _mm256_setzero_ps();
    let mut i = 0usize;

    while i + 16 <= n {
        let xv0 = _mm256_loadu_ps(x.as_ptr().add(i));
        let xv1 = _mm256_loadu_ps(x.as_ptr().add(i + 8));
        let y1v0 = _mm256_loadu_ps(y1.as_ptr().add(i));
        let y2v0 = _mm256_loadu_ps(y2.as_ptr().add(i));
        let y1v1 = _mm256_loadu_ps(y1.as_ptr().add(i + 8));
        let y2v1 = _mm256_loadu_ps(y2.as_ptr().add(i + 8));
        acc1 = _mm256_fmadd_ps(xv0, y1v0, acc1);
        acc2 = _mm256_fmadd_ps(xv0, y2v0, acc2);
        acc1b = _mm256_fmadd_ps(xv1, y1v1, acc1b);
        acc2b = _mm256_fmadd_ps(xv1, y2v1, acc2b);
        i += 16;
    }

    while i + 8 <= n {
        let xv = _mm256_loadu_ps(x.as_ptr().add(i));
        let y1v = _mm256_loadu_ps(y1.as_ptr().add(i));
        let y2v = _mm256_loadu_ps(y2.as_ptr().add(i));
        acc1 = _mm256_fmadd_ps(xv, y1v, acc1);
        acc2 = _mm256_fmadd_ps(xv, y2v, acc2);
        i += 8;
    }

    let acc1 = _mm256_add_ps(acc1, acc1b);
    let acc2 = _mm256_add_ps(acc2, acc2b);

    let hi1 = _mm256_extractf128_ps(acc1, 1);
    let lo1 = _mm256_castps256_ps128(acc1);
    let sum41 = _mm_add_ps(lo1, hi1);
    let t11 = _mm_movehl_ps(sum41, sum41);
    let s21 = _mm_add_ps(sum41, t11);
    let t12 = _mm_shuffle_ps(s21, s21, 0x55);
    let mut s1 = _mm_cvtss_f32(_mm_add_ss(s21, t12));

    let hi2 = _mm256_extractf128_ps(acc2, 1);
    let lo2 = _mm256_castps256_ps128(acc2);
    let sum42 = _mm_add_ps(lo2, hi2);
    let t21 = _mm_movehl_ps(sum42, sum42);
    let s22 = _mm_add_ps(sum42, t21);
    let t22 = _mm_shuffle_ps(s22, s22, 0x55);
    let mut s2 = _mm_cvtss_f32(_mm_add_ss(s22, t22));

    while i < n {
        s1 += x[i] * y1[i];
        s2 += x[i] * y2[i];
        i += 1;
    }

    (s1, s2)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx,fma")]
unsafe fn pitch_xcorr_avx(x: &[f32], y: &[f32], xcorr: &mut [f32], len: usize, max_pitch: usize) {
    let mut i = 0;

    while i + 4 <= max_pitch {
        let mut sum = [0.0f32; 4];
        xcorr_kernel_avx(x, &y[i..], &mut sum, len);
        xcorr[i] = sum[0];
        xcorr[i + 1] = sum[1];
        xcorr[i + 2] = sum[2];
        xcorr[i + 3] = sum[3];
        i += 4;
    }

    for j in i..max_pitch {
        xcorr[j] = inner_prod_avx(x, &y[j..], len);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx,fma")]
unsafe fn xcorr_kernel_avx(x: &[f32], y: &[f32], sum: &mut [f32; 4], len: usize) {
    use std::arch::x86_64::*;

    // Four taps of `x` per step, each against a four-wide window of `y` that
    // starts three elements later, so the reads reach `x[len - 1]` and
    // `y[len + 2]`.
    let x = &x[..len];
    let y = &y[..len + 3];

    let mut xsum1 = _mm_loadu_ps(sum.as_ptr());
    let mut xsum2 = _mm_setzero_ps();

    let mut j = 0;

    while j + 8 <= len {
        let x0 = _mm_loadu_ps(x.as_ptr().add(j));
        let yj = _mm_loadu_ps(y.as_ptr().add(j));
        let y3 = _mm_loadu_ps(y.as_ptr().add(j + 3));

        xsum1 = _mm_fmadd_ps(_mm_shuffle_ps(x0, x0, 0x00), yj, xsum1);
        xsum2 = _mm_fmadd_ps(
            _mm_shuffle_ps(x0, x0, 0x55),
            _mm_shuffle_ps(yj, y3, 0x49),
            xsum2,
        );
        xsum1 = _mm_fmadd_ps(
            _mm_shuffle_ps(x0, x0, 0xaa),
            _mm_shuffle_ps(yj, y3, 0x9e),
            xsum1,
        );
        xsum2 = _mm_fmadd_ps(_mm_shuffle_ps(x0, x0, 0xff), y3, xsum2);

        let x1 = _mm_loadu_ps(x.as_ptr().add(j + 4));
        let yj4 = _mm_loadu_ps(y.as_ptr().add(j + 4));
        let y7 = _mm_loadu_ps(y.as_ptr().add(j + 7));

        xsum1 = _mm_fmadd_ps(_mm_shuffle_ps(x1, x1, 0x00), yj4, xsum1);
        xsum2 = _mm_fmadd_ps(
            _mm_shuffle_ps(x1, x1, 0x55),
            _mm_shuffle_ps(yj4, y7, 0x49),
            xsum2,
        );
        xsum1 = _mm_fmadd_ps(
            _mm_shuffle_ps(x1, x1, 0xaa),
            _mm_shuffle_ps(yj4, y7, 0x9e),
            xsum1,
        );
        xsum2 = _mm_fmadd_ps(_mm_shuffle_ps(x1, x1, 0xff), y7, xsum2);

        j += 8;
    }

    if j + 4 <= len {
        let x0 = _mm_loadu_ps(x.as_ptr().add(j));
        let yj = _mm_loadu_ps(y.as_ptr().add(j));
        let y3 = _mm_loadu_ps(y.as_ptr().add(j + 3));

        xsum1 = _mm_fmadd_ps(_mm_shuffle_ps(x0, x0, 0x00), yj, xsum1);
        xsum2 = _mm_fmadd_ps(
            _mm_shuffle_ps(x0, x0, 0x55),
            _mm_shuffle_ps(yj, y3, 0x49),
            xsum2,
        );
        xsum1 = _mm_fmadd_ps(
            _mm_shuffle_ps(x0, x0, 0xaa),
            _mm_shuffle_ps(yj, y3, 0x9e),
            xsum1,
        );
        xsum2 = _mm_fmadd_ps(_mm_shuffle_ps(x0, x0, 0xff), y3, xsum2);

        j += 4;
    }

    if j < len {
        xsum1 = _mm_fmadd_ps(
            _mm_set1_ps(*x.as_ptr().add(j)),
            _mm_loadu_ps(y.as_ptr().add(j)),
            xsum1,
        );
        j += 1;
        if j < len {
            xsum2 = _mm_fmadd_ps(
                _mm_set1_ps(*x.as_ptr().add(j)),
                _mm_loadu_ps(y.as_ptr().add(j)),
                xsum2,
            );
            j += 1;
            if j < len {
                xsum1 = _mm_fmadd_ps(
                    _mm_set1_ps(*x.as_ptr().add(j)),
                    _mm_loadu_ps(y.as_ptr().add(j)),
                    xsum1,
                );
            }
        }
    }

    _mm_storeu_ps(sum.as_mut_ptr(), _mm_add_ps(xsum1, xsum2));
}

/// libopus `celt_fir5`: the fixed order-5 FIR the pitch downsampler whitens
/// with. Scalar on every target — five taps do not repay vectorising.
fn celt_fir5(x: &mut [f32], num: &[f32], n: usize) {
    let mut mem = [0.0f32; 5];

    let num0 = num[0];
    let num1 = num[1];
    let num2 = num[2];
    let num3 = num[3];
    let num4 = num[4];

    for i in 0..n {
        let mut sum = x[i];
        sum += num0 * mem[0];
        sum += num1 * mem[1];
        sum += num2 * mem[2];
        sum += num3 * mem[3];
        sum += num4 * mem[4];

        mem[4] = mem[3];
        mem[3] = mem[2];
        mem[2] = mem[1];
        mem[1] = mem[0];
        mem[0] = x[i];

        x[i] = sum;
    }
}

pub fn pitch_downsample(x: &[&[f32]], x_lp: &mut [f32], len: usize, c: usize, factor: usize) {
    let offset = factor / 2;

    if x_lp.len() < len {
        return;
    }

    // Stage 1: the three-tap decimating filter. Only this part is vectorised.
    #[cfg(target_arch = "aarch64")]
    let taps_done = if factor == 2 && c <= 2 {
        pitch_downsample_taps_neon(x, x_lp, len, c, offset);
        true
    } else {
        false
    };
    #[cfg(not(target_arch = "aarch64"))]
    let taps_done = false;

    if !taps_done {
        for i in 1..len {
            let mut val = 0.0f32;
            for k in 0..c {
                let x_k = x[k];

                let idx_m = factor * i - offset;
                let idx_p = factor * i + offset;
                let idx_c = factor * i;

                if idx_p < x_k.len() {
                    val += 0.25 * x_k[idx_m] + 0.25 * x_k[idx_p] + 0.5 * x_k[idx_c];
                }
            }
            x_lp[i] = val;
        }
    }

    pitch_downsample_boundary(x, x_lp, c, offset);

    // Stage 2: the whitening libopus applies to the decimated buffer. It is
    // not an optimisation and it is not optional — the pitch search that reads
    // this buffer is tuned for a whitened signal.
    pitch_downsample_whiten(x_lp, len);
}

/// Order-4 LPC whitening with an added zero, applied in place to the decimated
/// buffer (libopus `pitch_downsample`, everything after the three-tap filter).
fn pitch_downsample_whiten(x_lp: &mut [f32], len: usize) {
    let mut ac = [0.0f32; 5];
    autocorr(&x_lp[0..len], &mut ac, None, 0, 4, len);

    ac[0] *= 1.0001;

    for i in 1..=4 {
        let f = 0.008 * (i as f32);
        ac[i] -= ac[i] * f * f;
    }

    let mut lpc_coeffs = [0.0f32; 4];
    lpc(&mut lpc_coeffs, &ac, 4);

    let mut tmp = 1.0f32;
    for coeff in lpc_coeffs.iter_mut() {
        tmp *= 0.9;
        *coeff *= tmp;
    }

    let c1 = 0.8f32;
    let lpc2 = [
        lpc_coeffs[0] + c1,
        lpc_coeffs[1] + c1 * lpc_coeffs[0],
        lpc_coeffs[2] + c1 * lpc_coeffs[1],
        lpc_coeffs[3] + c1 * lpc_coeffs[2],
        c1 * lpc_coeffs[3],
    ];

    celt_fir5(x_lp, &lpc2, len);
}

#[inline]
fn pitch_downsample_boundary(x: &[&[f32]], x_lp: &mut [f32], c: usize, offset: usize) {
    {
        let mut val = 0.0f32;
        for k in 0..c {
            let x_k = x[k];

            let idx_offset = offset;
            let idx_0 = 0;
            if idx_offset < x_k.len() {
                val += 0.25 * x_k[idx_offset] + 0.5 * x_k[idx_0];
            }
        }
        x_lp[0] = val;
    }
}

#[cfg(target_arch = "aarch64")]
fn pitch_downsample_taps_neon(x: &[&[f32]], x_lp: &mut [f32], len: usize, c: usize, offset: usize) {
    use std::arch::aarch64::*;

    unsafe {
        let v025 = vdupq_n_f32(0.25);
        let v05 = vdupq_n_f32(0.5);

        if c == 1 {
            let x0 = x[0];
            debug_assert!(x0.len() >= 2 * len);

            let mut i = 1;
            // Consecutive outputs are two input samples apart, so the three
            // taps have to be gathered with a stride of two: `vld2q` splits a
            // run of eight inputs into its even and odd halves, which is
            // exactly the pair this filter wants. A plain `vld1q` per tap
            // walks the input one sample per output instead of two, and only
            // the first lane of each group of four lands on the right sample.
            while i + 4 <= len {
                let p = x0.as_ptr();
                let mid = vld2q_f32(p.add(2 * i)); // .0 = x0[2i+2k], .1 = x0[2i+2k+1]
                let lo = vld2q_f32(p.add(2 * i - 2)); // .1 = x0[2i+2k-1]

                let mut val = vmulq_f32(lo.1, v025);
                val = vfmaq_f32(val, mid.1, v025);
                val = vfmaq_f32(val, mid.0, v05);

                vst1q_f32(x_lp.as_mut_ptr().add(i), val);
                i += 4;
            }

            while i < len {
                let idx_m = 2 * i - offset;
                let idx_p = 2 * i + offset;
                let idx_c = 2 * i;

                if idx_p < x0.len() {
                    x_lp[i] = 0.25 * x0[idx_m] + 0.25 * x0[idx_p] + 0.5 * x0[idx_c];
                }
                i += 1;
            }
        } else {
            let x0 = x[0];
            let x1 = x[1];
            let mut i = 1;
            while i < len {
                let idx_m = 2 * i - offset;
                let idx_p = 2 * i + offset;
                let idx_c = 2 * i;

                if idx_p < x0.len() {
                    let v0 = 0.25 * x0[idx_m] + 0.25 * x0[idx_p] + 0.5 * x0[idx_c];
                    let v1 = 0.25 * x1[idx_m] + 0.25 * x1[idx_p] + 0.5 * x1[idx_c];
                    x_lp[i] = v0 + v1;
                }
                i += 1;
            }
        }
    }
}

#[inline(always)]
fn find_best_pitch(
    xcorr: &[f32],
    y: &[f32],
    len: usize,
    max_pitch: usize,
    best_pitch: &mut [usize; 2],
) {
    let mut best_num = [-1.0f32, -1.0f32];
    let mut best_den = [0.0f32, 0.0f32];

    best_pitch[0] = 0;
    best_pitch[1] = 1;

    #[cfg(target_arch = "aarch64")]
    let mut syy = unsafe {
        use std::arch::aarch64::*;
        let mut sum_vec = vdupq_n_f32(0.0);
        let mut j = 0;
        while j + 16 <= len {
            let y0 = vld1q_f32(y.as_ptr().add(j));
            let y1 = vld1q_f32(y.as_ptr().add(j + 4));
            let y2 = vld1q_f32(y.as_ptr().add(j + 8));
            let y3 = vld1q_f32(y.as_ptr().add(j + 12));
            sum_vec = vfmaq_f32(sum_vec, y0, y0);
            sum_vec = vfmaq_f32(sum_vec, y1, y1);
            sum_vec = vfmaq_f32(sum_vec, y2, y2);
            sum_vec = vfmaq_f32(sum_vec, y3, y3);
            j += 16;
        }
        while j + 4 <= len {
            let y0 = vld1q_f32(y.as_ptr().add(j));
            sum_vec = vfmaq_f32(sum_vec, y0, y0);
            j += 4;
        }
        let mut sum = 1.0f32 + vaddvq_f32(sum_vec);
        while j < len {
            sum += y[j] * y[j];
            j += 1;
        }
        sum
    };
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let mut syy = unsafe {
        if std::arch::is_x86_feature_detected!("avx") {
            use std::arch::x86_64::*;
            let mut acc0 = _mm256_setzero_ps();
            let mut acc1 = _mm256_setzero_ps();
            let mut j = 0;
            while j + 16 <= len {
                let y0 = _mm256_loadu_ps(y.as_ptr().add(j));
                let y1 = _mm256_loadu_ps(y.as_ptr().add(j + 8));
                acc0 = _mm256_add_ps(acc0, _mm256_mul_ps(y0, y0));
                acc1 = _mm256_add_ps(acc1, _mm256_mul_ps(y1, y1));
                j += 16;
            }
            while j + 8 <= len {
                let y0 = _mm256_loadu_ps(y.as_ptr().add(j));
                acc0 = _mm256_add_ps(acc0, _mm256_mul_ps(y0, y0));
                j += 8;
            }
            let acc = _mm256_add_ps(acc0, acc1);
            let hi = _mm256_extractf128_ps(acc, 1);
            let lo = _mm256_castps256_ps128(acc);
            let sum4 = _mm_add_ps(lo, hi);
            let tmp = _mm_movehl_ps(sum4, sum4);
            let sum2 = _mm_add_ps(sum4, tmp);
            let tmp2 = _mm_shuffle_ps(sum2, sum2, 0x55);
            let mut sum = 1.0f32 + _mm_cvtss_f32(_mm_add_ss(sum2, tmp2));
            while j < len {
                sum += y[j] * y[j];
                j += 1;
            }
            sum
        } else {
            let mut sum = 1.0f32;
            for j in 0..len {
                sum += y[j] * y[j];
            }
            sum
        }
    };
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86", target_arch = "x86_64")))]
    let mut syy = {
        let mut sum = 1.0f32;
        for j in 0..len {
            sum += y[j] * y[j];
        }
        sum
    };

    for i in 0..max_pitch {
        if xcorr[i] > 0.0 {
            let num = xcorr[i] * xcorr[i];
            if num * best_den[1] > best_num[1] * syy {
                if num * best_den[0] > best_num[0] * syy {
                    best_num[1] = best_num[0];
                    best_den[1] = best_den[0];
                    best_pitch[1] = best_pitch[0];
                    best_num[0] = num;
                    best_den[0] = syy;
                    best_pitch[0] = i;
                } else {
                    best_num[1] = num;
                    best_den[1] = syy;
                    best_pitch[1] = i;
                }
            }
        }
        syy += y[i + len] * y[i + len] - y[i] * y[i];
        if syy < 1.0 {
            syy = 1.0;
        }
    }
}

pub fn pitch_search(x_lp: &[f32], y: &[f32], mut len: usize, mut max_pitch: usize) -> usize {
    max_pitch >>= 1;
    len >>= 1;
    let lag = len + max_pitch;
    let len4 = len >> 1;
    let lag4 = lag >> 1;

    const MAX_LEN4: usize = 512;
    const MAX_LAG4: usize = 1024;
    const MAX_PITCH: usize = 512;

    // Same search either way; the only difference is where the three scratch
    // buffers live. Every size CELT actually asks for fits the stack bounds.
    if len4 <= MAX_LEN4 && lag4 <= MAX_LAG4 && max_pitch <= MAX_PITCH {
        let mut x_lp4 = [0.0f32; MAX_LEN4];
        let mut y_lp4 = [0.0f32; MAX_LAG4];
        let mut xcorr = [0.0f32; MAX_PITCH];
        pitch_search_impl(
            x_lp,
            y,
            len,
            max_pitch,
            &mut x_lp4[..len4],
            &mut y_lp4[..lag4],
            &mut xcorr[..max_pitch],
        )
    } else {
        let mut x_lp4 = vec![0.0f32; len4];
        let mut y_lp4 = vec![0.0f32; lag4];
        let mut xcorr = vec![0.0f32; max_pitch];
        pitch_search_impl(x_lp, y, len, max_pitch, &mut x_lp4, &mut y_lp4, &mut xcorr)
    }
}

/// The pitch search proper. `len` and `max_pitch` arrive already halved, and
/// the three scratch slices are sized `len >> 1`, `(len + max_pitch) >> 1` and
/// `max_pitch` respectively.
fn pitch_search_impl(
    x_lp: &[f32],
    y: &[f32],
    len: usize,
    max_pitch: usize,
    x_lp4: &mut [f32],
    y_lp4: &mut [f32],
    xcorr: &mut [f32],
) -> usize {
    let mut best_pitch = [0, 0];

    for j in 0..x_lp4.len() {
        x_lp4[j] = x_lp[2 * j];
    }
    for j in 0..y_lp4.len() {
        y_lp4[j] = y[2 * j];
    }

    pitch_xcorr(x_lp4, y_lp4, xcorr, len >> 1, max_pitch >> 1);

    find_best_pitch(xcorr, y_lp4, len >> 1, max_pitch >> 1, &mut best_pitch);

    for i in 0..max_pitch {
        // Lags the coarse pass ruled out stay at 0, not -1. `find_best_pitch`
        // ignores both (it only considers a positive correlation), but the
        // pseudo-interpolation below reads the winner's two neighbours
        // directly, and a skipped neighbour of -1 instead of 0 tips
        // `(c - a) > 0.7 * (b - a)` the other way — an off-by-one in the
        // reported lag (libopus pitch.c pitch_search).
        xcorr[i] = 0.0;
        if (i as i32 - 2 * best_pitch[0] as i32).abs() > 2
            && (i as i32 - 2 * best_pitch[1] as i32).abs() > 2
        {
            continue;
        }
        xcorr[i] = inner_prod(x_lp, &y[i..], len);
        if xcorr[i] < -1.0 {
            xcorr[i] = -1.0;
        }
    }

    find_best_pitch(xcorr, y, len, max_pitch, &mut best_pitch);

    let mut offset = 0;
    if best_pitch[0] > 0 && best_pitch[0] < max_pitch - 1 {
        let a = xcorr[best_pitch[0] - 1];
        let b = xcorr[best_pitch[0]];
        let c = xcorr[best_pitch[0] + 1];
        if (c - a) > 0.7 * (b - a) {
            offset = 1;
        } else if (a - c) > 0.7 * (b - c) {
            offset = -1;
        }
    }

    ((2 * best_pitch[0]) as isize).wrapping_sub(offset as isize) as usize
}

fn compute_pitch_gain(xy: f32, xx: f32, yy: f32) -> f32 {
    if xy <= 0.0 || xx <= 0.0 || yy <= 0.0 {
        return 0.0;
    }
    xy / (1.0 + xx * yy).sqrt()
}

static SECOND_CHECK: [usize; 16] = [0, 0, 3, 2, 3, 2, 5, 2, 3, 2, 3, 2, 5, 2, 3, 2];

#[inline(always)]
fn sum_squares(x: &[f32], n: usize) -> f32 {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        inner_prod_neon(x, x, n)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut sum = 0.0f32;
        for i in 0..n {
            sum += x[i] * x[i];
        }
        sum
    }
}

pub fn remove_doubling(
    x: &[f32],
    mut max_period: usize,
    mut min_period: usize,
    mut n: usize,
    t0_ptr: &mut usize,
    mut prev_period: usize,
    prev_gain: f32,
) -> f32 {
    let min_period0 = min_period;
    max_period /= 2;
    min_period /= 2;
    *t0_ptr /= 2;
    prev_period /= 2;
    n /= 2;

    let x_target = &x[max_period..];

    if *t0_ptr >= max_period {
        *t0_ptr = max_period - 1;
    }

    let mut t = *t0_ptr;
    let t0 = *t0_ptr;

    const MAX_YY_SIZE: usize = 1024;
    let mut yy_lookup_buf = [0.0f32; MAX_YY_SIZE];
    let yy_lookup = &mut yy_lookup_buf[..=max_period];

    let xx = sum_squares(x_target, n);
    let xy = inner_prod(x_target, &x[max_period - t0..], n);

    yy_lookup[0] = xx;
    let mut yy_curr = xx;
    for i in 1..=max_period {
        yy_curr = yy_curr + x[max_period - i] * x[max_period - i]
            - x[max_period + n - i] * x[max_period + n - i];
        if yy_curr < 0.0 {
            yy_curr = 0.0;
        }
        yy_lookup[i] = yy_curr;
    }

    let mut best_xy = xy;
    let mut best_yy = yy_lookup[t0];
    let mut g = compute_pitch_gain(best_xy, xx, best_yy);
    let g0 = g;

    for k in 2..=15 {
        let t1 = (2 * t0 + k) / (2 * k);
        if t1 < min_period {
            break;
        }

        let t1b;
        if k == 2 {
            if t1 + t0 > max_period {
                t1b = t0;
            } else {
                t1b = t0 + t1;
            }
        } else {
            t1b = (2 * SECOND_CHECK[k] * t0 + k) / (2 * k);
        }

        let (xy_a, xy_b) =
            dual_inner_prod(x_target, &x[max_period - t1..], &x[max_period - t1b..], n);
        let xy_new = 0.5 * (xy_a + xy_b);
        let yy_new = 0.5 * (yy_lookup[t1] + yy_lookup[t1b]);
        let g1 = compute_pitch_gain(xy_new, xx, yy_new);

        let mut cont = 0.0f32;
        if (t1 as i32 - prev_period as i32).abs() <= 1 {
            cont = prev_gain;
        } else if (t1 as i32 - prev_period as i32).abs() <= 2 && 5 * k * k < t0 {
            cont = 0.5 * prev_gain;
        }

        let mut thresh = (0.7 * g0 - cont).max(0.3);
        if t1 < 3 * min_period {
            thresh = (0.85 * g0 - cont).max(0.4);
        } else if t1 < 2 * min_period {
            thresh = (0.9 * g0 - cont).max(0.5);
        }

        if g1 > thresh {
            best_xy = xy_new;
            best_yy = yy_new;
            t = t1;
            g = g1;
        }
    }

    best_xy = best_xy.max(0.0);
    let pg = if best_yy <= best_xy {
        1.0f32
    } else {
        best_xy / (best_yy + 1.0)
    };

    let mut xcorr_res = [0.0f32; 3];
    for k_idx in 0..3 {
        let lag = (t as i32 + k_idx as i32 - 1) as usize;
        xcorr_res[k_idx] = inner_prod(x_target, &x[max_period - lag..], n);
    }

    let mut offset = 0;
    if (xcorr_res[2] - xcorr_res[0]) > 0.7 * (xcorr_res[1] - xcorr_res[0]) {
        offset = 1;
    } else if (xcorr_res[0] - xcorr_res[2]) > 0.7 * (xcorr_res[1] - xcorr_res[2]) {
        offset = -1;
    }

    let pg = pg.min(g);
    *t0_ptr = (2 * t as i32 + offset) as usize;
    if *t0_ptr < min_period0 {
        *t0_ptr = min_period0;
    }

    pg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1 << 23) as f32 - 1.0
            })
            .collect()
    }

    /// The three-tap decimating filter at the head of libopus `pitch_downsample`,
    /// before the LPC whitening it shares with the real function.
    fn downsample_taps_reference(x: &[&[f32]], len: usize, c: usize, factor: usize) -> Vec<f32> {
        let offset = factor / 2;
        let mut out = vec![0.0f32; len];
        for xk in x.iter().take(c) {
            out[0] += 0.25 * xk[offset] + 0.5 * xk[0];
            for (i, o) in out.iter_mut().enumerate().skip(1).take(len - 1) {
                *o += 0.25 * xk[factor * i - offset]
                    + 0.25 * xk[factor * i + offset]
                    + 0.5 * xk[factor * i];
            }
        }
        out
    }

    /// `pitch_downsample` must decimate, not merely low-pass.
    ///
    /// The aarch64 path used one `vld1q` per tap and stored four outputs per
    /// iteration, which walks the input one sample per output instead of
    /// `factor`. Only the first lane of each group of four landed on the right
    /// input sample, so three quarters of the pitch buffer was wrong.
    /// Downstream that moved the pitch estimate itself: in the CELT
    /// concealment it shifted the extrapolated period by whole samples, and
    /// the encoder's prefilter analysis reads the same buffer.
    #[test]
    fn downsample_simd_matches_the_scalar_path() {
        for &len in &[8usize, 33, 100, 512, 1024] {
            let a = noise(2 * len, 0x5EED_0001 ^ len as u32);
            let b = noise(2 * len, 0x5EED_0002 ^ len as u32);

            for &c in &[1usize, 2] {
                let chans: Vec<&[f32]> = if c == 1 {
                    vec![&a[..]]
                } else {
                    vec![&a[..], &b[..]]
                };
                let mut got = vec![0.0f32; len];
                pitch_downsample(&chans, &mut got, len, c, 2);

                // Reference: the same taps, then the same whitening.
                let mut want = downsample_taps_reference(&chans, len, c, 2);
                let mut ac = [0.0f32; 5];
                autocorr(&want[0..len], &mut ac, None, 0, 4, len);
                ac[0] *= 1.0001;
                for i in 1..=4 {
                    let f = 0.008 * (i as f32);
                    ac[i] -= ac[i] * f * f;
                }
                let mut lc = [0.0f32; 4];
                lpc(&mut lc, &ac, 4);
                let mut tmp = 1.0f32;
                for v in lc.iter_mut() {
                    tmp *= 0.9;
                    *v *= tmp;
                }
                let c1 = 0.8f32;
                let lpc2 = [
                    lc[0] + c1,
                    lc[1] + c1 * lc[0],
                    lc[2] + c1 * lc[1],
                    lc[3] + c1 * lc[2],
                    c1 * lc[3],
                ];
                let mut mem = [0.0f32; 5];
                for i in 0..len {
                    let xi = want[i];
                    let mut sum = xi;
                    for (k, m) in mem.iter().enumerate() {
                        sum += lpc2[k] * m;
                    }
                    for k in (1..5).rev() {
                        mem[k] = mem[k - 1];
                    }
                    mem[0] = xi;
                    want[i] = sum;
                }

                let scale = want.iter().fold(1.0f32, |m, v| m.max(v.abs()));
                for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                    assert!(
                        (g - w).abs() <= 1e-4 * scale,
                        "pitch_downsample len={len} c={c} sample {i}: {g} vs {w}"
                    );
                }
            }
        }
    }

    /// `inner_prod`, `dual_inner_prod` and `pitch_xcorr` all have SIMD paths;
    /// each must match the plain loop it stands in for.
    #[test]
    fn simd_correlations_match_the_scalar_definition() {
        for &n in &[1usize, 4, 7, 16, 63, 256, 664] {
            let x = noise(n, 0xABCD_0001 ^ n as u32);
            let y1 = noise(n, 0xABCD_0002 ^ n as u32);
            let y2 = noise(n, 0xABCD_0003 ^ n as u32);
            let want: f32 = (0..n).map(|i| x[i] * y1[i]).sum();
            let got = inner_prod(&x, &y1, n);
            assert!(
                (got - want).abs() <= 1e-4 * (1.0 + want.abs()),
                "inner_prod n={n}: {got} vs {want}"
            );

            let want2: f32 = (0..n).map(|i| x[i] * y2[i]).sum();
            let (g1, g2) = dual_inner_prod(&x, &y1, &y2, n);
            assert!(
                (g1 - want).abs() <= 1e-4 * (1.0 + want.abs())
                    && (g2 - want2).abs() <= 1e-4 * (1.0 + want2.abs()),
                "dual_inner_prod n={n}"
            );
        }

        for &(len, max_pitch) in &[(64usize, 8usize), (128, 33), (332, 155), (664, 310)] {
            let x = noise(len, 0x7777_0001 ^ len as u32);
            let y = noise(len + max_pitch + 4, 0x7777_0002 ^ len as u32);
            let mut got = vec![0.0f32; max_pitch];
            pitch_xcorr(&x, &y, &mut got, len, max_pitch);
            for i in 0..max_pitch {
                let want: f32 = (0..len).map(|j| x[j] * y[i + j]).sum();
                assert!(
                    (got[i] - want).abs() <= 1e-3 * (1.0 + want.abs()),
                    "pitch_xcorr len={len} lag {i}: {} vs {want}",
                    got[i]
                );
            }
        }
    }
}

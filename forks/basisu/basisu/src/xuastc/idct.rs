//! The XUASTC weight-grid inverse DCT: a 1D orthonormal DCT-III applied per
//! column and then per row, driven entirely by the baked single-precision
//! coefficient matrices in `idct_tables`. Those matrices are stored as
//! literals rather than recomputed because they carry per-cell rounding noise
//! that a fresh computation would not reproduce. All arithmetic is plain f32
//! multiply-then-add in a fixed accumulation order (ascending source
//! coefficient `k`, then ascending output index `x`) with no fused
//! multiply-add, so the accumulation is bit-exact and reproducible.

use super::idct_tables::IDCT_COEFFS;

/// One 1D IDCT of size `n` (2..=12): `dst[x*stride] = sum_k C[k][x] *
/// src[k*stride]`. Source coefficients that are exactly zero are skipped
/// (`if v != 0.0`) since they contribute nothing, and coefficient cells that
/// are structurally zero are stored as `0.0`; adding a zero term is an exact
/// no-op under f32 accumulation, so the result equals a full dense sum.
fn idct_1d(n: usize, src: &[f32], src_stride: usize, dst: &mut [f32], dst_stride: usize) {
    let coeffs = IDCT_COEFFS[n - 2];
    let mut s = [0f32; 12];
    for k in 0..n {
        let v = src[k * src_stride];
        if v != 0.0 {
            for (x, sx) in s[..n].iter_mut().enumerate() {
                *sx += coeffs[k * n + x] * v;
            }
        }
    }
    for (x, &sx) in s[..n].iter().enumerate() {
        dst[x * dst_stride] = sx;
    }
}

/// Full 2D IDCT: a column pass (length `num_rows`, both strides `num_cols`)
/// into a scratch buffer, then a row pass (length `num_cols`, unit stride)
/// into `dst`. Both dimensions are in `2..=12`.
pub fn idct_2d(src: &[f32], dst: &mut [f32], num_rows: usize, num_cols: usize) {
    debug_assert!((2..=12).contains(&num_rows) && (2..=12).contains(&num_cols));
    let mut temp = [0f32; 12 * 12];

    for c in 0..num_cols {
        idct_1d(num_rows, &src[c..], num_cols, &mut temp[c..], num_cols);
    }
    for r in 0..num_rows {
        idct_1d(
            num_cols,
            &temp[r * num_cols..],
            1,
            &mut dst[r * num_cols..],
            1,
        );
    }
}

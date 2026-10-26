// f32 square roots with the FPU's square root steps, on both chips.
//
// Rust's compiler_builtins takes the square root of an f32 in software, from
// flash: its sqrtf took about 4% of a frame of the dashboard on the ES3C35P,
// where every pixel near the edge of a rounded shape, an arc or a stroke
// takes one. The FPU of the classic ESP32 and of the ESP32-S3 has the steps
// of a square root (sqrt0.s, nexp01.s, maddn.s, mksadj.s, addexp.s, divn.s):
// the sequence below is the one newlib's __ieee754_sqrtf for both chips
// runs, rounded as IEEE 754 asks and with its special values, without a
// branch.
//
// The link of this component wraps sqrtf, so the calls Rust's code makes
// land here whichever archive the linker reads first. It sits in IRAM, where
// the CPU reads it without the cache.

    .section .iram1.cranpose_fast_fsqrt, "ax", @progbits
    .global __wrap_sqrtf
    .type __wrap_sqrtf, @function
    .align 4
__wrap_sqrtf:
    entry       a1, 16
    wfr         f1, a2
    sqrt0.s     f2, f1
    const.s     f3, 0
    maddn.s     f3, f2, f2
    nexp01.s    f4, f1
    const.s     f0, 3
    addexp.s    f4, f0
    maddn.s     f0, f3, f4
    nexp01.s    f3, f1
    neg.s       f5, f3
    maddn.s     f2, f0, f2
    const.s     f0, 0
    const.s     f6, 0
    const.s     f7, 0
    maddn.s     f0, f5, f2
    maddn.s     f6, f2, f4
    const.s     f4, 3
    maddn.s     f7, f4, f2
    maddn.s     f3, f0, f0
    maddn.s     f4, f6, f2
    neg.s       f2, f7
    maddn.s     f0, f3, f2
    maddn.s     f7, f4, f7
    mksadj.s    f2, f1
    nexp01.s    f1, f1
    maddn.s     f1, f0, f0
    neg.s       f3, f7
    addexpm.s   f0, f2
    addexp.s    f3, f2
    divn.s      f0, f1, f3
    rfr         a2, f0
    retw.n
    .size __wrap_sqrtf, . - __wrap_sqrtf

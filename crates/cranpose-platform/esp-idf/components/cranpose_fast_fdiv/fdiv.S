// f32 division with the FPU's divide steps, on the classic ESP32.
//
// Rust's compiler_builtins divides f32 in software: its __divsf3 normalizes,
// divides the mantissas bit by bit and rounds, from flash, and took about 2%
// of a frame of text on the E32R35T. The classic ESP32's FPU has the steps of
// a division (div0.s, nexp01.s, maddn.s, mkdadj.s, addexp.s, divn.s): the
// sequence below is the one libgcc's __divsf3 for the chip runs, rounded as
// IEEE 754 asks and with its special values, without a branch. The ESP32-S3
// needs none of this: its ROM's __divsf3 is the same sequence.
//
// The link of this component wraps __divsf3, so the calls Rust's code makes
// land here whichever archive the linker reads first. It sits in IRAM, where
// the CPU reads it without the cache.

    .section .iram1.cranpose_fast_fdiv, "ax", @progbits
    .global __wrap___divsf3
    .type __wrap___divsf3, @function
    .align 4
__wrap___divsf3:
    entry       a1, 16
    wfr         f1, a2
    wfr         f2, a3
    div0.s      f3, f2
    nexp01.s    f4, f2
    const.s     f5, 1
    maddn.s     f5, f4, f3
    mov.s       f6, f3
    mov.s       f7, f2
    nexp01.s    f2, f1
    maddn.s     f6, f5, f6
    const.s     f5, 1
    const.s     f0, 0
    neg.s       f8, f2
    maddn.s     f5, f4, f6
    maddn.s     f0, f8, f3
    mkdadj.s    f7, f1
    maddn.s     f6, f5, f6
    maddn.s     f8, f4, f0
    const.s     f3, 1
    maddn.s     f3, f4, f6
    maddn.s     f0, f8, f6
    neg.s       f2, f2
    maddn.s     f6, f3, f6
    maddn.s     f2, f4, f0
    addexpm.s   f0, f7
    addexp.s    f6, f7
    divn.s      f0, f2, f6
    rfr         a2, f0
    retw.n
    .size __wrap___divsf3, . - __wrap___divsf3

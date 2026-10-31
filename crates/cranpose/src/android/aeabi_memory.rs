#![expect(
    unsafe_code,
    reason = "the AEABI memory routines are C ABI symbols that forward to bionic"
)]
//! The 32-bit ARM memory routines, forwarded to bionic.
//!
//! On 32-bit ARM, LLVM lowers each copy, move and fill it does not inline
//! (a medium struct move, a slice copy, a buffer clear) to a call to the
//! AEABI routines `__aeabi_memcpy`, `__aeabi_memmove`, `__aeabi_memset`
//! and `__aeabi_memclr`. The Rust runtime's compiler-builtins defines them
//! weakly, as a loop of 4-byte words, and the linker takes its definitions
//! before it reaches bionic, whose `memcpy` copies with NEON. These strong
//! definitions replace them and call bionic's `memcpy`, `memmove` and
//! `memset`. Each calls through a pointer the optimizer cannot see into:
//! it would otherwise turn the `memcpy` call back into an AEABI call to the
//! routine itself.

use std::{
    ffi::{c_int, c_void},
    hint::black_box,
};

type CopyRoutine = unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> *mut c_void;
type FillRoutine = unsafe extern "C" fn(*mut c_void, c_int, usize) -> *mut c_void;

unsafe extern "C" {
    fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
    fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
    fn memset(dest: *mut c_void, c: c_int, n: usize) -> *mut c_void;
}

// Each routine takes the C function's arguments, in the AEABI order.
macro_rules! forward_copy {
    ($libc:ident: $($name:ident),+) => {$(
        #[unsafe(no_mangle)]
        unsafe extern "aapcs" fn $name(dest: *mut c_void, src: *const c_void, n: usize) {
            // SAFETY: the caller passes what the C function requires.
            unsafe { black_box($libc as CopyRoutine)(dest, src, n) };
        }
    )+};
}

macro_rules! forward_fill {
    ($($name:ident),+) => {$(
        #[unsafe(no_mangle)]
        unsafe extern "aapcs" fn $name(dest: *mut c_void, n: usize, c: c_int) {
            // SAFETY: the caller passes what `memset` requires.
            unsafe { black_box(memset as FillRoutine)(dest, c, n) };
        }
    )+};
}

macro_rules! forward_clear {
    ($($name:ident),+) => {$(
        #[unsafe(no_mangle)]
        unsafe extern "aapcs" fn $name(dest: *mut c_void, n: usize) {
            // SAFETY: the caller passes what `memset` requires.
            unsafe { black_box(memset as FillRoutine)(dest, 0, n) };
        }
    )+};
}

forward_copy!(memcpy: __aeabi_memcpy, __aeabi_memcpy4, __aeabi_memcpy8);
forward_copy!(memmove: __aeabi_memmove, __aeabi_memmove4, __aeabi_memmove8);
forward_fill!(__aeabi_memset, __aeabi_memset4, __aeabi_memset8);
forward_clear!(__aeabi_memclr, __aeabi_memclr4, __aeabi_memclr8);

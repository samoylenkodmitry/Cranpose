#![allow(unsafe_code)]

use std::{rc::Rc, time::Duration};

use cranpose_services::{DeviceInfo, DeviceInfoRef, device_info, set_platform_device_info};

pub(crate) fn install() {
    let inner = device_info();
    set_platform_device_info(Rc::new(ProcessDeviceInfo { inner }));
}

struct ProcessDeviceInfo {
    inner: DeviceInfoRef,
}

impl DeviceInfo for ProcessDeviceInfo {
    fn total_memory_bytes(&self) -> Option<u64> {
        self.inner.total_memory_bytes()
    }

    fn resident_memory_bytes(&self) -> Option<u64> {
        resident_memory_bytes().or_else(|| self.inner.resident_memory_bytes())
    }

    fn available_memory_bytes(&self) -> Option<u64> {
        available_memory_bytes().or_else(|| self.inner.available_memory_bytes())
    }

    fn process_cpu_time(&self) -> Option<Duration> {
        process_cpu_time()
    }

    fn release_free_memory(&self) -> bool {
        release_free_memory()
    }
}

fn resident_memory_bytes() -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let text = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = text.split_whitespace().nth(1)?.parse().ok()?;
        pages.checked_mul(page_size_bytes())
    }
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    {
        darwin_resident_memory_bytes()
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "ios",
        target_os = "macos"
    )))]
    {
        None
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn darwin_resident_memory_bytes() -> Option<u64> {
    use mach2::{
        kern_return::KERN_SUCCESS,
        task::task_info,
        task_info::{MACH_TASK_BASIC_INFO, MACH_TASK_BASIC_INFO_COUNT, mach_task_basic_info},
        traps::mach_task_self,
    };

    let mut info = mach_task_basic_info::default();
    let mut count = MACH_TASK_BASIC_INFO_COUNT;
    // SAFETY: `mach_task_self` returns this process's own task port, a plain
    // integer with no ownership to release. `task_info` writes into `info`
    // through the pointer and length this call gives it; the pointer is a
    // live, correctly sized local (`count` names its capacity in the same
    // `natural_t` words the call measures in) and `count` is read back
    // afterwards to confirm it wrote the whole struct before any field of
    // `info` is used.
    let result = unsafe {
        task_info(
            mach_task_self(),
            MACH_TASK_BASIC_INFO,
            (&mut info as *mut mach_task_basic_info).cast(),
            &mut count,
        )
    };
    if result != KERN_SUCCESS || count != MACH_TASK_BASIC_INFO_COUNT {
        return None;
    }
    Some(info.resident_size)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn page_size_bytes() -> u64 {
    // SAFETY: `sysconf` takes an integer name and returns a long. It reads
    // process-wide configuration, touches no memory the caller owns, and is
    // safe to call from any thread at any time. A non-positive return means
    // the name is unsupported, which is checked rather than cast.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 { size as u64 } else { 4096 }
}

fn process_cpu_time() -> Option<Duration> {
    // SAFETY: `getrusage` fills the caller's `rusage`. `zeroed` is a valid bit
    // pattern for it beforehand — it is plain integers and `timeval`s — and
    // the call initializes it on success.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `RUSAGE_SELF` is a valid `who`, and the pointer is to a live,
    // correctly typed local the call writes at most once.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return None;
    }
    let spent = |time: libc::timeval| {
        Duration::from_secs(time.tv_sec.max(0) as u64)
            + Duration::from_micros(time.tv_usec.max(0) as u64)
    };
    Some(spent(usage.ru_utime) + spent(usage.ru_stime))
}

fn available_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "ios")]
    {
        unsafe extern "C" {
            fn os_proc_available_memory() -> usize;
        }
        // SAFETY: a nullary call into libSystem returning a byte count. It
        // reads this process's memory accounting and touches nothing the
        // caller owns. Zero means the call is unavailable — it is not, in an
        // app extension — which is reported as unknown rather than as "no
        // memory left".
        let available = unsafe { os_proc_available_memory() };
        (available > 0).then_some(available as u64)
    }
    #[cfg(not(target_os = "ios"))]
    {
        None
    }
}

fn release_free_memory() -> bool {
    #[cfg(target_os = "android")]
    {
        const M_PURGE: libc::c_int = -101;
        unsafe extern "C" {
            fn mallopt(param: libc::c_int, value: libc::c_int) -> libc::c_int;
        }
        // SAFETY: two integers in, one out. `M_PURGE` asks the allocator to
        // release its own free pages; it frees nothing the caller holds a
        // pointer to, and an unrecognized parameter is a no-op returning zero.
        unsafe { mallopt(M_PURGE, 0) };
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

#[cfg(test)]
#[path = "tests/process_info_tests.rs"]
mod tests;

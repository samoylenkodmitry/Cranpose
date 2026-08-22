//! What this process is using, from the calls that need `libc`.
//!
//! [`cranpose_services::DeviceInfo`] states the questions; the parts of the
//! answer that can be read from safe Rust — `/proc/meminfo`, `/proc/self/statm`
//! — are answered by its own default. This supplies the rest, which cannot be:
//! processor time comes from `getrusage`, the page size `/proc/self/statm`
//! counts in comes from `sysconf`, the memory an application may still have
//! comes from `os_proc_available_memory`, and returning free pages to the
//! system is a `mallopt` extension bionic has and glibc does not.
//!
//! An application never writes any of this. It is the same handful of calls in
//! every application that watches its own footprint, each needing an `unsafe`
//! block and a platform guard, in a crate that would rather have neither.

#![allow(unsafe_code)]

use cranpose_services::{device_info, set_platform_device_info, DeviceInfo, DeviceInfoRef};
use std::rc::Rc;
use std::time::Duration;

/// Installs the process readings over whatever device info is registered.
///
/// Called by each platform's startup after that platform has registered its
/// own device info, so a target gains the process readings without losing the
/// device-wide facts it already reported.
pub(crate) fn install() {
    let inner = device_info();
    set_platform_device_info(Rc::new(ProcessDeviceInfo { inner }));
}

/// Reads what this process is using through `libc`, delegating the
/// device-wide facts to the implementation it wraps.
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

/// Memory this process holds resident.
///
/// The services default reads the same file, assuming 4 KiB pages because
/// reading the real page size is not something safe Rust can do. This reads it.
fn resident_memory_bytes() -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let text = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = text.split_whitespace().nth(1)?.parse().ok()?;
        pages.checked_mul(page_size_bytes())
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        None
    }
}

/// The page size `/proc/self/statm` counts in.
///
/// 4 KiB has been the answer for the whole of Linux's history on the
/// architectures Cranpose runs on, and Android 15 made 16 KiB real on arm64.
/// One `sysconf` call is cheaper than being wrong by a factor of four about
/// how much memory the process is holding.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn page_size_bytes() -> u64 {
    // SAFETY: `sysconf` takes an integer name and returns a long. It reads
    // process-wide configuration, touches no memory the caller owns, and is
    // safe to call from any thread at any time. A non-positive return means
    // the name is unsupported, which is checked rather than cast.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 {
        size as u64
    } else {
        4096
    }
}

/// Processor time this process has used, user and system together.
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

/// Memory this process may still allocate before the platform stops it.
fn available_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "ios")]
    {
        // The number iOS itself uses to decide when to kill an application. It
        // is not free system memory: the limit is per-process, and a device
        // with gigabytes free will still stop this one at its own ceiling.
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

/// Asks the allocator to return free pages to the system.
fn release_free_memory() -> bool {
    #[cfg(target_os = "android")]
    {
        // bionic's `M_PURGE`: walk the arenas and give back what is free.
        // Android kills by resident size, and an allocator holding pages a
        // finished decode no longer needs is counted against this process just
        // as if they were live.
        //
        // Declared here because it is a bionic extension the `libc` crate does
        // not bind, not because the signature is in doubt: `int mallopt(int,
        // int)`, unchanged since it arrived.
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
mod tests {
    use super::*;
    use cranpose_services::clear_platform_device_info;

    struct Inner {
        total: Option<u64>,
    }

    impl DeviceInfo for Inner {
        fn total_memory_bytes(&self) -> Option<u64> {
            self.total
        }
    }

    fn wrapping(total: Option<u64>) -> ProcessDeviceInfo {
        ProcessDeviceInfo {
            inner: Rc::new(Inner { total }),
        }
    }

    /// The wrapper adds process readings without taking away what the platform
    /// it wraps already answered.
    #[test]
    fn the_wrapped_device_info_still_answers_what_it_knew() {
        assert_eq!(wrapping(Some(6 << 30)).total_memory_bytes(), Some(6 << 30));
        assert_eq!(wrapping(None).total_memory_bytes(), None);
    }

    #[test]
    fn installing_wraps_whatever_was_registered_rather_than_replacing_it() {
        clear_platform_device_info();
        set_platform_device_info(Rc::new(Inner {
            total: Some(3 << 30),
        }));

        install();

        let info = device_info();
        assert_eq!(
            info.total_memory_bytes(),
            Some(3 << 30),
            "the platform's own answer must survive the wrap"
        );
        assert!(
            info.process_cpu_time().is_some(),
            "unix reports processor time"
        );
        clear_platform_device_info();
    }

    /// Processor time is monotonic — a reading that went backwards would make
    /// every rate computed from it negative.
    #[test]
    fn processor_time_only_goes_forwards() {
        let info = wrapping(None);
        let first = info
            .process_cpu_time()
            .expect("unix reports processor time");

        // Something to spend time on that the optimizer cannot remove.
        let mut sum = 0u64;
        for value in 0..2_000_000u64 {
            sum = sum.wrapping_add(value * value);
        }
        assert_ne!(sum, u64::MAX);

        let second = info
            .process_cpu_time()
            .expect("unix reports processor time");
        assert!(
            second >= first,
            "processor time went backwards: {first:?} then {second:?}"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn the_resident_set_is_a_whole_number_of_pages() {
        let resident = wrapping(None)
            .resident_memory_bytes()
            .expect("linux reports a resident set");
        assert!(
            resident > 0 && resident.is_multiple_of(page_size_bytes()),
            "a resident set of {resident} bytes is not a whole number of pages"
        );
    }

    /// Whether the platform can return pages is a fact about the platform, and
    /// asking twice must give the same answer.
    #[test]
    fn releasing_free_memory_reports_whether_the_platform_has_the_call() {
        let info = wrapping(None);
        assert_eq!(info.release_free_memory(), cfg!(target_os = "android"));
        assert_eq!(info.release_free_memory(), cfg!(target_os = "android"));
    }
}

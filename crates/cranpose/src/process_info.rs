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
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 { size as u64 } else { 4096 }
}

fn process_cpu_time() -> Option<Duration> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
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
    use cranpose_services::clear_platform_device_info;

    use super::*;

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

    #[test]
    fn processor_time_only_goes_forwards() {
        let info = wrapping(None);
        let first = info
            .process_cpu_time()
            .expect("unix reports processor time");

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

    #[test]
    fn releasing_free_memory_reports_whether_the_platform_has_the_call() {
        let info = wrapping(None);
        assert_eq!(info.release_free_memory(), cfg!(target_os = "android"));
        assert_eq!(info.release_free_memory(), cfg!(target_os = "android"));
    }
}

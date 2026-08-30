//! What the device has, and what this process is using of it.
//!
//! Applications ask two different questions here. *How much memory does this
//! device have* sizes a decision made once — whether a large model fits at all.
//! *How much is this process holding, and how much may it still have* is asked
//! while work is running, because the answer moves and because the platform
//! kills a process that gets it wrong.
//!
//! Every reading is optional. A platform that will not say reports `None`
//! rather than a zero an application would divide by, and
//! [`release_free_memory`] reports whether the platform has such a call at all
//! instead of quietly doing nothing.
//!
//! The default reads what can be read without leaving safe Rust:
//! `/proc/meminfo` and `/proc/self/statm` on Linux and Android. The platform
//! backends install richer implementations through
//! [`set_platform_device_info`].

use std::{cell::RefCell, rc::Rc, time::Duration};

/// Provides device and process information.
pub trait DeviceInfo {
    /// Total physical memory in bytes, or `None` if unknown.
    fn total_memory_bytes(&self) -> Option<u64>;

    /// Memory this process currently holds resident, or `None` where the
    /// platform will not say.
    fn resident_memory_bytes(&self) -> Option<u64> {
        None
    }

    /// Memory this process may still allocate before the platform stops it.
    ///
    /// Not the same as free system memory: a device with gigabytes free may
    /// still refuse this process another hundred megabytes, and it is the
    /// second number that decides whether the next allocation is the one that
    /// gets the application killed.
    fn available_memory_bytes(&self) -> Option<u64> {
        None
    }

    /// Processor time this process has used, user and system together.
    ///
    /// Wall-clock time says how long something took; this says how much of a
    /// core it took, which is what a background lane that must not heat the
    /// device up is actually rationing.
    fn process_cpu_time(&self) -> Option<Duration> {
        None
    }

    /// Asks the allocator to return free pages to the system.
    ///
    /// Returns whether the platform has such a call. Freeing a large buffer
    /// does not necessarily shrink the process — an allocator keeps the pages
    /// for the next allocation — and on a platform that kills by resident size
    /// that is the difference between finishing and being killed.
    fn release_free_memory(&self) -> bool {
        false
    }
}

pub type DeviceInfoRef = Rc<dyn DeviceInfo>;

struct DefaultDeviceInfo;

impl DeviceInfo for DefaultDeviceInfo {
    fn total_memory_bytes(&self) -> Option<u64> {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let text = std::fs::read_to_string("/proc/meminfo").ok()?;
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                    return Some(kb * 1024);
                }
            }
            None
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            None
        }
    }

    fn resident_memory_bytes(&self) -> Option<u64> {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let text = std::fs::read_to_string("/proc/self/statm").ok()?;
            resident_bytes_from_statm(&text, page_size_bytes())
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            None
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android", test))]
fn resident_bytes_from_statm(text: &str, page_size: u64) -> Option<u64> {
    text.split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()?
        .checked_mul(page_size)
}

#[cfg(any(target_os = "linux", target_os = "android", test))]
const fn page_size_bytes() -> u64 {
    4096
}

thread_local! {
    static PLATFORM_DEVICE_INFO: RefCell<Option<DeviceInfoRef>> = const { RefCell::new(None) };
}

/// Installs a platform device-info implementation, replacing any previous one.
pub fn set_platform_device_info(info: DeviceInfoRef) {
    PLATFORM_DEVICE_INFO.with(|cell| *cell.borrow_mut() = Some(info));
}

/// Removes any registered platform device info (tests and teardown).
pub fn clear_platform_device_info() {
    PLATFORM_DEVICE_INFO.with(|cell| *cell.borrow_mut() = None);
}

/// The active device info: the platform implementation if installed, otherwise
/// the built-in default.
pub fn device_info() -> DeviceInfoRef {
    PLATFORM_DEVICE_INFO
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| Rc::new(DefaultDeviceInfo))
}

/// Asks the allocator to return free pages to the system.
///
/// Returns whether the platform has such a call — see
/// [`DeviceInfo::release_free_memory`]. Worth doing after finishing with a
/// large buffer on a platform that kills by resident size, and worth doing
/// nowhere else: it walks the allocator's arenas.
pub fn release_free_memory() -> bool {
    device_info().release_free_memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_device_info_takes_precedence() {
        clear_platform_device_info();
        struct Fake;
        impl DeviceInfo for Fake {
            fn total_memory_bytes(&self) -> Option<u64> {
                Some(8 * 1024 * 1024 * 1024)
            }
        }
        set_platform_device_info(Rc::new(Fake));
        assert_eq!(device_info().total_memory_bytes(), Some(8 << 30));
        clear_platform_device_info();
    }

    #[test]
    fn a_platform_that_will_not_say_reports_nothing_rather_than_zero() {
        clear_platform_device_info();
        struct Silent;
        impl DeviceInfo for Silent {
            fn total_memory_bytes(&self) -> Option<u64> {
                None
            }
        }
        set_platform_device_info(Rc::new(Silent));

        let info = device_info();
        assert_eq!(info.total_memory_bytes(), None);
        assert_eq!(info.resident_memory_bytes(), None);
        assert_eq!(info.available_memory_bytes(), None);
        assert_eq!(info.process_cpu_time(), None);
        assert!(!info.release_free_memory());
        assert!(!release_free_memory());
        clear_platform_device_info();
    }

    #[test]
    fn a_platform_that_can_answer_is_asked_through_the_free_function() {
        clear_platform_device_info();
        struct Rich;
        impl DeviceInfo for Rich {
            fn total_memory_bytes(&self) -> Option<u64> {
                Some(4 << 30)
            }
            fn resident_memory_bytes(&self) -> Option<u64> {
                Some(256 << 20)
            }
            fn available_memory_bytes(&self) -> Option<u64> {
                Some(512 << 20)
            }
            fn process_cpu_time(&self) -> Option<Duration> {
                Some(Duration::from_millis(1_250))
            }
            fn release_free_memory(&self) -> bool {
                true
            }
        }
        set_platform_device_info(Rc::new(Rich));

        let info = device_info();
        assert_eq!(info.resident_memory_bytes(), Some(256 << 20));
        assert_eq!(info.available_memory_bytes(), Some(512 << 20));
        assert_eq!(info.process_cpu_time(), Some(Duration::from_millis(1_250)));
        assert!(release_free_memory());
        clear_platform_device_info();
    }

    #[test]
    fn the_resident_set_is_the_second_field_of_statm_in_pages() {
        let statm = "123456 2048 512 64 0 1024 0\n";
        assert_eq!(resident_bytes_from_statm(statm, 4096), Some(2048 * 4096));
        assert_eq!(resident_bytes_from_statm(statm, 16384), Some(2048 * 16384));
    }

    #[test]
    fn an_unreadable_statm_line_is_unknown_rather_than_no_memory() {
        for broken in ["", "123456", "123456 notanumber 512", "   "] {
            assert_eq!(
                resident_bytes_from_statm(broken, page_size_bytes()),
                None,
                "{broken:?} should read as unknown"
            );
        }
    }
}

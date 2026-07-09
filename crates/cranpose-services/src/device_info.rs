//! Device information (RAM, so far). Apps use it to size in-memory work (e.g.
//! whether a large model fits).
//!
//! The default reads `/proc/meminfo` on Linux/Android and returns `None`
//! elsewhere; the iOS backend installs a `ProcessInfo`-based implementation via
//! [`set_platform_device_info`].

use std::cell::RefCell;
use std::rc::Rc;

/// Provides device information.
pub trait DeviceInfo {
    /// Total physical memory in bytes, or `None` if unknown.
    fn total_memory_bytes(&self) -> Option<u64>;
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
}

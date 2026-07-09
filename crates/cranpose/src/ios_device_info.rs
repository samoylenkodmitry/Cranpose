//! iOS device information via `NSProcessInfo`.
//!
//! Registered as the platform device info (see
//! [`cranpose_services::set_platform_device_info`]) by the iOS backend.

use cranpose_services::{set_platform_device_info, DeviceInfo};
use objc2_foundation::NSProcessInfo;
use std::rc::Rc;

/// Installs the iOS device info as the platform device info.
pub(crate) fn register() {
    set_platform_device_info(Rc::new(IosDeviceInfo));
}

struct IosDeviceInfo;

impl DeviceInfo for IosDeviceInfo {
    fn total_memory_bytes(&self) -> Option<u64> {
        Some(NSProcessInfo::processInfo().physicalMemory())
    }
}

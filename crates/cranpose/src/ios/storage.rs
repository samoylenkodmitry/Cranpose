#![expect(unsafe_code)]

use std::{path::Path, sync::Arc};

use cranpose_services::{StorageSpace, set_platform_storage_space};
use objc2_foundation::{
    NSArray, NSNumber, NSURL, NSURLVolumeAvailableCapacityForImportantUsageKey,
};

pub(crate) fn register() {
    set_platform_storage_space(Arc::new(IosStorageSpace));
}

/// The space iOS gives for data the person asked for, which counts what the
/// system frees on demand; the file system's own count leaves that out.
struct IosStorageSpace;

impl StorageSpace for IosStorageSpace {
    fn available(&self, path: &Path) -> Option<u64> {
        let url = NSURL::from_file_path(path)?;
        let key = unsafe { NSURLVolumeAvailableCapacityForImportantUsageKey };
        let values = url
            .resourceValuesForKeys_error(&NSArray::from_slice(&[key]))
            .ok()?;
        let capacity = values.objectForKey(key)?.downcast::<NSNumber>().ok()?;
        u64::try_from(capacity.longLongValue()).ok()
    }
}

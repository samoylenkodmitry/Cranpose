//! Free space on the device, so an application can tell the person before a
//! large download or save that it will not fit.

use std::{path::Path, sync::Arc};

use crate::registry::ServiceRegistry;

/// Reports the free space a platform measures its own way.
pub trait StorageSpace: Send + Sync {
    /// The bytes the application can still write on the volume that holds
    /// `path`, or `None` where the platform cannot tell.
    fn available(&self, path: &Path) -> Option<u64>;
}

pub type StorageSpaceRef = Arc<dyn StorageSpace>;

static PLATFORM_STORAGE_SPACE: ServiceRegistry<dyn StorageSpace> = ServiceRegistry::new();

/// Installs the platform's measure of free space. iOS installs one that counts
/// the space the system frees on demand for important data.
// forwards on purpose: the registry is private to this crate, and this is the
// one way a platform backend installs into it.
pub fn set_platform_storage_space(space: StorageSpaceRef) {
    PLATFORM_STORAGE_SPACE.set(space);
}

/// The bytes the application can still write on the volume that holds `path`,
/// or `None` where the platform cannot tell. `path` need not exist yet: the
/// nearest folder above it that does is measured.
pub fn available_storage(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|folder| folder.exists())?;
    match PLATFORM_STORAGE_SPACE.get() {
        Some(platform) => platform.available(existing),
        None => file_system_available(existing),
    }
}

#[cfg(unix)]
fn file_system_available(path: &Path) -> Option<u64> {
    let stat = rustix::fs::statvfs(path).ok()?;
    Some(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(not(unix))]
fn file_system_available(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
#[path = "tests/storage_tests.rs"]
mod tests;

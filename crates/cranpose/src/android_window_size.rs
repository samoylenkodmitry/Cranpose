//! The size the activity's window had when the app last ran, kept so the
//! next launch composes its first frame while Android is still making the
//! window rather than after.

use std::path::PathBuf;

#[cfg_attr(not(target_os = "android"), expect(dead_code))]
const FILE_NAME: &str = "window-size.bin";

/// A window's size in physical pixels and the density it showed at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) density: f32,
}

impl WindowSize {
    fn to_bytes(self) -> [u8; 12] {
        let mut bytes = [0; 12];
        bytes[..4].copy_from_slice(&self.width.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.height.to_le_bytes());
        bytes[8..].copy_from_slice(&self.density.to_le_bytes());
        bytes
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; 12] = bytes.try_into().ok()?;
        let (width, rest) = bytes.split_first_chunk::<4>()?;
        let (height, density) = rest.split_first_chunk::<4>()?;
        let size = Self {
            width: u32::from_le_bytes(*width),
            height: u32::from_le_bytes(*height),
            density: f32::from_le_bytes(density.first_chunk::<4>().copied()?),
        };
        (size.width > 0 && size.height > 0 && size.density.is_finite() && size.density > 0.0)
            .then_some(size)
    }
}

#[cfg_attr(not(target_os = "android"), expect(dead_code))]
fn file() -> Option<PathBuf> {
    cranpose_services::application_directories()
        .ok()
        .map(|directories| directories.data.join(FILE_NAME))
}

/// The window size the app last ran at, if it kept one.
#[cfg_attr(not(target_os = "android"), expect(dead_code))]
pub(crate) fn last() -> Option<WindowSize> {
    WindowSize::from_bytes(&std::fs::read(file()?).ok()?)
}

/// Keeps `size` for the next launch.
#[cfg_attr(not(target_os = "android"), expect(dead_code))]
pub(crate) fn remember(size: WindowSize) {
    let Some(path) = file() else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        log::warn!("[window-size] create_dir_all {parent:?}: {error}");
        return;
    }
    if let Err(error) = std::fs::write(&path, size.to_bytes()) {
        log::warn!("[window-size] write {path:?}: {error}");
    }
}

#[cfg(test)]
#[path = "tests/android_window_size_tests.rs"]
mod tests;

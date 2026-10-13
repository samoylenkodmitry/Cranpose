//! Font files an app installs while it runs, such as a font pack it
//! downloaded after launch.
//!
//! A font set built before an install keeps drawing with its own faces. Its
//! holder asks [`SoftwareTextFontSet::with_installed_faces`] once a frame and,
//! after an install, gets the set with the installed faces after its own.
//!
//! [`SoftwareTextFontSet::with_installed_faces`]: crate::software_text_raster::SoftwareTextFontSet::with_installed_faces

use std::{
    path::Path,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

use cranpose_ui::text::{FontFamily, FontStyle, FontWeight};

use crate::{
    font_source::{FontLoadError, read_font_file},
    software_text_raster::SoftwareTextFont,
};

static INSTALLED: Mutex<Vec<SoftwareTextFont>> = Mutex::new(Vec::new());
/// How many faces `INSTALLED` holds, read without its lock every frame.
static INSTALLED_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Installs the font file at `path` as `family`, with one face at each of
/// `weights`; a variable font is instanced at each weight.
///
/// Text laid out from the next frame on uses the faces for the characters the
/// app's own fonts lack, and a style naming `family` selects them. The file is
/// read once and kept for the life of the process; installing it again adds
/// nothing.
pub fn install_font_file(
    family: &FontFamily,
    path: &Path,
    weights: &[FontWeight],
) -> Result<(), FontLoadError> {
    let bytes = read_font_file(path)?;
    let faces = weights
        .iter()
        .map(|&weight| {
            SoftwareTextFont::from_registered_bytes(family, weight, FontStyle::Normal, bytes)
                .map_err(|source| FontLoadError::Parse {
                    path: path.to_path_buf(),
                    source,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut installed = INSTALLED.lock().unwrap_or_else(PoisonError::into_inner);
    for face in faces {
        let known = installed.iter().any(|known| {
            known.content_hash() == face.content_hash()
                && known.weight() == face.weight()
                && known.registered_family() == face.registered_family()
        });
        if !known {
            installed.push(face);
        }
    }
    INSTALLED_COUNT.store(installed.len(), Ordering::Release);
    Ok(())
}

/// How many faces were installed: it grows with every install, so a holder
/// that saw the same number has every installed face.
pub fn installed_font_generation() -> usize {
    INSTALLED_COUNT.load(Ordering::Acquire)
}

pub(crate) fn installed_faces() -> Vec<SoftwareTextFont> {
    INSTALLED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

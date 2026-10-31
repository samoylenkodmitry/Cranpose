//! Font files an app installs while it runs, such as a font pack it
//! downloaded after launch.
//!
//! Fonts made before an install keep laying out with their own faces. Their
//! holder asks [`TextFonts::with_installed_faces`] once a frame and, after an
//! install, gets the fonts with the installed faces after their own.
//!
//! [`TextFonts::with_installed_faces`]: crate::text_fonts::TextFonts::with_installed_faces

use std::{
    path::Path,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

use cranpose_text::{Typeface, TypefaceError};
use cranpose_ui::text::{FontFamily, FontStyle, FontWeight};

use crate::font_source::{FontLoadError, read_font_file};

static INSTALLED: Mutex<Vec<Typeface>> = Mutex::new(Vec::new());
/// How many faces `INSTALLED` holds, read without its lock every frame.
static INSTALLED_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Installs the font file at `path` as `family`, with one face at each of
/// `weights`; a variable font is instanced at each weight.
///
/// Text laid out from the next frame on uses the faces for the characters the
/// app's own fonts lack, and a style naming `family` selects them; text that
/// names no family keeps the app's own faces, also for the letters the
/// installed font has too. The file is read once and kept for the life of
/// the process; installing it again adds nothing.
pub fn install_font_file(
    family: &FontFamily,
    path: &Path,
    weights: &[FontWeight],
) -> Result<(), FontLoadError> {
    let bytes = read_font_file(path)?;
    install(family, bytes, weights).map_err(|source| FontLoadError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Installs a font the app holds for the life of the process, such as one
/// embedded in it, as [`install_font_file`] installs a file.
pub fn install_font_bytes(
    family: &FontFamily,
    bytes: &'static [u8],
    weights: &[FontWeight],
) -> Result<(), FontLoadError> {
    install(family, bytes, weights).map_err(|source| FontLoadError::ParseBytes { source })
}

fn install(
    family: &FontFamily,
    bytes: &'static [u8],
    weights: &[FontWeight],
) -> Result<(), TypefaceError> {
    let faces = weights
        .iter()
        .map(|&weight| Typeface::from_registered(family, weight, FontStyle::Normal, bytes, &[]))
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

pub(crate) fn installed_faces() -> Vec<Typeface> {
    INSTALLED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

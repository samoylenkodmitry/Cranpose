//! The typefaces a renderer hands the app it draws, which the app's text
//! lays out with.

use std::sync::Arc;

use cranpose_text::{FontFamilyResolver, Typeface};

/// The bytes of the embedded default font.
#[cfg(feature = "embedded-default-font")]
pub const DEFAULT_FONT_BYTES: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");

/// The embedded default font, or `None` in a build without the
/// `embedded-default-font` feature.
pub fn default_typeface() -> Option<Typeface> {
    #[cfg(feature = "embedded-default-font")]
    {
        Typeface::from_bytes(DEFAULT_FONT_BYTES).ok()
    }
    #[cfg(not(feature = "embedded-default-font"))]
    {
        None
    }
}

/// The typefaces an app's text lays out and draws with: the app's own, then
/// the faces installed while it runs.
#[derive(Clone, Default)]
pub struct TextFonts {
    typefaces: Arc<[Typeface]>,
    /// How many of `typefaces`, at its end, are installed faces.
    #[cfg(feature = "installed-fonts")]
    installed: usize,
}

impl TextFonts {
    /// The typefaces `typefaces`, the first preferred.
    pub fn new(typefaces: Vec<Typeface>) -> Self {
        Self {
            typefaces: Arc::from(typefaces),
            #[cfg(feature = "installed-fonts")]
            installed: 0,
        }
    }

    /// Parses each of `fonts` and keeps those that parse; with none that
    /// parses, the embedded default font when the build has it.
    pub fn from_fonts_or_default(fonts: &[&'static [u8]]) -> Self {
        let mut typefaces: Vec<Typeface> = fonts
            .iter()
            .filter_map(|bytes| Typeface::from_bytes(*bytes).ok())
            .collect();
        if typefaces.is_empty() {
            typefaces.extend(default_typeface());
        }
        Self::new(typefaces)
    }

    /// The typefaces, the first preferred.
    pub fn typefaces(&self) -> &[Typeface] {
        &self.typefaces
    }

    /// The resolver text lays out with over these typefaces.
    pub fn resolver(&self) -> FontFamilyResolver {
        FontFamilyResolver::new(self.typefaces.iter().cloned())
    }

    /// These fonts with the faces installed through
    /// [`install_font_file`](crate::installed_fonts::install_font_file) after
    /// their own, or `None` while no face was installed since they were made.
    #[cfg(feature = "installed-fonts")]
    pub fn with_installed_faces(&self) -> Option<Self> {
        if crate::installed_fonts::installed_font_generation() == self.installed {
            return None;
        }
        let installed = crate::installed_fonts::installed_faces();
        let own = &self.typefaces[..self.typefaces.len() - self.installed];
        let mut typefaces = Vec::with_capacity(own.len() + installed.len());
        typefaces.extend_from_slice(own);
        typefaces.extend_from_slice(&installed);
        Some(Self {
            typefaces: Arc::from(typefaces),
            installed: installed.len(),
        })
    }
}

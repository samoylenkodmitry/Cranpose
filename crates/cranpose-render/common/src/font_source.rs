//! Turning app-declared font families into parsed faces.
//!
//! [`FontFamily::FileBacked`] and [`FontFamily::LoadedTypeface`] name faces by
//! file rather than by a name inside the font, so something has to read those
//! files and hand the bytes to the rasterizer. That is this module: it parses
//! each face exactly once, at registration, and produces an immutable
//! [`SoftwareTextFontSet`] that measurement and rasterization then share.
//!
//! Nothing here runs per frame or per string. A registry is built at startup,
//! consumed into a font set, and the font set is cloned (it is `Arc`-backed)
//! into every measurer and rasterizer that needs it.

use std::io::Read;
use std::path::{Path, PathBuf};

use cranpose_ui::text::{FontFamily, FontFile, FontStyle, FontWeight};

use crate::software_text_raster::{
    default_software_text_font, SoftwareTextFont, SoftwareTextFontError, SoftwareTextFontSet,
};

/// Directory Android keeps its system font files in.
pub const ANDROID_SYSTEM_FONT_DIR: &str = "/system/fonts";

/// The weights [`SoftwareTextFontRegistry::register_system_family`] registers
/// when an app does not name its own: Compose's Regular/Medium/Bold set.
pub const DEFAULT_SYSTEM_FAMILY_WEIGHTS: &[FontWeight] =
    &[FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::BOLD];

/// Why an app-supplied face could not be registered.
///
/// Every variant is recoverable: the caller logs it and keeps whatever faces
/// did load, and resolution falls back to the default face for families that
/// ended up with none.
#[derive(Debug, thiserror::Error)]
pub enum FontLoadError {
    #[error("font family declares no faces")]
    EmptyFamily,
    #[error("font family is not backed by files, so it has nothing to load")]
    NotFileBacked,
    #[error("no system font file for this family under {directory}")]
    NoSystemFontFile { directory: PathBuf },
    #[error("failed to read font file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse font file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: SoftwareTextFontError,
    },
    #[error("failed to parse font bytes: {source}")]
    ParseBytes {
        #[source]
        source: SoftwareTextFontError,
    },
}

/// Parsed app-supplied faces, on their way to a [`SoftwareTextFontSet`].
///
/// Register everything an app needs once at startup, then call
/// [`SoftwareTextFontRegistry::into_font_set_or_default`]. Registration is
/// where files are read and faces parsed; nothing after it touches the disk.
#[derive(Clone, Default)]
pub struct SoftwareTextFontRegistry {
    faces: Vec<SoftwareTextFont>,
}

impl SoftwareTextFontRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register every face of a file-backed family, reading each file from the
    /// filesystem.
    ///
    /// Each [`FontFile`]'s declared weight and style are what resolution
    /// matches on, so one family can carry Regular/Medium/Bold/Italic faces and
    /// a `FontWeight`/`FontStyle` picks between them. A family whose files all
    /// fail to load registers nothing and reports the first failure; text
    /// asking for it then falls back to the default face rather than
    /// disappearing.
    pub fn register_family(&mut self, family: &FontFamily) -> Result<(), FontLoadError> {
        let files = font_files_for(family)?;
        if files.is_empty() {
            return Err(FontLoadError::EmptyFamily);
        }

        let mut first_error = None;
        let mut loaded = 0usize;
        for file in &files {
            match self.register_face_path(family, file.weight, file.style, &file.path) {
                Ok(()) => loaded += 1,
                Err(error) => first_error = first_error.or(Some(error)),
            }
        }

        match first_error {
            Some(error) if loaded == 0 => Err(error),
            _ => Ok(()),
        }
    }

    /// Register one face read from `path`.
    pub fn register_face_path(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        path: impl AsRef<Path>,
    ) -> Result<(), FontLoadError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| FontLoadError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let face = SoftwareTextFont::from_registered_bytes(family, weight, style, bytes).map_err(
            |source| FontLoadError::Parse {
                path: path.to_path_buf(),
                source,
            },
        )?;
        self.faces.push(face);
        Ok(())
    }

    /// Register one face read from an arbitrary stream.
    ///
    /// This is the seam for fonts that are not files on disk — an APK asset
    /// opened through `AndroidApp::asset_manager()`, an archive entry, a
    /// download cache. It mirrors
    /// `SoftwareTextMeasurer::register_hyphenation_dictionary_reader`.
    pub fn register_face_reader(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        reader: &mut impl Read,
    ) -> Result<(), FontLoadError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|source| FontLoadError::Read {
                path: PathBuf::new(),
                source,
            })?;
        self.register_face_bytes(family, weight, style, bytes)
    }

    /// Register one face from bytes the app already holds.
    pub fn register_face_bytes(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<(), FontLoadError> {
        let face = SoftwareTextFont::from_registered_bytes(family, weight, style, bytes)
            .map_err(|source| FontLoadError::ParseBytes { source })?;
        self.faces.push(face);
        Ok(())
    }

    /// Register a face that belongs to no declared family.
    ///
    /// These are the fallbacks: they are eligible for any request that names no
    /// family, and for a `Named` request their own `name` table decides.
    pub fn register_fallback_bytes(
        &mut self,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<(), FontLoadError> {
        let face = SoftwareTextFont::from_bytes(bytes)
            .map_err(|source| FontLoadError::ParseBytes { source })?;
        self.faces.push(face);
        Ok(())
    }

    /// Register the platform's own face for a generic family alias, at each of
    /// `weights`, so styles keep naming `FontFamily::SansSerif` and get the
    /// real system typeface.
    ///
    /// Android backs `sans-serif` with a single variable `Roboto-Regular.ttf`
    /// and describes each weight as a `wght` axis position on it, so most
    /// devices resolve every weight to one file instanced several ways. Where a
    /// build does ship weight-specific static files (`Roboto-Medium.ttf`), they
    /// are preferred. Faces are registered in `FontStyle::Normal`; an app that
    /// wants a real italic rather than a synthesized slant should call
    /// [`SoftwareTextFontRegistry::register_system_face`] for it, because each
    /// extra face is another copy of the file's bytes.
    pub fn register_system_family(
        &mut self,
        directory: impl AsRef<Path>,
        family: &FontFamily,
        weights: &[FontWeight],
    ) -> Result<(), FontLoadError> {
        let directory = directory.as_ref();
        let mut first_error = None;
        let mut loaded = 0usize;
        for weight in weights {
            match self.register_system_face(directory, family, *weight, FontStyle::Normal) {
                Ok(()) => loaded += 1,
                Err(error) => first_error = first_error.or(Some(error)),
            }
        }

        match first_error {
            Some(error) if loaded == 0 => Err(error),
            _ => Ok(()),
        }
    }

    /// Register one weight/style of a generic family alias from the platform's
    /// font directory.
    pub fn register_system_face(
        &mut self,
        directory: impl AsRef<Path>,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
    ) -> Result<(), FontLoadError> {
        let directory = directory.as_ref();
        let path = system_font_file(directory, family, weight).ok_or_else(|| {
            FontLoadError::NoSystemFontFile {
                directory: directory.to_path_buf(),
            }
        })?;
        self.register_face_path(family, weight, style, path)
    }

    /// The faces registered so far.
    pub fn faces(&self) -> &[SoftwareTextFont] {
        &self.faces
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Finish, folding in the static byte slices from `AppLauncher::with_fonts`
    /// as unregistered fallbacks and the embedded default face when nothing
    /// else loaded.
    pub fn into_font_set_or_default(mut self, fonts: &[&[u8]]) -> SoftwareTextFontSet {
        for bytes in fonts {
            // Unparseable app bytes have always been skipped rather than fatal.
            let _ = self.register_fallback_bytes((*bytes).to_vec());
        }
        if self.faces.is_empty() {
            if let Some(default_font) = default_software_text_font() {
                self.faces.push(default_font);
            }
        }
        SoftwareTextFontSet::from_faces(self.faces)
    }
}

/// The file a platform backs `family` with at `weight`, if one is present.
///
/// Weight-specific static files win when the build ships them; otherwise the
/// family's regular file is returned and instanced on its `wght` axis at
/// registration.
pub fn system_font_file(
    directory: &Path,
    family: &FontFamily,
    weight: FontWeight,
) -> Option<PathBuf> {
    let files = system_family_files(family)?;
    files
        .weighted
        .iter()
        .filter(|(candidate_weight, _)| *candidate_weight == weight.value())
        .map(|(_, name)| directory.join(name))
        .chain(files.regular.iter().map(|name| directory.join(name)))
        .find(|path| path.is_file())
}

/// Files a platform is known to back a generic family with, best first.
struct SystemFamilyFiles {
    regular: &'static [&'static str],
    weighted: &'static [(u16, &'static str)],
}

fn system_family_files(family: &FontFamily) -> Option<SystemFamilyFiles> {
    // Names come from Android's `/system/fonts`; the alias-to-file mapping is
    // the one `/system/etc/fonts.xml` describes, which that file itself warns
    // third parties not to parse.
    match family {
        FontFamily::Default | FontFamily::SansSerif => Some(SystemFamilyFiles {
            regular: &[
                "Roboto-Regular.ttf",
                "RobotoStatic-Regular.ttf",
                "NotoSans-Regular.ttf",
                "DroidSans.ttf",
            ],
            weighted: &[
                (300, "Roboto-Light.ttf"),
                (500, "Roboto-Medium.ttf"),
                (700, "Roboto-Bold.ttf"),
                (900, "Roboto-Black.ttf"),
            ],
        }),
        // Android aliases `fantasy` to `serif`.
        FontFamily::Serif | FontFamily::Fantasy => Some(SystemFamilyFiles {
            regular: &["NotoSerif-Regular.ttf", "DroidSerif-Regular.ttf"],
            weighted: &[(700, "NotoSerif-Bold.ttf"), (700, "DroidSerif-Bold.ttf")],
        }),
        FontFamily::Monospace => Some(SystemFamilyFiles {
            regular: &[
                "DroidSansMono.ttf",
                "RobotoMono-Regular.ttf",
                "CutiveMono-Regular.ttf",
            ],
            weighted: &[(700, "RobotoMono-Bold.ttf")],
        }),
        FontFamily::Cursive => Some(SystemFamilyFiles {
            regular: &["DancingScript-Regular.ttf"],
            weighted: &[(700, "DancingScript-Bold.ttf")],
        }),
        FontFamily::Named(_) | FontFamily::FileBacked(_) | FontFamily::LoadedTypeface(_) => None,
    }
}

fn font_files_for(family: &FontFamily) -> Result<Vec<FontFile>, FontLoadError> {
    match family {
        FontFamily::FileBacked(file_backed) => Ok(file_backed.fonts.clone()),
        FontFamily::LoadedTypeface(typeface) => Ok(vec![FontFile::new(typeface.path.clone())]),
        _ => Err(FontLoadError::NotFileBacked),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui::text::{SpanStyle, TextStyle};
    use std::io::Cursor;

    const REGULAR: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");
    const BOLD: &[u8] = include_bytes!("../assets/NotoSansBold.ttf");

    fn style_for(family: &FontFamily, weight: FontWeight) -> TextStyle {
        TextStyle {
            span_style: SpanStyle {
                font_family: Some(family.clone()),
                font_weight: Some(weight),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// A directory that removes itself, so font-loading tests can exercise real
    /// filesystem failures instead of a stubbed reader. Lives under the
    /// workspace `target/test-output`, never tmpfs.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(name: &str) -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../target/test-output/cranpose-font-source")
                .join(name);
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("scratch directory");
            Self(path)
        }

        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, bytes).expect("scratch font file");
            path
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn register_family_picks_the_face_matching_the_requested_weight() {
        let dir = ScratchDir::new("weights");
        let regular = dir.write("Test-Regular.ttf", REGULAR);
        let bold = dir.write("Test-Bold.ttf", BOLD);
        let family = FontFamily::file_backed(vec![
            FontFile::new(regular.to_string_lossy().into_owned()),
            FontFile::new(bold.to_string_lossy().into_owned()).with_weight(FontWeight::BOLD),
        ])
        .expect("file-backed family");

        let mut registry = SoftwareTextFontRegistry::new();
        registry.register_family(&family).expect("family loads");
        let fonts = registry.into_font_set_or_default(&[]);

        let resolved_regular = fonts
            .resolve(&style_for(&family, FontWeight::NORMAL))
            .expect("regular face");
        let resolved_bold = fonts
            .resolve(&style_for(&family, FontWeight::BOLD))
            .expect("bold face");

        assert_eq!(resolved_regular.weight(), FontWeight::NORMAL);
        assert_eq!(resolved_bold.weight(), FontWeight::BOLD);
        assert_ne!(
            resolved_regular.content_hash(),
            resolved_bold.content_hash(),
            "distinct faces must key the glyph atlas distinctly"
        );
    }

    #[test]
    fn register_family_honours_a_declared_weight_over_the_face_header() {
        let dir = ScratchDir::new("declared");
        let path = dir.write("Test-Regular.ttf", REGULAR);
        let family =
            FontFamily::file_backed(vec![
                FontFile::new(path.to_string_lossy().into_owned()).with_weight(FontWeight::MEDIUM)
            ])
            .expect("file-backed family");

        let mut registry = SoftwareTextFontRegistry::new();
        registry.register_family(&family).expect("family loads");
        let fonts = registry.into_font_set_or_default(&[]);

        let resolved = fonts
            .resolve(&style_for(&family, FontWeight::MEDIUM))
            .expect("declared face");
        assert_eq!(resolved.weight(), FontWeight::MEDIUM);
    }

    #[test]
    fn register_family_reports_a_missing_file_without_panicking() {
        let dir = ScratchDir::new("missing");
        let family = FontFamily::file_backed(vec![FontFile::new(
            dir.path().join("Absent.ttf").to_string_lossy().into_owned(),
        )])
        .expect("file-backed family");

        let mut registry = SoftwareTextFontRegistry::new();
        let error = registry
            .register_family(&family)
            .expect_err("a missing file cannot register");
        assert!(matches!(error, FontLoadError::Read { .. }), "{error}");
        assert!(registry.is_empty());
    }

    #[test]
    fn register_family_reports_a_corrupt_file_without_panicking() {
        let dir = ScratchDir::new("corrupt");
        let path = dir.write("Corrupt.ttf", b"this is not a font");
        let family =
            FontFamily::file_backed(vec![FontFile::new(path.to_string_lossy().into_owned())])
                .expect("file-backed family");

        let mut registry = SoftwareTextFontRegistry::new();
        let error = registry
            .register_family(&family)
            .expect_err("a corrupt file cannot register");
        assert!(matches!(error, FontLoadError::Parse { .. }), "{error}");
        assert!(registry.is_empty());
    }

    #[test]
    fn a_family_that_failed_to_load_falls_back_to_the_default_face() {
        let dir = ScratchDir::new("fallback");
        let family = FontFamily::file_backed(vec![FontFile::new(
            dir.path().join("Absent.ttf").to_string_lossy().into_owned(),
        )])
        .expect("file-backed family");

        let mut registry = SoftwareTextFontRegistry::new();
        let _ = registry.register_family(&family);
        let fonts = registry.into_font_set_or_default(&[]);

        let resolved = fonts
            .resolve(&style_for(&family, FontWeight::NORMAL))
            .expect("fallback face");
        assert_eq!(
            resolved.content_hash(),
            fonts.default_font().expect("default face").content_hash()
        );
    }

    #[test]
    fn a_partly_loadable_family_keeps_the_faces_that_did_load() {
        let dir = ScratchDir::new("partial");
        let regular = dir.write("Test-Regular.ttf", REGULAR);
        let family = FontFamily::file_backed(vec![
            FontFile::new(regular.to_string_lossy().into_owned()),
            FontFile::new(dir.path().join("Absent.ttf").to_string_lossy().into_owned())
                .with_weight(FontWeight::BOLD),
        ])
        .expect("file-backed family");

        let mut registry = SoftwareTextFontRegistry::new();
        registry
            .register_family(&family)
            .expect("one readable face is enough");
        assert_eq!(registry.faces().len(), 1);
    }

    #[test]
    fn register_face_reader_accepts_a_font_that_is_not_a_file() {
        let family = FontFamily::named("Bundled");
        let mut registry = SoftwareTextFontRegistry::new();
        registry
            .register_face_reader(
                &family,
                FontWeight::NORMAL,
                FontStyle::Normal,
                &mut Cursor::new(REGULAR.to_vec()),
            )
            .expect("streamed face loads");

        let fonts = registry.into_font_set_or_default(&[]);
        let resolved = fonts
            .resolve(&style_for(&family, FontWeight::NORMAL))
            .expect("streamed face");
        assert_eq!(resolved.registered_family(), {
            let mut expected = SoftwareTextFontRegistry::new();
            expected
                .register_face_bytes(
                    &family,
                    FontWeight::NORMAL,
                    FontStyle::Normal,
                    REGULAR.to_vec(),
                )
                .expect("face loads");
            expected.faces()[0].registered_family()
        });
    }

    #[test]
    fn an_empty_registry_falls_back_to_the_embedded_default_face() {
        let fonts = SoftwareTextFontRegistry::new().into_font_set_or_default(&[]);
        assert!(
            fonts.default_font().is_some(),
            "the embedded default font must still serve apps that supply nothing"
        );
        assert!(fonts
            .resolve(&TextStyle::default())
            .is_some_and(|font| font.registered_family().is_none()));
    }

    #[test]
    fn system_font_file_prefers_a_weight_specific_static_face() {
        let dir = ScratchDir::new("system-static");
        dir.write("Roboto-Regular.ttf", REGULAR);
        let medium = dir.write("Roboto-Medium.ttf", BOLD);

        assert_eq!(
            system_font_file(dir.path(), &FontFamily::SansSerif, FontWeight::MEDIUM),
            Some(medium)
        );
    }

    #[test]
    fn system_font_file_falls_back_to_the_regular_face_for_other_weights() {
        let dir = ScratchDir::new("system-regular");
        let regular = dir.write("Roboto-Regular.ttf", REGULAR);

        assert_eq!(
            system_font_file(dir.path(), &FontFamily::SansSerif, FontWeight::MEDIUM),
            Some(regular)
        );
    }

    #[test]
    fn system_font_file_reports_nothing_when_the_directory_is_empty() {
        let dir = ScratchDir::new("system-empty");
        assert_eq!(
            system_font_file(dir.path(), &FontFamily::SansSerif, FontWeight::NORMAL),
            None
        );
        assert_eq!(
            system_font_file(dir.path(), &FontFamily::named("Roboto"), FontWeight::NORMAL),
            None
        );
    }

    #[test]
    fn register_system_family_binds_the_generic_alias_to_the_platform_face() {
        let dir = ScratchDir::new("system-family");
        dir.write("Roboto-Regular.ttf", REGULAR);
        dir.write("Roboto-Bold.ttf", BOLD);

        let mut registry = SoftwareTextFontRegistry::new();
        registry
            .register_system_family(
                dir.path(),
                &FontFamily::SansSerif,
                DEFAULT_SYSTEM_FAMILY_WEIGHTS,
            )
            .expect("system family loads");
        let fonts = registry.into_font_set_or_default(&[]);

        assert!(fonts.has_registered_family(&FontFamily::SansSerif));
        let bold = fonts
            .resolve(&style_for(&FontFamily::SansSerif, FontWeight::BOLD))
            .expect("bold system face");
        assert_eq!(bold.weight(), FontWeight::BOLD);
        assert_eq!(bold.registered_family(), {
            let key = fonts
                .resolve(&style_for(&FontFamily::SansSerif, FontWeight::NORMAL))
                .expect("regular system face")
                .registered_family();
            key
        });
    }

    #[test]
    fn register_system_family_reports_an_absent_font_directory() {
        let mut registry = SoftwareTextFontRegistry::new();
        let error = registry
            .register_system_family(
                Path::new("/definitely/not/a/font/directory"),
                &FontFamily::SansSerif,
                DEFAULT_SYSTEM_FAMILY_WEIGHTS,
            )
            .expect_err("an absent directory cannot register");
        assert!(
            matches!(error, FontLoadError::NoSystemFontFile { .. }),
            "{error}"
        );
        assert!(registry.is_empty());
    }

    #[test]
    fn register_family_rejects_a_family_that_names_no_files() {
        let mut registry = SoftwareTextFontRegistry::new();
        let error = registry
            .register_family(&FontFamily::named("Roboto"))
            .expect_err("a named family has nothing to read");
        assert!(matches!(error, FontLoadError::NotFileBacked), "{error}");
    }

    #[test]
    fn loaded_typeface_families_register_their_single_file() {
        let dir = ScratchDir::new("typeface");
        let path = dir.write("Test-Regular.ttf", REGULAR);
        let family = FontFamily::loaded_typeface_path(path.to_string_lossy().into_owned());

        let mut registry = SoftwareTextFontRegistry::new();
        registry.register_family(&family).expect("typeface loads");
        let fonts = registry.into_font_set_or_default(&[]);

        assert!(fonts.has_registered_family(&family));
        assert!(fonts
            .resolve(&style_for(&family, FontWeight::NORMAL))
            .is_some_and(|font| font.registered_family().is_some()));
    }
}

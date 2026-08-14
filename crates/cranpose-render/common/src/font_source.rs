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
//!
//! Fonts that are not files on disk come in through
//! [`SoftwareTextFontRegistry::register_face_reader`] or
//! [`SoftwareTextFontRegistry::register_face_bytes`]: an APK asset opened with
//! `AndroidApp::asset_manager()`, or anything `cranpose-assets` resolved out of
//! a desktop bundle. `cranpose-assets` is a filesystem path resolver, so it
//! covers bundles but not APK entries, which are not filesystem paths.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cranpose_ui::text::{FontFamily, FontFile, FontStyle, FontWeight};

use crate::software_text_raster::{
    default_software_text_font, FontFamilyKey, SoftwareTextFont, SoftwareTextFontError,
    SoftwareTextFontSet,
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
    /// The declared entries the system-font path has already loaded, so a
    /// second request resolving to the same one does not copy the file again.
    system_faces: Vec<(FontFamilyKey, FontWeight, FontStyle)>,
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

        let mut reads = FontFileReads::default();
        let mut first_error = None;
        let mut loaded = 0usize;
        for file in &files {
            match self.register_read_face(
                &mut reads,
                family,
                file.weight,
                file.style,
                Path::new(&file.path),
            ) {
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
        self.register_read_face(
            &mut FontFileReads::default(),
            family,
            weight,
            style,
            path.as_ref(),
        )
    }

    fn register_read_face(
        &mut self,
        reads: &mut FontFileReads,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        path: &Path,
    ) -> Result<(), FontLoadError> {
        let bytes = reads.read(path)?;
        let face = SoftwareTextFont::from_registered_bytes(family, weight, style, bytes.to_vec())
            .map_err(|source| FontLoadError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
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
    /// are preferred. Each weight is first resolved through
    /// [`system_declared_weight`], so a request the platform's font config does
    /// not declare registers the face Android would have returned for it rather
    /// than a `wght` position Android cannot reach. Faces are registered in
    /// `FontStyle::Normal`; an app that
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
        let mut reads = FontFileReads::default();
        let mut first_error = None;
        let mut loaded = 0usize;
        for weight in weights {
            match self.register_read_system_face(
                &mut reads,
                directory,
                family,
                *weight,
                FontStyle::Normal,
            ) {
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
    ///
    /// `weight` is resolved through [`system_declared_weight`] before anything
    /// is read, so the registered face is one the platform's font config
    /// declares. A caller that wants an arbitrary `wght` position must supply
    /// its own font file and register it as its own family — the system aliases
    /// are the platform's, and only the platform's entries exist in them.
    pub fn register_system_face(
        &mut self,
        directory: impl AsRef<Path>,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
    ) -> Result<(), FontLoadError> {
        self.register_read_system_face(
            &mut FontFileReads::default(),
            directory.as_ref(),
            family,
            weight,
            style,
        )
    }

    fn register_read_system_face(
        &mut self,
        reads: &mut FontFileReads,
        directory: &Path,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
    ) -> Result<(), FontLoadError> {
        // Resolve first, register second: what lands in the set is a weight the
        // platform's font config declares, instanced at the axis position that
        // config pins for it. Asking for 450 registers Android's 400 face, so
        // no caller can reach a face the platform cannot draw.
        let weight = system_declared_weight(family, weight);
        if self.has_system_face(family, weight, style) {
            // Two requests that resolve to one declared entry are one face, as
            // they are one `<font>` element to Android. Registering it twice
            // would copy the file's bytes again — 2.3 MiB for Wear's Roboto —
            // for a face byte-identical to the one already here.
            return Ok(());
        }
        let path = system_font_file(directory, family, weight).ok_or_else(|| {
            FontLoadError::NoSystemFontFile {
                directory: directory.to_path_buf(),
            }
        })?;
        self.register_read_face(reads, family, weight, style, &path)?;
        self.system_faces
            .push((FontFamilyKey::of(family), weight, style));
        Ok(())
    }

    /// Whether the system path already registered this declared entry.
    ///
    /// Only faces this registry loaded from the platform's font directory
    /// count. A face an app registered under the same generic family is its own
    /// font and must not be shadowed by one of ours.
    fn has_system_face(&self, family: &FontFamily, weight: FontWeight, style: FontStyle) -> bool {
        self.system_faces
            .contains(&(FontFamilyKey::of(family), weight, style))
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
    ///
    /// Registered faces come first, so when a request names no family and the
    /// scores tie, a face the app declared wins over one it merely handed over
    /// as bytes.
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

/// Font files read during one registration call.
///
/// A family that instances a single variable file at several weights names that
/// file once per weight, and on Wear the file backing `sans-serif` is 2.3 MiB —
/// worth reading once. Each face still needs its own copy of the bytes, because
/// `ab_glyph` bakes the axis values into the parsed face.
#[derive(Default)]
struct FontFileReads {
    entries: Vec<(PathBuf, Arc<[u8]>)>,
}

impl FontFileReads {
    fn read(&mut self, path: &Path) -> Result<Arc<[u8]>, FontLoadError> {
        if let Some((_, bytes)) = self.entries.iter().find(|(read, _)| read == path) {
            return Ok(Arc::clone(bytes));
        }
        let bytes: Arc<[u8]> = std::fs::read(path)
            .map_err(|source| FontLoadError::Read {
                path: path.to_path_buf(),
                source,
            })?
            .into();
        self.entries.push((path.to_path_buf(), Arc::clone(&bytes)));
        Ok(bytes)
    }
}

/// The weight a platform's own matcher resolves `weight` to for a generic
/// family alias — never a value that family's font config does not declare.
///
/// Android's `sans-serif` is one variable `Roboto-Regular.ttf` described by
/// `/system/etc/fonts.xml` as nine `<font weight="…">` entries, one per hundred,
/// each pinning `wght` to that hundred. The `wght` axis is therefore reachable
/// to the *font config*, not to a caller: an app asking `sans-serif` for 450
/// gets whichever declared entry Minikin's matcher picks, and Minikin has no
/// way to express a weight between two entries. Instancing the axis at 450
/// anyway draws a face the platform cannot draw — text that measures and lays
/// out perfectly and is simply heavier than every other app on the device.
///
/// The rule is `computeMatch` in `frameworks/minikin/libs/minikin/FontFamily.cpp`:
///
/// ```text
/// int score = abs(style1.weight() / 100 - style2.weight() / 100);
/// if (style1.slant() != style2.slant()) score += 2;
/// ```
///
/// picked by `getClosestMatch`, which keeps a candidate only on `match <
/// bestMatch`. Two consequences carry the behaviour, and both are load-bearing:
///
/// * The division is **integer**, so the request is truncated to its hundred
///   before anything is compared. Against a full hundreds grid the nearest
///   declared entry is therefore always the hundred *below* — 450 resolves to
///   400 and 550 to 500, not by a tie-break but outright, and 599 resolves to
///   500 too.
/// * Where a request does tie between two declared entries — 500 against a
///   `serif` declaring only 400 and 700 scores 1 and 2, but 600 scores 2 and 1,
///   and a family declaring 300 and 500 ties at 400 — the strict `<` keeps the
///   entry declared **first**. Android declares ascending, so a tie goes to the
///   lighter face.
///
/// A slant mismatch costs 2, i.e. 200 weight units, which is why `style` never
/// competes with `weight` here: this resolves within one slant, as
/// [`SoftwareTextFontRegistry::register_system_face`] registers one.
///
/// Families this crate knows no system files for resolve to `weight` unchanged;
/// there is no declared set to honour, and nothing will register for them.
///
/// This applies to the system-font path alone. A face an app supplies is its
/// own font, not an entry in the platform's config, and stays instanceable at
/// any axis value — that is what a variable font is for.
pub fn system_declared_weight(family: &FontFamily, weight: FontWeight) -> FontWeight {
    let Some(files) = system_family_files(family) else {
        return weight;
    };
    closest_declared_weight(files.declared, weight).unwrap_or(weight)
}

/// `getClosestMatch` over a declared weight set, at one slant.
fn closest_declared_weight(declared: &[u16], requested: FontWeight) -> Option<FontWeight> {
    let mut best: Option<(u16, u16)> = None;
    for candidate in declared {
        let score = weight_match_score(*candidate, requested.value());
        // Strictly better only, so the first-declared entry keeps a tie.
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((*candidate, score));
        }
    }
    best.map(|(candidate, _)| FontWeight(candidate))
}

/// Minikin's `computeMatch`, weight half — integer hundreds, then distance.
fn weight_match_score(declared: u16, requested: u16) -> u16 {
    (declared / 100).abs_diff(requested / 100)
}

/// The file a platform backs `family` with at `weight`, if one is present.
///
/// Weight-specific static files win when the build ships them; otherwise the
/// family's regular file is returned and instanced on its `wght` axis at
/// registration. `weight` is expected to be one the family declares — callers
/// on the system path run it through [`system_declared_weight`] first.
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

/// Files a platform is known to back a generic family with, best first, and the
/// weights its font config declares for that alias.
struct SystemFamilyFiles {
    regular: &'static [&'static str],
    weighted: &'static [(u16, &'static str)],
    /// Every `weight` an Android `<family>` element declares for this alias, in
    /// declaration order — which is what
    /// [`system_declared_weight`] matches against, and the order its
    /// tie-break depends on.
    declared: &'static [u16],
}

/// The weights Android declares for an alias its font config backs with one
/// variable file: an entry per hundred, each naming the same file at a
/// different `wght` axis position.
const DECLARED_HUNDREDS: &[u16] = &[100, 200, 300, 400, 500, 600, 700, 800, 900];

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
            declared: DECLARED_HUNDREDS,
        }),
        // Android aliases `fantasy` to `serif`.
        FontFamily::Serif | FontFamily::Fantasy => Some(SystemFamilyFiles {
            regular: &["NotoSerif-Regular.ttf", "DroidSerif-Regular.ttf"],
            weighted: &[(700, "NotoSerif-Bold.ttf"), (700, "DroidSerif-Bold.ttf")],
            declared: &[400, 700],
        }),
        FontFamily::Monospace => Some(SystemFamilyFiles {
            regular: &[
                "DroidSansMono.ttf",
                "RobotoMono-Regular.ttf",
                "CutiveMono-Regular.ttf",
            ],
            weighted: &[(700, "RobotoMono-Bold.ttf")],
            declared: &[400, 700],
        }),
        FontFamily::Cursive => Some(SystemFamilyFiles {
            regular: &["DancingScript-Regular.ttf"],
            weighted: &[(700, "DancingScript-Bold.ttf")],
            declared: &[400, 700],
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
    #[cfg(feature = "embedded-default-font")]
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
    #[cfg(feature = "embedded-default-font")]
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
    fn a_system_weight_the_font_config_does_not_declare_resolves_to_the_declared_one() {
        // Minikin truncates to the hundred before it compares, so against a
        // full hundreds grid the entry below always wins outright.
        for (requested, expected) in [(450u16, 400u16), (550, 500), (401, 400), (599, 500)] {
            assert_eq!(
                system_declared_weight(&FontFamily::SansSerif, FontWeight(requested)),
                FontWeight(expected),
                "sans-serif {requested}"
            );
        }
        // A weight the config does declare is returned untouched.
        for weight in DECLARED_HUNDREDS {
            assert_eq!(
                system_declared_weight(&FontFamily::SansSerif, FontWeight(*weight)),
                FontWeight(*weight)
            );
        }
        // Off the ends there is nothing below to fall to.
        assert_eq!(
            system_declared_weight(&FontFamily::SansSerif, FontWeight(50)),
            FontWeight(100)
        );
        assert_eq!(
            system_declared_weight(&FontFamily::SansSerif, FontWeight(1000)),
            FontWeight(900)
        );
    }

    #[test]
    fn a_sparse_system_family_resolves_by_distance_and_keeps_the_lighter_face_on_a_tie() {
        // `serif` declares 400 and 700 only. 500 is one hundred from 400 and
        // two from 700; 600 is the other way round; 550 truncates to 5 and so
        // is not the tie it looks like.
        for (requested, expected) in [(500u16, 400u16), (550, 400), (600, 700), (650, 700)] {
            assert_eq!(
                system_declared_weight(&FontFamily::Serif, FontWeight(requested)),
                FontWeight(expected),
                "serif {requested}"
            );
        }
        // The tie-break proper: equal scores keep the entry declared first,
        // which for Android's ascending declarations is the lighter face.
        assert_eq!(
            closest_declared_weight(&[300, 500], FontWeight(400)),
            Some(FontWeight(300))
        );
        assert_eq!(
            closest_declared_weight(&[500, 300], FontWeight(400)),
            Some(FontWeight(500)),
            "declaration order, not magnitude, is what breaks the tie"
        );
    }

    #[test]
    fn a_family_with_no_system_files_keeps_the_weight_it_was_given() {
        // Nothing declares a weight set for these, and nothing will register
        // for them either, so there is no rule to apply.
        assert_eq!(
            system_declared_weight(&FontFamily::named("Roboto"), FontWeight(450)),
            FontWeight(450)
        );
    }

    #[test]
    fn register_system_face_registers_the_declared_weight_not_the_requested_one() {
        let dir = ScratchDir::new("system-undeclared-weight");
        dir.write("Roboto-Regular.ttf", REGULAR);

        let mut registry = SoftwareTextFontRegistry::new();
        registry
            .register_system_face(
                dir.path(),
                &FontFamily::SansSerif,
                FontWeight(450),
                FontStyle::Normal,
            )
            .expect("system face loads");
        let fonts = registry.into_font_set_or_default(&[]);

        // The assertion is on the resolved face, not on the constant that was
        // asked for: what matters is which face text actually draws with.
        let resolved = fonts
            .resolve(&style_for(&FontFamily::SansSerif, FontWeight(450)))
            .expect("a face for the off-grid request");
        assert_eq!(
            resolved.weight(),
            FontWeight::NORMAL,
            "450 must land on the 400 entry Android's matcher returns"
        );

        let mut declared = SoftwareTextFontRegistry::new();
        declared
            .register_system_face(
                dir.path(),
                &FontFamily::SansSerif,
                FontWeight::NORMAL,
                FontStyle::Normal,
            )
            .expect("system face loads");
        let declared = declared.into_font_set_or_default(&[]);
        assert_eq!(
            resolved.content_hash(),
            declared
                .resolve(&style_for(&FontFamily::SansSerif, FontWeight::NORMAL))
                .expect("the 400 face")
                .content_hash(),
            "the off-grid request must produce the very same face, glyph masks included"
        );
    }

    #[test]
    fn system_requests_that_resolve_alike_register_one_face() {
        let dir = ScratchDir::new("system-dedupe");
        dir.write("Roboto-Regular.ttf", REGULAR);

        let mut registry = SoftwareTextFontRegistry::new();
        for weight in [400u16, 450, 499] {
            registry
                .register_system_face(
                    dir.path(),
                    &FontFamily::SansSerif,
                    FontWeight(weight),
                    FontStyle::Normal,
                )
                .expect("system face loads");
        }

        assert_eq!(
            registry.faces().len(),
            1,
            "three requests for one declared entry are one face"
        );
    }

    #[test]
    fn an_app_registered_face_keeps_the_weight_it_declares() {
        // The counterpart to the system rule: an app's own file is not an entry
        // in the platform's config, so an arbitrary axis position is legitimate
        // and must survive registration untouched.
        let dir = ScratchDir::new("app-off-grid");
        let path = dir.write("Test-Regular.ttf", REGULAR);
        let family =
            FontFamily::file_backed(vec![
                FontFile::new(path.to_string_lossy().into_owned()).with_weight(FontWeight(450))
            ])
            .expect("file-backed family");

        let mut registry = SoftwareTextFontRegistry::new();
        registry.register_family(&family).expect("family loads");
        let fonts = registry.into_font_set_or_default(&[]);

        assert_eq!(
            fonts
                .resolve(&style_for(&family, FontWeight(450)))
                .expect("app face")
                .weight(),
            FontWeight(450)
        );
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

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

use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use cranpose_ui::text::{FontFamily, FontFile, FontStyle, FontWeight};

use crate::software_text_raster::{
    FontFamilyKey, SoftwareTextFont, SoftwareTextFontError, SoftwareTextFontSet,
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
/// [`SoftwareTextFontRegistry::into_font_set`]. Registration is
/// where files are read and faces parsed; nothing after it touches the disk.
#[derive(Clone, Default)]
pub struct SoftwareTextFontRegistry {
    faces: Vec<SoftwareTextFont>,
    system_faces: Vec<(FontFamilyKey, FontWeight, FontStyle)>,
}

#[derive(Default)]
struct TolerantLoad {
    first_error: Option<FontLoadError>,
    loaded: usize,
}

impl TolerantLoad {
    fn record(&mut self, result: Result<(), FontLoadError>) {
        match result {
            Ok(()) => self.loaded += 1,
            Err(error) => self.first_error = self.first_error.take().or(Some(error)),
        }
    }

    fn finish(self) -> Result<(), FontLoadError> {
        match self.first_error {
            Some(error) if self.loaded == 0 => Err(error),
            _ => Ok(()),
        }
    }
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
        let mut load = TolerantLoad::default();
        for file in &files {
            load.record(self.register_read_face(
                &mut reads,
                family,
                file.weight,
                file.style,
                Path::new(&file.path),
                &[],
            ));
        }
        load.finish()
    }

    fn register_read_face(
        &mut self,
        reads: &mut FontFileReads,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        path: &Path,
        variations: &[([u8; 4], f32)],
    ) -> Result<(), FontLoadError> {
        let bytes = reads.read(path)?;
        let face = SoftwareTextFont::from_registered_bytes_with_variations(
            family,
            weight,
            style,
            bytes.to_vec(),
            variations,
        )
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
        self.register_face_bytes_with_variations(family, weight, style, bytes, &[])
    }

    /// Register bytes with explicit OpenType axes, overriding the declared weight/style
    /// coordinates. Invalid axes fail without registering a face.
    pub fn register_face_bytes_with_variations(
        &mut self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        bytes: impl Into<Vec<u8>>,
        variations: &[([u8; 4], f32)],
    ) -> Result<(), FontLoadError> {
        let face = SoftwareTextFont::from_registered_bytes_with_variations(
            family, weight, style, bytes, variations,
        )
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
        let mut load = TolerantLoad::default();
        for weight in weights {
            load.record(self.register_read_system_face(
                &mut reads,
                directory,
                family,
                *weight,
                FontStyle::Normal,
                &[],
            ));
        }
        load.finish()
    }

    /// Register one weight/style of a generic family alias from the platform's
    /// font directory.
    ///
    /// `weight` is resolved through [`system_declared_weight`] before anything
    /// is read, so the registered face is one the platform's font config
    /// declares. Use [`Self::register_system_face_with_variations`] when matching
    /// explicit coordinates supplied by the platform's text API.
    pub fn register_system_face(
        &mut self,
        directory: impl AsRef<Path>,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
    ) -> Result<(), FontLoadError> {
        self.register_system_face_with_variations(directory, family, weight, style, &[])
    }

    /// Register a system face at explicit OpenType coordinates, such as the optical
    /// size and weight returned by a platform text API. The first registration of a
    /// family/weight/style wins, as with [`Self::register_system_face`].
    pub fn register_system_face_with_variations(
        &mut self,
        directory: impl AsRef<Path>,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        variations: &[([u8; 4], f32)],
    ) -> Result<(), FontLoadError> {
        self.register_read_system_face(
            &mut FontFileReads::default(),
            directory.as_ref(),
            family,
            weight,
            style,
            variations,
        )
    }

    fn register_read_system_face(
        &mut self,
        reads: &mut FontFileReads,
        directory: &Path,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        variations: &[([u8; 4], f32)],
    ) -> Result<(), FontLoadError> {
        let weight = system_declared_weight(family, weight);
        if self.has_system_face(family, weight, style) {
            return Ok(());
        }
        let path = system_font_file(directory, family, weight).ok_or_else(|| {
            FontLoadError::NoSystemFontFile {
                directory: directory.to_path_buf(),
            }
        })?;
        self.register_read_face(reads, family, weight, style, &path, variations)?;
        self.system_faces
            .push((FontFamilyKey::of(family), weight, style));
        Ok(())
    }

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
    /// as unregistered fallbacks.
    ///
    /// Registered faces come first, so when a request names no family and the
    /// scores tie, a face the app declared wins over one it merely handed over
    /// as bytes. The set holds only what the app supplied; whether the
    /// embedded face serves an app that supplied nothing is the launcher's
    /// decision, made where the binary can leave the face out.
    pub fn into_font_set(mut self, fonts: &[&[u8]]) -> SoftwareTextFontSet {
        for bytes in fonts {
            let _ = self.register_fallback_bytes((*bytes).to_vec());
        }
        SoftwareTextFontSet::from_faces(self.faces)
    }
}

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

fn closest_declared_weight(declared: &[u16], requested: FontWeight) -> Option<FontWeight> {
    let mut best: Option<(u16, u16)> = None;
    for candidate in declared {
        let score = weight_match_score(*candidate, requested.value());
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((*candidate, score));
        }
    }
    best.map(|(candidate, _)| FontWeight(candidate))
}

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

struct SystemFamilyFiles {
    regular: &'static [&'static str],
    weighted: &'static [(u16, &'static str)],
    declared: &'static [u16],
}

const DECLARED_HUNDREDS: &[u16] = &[100, 200, 300, 400, 500, 600, 700, 800, 900];

fn system_family_files(family: &FontFamily) -> Option<SystemFamilyFiles> {
    match family {
        FontFamily::Default | FontFamily::SansSerif => Some(SystemFamilyFiles {
            regular: &[
                "Roboto-Regular.ttf",
                "RobotoStatic-Regular.ttf",
                "NotoSans-Regular.ttf",
                "DroidSans.ttf",
                "Core/SFUI.ttf",
                "SFNS.ttf",
            ],
            weighted: &[
                (300, "Roboto-Light.ttf"),
                (500, "Roboto-Medium.ttf"),
                (700, "Roboto-Bold.ttf"),
                (900, "Roboto-Black.ttf"),
            ],
            declared: DECLARED_HUNDREDS,
        }),
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
#[path = "tests/font_source_tests.rs"]
mod tests;

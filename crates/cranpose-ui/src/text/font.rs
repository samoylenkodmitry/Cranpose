/// One font file of a file-backed family, with the weight and style the file provides.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontFile {
    /// Path of the font file.
    pub path: String,
    /// Weight of the face in the file.
    pub weight: FontWeight,
    /// Upright or italic style of the face in the file.
    pub style: FontStyle,
}

impl FontFile {
    /// Creates an entry for the font file at `path` with normal weight and upright style.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        }
    }

    /// Sets the weight the file provides.
    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Sets the style the file provides.
    pub fn with_style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }
}

/// A font family made of font files, one for each weight and style.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileBackedFontFamily {
    /// The files of the family. Never empty when built with [`FileBackedFontFamily::new`].
    pub fonts: Vec<FontFile>,
}

/// Error returned when a font family cannot be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFamilyError {
    /// The list of font files was empty.
    EmptyFileList,
}

impl std::fmt::Display for FontFamilyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontFamilyError::EmptyFileList => {
                write!(f, "file-backed font family requires at least one font file")
            }
        }
    }
}

impl std::error::Error for FontFamilyError {}

impl FileBackedFontFamily {
    /// Builds a family from `fonts`, or returns [`FontFamilyError::EmptyFileList`] when the list
    /// is empty.
    pub fn new(fonts: Vec<FontFile>) -> Result<Self, FontFamilyError> {
        if fonts.is_empty() {
            return Err(FontFamilyError::EmptyFileList);
        }
        Ok(Self { fonts })
    }
}

/// A font family identified by the path of one typeface file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LoadedTypefacePath {
    /// Path of the typeface file.
    pub path: String,
}

impl LoadedTypefacePath {
    /// Creates a family for the typeface file at `path`.
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// The font family of a text. Compose's `FontFamily`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontFamily {
    /// The default font of the platform.
    #[default]
    Default,
    /// A generic sans-serif font.
    SansSerif,
    /// A generic serif font.
    Serif,
    /// A generic monospace font.
    Monospace,
    /// A generic cursive font.
    Cursive,
    /// A generic fantasy font.
    Fantasy,
    /// A family looked up by name among the fonts the renderer has.
    Named(String),
    /// A family built from font files.
    FileBacked(FileBackedFontFamily),
    /// A family identified by the path of a typeface file.
    LoadedTypeface(LoadedTypefacePath),
}

impl FontFamily {
    /// Returns the family for `name`, as [`FontFamily::from_name`] does.
    pub fn named(name: impl Into<String>) -> Self {
        Self::from_name(&name.into())
    }

    /// Returns the generic family for `name` when it is a generic name such as `serif` or
    /// `sans-serif`, or an empty or default name, and `Named` for any other name.
    pub fn from_name(name: &str) -> Self {
        match name {
            "" | "Default" | "default" => Self::Default,
            "SansSerif" | "sans-serif" => Self::SansSerif,
            "Serif" | "serif" => Self::Serif,
            "Monospace" | "monospace" => Self::Monospace,
            "Cursive" | "cursive" => Self::Cursive,
            "Fantasy" | "fantasy" => Self::Fantasy,
            value => Self::Named(value.to_string()),
        }
    }

    /// Builds a `FileBacked` family from `fonts`, or fails when the list is empty.
    pub fn file_backed(fonts: Vec<FontFile>) -> Result<Self, FontFamilyError> {
        FileBackedFontFamily::new(fonts).map(Self::FileBacked)
    }

    /// Builds a `LoadedTypeface` family for the typeface file at `path`.
    pub fn loaded_typeface_path(path: impl Into<String>) -> Self {
        Self::LoadedTypeface(LoadedTypefacePath::new(path))
    }

    /// Returns the name of a `Named` family, and `None` for every other variant.
    pub fn family_name(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name.as_str()),
            _ => None,
        }
    }
}

impl From<&str> for FontFamily {
    fn from(value: &str) -> Self {
        Self::from_name(value)
    }
}

impl From<String> for FontFamily {
    fn from(value: String) -> Self {
        Self::from_name(value.as_str())
    }
}

/// Weight of a font, from 1 (thinnest) to 1000 (heaviest), as in Compose's `FontWeight`.
/// 400 is normal and 700 is bold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontWeight(pub u16);

impl FontWeight {
    /// Creates a weight, clamping `weight` into the range 1 to 1000.
    pub const fn new(weight: u16) -> Self {
        Self::clamped(weight)
    }

    /// Creates a weight, or returns `None` when `weight` is outside the range 1 to 1000.
    pub const fn try_new(weight: u16) -> Option<Self> {
        if weight >= 1 && weight <= 1000 {
            Some(Self(weight))
        } else {
            None
        }
    }

    /// Creates a weight, clamping `weight` into the range 1 to 1000.
    pub const fn clamped(weight: u16) -> Self {
        if weight < 1 {
            Self(1)
        } else if weight > 1000 {
            Self(1000)
        } else {
            Self(weight)
        }
    }

    /// Returns the weight as a number from 1 to 1000.
    pub const fn value(self) -> u16 {
        self.0
    }

    /// Weight 100.
    pub const THIN: Self = Self(100);
    /// Weight 200.
    pub const EXTRA_LIGHT: Self = Self(200);
    /// Weight 300.
    pub const LIGHT: Self = Self(300);
    /// Weight 400.
    pub const NORMAL: Self = Self(400);
    /// Weight 500.
    pub const MEDIUM: Self = Self(500);
    /// Weight 600.
    pub const SEMI_BOLD: Self = Self(600);
    /// Weight 700.
    pub const BOLD: Self = Self(700);
    /// Weight 800.
    pub const EXTRA_BOLD: Self = Self(800);
    /// Weight 900.
    pub const BLACK: Self = Self(900);
    /// Weight 100, the same as [`FontWeight::THIN`].
    pub const W100: Self = Self(100);
    /// Weight 200, the same as [`FontWeight::EXTRA_LIGHT`].
    pub const W200: Self = Self(200);
    /// Weight 300, the same as [`FontWeight::LIGHT`].
    pub const W300: Self = Self(300);
    /// Weight 400, the same as [`FontWeight::NORMAL`].
    pub const W400: Self = Self(400);
    /// Weight 500, the same as [`FontWeight::MEDIUM`].
    pub const W500: Self = Self(500);
    /// Weight 600, the same as [`FontWeight::SEMI_BOLD`].
    pub const W600: Self = Self(600);
    /// Weight 700, the same as [`FontWeight::BOLD`].
    pub const W700: Self = Self(700);
    /// Weight 800, the same as [`FontWeight::EXTRA_BOLD`].
    pub const W800: Self = Self(800);
    /// Weight 900, the same as [`FontWeight::BLACK`].
    pub const W900: Self = Self(900);
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// Whether glyphs are upright or italic. Compose's `FontStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    /// Upright glyphs.
    #[default]
    Normal,
    /// Italic glyphs.
    Italic,
}

/// Which missing styles the renderer may fake when the chosen font lacks them.
/// Compose's `FontSynthesis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontSynthesis {
    /// Nothing is synthesized.
    #[default]
    None,
    /// Both bold and italic may be synthesized.
    All,
    /// Only bold may be synthesized.
    Weight,
    /// Only italic may be synthesized.
    Style,
}

#[cfg(test)]
#[path = "tests/font_tests.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontFile {
    pub path: String,
    pub weight: FontWeight,
    pub style: FontStyle,
}

impl FontFile {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        }
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileBackedFontFamily {
    pub fonts: Vec<FontFile>,
}

impl FileBackedFontFamily {
    pub fn new(fonts: Vec<FontFile>) -> Self {
        assert!(
            !fonts.is_empty(),
            "FileBackedFontFamily requires at least one font file"
        );
        Self { fonts }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LoadedTypefacePath {
    pub path: String,
}

impl LoadedTypefacePath {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontFamily {
    Default,
    SansSerif,
    Serif,
    Monospace,
    Cursive,
    Fantasy,
    Named(String),
    FileBacked(FileBackedFontFamily),
    LoadedTypeface(LoadedTypefacePath),
}

impl FontFamily {
    pub fn named(name: impl Into<String>) -> Self {
        Self::from_name(&name.into())
    }

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

    pub fn file_backed(fonts: Vec<FontFile>) -> Self {
        Self::FileBacked(FileBackedFontFamily::new(fonts))
    }

    pub fn loaded_typeface_path(path: impl Into<String>) -> Self {
        Self::LoadedTypeface(LoadedTypefacePath::new(path))
    }

    pub fn family_name(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name.as_str()),
            _ => None,
        }
    }
}

impl Default for FontFamily {
    fn default() -> Self {
        Self::Default
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub fn new(weight: u16) -> Self {
        match Self::try_new(weight) {
            Some(value) => value,
            None => panic!("Font weight must be in range [1, 1000], got {weight}"),
        }
    }

    pub const fn try_new(weight: u16) -> Option<Self> {
        if weight >= 1 && weight <= 1000 {
            Some(Self(weight))
        } else {
            None
        }
    }

    pub const fn value(self) -> u16 {
        self.0
    }

    pub const THIN: Self = Self(100);
    pub const EXTRA_LIGHT: Self = Self(200);
    pub const LIGHT: Self = Self(300);
    pub const NORMAL: Self = Self(400);
    pub const MEDIUM: Self = Self(500);
    pub const SEMI_BOLD: Self = Self(600);
    pub const BOLD: Self = Self(700);
    pub const EXTRA_BOLD: Self = Self(800);
    pub const BLACK: Self = Self(900);
    pub const W100: Self = Self(100);
    pub const W200: Self = Self(200);
    pub const W300: Self = Self(300);
    pub const W400: Self = Self(400);
    pub const W500: Self = Self(500);
    pub const W600: Self = Self(600);
    pub const W700: Self = Self(700);
    pub const W800: Self = Self(800);
    pub const W900: Self = Self(900);
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontSynthesis {
    #[default]
    None,
    All,
    Weight,
    Style,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_family_maps_compose_generic_names() {
        assert_eq!(FontFamily::from_name("Default"), FontFamily::Default);
        assert_eq!(FontFamily::from_name("sans-serif"), FontFamily::SansSerif);
        assert_eq!(FontFamily::from_name("serif"), FontFamily::Serif);
        assert_eq!(FontFamily::from_name("monospace"), FontFamily::Monospace);
        assert_eq!(FontFamily::from_name("cursive"), FontFamily::Cursive);
        assert_eq!(FontFamily::from_name("fantasy"), FontFamily::Fantasy);
    }

    #[test]
    fn font_family_preserves_custom_names() {
        let family = FontFamily::named("Fira Sans");
        assert_eq!(family, FontFamily::Named("Fira Sans".to_string()));
        assert_eq!(family.family_name(), Some("Fira Sans"));
    }

    #[test]
    fn font_family_file_backed_preserves_font_entries() {
        let family = FontFamily::file_backed(vec![
            FontFile::new("/tmp/Roboto-Regular.ttf"),
            FontFile::new("/tmp/Roboto-Bold.ttf").with_weight(FontWeight::BOLD),
        ]);
        let FontFamily::FileBacked(file_backed) = family else {
            panic!("expected file-backed family");
        };
        assert_eq!(file_backed.fonts.len(), 2);
        assert_eq!(file_backed.fonts[0].path, "/tmp/Roboto-Regular.ttf");
        assert_eq!(file_backed.fonts[0].weight, FontWeight::NORMAL);
        assert_eq!(file_backed.fonts[1].path, "/tmp/Roboto-Bold.ttf");
        assert_eq!(file_backed.fonts[1].weight, FontWeight::BOLD);
    }

    #[test]
    fn font_file_builder_applies_style_and_weight() {
        let file = FontFile::new("/tmp/Roboto-Italic.ttf")
            .with_weight(FontWeight::MEDIUM)
            .with_style(FontStyle::Italic);
        assert_eq!(file.path, "/tmp/Roboto-Italic.ttf");
        assert_eq!(file.weight, FontWeight::MEDIUM);
        assert_eq!(file.style, FontStyle::Italic);
    }

    #[test]
    #[should_panic(expected = "requires at least one font file")]
    fn file_backed_font_family_rejects_empty_font_list() {
        let _ = FileBackedFontFamily::new(Vec::new());
    }

    #[test]
    fn font_family_loaded_typeface_path_preserves_path() {
        let family = FontFamily::loaded_typeface_path("/tmp/FiraSans-Regular.ttf");
        let FontFamily::LoadedTypeface(typeface) = family else {
            panic!("expected loaded typeface family");
        };
        assert_eq!(typeface.path, "/tmp/FiraSans-Regular.ttf");
    }

    #[test]
    fn font_weight_default_is_normal() {
        assert_eq!(FontWeight::default(), FontWeight::NORMAL);
    }

    #[test]
    fn font_weight_try_new_validates_range() {
        assert_eq!(FontWeight::try_new(0), None);
        assert_eq!(FontWeight::try_new(1), Some(FontWeight(1)));
        assert_eq!(FontWeight::try_new(1000), Some(FontWeight(1000)));
        assert_eq!(FontWeight::try_new(1001), None);
    }
}

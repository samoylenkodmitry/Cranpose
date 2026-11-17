// The languages the page section switches between. Its words come from the
// app's Fluent catalogs (`locales/<tag>/desktop-app.ftl`); the locale sets
// the layout's direction, so Arabic lays out from the right.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Language {
    English,
    Spanish,
    Japanese,
    Arabic,
}

impl Language {
    pub(super) const ALL: [Self; 4] = [Self::English, Self::Spanish, Self::Japanese, Self::Arabic];

    // The BCP 47 tag of the language's catalog.
    pub(super) fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
            Self::Japanese => "ja",
            Self::Arabic => "ar",
        }
    }

    // The language's name in itself, as its chip shows it in every
    // language.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
            Self::Japanese => "日本語",
            Self::Arabic => "العربية",
        }
    }
}

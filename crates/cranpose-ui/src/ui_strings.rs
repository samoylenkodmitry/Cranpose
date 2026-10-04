/// Framework-owned strings. Applications override them in the `cranpose-ui` namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiString {
    /// Copies selected text.
    Copy,
    /// Cuts selected text.
    Cut,
    /// Pastes the clipboard.
    Paste,
    /// Selects all text.
    SelectAll,
    /// Reverts an edit.
    Undo,
    /// Reapplies an edit.
    Redo,
    /// Default search-field hint and search-button accessibility label.
    Search,
}

impl UiString {
    /// Resolves this label during composition. Without localization, returns English.
    #[track_caller]
    pub fn resolve(self) -> String {
        #[cfg(feature = "localization")]
        {
            crate::localization::localized(&MESSAGES[self as usize], []).into()
        }
        #[cfg(not(feature = "localization"))]
        {
            [
                "Copy",
                "Cut",
                "Paste",
                "Select all",
                "Undo",
                "Redo",
                "Search",
            ][self as usize]
                .to_owned()
        }
    }
}

#[cfg(feature = "localization")]
static SOURCE: crate::localization::SourceCatalog =
    crate::localization::SourceCatalog::new("en", include_str!("../locales/en/cranpose-ui.ftl"));

#[cfg(feature = "localization")]
static MESSAGES: [crate::localization::Message; 7] = {
    use crate::localization::Message;
    [
        Message::new("cranpose-ui", "copy", &SOURCE),
        Message::new("cranpose-ui", "cut", &SOURCE),
        Message::new("cranpose-ui", "paste", &SOURCE),
        Message::new("cranpose-ui", "select-all", &SOURCE),
        Message::new("cranpose-ui", "undo", &SOURCE),
        Message::new("cranpose-ui", "redo", &SOURCE),
        Message::new("cranpose-ui", "search", &SOURCE),
    ]
};

#[cfg(feature = "localization")]
pub(crate) fn catalog() -> crate::localization::Catalog {
    crate::translations!("locales", fallback = "en")
}

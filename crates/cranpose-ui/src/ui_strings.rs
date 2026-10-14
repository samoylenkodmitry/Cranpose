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
    /// Shows the month before in a date picker.
    PreviousMonth,
    /// Shows the month after in a date picker.
    NextMonth,
    /// Names the hour wheel of a time picker.
    Hour,
    /// Names the minute wheel of a time picker.
    Minute,
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
                "Previous month",
                "Next month",
                "Hour",
                "Minute",
            ][self as usize]
                .to_owned()
        }
    }
}

#[cfg(feature = "localization")]
static SOURCE: crate::localization::SourceCatalog =
    crate::localization::SourceCatalog::new("en", include_str!("../locales/en/cranpose-ui.ftl"));

#[cfg(feature = "localization")]
static MESSAGES: [crate::localization::Message; 11] = {
    use crate::localization::Message;
    [
        Message::new("cranpose-ui", "copy", &SOURCE),
        Message::new("cranpose-ui", "cut", &SOURCE),
        Message::new("cranpose-ui", "paste", &SOURCE),
        Message::new("cranpose-ui", "select-all", &SOURCE),
        Message::new("cranpose-ui", "undo", &SOURCE),
        Message::new("cranpose-ui", "redo", &SOURCE),
        Message::new("cranpose-ui", "search", &SOURCE),
        Message::new("cranpose-ui", "previous-month", &SOURCE),
        Message::new("cranpose-ui", "next-month", &SOURCE),
        Message::new("cranpose-ui", "hour", &SOURCE),
        Message::new("cranpose-ui", "minute", &SOURCE),
    ]
};

#[cfg(feature = "localization")]
/// Translations for Cranpose controls, with namespaces applications can override.
pub fn catalog() -> crate::localization::Catalog {
    crate::translations!("locales", fallback = "en")
}

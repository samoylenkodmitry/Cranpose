//! The rows the Credits screen is built from.
//!
//! Trimmed to what this screen uses. The original carries a `Toggle` kind and
//! an action enum for the Settings screen; neither appears on Credits, so
//! neither is here.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Header,
    Toggle,
    Action,
    Label,
}

#[derive(Clone, Debug)]
pub struct SettingsRow {
    pub kind: RowKind,
    pub label: String,
    pub value: String,
    pub checked: bool,
}

impl SettingsRow {
    fn of(kind: RowKind, label: &str) -> Self {
        Self {
            kind,
            label: label.to_string(),
            value: String::new(),
            checked: false,
        }
    }

    pub fn header(label: &str) -> Self {
        Self::of(RowKind::Header, label)
    }

    pub fn label(label: &str) -> Self {
        Self::of(RowKind::Label, label)
    }

    pub fn action(label: &str) -> Self {
        Self::of(RowKind::Action, label)
    }
}

/// The Credits screen's text, verbatim.
///
/// The wrapping is what makes this a useful fixture: these are long paragraphs
/// rather than button labels, so the screen carries several hundred glyphs and
/// every one of them is re-laid as the scaling curve moves its row.
pub fn credits_rows() -> Vec<SettingsRow> {
    let mut rows = vec![
        SettingsRow::header("ORBIT BREAKER"),
        SettingsRow::label("Version 1.0.0"),
        SettingsRow::label("Designed and built for Wear OS."),
        SettingsRow::label("Every graphic and sound in this game is generated inside the project."),
        SettingsRow::label(
            "Built with Rust, the Cranpose UI framework, and wgpu, under the Apache License 2.0 \
             or the MIT License.",
        ),
        SettingsRow::label(
            "Rust, winit, serde and log by their authors, under the Apache License 2.0 or MIT.",
        ),
        SettingsRow::label(
            "Scores, settings, and saved runs stay on this watch. The game's own code makes no \
             network call; Google Play is used for the price, the purchase, and the restore.",
        ),
        SettingsRow::label("The purchase stays with this watch once Google Play reports it."),
        SettingsRow::label("Privacy policy: orbitbreaker.dmitrysamoylenko.in/privacy"),
    ];
    rows.push(SettingsRow::action("Back"));
    rows
}

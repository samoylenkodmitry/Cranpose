use cranpose_core::collections::rare::RareProperties;
use cranpose_ui::{
    CollectionInfo, LiveRegionMode, ProgressBarRangeInfo, ScrollAxisRange, SemanticsDetails,
    SemanticsNode, SemanticsWidgetRole,
};

use super::{reuse_text, reuse_texts};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccessibilityDetails {
    pub(crate) state_description: Option<String>,
    pub(crate) click_label: Option<String>,
    /// What a long press on the control does, named for a reader. Present
    /// only when the control declared a long press at all.
    pub(crate) long_click_label: Option<String>,
    /// What the magic tap does, named for a reader. Present only when the
    /// control declared one.
    pub(crate) magic_tap_label: Option<String>,
    /// The short names Voice Control shows for the control.
    pub(crate) input_labels: Vec<String>,
    /// The language of the control's text, as a BCP 47 tag.
    pub(crate) language: Option<String>,
    pub(crate) custom_actions: Vec<String>,
    pub(crate) live_region: Option<LiveRegionMode>,
    pub(crate) progress: Option<ProgressBarRangeInfo>,
    pub(crate) adjustable: bool,
    pub(crate) vertical_scroll: Option<ScrollAxisRange>,
    pub(crate) horizontal_scroll: Option<ScrollAxisRange>,
    /// Whether a screen reader may ask this list for the row at an index.
    pub(crate) scroll_to_index: bool,
    pub(crate) collection: Option<CollectionInfo>,
    pub(crate) pane_title: Option<String>,
    pub(crate) error: Option<String>,
    /// What using the control does or what it wants, read after the rest.
    pub(crate) hint_text: Option<String>,
    pub(crate) password: bool,
    pub(crate) expanded: Option<bool>,
    pub(crate) dismissable: bool,
    pub(crate) is_modal: bool,
    /// Where the caret of an editable field sits, or which stretch of its
    /// text is picked: the anchor and the end that moves, as byte offsets into
    /// the element's `value`. A field that holds a secret publishes none.
    pub(crate) text_selection: Option<(usize, usize)>,
    pub(crate) multiline: bool,
}

impl AccessibilityDetails {
    pub(super) const NONE: Self = Self {
        state_description: None,
        click_label: None,
        long_click_label: None,
        magic_tap_label: None,
        input_labels: Vec::new(),
        language: None,
        custom_actions: Vec::new(),
        live_region: None,
        progress: None,
        adjustable: false,
        vertical_scroll: None,
        horizontal_scroll: None,
        scroll_to_index: false,
        collection: None,
        pane_title: None,
        error: None,
        hint_text: None,
        password: false,
        expanded: None,
        dismissable: false,
        is_modal: false,
        text_selection: None,
        multiline: false,
    };
}

impl Default for AccessibilityDetails {
    fn default() -> Self {
        Self::NONE
    }
}

impl RareProperties for AccessibilityDetails {
    const EMPTY: &'static Self = &Self::NONE;
}

pub(super) fn fill_node_details(
    held: &mut AccessibilityDetails,
    node: &SemanticsNode,
    live_region: Option<LiveRegionMode>,
) {
    let details = node.details();
    *held = AccessibilityDetails {
        state_description: reuse_text(
            held.state_description.take(),
            details.state_description.as_deref(),
        ),
        click_label: reuse_text(held.click_label.take(), details.on_click_label.as_deref()),
        long_click_label: reuse_text(held.long_click_label.take(), long_click_label(details)),
        magic_tap_label: reuse_text(held.magic_tap_label.take(), magic_tap_label(details)),
        input_labels: reuse_texts(
            std::mem::take(&mut held.input_labels),
            details.input_labels.iter().map(String::as_str),
        ),
        language: reuse_text(held.language.take(), details.language.as_deref()),
        custom_actions: reuse_texts(
            std::mem::take(&mut held.custom_actions),
            details
                .custom_actions
                .iter()
                .map(|action| action.label.as_str()),
        ),
        live_region: live_region.or_else(|| {
            (node.widget_role == Some(SemanticsWidgetRole::Alert))
                .then_some(LiveRegionMode::Assertive)
        }),
        progress: details.progress,
        adjustable: details.set_progress.is_some(),
        vertical_scroll: details.vertical_scroll,
        horizontal_scroll: details.horizontal_scroll,
        scroll_to_index: details.scroll_to_index.is_some(),
        collection: details.collection,
        pane_title: reuse_text(held.pane_title.take(), details.pane_title.as_deref()),
        error: reuse_text(held.error.take(), details.error.as_deref()),
        hint_text: reuse_text(held.hint_text.take(), details.hint_text.as_deref()),
        password: details.password,
        expanded: expansion(details),
        dismissable: details.dismiss.is_some(),
        is_modal: details.is_modal,
        text_selection: details
            .text_selection
            .filter(|_| details.editable_text && !details.password)
            .map(|range| (range.start, range.end)),
        multiline: details.multiline,
    };
}

/// Whether a control reads as open or as closed: one that says what closing
/// it does is open now, and one that says what opening it does is closed.
/// A control that says neither is not a thing a reader opens at all.
fn expansion(details: &SemanticsDetails) -> Option<bool> {
    details
        .collapse
        .is_some()
        .then_some(true)
        .or_else(|| details.expand.is_some().then_some(false))
}

/// What a reader reads out for a control's long press: the verb phrase the
/// app gave, and for a control that declared the action with no phrase the
/// plain words for what it is. A control with no long press gets nothing.
fn long_click_label(details: &SemanticsDetails) -> Option<&str> {
    details.on_long_click.as_ref()?;
    let named = details
        .on_long_click_label
        .as_deref()
        .filter(|label| !label.trim().is_empty());
    Some(named.unwrap_or("long press"))
}

/// What a reader lists for a control's magic tap: the verb phrase the app
/// gave, or the plain words for the gesture. A control with no magic tap
/// gets nothing.
fn magic_tap_label(details: &SemanticsDetails) -> Option<&str> {
    details.on_magic_tap.as_ref()?;
    let named = details
        .on_magic_tap_label
        .as_deref()
        .filter(|label| !label.trim().is_empty());
    Some(named.unwrap_or("magic tap"))
}

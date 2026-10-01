use cranpose_core::collections::rare::{RareProperties, rare, set_rare, update_rare};
use cranpose_foundation::{
    CanvasSemanticsNode, CollectionInfo, LiveRegionMode, ProgressBarRangeInfo, ScrollAxisRange,
    SemanticsConfiguration, SemanticsCustomAction, SemanticsDismiss, SemanticsExpand,
    SemanticsLongClick, SemanticsMagicTap, SemanticsScrollBy, SemanticsScrollToIndex,
    SemanticsSetProgress, SemanticsSetSelection, SemanticsSetText, text::TextRange,
};

use super::SemanticsNode;

/// What a semantics node reports beyond its place, its name, its value and
/// its click: the properties few nodes set. A node holds them only when it
/// sets one, so a node without any stays small.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsDetails {
    /// What the control says about itself after its name.
    pub state_description: Option<String>,
    /// What activating the control does, as a verb phrase a reader reads out.
    pub on_click_label: Option<String>,
    /// What this control does when a screen reader asks for its long press.
    pub on_long_click: Option<SemanticsLongClick>,
    /// What the long press does, as a verb phrase a reader reads out.
    pub on_long_click_label: Option<String>,
    /// What this control does on VoiceOver's magic tap.
    pub on_magic_tap: Option<SemanticsMagicTap>,
    /// What the magic tap does, as a verb phrase a reader reads out.
    pub on_magic_tap_label: Option<String>,
    /// The short names a person says to Voice Control to reach this control.
    pub input_labels: Vec<String>,
    /// The language of this control's text, as a BCP 47 tag.
    pub language: Option<String>,
    /// The actions a screen reader lists for the control beyond its click.
    pub custom_actions: Vec<SemanticsCustomAction>,
    /// Controls this node drew rather than laid out; their bounds are relative
    /// to this node's own top-left. See [`CanvasSemanticsNode`].
    pub canvas_children: Vec<CanvasSemanticsNode>,
    /// Whether this node is a field a person types into.
    pub editable_text: bool,
    /// Whether an editable field accepts line breaks, independent of its current text.
    pub multiline: bool,
    /// Whether this subtree makes content outside it unavailable to assistive technology.
    pub is_modal: bool,
    /// Whether the selectable controls under this node form one group.
    pub selectable_group: bool,
    /// The title of the screen or pane this node is the root of.
    pub pane_title: Option<String>,
    /// Why the control's content is wrong, when it is.
    pub error: Option<String>,
    /// Whether this field holds a secret, so its text stays unspoken.
    pub password: bool,
    /// Where the caret of an editable field sits, or which stretch of its
    /// text is picked.
    pub text_selection: Option<TextRange>,
    /// How urgently a screen reader reads this node when its text changes.
    /// Compose's `SemanticsProperties.LiveRegion`.
    pub live_region: Option<LiveRegionMode>,
    /// The value this control holds inside a range. Compose's
    /// `ProgressBarRangeInfo`.
    pub progress: Option<ProgressBarRangeInfo>,
    /// What this control does when a screen reader moves its value.
    pub set_progress: Option<SemanticsSetProgress>,
    /// What this field does when a screen reader hands it text.
    pub set_text: Option<SemanticsSetText>,
    /// What this field does when a screen reader moves its caret or picks a
    /// stretch of its text.
    pub set_selection: Option<SemanticsSetSelection>,
    /// What this control does when a screen reader asks it to open.
    pub expand: Option<SemanticsExpand>,
    /// What this control does when a screen reader asks it to close.
    pub collapse: Option<SemanticsExpand>,
    /// What this control does when a screen reader asks to send it away.
    pub dismiss: Option<SemanticsDismiss>,
    /// How far this container scrolled up and down, when it scrolls.
    pub vertical_scroll: Option<ScrollAxisRange>,
    /// How far this container scrolled left and right, when it scrolls.
    pub horizontal_scroll: Option<ScrollAxisRange>,
    /// What this container does when a screen reader pages it.
    pub scroll_by: Option<SemanticsScrollBy>,
    /// What this list does when a screen reader asks for the row at an index.
    pub scroll_to_index: Option<SemanticsScrollToIndex>,
    /// How many rows and columns this list holds, when it is a list.
    pub collection: Option<CollectionInfo>,
}

impl SemanticsDetails {
    /// Details with nothing set: what a node that holds none reports.
    pub const NONE: Self = Self {
        state_description: None,
        on_click_label: None,
        on_long_click: None,
        on_long_click_label: None,
        on_magic_tap: None,
        on_magic_tap_label: None,
        input_labels: Vec::new(),
        language: None,
        custom_actions: Vec::new(),
        canvas_children: Vec::new(),
        editable_text: false,
        multiline: false,
        is_modal: false,
        selectable_group: false,
        pane_title: None,
        error: None,
        password: false,
        text_selection: None,
        live_region: None,
        progress: None,
        set_progress: None,
        set_text: None,
        set_selection: None,
        expand: None,
        collapse: None,
        dismiss: None,
        vertical_scroll: None,
        horizontal_scroll: None,
        scroll_by: None,
        scroll_to_index: None,
        collection: None,
    };

    pub(super) fn from_configuration(
        config: SemanticsConfiguration,
        on_click_label: Option<String>,
        is_modal: bool,
    ) -> Self {
        Self {
            state_description: config.state_description,
            on_click_label,
            on_long_click: config.on_long_click,
            on_long_click_label: config.on_long_click_label,
            on_magic_tap: config.on_magic_tap,
            on_magic_tap_label: config.on_magic_tap_label,
            input_labels: config.input_labels,
            language: config.language,
            custom_actions: config.custom_actions,
            canvas_children: config.canvas_children,
            editable_text: config.is_editable_text,
            multiline: config.multiline,
            is_modal,
            selectable_group: config.selectable_group,
            pane_title: config.pane_title,
            error: config.error,
            password: config.password,
            text_selection: config.text_selection,
            live_region: config.live_region,
            progress: config.progress,
            set_progress: config.set_progress,
            set_text: config.set_text,
            set_selection: config.set_selection,
            expand: config.expand,
            collapse: config.collapse,
            dismiss: config.dismiss,
            vertical_scroll: config.vertical_scroll,
            horizontal_scroll: config.horizontal_scroll,
            scroll_by: config.scroll_by,
            scroll_to_index: config.scroll_to_index,
            collection: config.collection,
        }
    }
}

impl Default for SemanticsDetails {
    fn default() -> Self {
        Self::NONE
    }
}

impl RareProperties for SemanticsDetails {
    const EMPTY: &'static Self = &Self::NONE;
}

impl SemanticsNode {
    /// The properties few nodes set, with nothing set for a node that holds
    /// none.
    pub fn details(&self) -> &SemanticsDetails {
        rare(&self.details)
    }

    /// Replaces the node's details. A node holds them only when something is
    /// set, and keeps the space it already held for them.
    pub fn set_details(&mut self, details: SemanticsDetails) {
        set_rare(&mut self.details, details);
    }

    /// Changes some of the node's details in place, through
    /// [`SemanticsNode::set_details`].
    pub fn update_details(&mut self, update: impl FnOnce(&mut SemanticsDetails)) {
        update_rare(&mut self.details, update);
    }
}

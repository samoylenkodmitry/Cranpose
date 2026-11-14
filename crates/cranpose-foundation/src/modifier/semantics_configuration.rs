use super::{
    CanvasSemanticsNode, CollectionInfo, LiveRegionMode, ProgressBarRangeInfo, ScrollAxisRange,
    SemanticsCustomAction, SemanticsDismiss, SemanticsExpand, SemanticsLongClick,
    SemanticsMagicTap, SemanticsScrollBy, SemanticsScrollToIndex, SemanticsSetProgress,
    SemanticsSetSelection, SemanticsSetText, SemanticsWidgetRole, ToggleableState,
};

/// Semantics configuration for accessibility.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsConfiguration {
    /// What a screen reader reads as the name of the node. Compose's `contentDescription`.
    pub content_description: Option<String>,
    /// Compose's `stateDescription`.
    pub state_description: Option<String>,
    /// Compose's `onClick(label = …)`; implies clickable.
    pub on_click_label: Option<String>,
    /// Activation callback for controls whose pointer gesture is handled separately.
    /// Invoked by keyboard and native accessibility activation after validation.
    pub on_click: Option<SemanticsCustomAction>,
    /// What this control does when a screen reader asks for its long press.
    /// Compose's `onLongClick`.
    pub on_long_click: Option<SemanticsLongClick>,
    /// What the long press does, as a verb phrase a reader reads out:
    /// "Remove receipt". Compose's `onLongClick(label = …)`.
    pub on_long_click_label: Option<String>,
    /// What this control does on VoiceOver's magic tap, and on the other
    /// platforms as an action listed by [`Self::on_magic_tap_label`].
    /// SwiftUI's `accessibilityAction(.magicTap)`.
    pub on_magic_tap: Option<SemanticsMagicTap>,
    /// What the magic tap does, as a verb phrase a reader reads out: "Take
    /// the photo".
    pub on_magic_tap_label: Option<String>,
    /// The short names a person says to Voice Control to reach this control,
    /// when the name a reader hears is too long to say. SwiftUI's
    /// `accessibilityInputLabels`; iOS only.
    pub input_labels: Vec<String>,
    /// The language of this control's text as a BCP 47 tag, "de" or "pt-BR",
    /// so a reader picks the right voice for it. SwiftUI's
    /// `accessibilityLanguage`, ARIA's `lang`.
    pub language: Option<String>,
    /// Compose's `Role`.
    pub role: Option<SemanticsWidgetRole>,
    /// Whether the control is selected. `None` means the control has no selected state.
    pub selected: Option<bool>,
    /// Whether the control is on, off or partly on. Compose's
    /// `toggleableState`.
    pub toggled: Option<ToggleableState>,
    /// Whether the control accepts input. A disabled control is announced as such.
    pub enabled: bool,
    /// Whether the control reacts to a click.
    pub is_clickable: bool,
    /// Whether the node is a text field a user edits.
    pub is_editable_text: bool,
    /// Whether an editable field accepts line breaks, independent of its current text.
    pub multiline: bool,
    /// The text an editable field holds, read as its value. Compose's
    /// `editableText`.
    pub text: Option<String>,
    /// The selection or the caret of an editable field, as byte offsets into its text.
    pub text_selection: Option<crate::text::TextRange>,
    /// Named actions that a screen reader lists in its actions menu for the node.
    pub custom_actions: Vec<SemanticsCustomAction>,
    /// Controls this node drew itself instead of laying out. See
    /// [`CanvasSemanticsNode`].
    pub canvas_children: Vec<CanvasSemanticsNode>,
    /// Whether this node takes over the screen: everything outside it is
    /// inert, and a screen reader keeps its traversal inside.
    pub is_modal: bool,
    /// Whether a screen reader skips this node and everything under it: a
    /// decorative image, or a placeholder drawn under a named field. Compose's
    /// `hideFromAccessibility`.
    pub hidden: bool,
    /// Whether a screen reader takes this node and the text under it as one
    /// stop, the way it does for a button: a row whose name, count and price
    /// belong together. Compose's `mergeDescendants`.
    pub merge_descendants: bool,
    /// Whether the selectable controls under this node form one group, so a
    /// screen reader says which of how many a tab or a radio button is.
    /// Compose's `selectableGroup`.
    pub selectable_group: bool,
    /// The title of the screen or pane this node is the root of, read out when
    /// the app moves to it. Compose's `paneTitle`.
    pub pane_title: Option<String>,
    /// Why the control's content is wrong, read after its state: "invalid,
    /// the amount needs a number". Compose's `error`.
    pub error: Option<String>,
    /// What using the control does or what it wants, read after its other
    /// words: "double tap to choose a color". Compose's `hintText`.
    pub hint_text: Option<String>,
    /// Whether this field holds a secret, so screen readers never receive its
    /// text and native editors use protected input. Compose's `password`.
    pub password: bool,
    /// Where a screen reader visits this node among the ones beside it: a
    /// smaller number comes first, and nodes left at zero keep the order the
    /// app laid them out in. Compose's `traversalIndex`.
    pub traversal_index: f32,
    /// A name tests find this node by. Screen readers never receive it.
    /// Compose's `testTag`.
    pub test_tag: Option<String>,
    /// Compose's `liveRegion`. When set, a screen reader reads this node again
    /// whenever its text changes, without the user moving to it.
    pub live_region: Option<LiveRegionMode>,
    /// Compose's `progressBarRangeInfo`. A control with it is adjustable: a
    /// screen reader offers its own way to move the value.
    pub progress: Option<ProgressBarRangeInfo>,
    /// What the control does when a screen reader moves its value. Compose's
    /// `setProgress`.
    pub set_progress: Option<SemanticsSetProgress>,
    /// What this field does when a screen reader or a voice tool hands it
    /// text. Compose's `setText`.
    pub set_text: Option<SemanticsSetText>,
    /// What this field does when a screen reader moves its caret or picks a
    /// stretch of its text. Compose's `setSelection`.
    pub set_selection: Option<SemanticsSetSelection>,
    /// What this control does when a screen reader asks it to open. A control
    /// that says so reads as closed. Compose's `expand`.
    pub expand: Option<SemanticsExpand>,
    /// What this control does when a screen reader asks to send it away: a row
    /// a sighted person swipes off, a sheet a sighted person taps outside of.
    /// Compose's `dismiss`.
    pub dismiss: Option<SemanticsDismiss>,
    /// What this control does when a screen reader asks it to close. A control
    /// that says so reads as open. Compose's `collapse`.
    pub collapse: Option<SemanticsExpand>,
    /// How far this container scrolled up and down. Compose's
    /// `verticalScrollAxisRange`.
    pub vertical_scroll: Option<ScrollAxisRange>,
    /// How far this container scrolled left and right. Compose's
    /// `horizontalScrollAxisRange`.
    pub horizontal_scroll: Option<ScrollAxisRange>,
    /// What this container does when a screen reader pages it. Compose's
    /// `scrollBy`.
    pub scroll_by: Option<SemanticsScrollBy>,
    /// What this list does when a screen reader asks for the row at an index,
    /// so a reader reaches row 300 without paging to it. Compose's
    /// `scrollToIndex`.
    pub scroll_to_index: Option<SemanticsScrollToIndex>,
    /// How many rows and columns this list holds. Compose's `collectionInfo`.
    pub collection: Option<CollectionInfo>,
}

impl Default for SemanticsConfiguration {
    fn default() -> Self {
        Self {
            content_description: None,
            state_description: None,
            on_click_label: None,
            on_click: None,
            on_long_click: None,
            on_long_click_label: None,
            on_magic_tap: None,
            on_magic_tap_label: None,
            input_labels: Vec::new(),
            language: None,
            role: None,
            selected: None,
            toggled: None,
            enabled: true,
            is_clickable: false,
            is_editable_text: false,
            multiline: false,
            text: None,
            text_selection: None,
            custom_actions: Vec::new(),
            canvas_children: Vec::new(),
            is_modal: false,
            hidden: false,
            merge_descendants: false,
            selectable_group: false,
            pane_title: None,
            error: None,
            hint_text: None,
            password: false,
            traversal_index: 0.0,
            test_tag: None,
            live_region: None,
            progress: None,
            set_progress: None,
            set_text: None,
            set_selection: None,
            expand: None,
            dismiss: None,
            collapse: None,
            vertical_scroll: None,
            horizontal_scroll: None,
            scroll_by: None,
            scroll_to_index: None,
            collection: None,
        }
    }
}

/// One value that says what a screen reader reads for a node, built up with
/// the methods below and handed to `Modifier::semantics_spec`.
///
/// Compose has no such value: it takes a receiver lambda, which Kotlin makes
/// read well and Rust has no match for. `SemanticsSpec::new().content_description("Save")`
/// reads better than `|config| config.content_description = Some("Save".into())`,
/// it is one chain element rather than one per property, and two specs can be
/// compared. The closure form stays as `Modifier::semantics` for parity.
pub type SemanticsSpec = SemanticsConfiguration;

impl SemanticsConfiguration {
    /// An empty spec to build on. Every field is what it is with no semantics
    /// declared at all.
    pub fn new() -> Self {
        Self::default()
    }

    /// The name a screen reader reads for the control. Compose's
    /// `contentDescription`.
    pub fn content_description(mut self, name: impl Into<String>) -> Self {
        self.content_description = Some(name.into());
        self
    }

    /// A name tests find this node by. Screen readers never receive it.
    /// Compose's `testTag`.
    pub fn test_tag(mut self, tag: impl Into<String>) -> Self {
        self.test_tag = Some(tag.into());
        self
    }

    /// What the control says about itself after its name. Compose's
    /// `stateDescription`.
    pub fn state_description(mut self, state: impl Into<String>) -> Self {
        self.state_description = Some(state.into());
        self
    }

    /// A screen reader offers to activate the control. Compose's `onClick`.
    pub fn clickable(mut self) -> Self {
        self.is_clickable = true;
        self
    }

    /// Handles keyboard and screen-reader activation without adding a pointer gesture.
    /// An empty label uses the platform's default activation instruction.
    pub fn on_click(mut self, label: impl Into<String>, action: impl Fn() + 'static) -> Self {
        self.on_click = Some(SemanticsCustomAction::new(label, action));
        self
    }

    /// What the control does when a screen reader asks for its long press,
    /// and the verb phrase a reader reads out for it. Compose's
    /// `onLongClick(label) { … }`.
    pub fn on_long_click(
        mut self,
        label: impl Into<String>,
        action: impl Fn() -> bool + 'static,
    ) -> Self {
        self.on_long_click_label = Some(label.into());
        self.on_long_click = Some(SemanticsLongClick::new(action));
        self
    }

    /// What the control does on VoiceOver's magic tap, and the verb phrase
    /// the other platforms list it under. SwiftUI's
    /// `accessibilityAction(.magicTap)`.
    pub fn on_magic_tap(
        mut self,
        label: impl Into<String>,
        action: impl Fn() -> bool + 'static,
    ) -> Self {
        self.on_magic_tap_label = Some(label.into());
        self.on_magic_tap = Some(SemanticsMagicTap::new(action));
        self
    }

    /// The short names a person says to Voice Control to reach the control.
    /// SwiftUI's `accessibilityInputLabels`.
    pub fn input_labels<S: Into<String>>(mut self, labels: impl IntoIterator<Item = S>) -> Self {
        self.input_labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// The language of the control's text, as a BCP 47 tag. SwiftUI's
    /// `accessibilityLanguage`, ARIA's `lang`.
    pub fn language(mut self, tag: impl Into<String>) -> Self {
        self.language = Some(tag.into());
        self
    }

    /// Whether the control is on, off or partly on. Compose's
    /// `toggleableState`; a `bool` is on or off.
    pub fn toggled(mut self, toggled: impl Into<ToggleableState>) -> Self {
        self.toggled = Some(toggled.into());
        self
    }

    /// Whether the control is the one picked out of a group. Compose's
    /// `selected`.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// What kind of control a screen reader reads this as. Compose's `Role`.
    pub fn role(mut self, role: SemanticsWidgetRole) -> Self {
        self.role = Some(role);
        self
    }

    /// Reads as a heading, so a reader can jump between the headings of a
    /// screen. Compose's `heading()`.
    pub fn heading(self) -> Self {
        self.role(SemanticsWidgetRole::Header)
    }

    /// Why the control's content is wrong. Compose's `error`.
    pub fn error(mut self, message: impl Into<String>) -> Self {
        self.error = Some(message.into());
        self
    }

    /// What using the control does or what it wants. Compose's `hintText`.
    pub fn hint_text(mut self, hint: impl Into<String>) -> Self {
        self.hint_text = Some(hint.into());
        self
    }

    /// Holds a secret, so no reader reads the text out. Compose's `password`.
    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    /// Names the screen or pane this node is the root of. Compose's
    /// `paneTitle`.
    pub fn pane_title(mut self, title: impl Into<String>) -> Self {
        self.pane_title = Some(title.into());
        self
    }

    /// Where a reader visits this node among the ones beside it. Compose's
    /// `traversalIndex`.
    pub fn traversal_index(mut self, index: f32) -> Self {
        self.traversal_index = index;
        self
    }

    /// Skips this node and everything under it. Compose's
    /// `hideFromAccessibility`.
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// Takes this node and the text under it as one stop. Compose's
    /// `mergeDescendants`.
    pub fn merge_descendants(mut self) -> Self {
        self.merge_descendants = true;
        self
    }

    /// Makes the selectable controls under this node one group. Compose's
    /// `selectableGroup`.
    pub fn selectable_group(mut self) -> Self {
        self.selectable_group = true;
        self
    }

    /// Reads this node again whenever its text changes. Compose's
    /// `liveRegion`.
    pub fn live_region(mut self, mode: LiveRegionMode) -> Self {
        self.live_region = Some(mode);
        self
    }
    /// Folds `other` into this configuration. A value that `other` sets replaces the value here.
    /// `enabled` stays true only when both are enabled, the boolean flags turn true when either is
    /// true, and the lists are appended.
    pub fn merge(&mut self, other: &Self) {
        if let Some(description) = &other.content_description {
            self.content_description = Some(description.clone());
        }
        if let Some(state) = &other.state_description {
            self.state_description = Some(state.clone());
        }
        if let Some(label) = &other.on_click_label {
            self.on_click_label = Some(label.clone());
        }
        if let Some(label) = &other.on_long_click_label {
            self.on_long_click_label = Some(label.clone());
        }
        if let Some(label) = &other.on_magic_tap_label {
            self.on_magic_tap_label = Some(label.clone());
        }
        if !other.input_labels.is_empty() {
            self.input_labels.clone_from(&other.input_labels);
        }
        if let Some(language) = &other.language {
            self.language = Some(language.clone());
        }
        if let Some(role) = other.role {
            self.role = Some(role);
        }
        if let Some(selected) = other.selected {
            self.selected = Some(selected);
        }
        if let Some(toggled) = other.toggled {
            self.toggled = Some(toggled);
        }
        self.enabled &= other.enabled;
        self.is_clickable |= other.is_clickable;
        self.is_editable_text |= other.is_editable_text;
        self.multiline |= other.multiline;
        if let Some(text) = &other.text {
            self.text = Some(text.clone());
        }
        self.is_modal |= other.is_modal;
        self.hidden |= other.hidden;
        self.merge_descendants |= other.merge_descendants;
        self.selectable_group |= other.selectable_group;
        self.password |= other.password;
        if other.traversal_index != 0.0 {
            self.traversal_index = other.traversal_index;
        }
        if let Some(live_region) = other.live_region {
            self.live_region = Some(live_region);
        }
        self.merge_words(other);
        self.merge_actions(other);
        self.merge_ranges(other);
    }

    fn merge_words(&mut self, other: &Self) {
        if let Some(title) = &other.pane_title {
            self.pane_title = Some(title.clone());
        }
        if let Some(error) = &other.error {
            self.error = Some(error.clone());
        }
        if let Some(hint) = &other.hint_text {
            self.hint_text = Some(hint.clone());
        }
        if let Some(tag) = &other.test_tag {
            self.test_tag = Some(tag.clone());
        }
    }

    fn merge_actions(&mut self, other: &Self) {
        if let Some(on_click) = &other.on_click {
            self.on_click = Some(on_click.clone());
        }
        self.custom_actions
            .extend(other.custom_actions.iter().cloned());
        self.canvas_children
            .extend(other.canvas_children.iter().cloned());
        if let Some(set_progress) = &other.set_progress {
            self.set_progress = Some(set_progress.clone());
        }
        if let Some(set_text) = &other.set_text {
            self.set_text = Some(set_text.clone());
        }
        if let Some(set_selection) = &other.set_selection {
            self.set_selection = Some(set_selection.clone());
        }
        if let Some(expand) = &other.expand {
            self.expand = Some(expand.clone());
        }
        if let Some(collapse) = &other.collapse {
            self.collapse = Some(collapse.clone());
        }
        if let Some(dismiss) = &other.dismiss {
            self.dismiss = Some(dismiss.clone());
        }
        if let Some(long_click) = &other.on_long_click {
            self.on_long_click = Some(long_click.clone());
        }
        if let Some(magic_tap) = &other.on_magic_tap {
            self.on_magic_tap = Some(magic_tap.clone());
        }
        if let Some(scroll_by) = &other.scroll_by {
            self.scroll_by = Some(scroll_by.clone());
        }
        if let Some(scroll_to_index) = &other.scroll_to_index {
            self.scroll_to_index = Some(scroll_to_index.clone());
        }
    }

    fn merge_ranges(&mut self, other: &Self) {
        if let Some(selection) = other.text_selection {
            self.text_selection = Some(selection);
        }
        if let Some(progress) = other.progress {
            self.progress = Some(progress);
        }
        if let Some(range) = other.vertical_scroll {
            self.vertical_scroll = Some(range);
        }
        if let Some(range) = other.horizontal_scroll {
            self.horizontal_scroll = Some(range);
        }
        if let Some(collection) = other.collection {
            self.collection = Some(collection);
        }
    }

    /// Whether a screen reader should offer activation. A named click label is
    /// how Compose declares `onClick`, so it implies the action the same way.
    pub fn is_activatable(&self) -> bool {
        self.is_clickable || self.on_click_label.is_some() || self.on_click.is_some()
    }
}

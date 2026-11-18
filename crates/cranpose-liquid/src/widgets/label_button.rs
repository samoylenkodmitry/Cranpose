use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, SemanticsConfiguration, SemanticsWidgetRole,
    text::TextStyle,
    widgets::{Box, BoxSpec, Text},
};
use cranpose_ui_layout::Alignment;

// A button that shows `label` centered in `style` and reads it to readers as
// a button; a tap runs `on_click`. `modifier` sizes and draws it.
#[composable(no_skip)]
pub(super) fn LabelButton(
    modifier: Modifier,
    label: &str,
    style: TextStyle,
    on_click: impl Fn() + 'static,
) {
    let (label, reader_label) = (label.to_owned(), label.to_owned());
    Box(
        modifier.clickable(move |_| on_click()).semantics(
            move |config: &mut SemanticsConfiguration| {
                config.role = Some(SemanticsWidgetRole::Button);
                config.content_description = Some(reader_label.clone());
            },
        ),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                label.clone(),
                Modifier::empty().hide_from_accessibility(),
                style.clone(),
            );
        },
    );
}

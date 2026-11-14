// Three tagged buttons composed and published the way every platform bridge
// receives them. "Pay" sits outside any `test_tags_as_resource_id`, "Save"
// under a column that turns it on, and "Undo" under a column inside that one
// that turns it off again.

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{Button, ButtonSpec, Column, ColumnSpec, Modifier, Text, TextStyle, composable};

use crate::accessibility::{AccessibilityElement, AccessibilitySnapshot, snapshot_if_changed};

// Each button's label and the tag it carries.
pub(crate) const TAGS: [(&str, &str); 3] = [
    ("Pay", "pay-button"),
    ("Save", "save-button"),
    ("Undo", "undo-button"),
];

#[composable]
fn TaggedButton(label: &'static str, tag: &'static str) {
    Button(
        Modifier::empty().height(48.0).test_tag(tag),
        ButtonSpec::default(),
        || {},
        move || {
            Text(label, Modifier::empty(), TextStyle::default());
        },
    );
}

#[composable]
fn TaggedControls() {
    let [(pay, pay_tag), (save, save_tag), (undo, undo_tag)] = TAGS;
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        TaggedButton(pay, pay_tag);
        Column(
            Modifier::empty().semantics(|config| config.test_tags_as_resource_id = Some(true)),
            ColumnSpec::default(),
            move || {
                TaggedButton(save, save_tag);
                Column(
                    Modifier::empty()
                        .semantics(|config| config.test_tags_as_resource_id = Some(false)),
                    ColumnSpec::default(),
                    move || TaggedButton(undo, undo_tag),
                );
            },
        );
    });
}

// The elements a reader receives for the three buttons.
pub(crate) fn tagged_elements() -> Vec<AccessibilityElement> {
    let mut shell = AppShell::new(
        WgpuRenderer::new(&[]),
        cranpose_core::location_key(file!(), line!(), column!()),
        TaggedControls,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.set_semantics_enabled(true);
    shell.update();
    snapshot_if_changed(&mut shell, &mut None, &mut AccessibilitySnapshot::default())
        .expect("a first snapshot")
}

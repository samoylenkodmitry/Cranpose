# Cranpose UI

Compose screens from text, buttons, fields, images and layout containers.
Modifiers add size constraints, backgrounds, gestures, focus and accessibility
semantics. Lazy lists compose rows as the viewport needs them.

App projects usually import this API through
[`cranpose::prelude`](https://docs.rs/cranpose/latest/cranpose/prelude/).
Depend on `cranpose-ui` directly for a widget library or a custom host.

```sh
cargo add cranpose-ui
```

## Build a reusable control

A composable accepts a callback. The caller supplies the save action; the
control calls the callback after a click:

```rust
use cranpose_ui::{composable, Button, ButtonSpec, Modifier, Text, TextStyle};

#[composable]
fn SaveButton<F>(on_save: F)
where
    F: FnMut() + 'static,
{
    Button(
        Modifier::empty().height(48.0),
        ButtonSpec::default(),
        on_save,
        || {
            Text("Save", Modifier::empty(), TextStyle::default());
        },
    );
}
```

Call `SaveButton` from another composable. A platform host starts and drives the
composition. The [Cranpose app example](https://docs.rs/cranpose/latest/cranpose/)
shows desktop setup and state updates.

## Choose a layout

| Task | API |
| --- | --- |
| Stack children vertically | [`Column`](https://docs.rs/cranpose-ui/latest/cranpose_ui/widgets/column/fn.Column.html) |
| Place children in a horizontal row | [`Row`](https://docs.rs/cranpose-ui/latest/cranpose_ui/widgets/row/fn.Row.html) |
| Overlay children | [`Box`](https://docs.rs/cranpose-ui/latest/cranpose_ui/widgets/box_widget/fn.Box.html) |
| Wrap children across rows | [`FlowRow`](https://docs.rs/cranpose-ui/latest/cranpose_ui/widgets/flow_row/fn.FlowRow.html) |
| Compose a viewport from many items | [`LazyColumn`](https://docs.rs/cranpose-ui/latest/cranpose_ui/widgets/lazy_list/fn.LazyColumn.html), [`LazyRow`](https://docs.rs/cranpose-ui/latest/cranpose_ui/widgets/lazy_list/fn.LazyRow.html) |
| Define a measure policy | [`Layout`](https://docs.rs/cranpose-ui/latest/cranpose_ui/widgets/layout/fn.Layout.html) and [`MeasurePolicy`](https://docs.rs/cranpose-ui-layout/latest/cranpose_ui_layout/trait.MeasurePolicy.html) |

Modifier order affects geometry. For example,
`Modifier::empty().padding(12.0).background(color)` places the background inside
the outer space. `Modifier::empty().background(color).padding(12.0)` extends the
background across the control and its inner space.

## Text, input and graphics

Use [`BasicTextField`](https://docs.rs/cranpose-ui/latest/cranpose_ui/widgets/basic_text_field/fn.BasicTextField.html)
with a retained `TextFieldState` for editable text. Use `SelectionContainer` for
selectable labels and `LinkedText` for annotated links. `Canvas` and
`Modifier::draw_behind` support custom graphics.

Controls contribute semantics for screen readers and tests. Custom controls add
roles, labels and actions through `Modifier::semantics`. Pointer, keyboard,
focus and rotary modifiers connect custom behavior to the same input system.

The optional `svg` feature enables SVG image support. `inspection` adds the
inspector data path. `test-helpers` serves framework tests. The default feature
set keeps these additions optional.

Read the [layout guide](https://docs.rs/cranpose/latest/cranpose/_docs/layout/)
for modifier and lazy-list examples, and the
[Liquid UI guide](https://docs.rs/cranpose-liquid/latest/cranpose_liquid/) for
styled controls and themes.

use std::{cell::Cell, rc::Rc};

use cranpose::{
    native_view::{NativeView, NativeViewHost},
    prelude::*,
};
use cranpose_core::{MutableState, rememberMutableStateOf};

#[derive(Clone, Default)]
pub(crate) struct DemoState(Rc<Cell<Option<MutableState<i32>>>>);

impl DemoState {
    pub(crate) fn count(&self) -> i32 {
        self.0.get().map_or(0, |state| state.value())
    }

    pub(crate) fn increment(&self) {
        if let Some(state) = self.0.get() {
            state.set(state.value().saturating_add(1));
        }
    }
}

pub(crate) fn content(host: NativeViewHost, state: DemoState, native_children: bool) {
    let count = rememberMutableStateOf(|| 0_i32);
    let show = rememberMutableStateOf(|| native_children);
    state.0.set(Some(count));
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.95, 0.96, 1.0, 1.0))
            .padding(16.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            Text("Cranpose component", Modifier::empty(), body_style());
            Text(
                format!("Shared count: {}", count.value()),
                Modifier::empty(),
                body_style(),
            );
            Row(
                Modifier::empty(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(12.0)),
                move || {
                    Action("Add in Rust", move || {
                        count.set(count.value().saturating_add(1));
                    });
                    if native_children {
                        Action(
                            if show.value() {
                                "Remove WebView"
                            } else {
                                "Mount WebView"
                            },
                            move || show.set(!show.value()),
                        );
                    }
                },
            );
            if native_children && show.value() {
                let html = format!(
                    "<html><meta name='viewport' content='width=device-width,initial-scale=1'><body style='font:18px -apple-system,Roboto,sans-serif;background:#e0f4ed;padding:12px'><b>Native WebView</b><p>Configured by Rust. Count: {}</p><a href='cranpose:increment'>Add from the native WebView</a><p>This page is bundled; no network or API key is needed.</p></body></html>",
                    count.value()
                );
                NativeView(
                    host.clone(),
                    "web",
                    &html,
                    Modifier::empty().fill_max_width().height(210.0),
                    move |event| {
                        if event == "increment" {
                            count.set(count.value().saturating_add(1));
                        }
                    },
                );
            }
            Text(
                "Native buttons and Rust buttons update the same component.",
                Modifier::empty(),
                body_style(),
            );
        },
    );
}

#[composable]
fn Action(label: &'static str, on_click: impl FnMut() + 'static) {
    Button(
        Modifier::empty()
            .background(Color(0.15, 0.30, 0.62, 1.0))
            .rounded_corners(8.0)
            .padding(10.0),
        ButtonSpec::default(),
        on_click,
        move || {
            let style = TextStyle::from_span_style(SpanStyle {
                color: Some(Color(1.0, 1.0, 1.0, 1.0)),
                ..Default::default()
            });
            Text(label, Modifier::empty(), style);
        },
    );
}

fn body_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(Color(0.08, 0.12, 0.22, 1.0)),
        ..Default::default()
    })
}
